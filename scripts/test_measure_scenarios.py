"""Small ownership and complete-accounting checks; no RepoScout scan."""

import importlib.util
import hashlib
import json
import os
import signal
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("measure_scenarios", Path(__file__).with_name("measure-scenarios.py"))
MEASURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MEASURE)


class MeasurementTests(unittest.TestCase):
    def test_public_requested_range_contract_and_complete_source_bytes(self):
        with tempfile.TemporaryDirectory() as folder:
            fixture = Path(folder)
            source = b"first\nsecond\nthird\n"
            (fixture / "example.py").write_bytes(source)
            report = {"requested_targets": 1, "omitted_targets": 0,
                      "files": [{"id": 1, "path": "example.py",
                                 "snapshot": {"kind": "worktree"},
                                 "sha256": hashlib.sha256(source).hexdigest()}],
                      "results": [{"file": 1, "source": 1, "status": "complete",
                                   "selection": "range", "requested_range": {"start": 2, "end": 3}}],
                      "sources": [{"id": 1, "file": 1, "content": "second\nthird\n",
                                   "span": {"start_byte": 6, "end_byte": 19,
                                            "start_line": 2, "end_line": 3}}]}
            with patch.object(MEASURE, "RANGES", [("example.py", 2, 3)]):
                self.assertEqual(MEASURE.validate_response(json.dumps(report), fixture), report)
                report["results"][0]["requested_range"] = {"start_line": 2, "end_line": 3}
                with self.assertRaises(ValueError):
                    MEASURE.validate_response(json.dumps(report), fixture)
                report["results"][0]["requested_range"] = {"start": 2, "end": 3}
                report["sources"][0]["content"] = "second\nthird"
                with self.assertRaises(ValueError):
                    MEASURE.validate_response(json.dumps(report), fixture)
                report["sources"][0]["content"] = "second\nthird\n"
                for key, wrong in [("start_byte", 5), ("end_byte", 18),
                                   ("start_line", 1), ("end_line", 2)]:
                    original = report["sources"][0]["span"][key]
                    report["sources"][0]["span"][key] = wrong
                    with self.subTest(span=key), self.assertRaises(ValueError):
                        MEASURE.validate_response(json.dumps(report), fixture)
                    report["sources"][0]["span"][key] = original
                report["files"][0]["snapshot"] = {"kind": "tree", "revision": "a" * 40}
                with self.assertRaises(ValueError):
                    MEASURE.validate_response(json.dumps(report), fixture)

    def test_existing_and_symlink_outputs_preserve_unrelated_data(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            existing = root / "existing"
            existing.mkdir()
            marker = existing / "keep.txt"
            marker.write_bytes(b"user data")
            link = root / "alias"
            link.symlink_to(existing, target_is_directory=True)
            dangling = root / "dangling"
            dangling.symlink_to(root / "absent")
            for path in [existing, link, dangling, link / "new"]:
                with self.subTest(path=path), self.assertRaises(ValueError):
                    MEASURE.reserve_output(path)
            self.assertEqual(marker.read_bytes(), b"user data")
            self.assertFalse((existing / "new").exists())

    def test_token_pass_receives_every_raw_byte_and_keeps_separate_stream_costs(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            work = root / "work"
            output = root / "output"
            work.mkdir()
            output.mkdir()
            raw = {"stdout": b' {"source":"caf\xc3\xa9"}\n\n',
                   "stderr": b"warning and retry\n", "argv": b'["/binary","read","."]'}
            row = {}
            for stream, content in raw.items():
                path = output / stream
                path.write_bytes(content)
                row[stream + "_path"] = path.name

            def counter(argv, cwd, env, prefix, deadline):
                self.assertIn("--no-cache", argv)
                counts = {"stdout": 17, "stderr": 4, "argv": 9}
                for stream, content in raw.items():
                    self.assertEqual((cwd / f"0-{stream}.md").read_bytes(), content)
                path = output / "tokens.json"
                path.write_text(json.dumps({"files": [
                    {"path": f"0-{stream}.md", "tokens": count}
                    for stream, count in counts.items()]}))
                return {"exit_code": 0, "stopped": None, "stdout_path": path.name}

            with patch.object(MEASURE, "run_child", side_effect=counter):
                MEASURE.tokenize(Path("/binary"), [row], work, output, "o200k_base", time.monotonic() + 5)
            self.assertEqual(row["response_tokens"], 21)
            self.assertEqual(row["interaction_tokens"], 30)

    @unittest.skipUnless(sys.platform == "linux" and hasattr(os, "pidfd_open")
                         and hasattr(signal, "pidfd_send_signal"), "Linux pidfds required")
    def test_peak_is_per_child_and_large_stderr_cannot_deadlock_capture(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            env = MEASURE.child_environment(root)
            # The child can retain a pre-exec RSS floor from this runner; make its
            # allocation comfortably larger, without asserting a platform-dependent gap.
            high = MEASURE.run_child(
                [sys.executable, "-c", "import sys; x=bytearray(64*1024*1024); sys.stderr.write('x'*262144)"],
                root, env, root / "high", time.monotonic() + 5)
            low = MEASURE.run_child([sys.executable, "-c", "print('complete')"],
                                    root, env, root / "low", time.monotonic() + 5)
            MEASURE.require_success(high)
            MEASURE.require_success(low)
            self.assertEqual(high["stderr_bytes"], 262144)
            self.assertEqual((root / low["stdout_path"]).read_bytes(), b"complete\n")
            self.assertGreater(high["peak_rss_bytes"], low["peak_rss_bytes"],
                               "a later child must not inherit the earlier child's cumulative peak")


if __name__ == "__main__":
    unittest.main()
