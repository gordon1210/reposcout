import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from accounting import InvalidLedger, fingerprint
import review_cases
from codex_isolation import RUNTIME_PROBES, TOOL_ENVIRONMENT, runtime_tool_manifest


class ReviewCaseTests(unittest.TestCase):
    def test_external_large_bundle_preserves_application_docs_and_binds_its_own_limits(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundle = root / "bundle"
            (bundle / "application").mkdir(parents=True)
            (bundle / "large-case/head").mkdir(parents=True)
            (bundle / "application/README.md").write_text("# Public application contract\n")
            for index in range(36):
                (bundle / f"application/module_{index}.py").write_text(f"VALUE = {index}\n")
            (bundle / "large-case/head/module_0.py").write_text("VALUE = 7\n")
            case = {"case_id": "large-case", "title": "Large bounded application", "partition": "development",
                    "base_directory": "application", "revisions": ["head"],
                    "check_command": ["python3", "-B", "module_0.py"],
                    "steps": [{"base": "base", "head": "head", "task": "Review the public contract."}]}
            catalog = {"schema": 1, "source_limits": {"max_files": 64, "max_bytes": 65536}, "cases": [case]}
            (bundle / "catalog.json").write_text(json.dumps(catalog))
            snapshots = review_cases.snapshots("large-case", bundle_root=bundle)
            self.assertEqual(snapshots["head"]["README.md"], "# Public application contract\n")
            self.assertIn("python3 -B module_0.py", snapshots["head"]["REVIEW_CHECKS.md"])
            self.assertGreater(len(snapshots["base"]), 32)
            self.assertEqual(snapshots["base"]["module_0.py"], "VALUE = 0\n")
            (bundle / "large-case/oracle.json").write_text(json.dumps({"domain_expected": {"base": 0, "head": 7},
                "steps": [{"conclusion": "issues", "defects": [], "evidence_obligations": []}]}))
            (bundle / "large-case/probe.py").write_text(
                "import json, runpy, sys\nfrom pathlib import Path\nprint(json.dumps(runpy.run_path(str(Path(sys.argv[1])/'module_0.py'))['VALUE']))\n")
            self.assertTrue(review_cases.verify_domain("large-case", bundle_root=bundle)["passed"])
            signer = review_cases.create_signer(root / "shared-signer")
            first = review_cases.prepare_case("large-case", root / "first", bundle_root=bundle, signer=signer)
            second = review_cases.prepare_case("large-case", root / "second", bundle_root=bundle, signer=signer)
            other = review_cases.prepare_case("large-case", root / "other", bundle_root=bundle)
            self.assertEqual(first["steps"], second["steps"])
            self.assertEqual(first["oracle_sha256"], second["oracle_sha256"])
            self.assertEqual(first["check_commands"], [["python3", "-B", "module_0.py"]])
            self.assertEqual(first["steps"][0]["base_tree"], other["steps"][0]["base_tree"])
            self.assertNotEqual(first["steps"][0]["base_commit"], other["steps"][0]["base_commit"])
            self.assertNotEqual(first["oracle_sha256"], other["oracle_sha256"])
            from review_campaign import make_prompt
            self.assertEqual(make_prompt(first["steps"][0], "baseline")[1]["task_sha256"],
                             make_prompt(second["steps"][0], "reposcout")[1]["task_sha256"])
            self.assertNotEqual(make_prompt(first["steps"][0], "baseline")[1]["task_sha256"],
                                make_prompt(other["steps"][0], "reposcout")[1]["task_sha256"])
            catalog["source_limits"]["max_files"] = 32
            case["deletions"] = {"head": [f"module_{index}.py" for index in range(36)]}
            (bundle / "catalog.json").write_text(json.dumps(catalog))
            with self.assertRaises(InvalidLedger):
                review_cases.snapshots("large-case", bundle_root=bundle)

    def test_snapshots_rejects_oversized_source_before_reading_its_body(self):
        with tempfile.TemporaryDirectory() as temporary:
            bundle = Path(temporary)
            (bundle / "case/base").mkdir(parents=True)
            (bundle / "case/head").mkdir()
            oversized = bundle / "case/base/oversized.py"
            oversized.write_text("x" * 512)
            (bundle / "catalog.json").write_text(json.dumps({
                "source_limits": {"max_files": 4, "max_bytes": 128},
                "cases": [{"case_id": "case", "revisions": ["head"],
                           "check_command": ["python3", "-B", "oversized.py"]}]}))
            read_sources = []
            original = Path.read_text
            def observed_read(path, *args, **kwargs):
                if path == oversized:
                    read_sources.append(path.name)
                return original(path, *args, **kwargs)
            with patch.object(Path, "read_text", new=observed_read):
                with self.assertRaisesRegex(InvalidLedger, "bounds"):
                    review_cases.snapshots("case", bundle_root=bundle)
            self.assertEqual(read_sources, [], "oversized source bytes were read before enforcing the bound")

    def test_source_reader_checks_size_before_opening_an_oversized_blob(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "oversized.py").write_bytes(b"x" * 129)
            with patch.object(review_cases.os, "open", side_effect=AssertionError("oversized source was opened")):
                with self.assertRaisesRegex(InvalidLedger, "bounds"):
                    review_cases._directory_files(directory, max_files=4, max_bytes=128)

    def test_source_reader_stops_before_accumulating_excess_bytes_or_files(self):
        for max_files, max_bytes in ((1, 1024), (4, 100)):
            with self.subTest(files=max_files, size=max_bytes), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                (directory / "first.py").write_bytes(b"x" * 60)
                (directory / "second.py").write_bytes(b"y" * 60)
                with patch.object(review_cases.os, "open", wraps=os.open) as opened:
                    with self.assertRaisesRegex(InvalidLedger, "bounds"):
                        review_cases._directory_files(directory, max_files=max_files, max_bytes=max_bytes)
                    self.assertEqual(opened.call_count, 1)

    def test_source_reader_rejects_nonregular_inputs_without_blocking(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            os.mkfifo(directory / "input.py")
            with self.assertRaisesRegex(InvalidLedger, "regular"):
                review_cases._directory_files(directory, max_files=4, max_bytes=128)

    def test_main_inventory_and_frozen_holdout_are_separate(self):
        main = review_cases.list_cases()
        all_cases = review_cases.list_cases(include_holdout=True)
        self.assertEqual(len(main), 8)
        self.assertEqual(len(all_cases), 9)
        self.assertNotIn("renewal-holdout", {case["case_id"] for case in main})
        self.assertEqual(next(case["partition"] for case in all_cases if case["case_id"] == "renewal-holdout"), "holdout")
        self.assertTrue(set(review_cases.SMOKE_CASES).issubset({case["case_id"] for case in main}))
        self.assertEqual(len({case["fixture_sha256"] for case in all_cases}), 9)
        self.assertEqual(fingerprint(all_cases), "a6d81f05ac39286e0e86efaae985745996b4e1ca1ad4f89823cdb1cfa63e1876")

    def test_preparation_pins_verified_signed_commits_without_private_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case = review_cases.prepare_case("refund-boundary", root / "public", private_directory=root / "private")
            workspace = Path(case["workspace"])
            self.assertFalse(Path(case["private_oracle_path"]).is_relative_to(workspace))
            self.assertFalse((workspace / "oracle.json").exists())
            self.assertFalse((workspace / "probe.py").exists())
            for side in ("base", "head"):
                tree = case["steps"][0][side + "_tree"]
                commit = case["steps"][0][side + "_commit"]
                self.assertEqual(review_cases._git(workspace, "cat-file", "-t", tree), "tree")
                self.assertEqual(review_cases._git(workspace, "rev-parse", "refs/eval/" + side), commit)
                self.assertEqual(review_cases._git(workspace, "rev-parse", commit + "^{tree}"), tree)
                self.assertIn("gpgsig -----BEGIN SSH SIGNATURE-----", review_cases._git(workspace, "cat-file", "commit", commit))
                review_cases._git(workspace, "-c", "gpg.format=ssh", "-c",
                                  "gpg.ssh.allowedSignersFile=" + case["signer"]["allowed_signers"], "verify-commit", commit)
            self.assertNotIn("RepoScout", (workspace / "AGENTS.md").read_text())
            self.assertNotIn(str(root), (workspace / ".git/config").read_text())
            self.assertEqual(case["public_workspace_sha256"], review_cases._workspace_fingerprint(workspace))
            self.assertEqual((workspace / "policy.py").read_text(), review_cases.snapshots("refund-boundary")["head"]["policy.py"])
            with self.assertRaises(InvalidLedger):
                review_cases.prepare_case("refund-boundary", workspace)

    def test_future_repair_is_pinned_but_not_present_before_activation(self):
        with tempfile.TemporaryDirectory() as temporary:
            case = review_cases.prepare_case("review-followup", Path(temporary) / "public")
            workspace = Path(case["workspace"])
            future = review_cases.preview_step(case, 1)
            self.assertEqual(future["base_tree"], case["steps"][0]["head_tree"])
            self.assertEqual(case["active_step"], 0)
            with self.assertRaises(subprocess.CalledProcessError):
                review_cases._git(workspace, "cat-file", "-e", future["head_tree"])
            with self.assertRaises(subprocess.CalledProcessError):
                review_cases._git(workspace, "cat-file", "-e", future["head_commit"])
            text = review_cases.snapshots("review-followup")["followup"]["percentage.py"].encode()
            blob = hashlib.sha1(b"blob " + str(len(text)).encode() + b"\0" + text).hexdigest()
            with self.assertRaises(subprocess.CalledProcessError):
                review_cases._git(workspace, "cat-file", "-e", blob)
            activated = review_cases.activate_step(case, 1)
            self.assertEqual(activated, future)
            self.assertEqual(review_cases._git(workspace, "cat-file", "-t", blob), "blob")
            self.assertEqual(review_cases._git(workspace, "cat-file", "-t", future["head_commit"]), "commit")
            self.assertIn("divmod", (workspace / "percentage.py").read_text())
            with self.assertRaises(InvalidLedger):
                review_cases.activate_step(case, 1)

    def test_shared_task_signer_produces_identical_paired_commit_and_oracle_pins(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            signer = review_cases.create_signer(root / "signing")
            left = review_cases.prepare_case("review-followup", root / "left", signer=signer)
            right = review_cases.prepare_case("review-followup", root / "right", signer=signer)
            self.assertEqual(left["steps"], right["steps"])
            self.assertEqual(left["oracle_sha256"], right["oracle_sha256"])
            self.assertEqual(left["public_workspace_sha256"], right["public_workspace_sha256"])
            for record in (left, right):
                self.assertFalse(Path(signer["private_key"]).is_relative_to(record["workspace"]))
                self.assertFalse((Path(record["workspace"]) / "fixture_ed25519").exists())

    def test_initial_and_followup_repositories_have_clean_index_and_worktree(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            signer = review_cases.create_signer(root / "signing")
            for case in review_cases.list_cases(include_holdout=True):
                record = review_cases.prepare_case(case["case_id"], root / case["case_id"], signer=signer)
                workspace = Path(record["workspace"])
                for step_index in range(case["step_count"]):
                    with self.subTest(case=case["case_id"], step=step_index):
                        if step_index:
                            review_cases.activate_step(record, step_index)
                        self.assertEqual(review_cases._git(workspace, "status", "--porcelain=v1", "--untracked-files=all"), "")
                        self.assertEqual(review_cases._git(workspace, "diff", "--cached", "--exit-code", "HEAD", "--"), "")
                        self.assertEqual(review_cases._git(workspace, "diff", "--exit-code", "--"), "")
                        self.assertEqual(review_cases._git(workspace, "write-tree"), record["steps"][step_index]["head_tree"])
                        self.assertEqual(set(review_cases._git(workspace, "ls-files").splitlines()), set(record["installed_files"]))

    def test_changed_fixture_and_nested_oracle_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            public = Path(temporary) / "public"
            with self.assertRaises(InvalidLedger):
                review_cases.prepare_case("refund-boundary", public, private_directory=public / "hidden")
            self.assertFalse(public.exists())
            case = review_cases.prepare_case("review-followup", public)
            (public / "policy.py").write_text("tampered\n")
            with self.assertRaises(InvalidLedger):
                review_cases.activate_step(case, 1)

    def test_pure_git_tree_hash_matches_git_for_nested_name_sorting(self):
        files = {"a.c": "one\n", "a/file.py": "two\n", "a-/leaf": "three\n", "z": ""}
        with tempfile.TemporaryDirectory() as temporary:
            workspace = Path(temporary)
            review_cases._git(workspace, "init", "--quiet", "--template=", "--object-format=sha1")
            self.assertEqual(review_cases.snapshot_tree_id(files), review_cases._tree(workspace, files))

    def test_every_literal_domain_oracle_matches_the_independent_runtime(self):
        for case in review_cases.list_cases(include_holdout=True):
            with self.subTest(case=case["case_id"]):
                result = review_cases.verify_domain(case["case_id"])
                self.assertTrue(result["passed"], json.dumps(result, sort_keys=True))

    def test_package_regression_assertion_is_distinct_from_runtime_availability(self):
        node = runtime_tool_manifest(required=("node",))["node"]["path"]
        with tempfile.TemporaryDirectory() as temporary:
            for revision, files in review_cases.snapshots("package-wiring").items():
                directory = Path(temporary) / revision
                for name, contents in files.items():
                    target = directory / name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_text(contents)
                probe = subprocess.run([node, *RUNTIME_PROBES["node"][1:]], cwd=directory,
                                       env=TOOL_ENVIRONMENT, capture_output=True, timeout=5, check=False)
                self.assertEqual(probe.returncode, 0, "Node launcher/import prerequisite must succeed")
                check = subprocess.run([node, *review_cases.CHECK_COMMANDS["package-wiring"][1:]], cwd=directory,
                                       env=TOOL_ENVIRONMENT, capture_output=True, timeout=5, check=False)
                self.assertEqual(check.returncode, 0 if revision == "base" else 1)
                if revision != "base":
                    self.assertIn(b"AssertionError", check.stderr)


if __name__ == "__main__":
    unittest.main()
