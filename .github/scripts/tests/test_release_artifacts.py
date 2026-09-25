"""Bounded behavior tests for release binary and publish-file checks."""

import hashlib
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]


def load(name, filename):
    specification = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


SMOKE = load("release_smoke", "smoke-release-archive.py")
ARTIFACTS = load("release_artifacts", "verify-release-artifacts.py")


class PortabilityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "reposcout"
        self.binary.write_text("fixture")
        self.binary.chmod(0o700)
        self.environment = os.environ.copy()
        self.environment["PATH"] = f"{self.root}:{self.environment['PATH']}"

    def tool(self, name, body):
        path = self.root / name
        path.write_text(f"#!/bin/sh\n{body}\n")
        path.chmod(0o700)

    def verify(self, runner_os):
        return subprocess.run(
            [sys.executable, SCRIPTS / "verify-release-binary.py", "--os", runner_os, self.binary],
            env=self.environment,
            capture_output=True,
            text=True,
        )

    def test_macos_accepts_system_libraries_and_rejects_other_libraries(self):
        self.tool("otool", 'printf "fixture:\\n\\t/usr/lib/libSystem.B.dylib (1)\\n"')
        self.assertEqual(self.verify("macOS").returncode, 0)
        self.tool("otool", 'printf "fixture:\\n\\t/opt/local/lib/liblzma.dylib (1)\\n"')
        rejected = self.verify("macOS")
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("unexpected non-system dependency", rejected.stderr)

    def test_macos_analyzer_failure_and_invalid_output_fail_closed(self):
        self.tool("otool", "exit 42")
        self.assertNotEqual(self.verify("macOS").returncode, 0)
        self.tool("otool", 'printf "garbage\\n"')
        rejected = self.verify("macOS")
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("invalid library listing", rejected.stderr)

    def test_linux_accepts_other_needed_libraries_and_rejects_liblzma(self):
        self.tool("objdump", 'printf "  NEEDED               libc.so.6\\n"')
        self.assertEqual(self.verify("Linux").returncode, 0)
        self.tool("objdump", 'printf "  NEEDED               liblzma.so.5\\n"')
        rejected = self.verify("Linux")
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("unexpected dynamic liblzma", rejected.stderr)

    def test_linux_analyzer_failure_and_large_output_fail_closed(self):
        self.tool("objdump", "exit 42")
        self.assertNotEqual(self.verify("Linux").returncode, 0)
        self.tool(
            "objdump",
            'printf "  NEEDED liblzma.so.5\\n"; head -c 200000 /dev/zero | tr "\\000" x',
        )
        rejected = self.verify("Linux")
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("unexpected dynamic liblzma", rejected.stderr)


class PackedArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.archive = self.root / "reposcout-test.tar.xz"
        self.config = self.root / "test-global.toml"
        self.config.write_text("[scan]\n")

    def package(self, mode):
        executable = b"""#!/bin/sh
if [ "$1" = "--version" ]; then
  echo reposcout-fixture
  exit 0
fi
test "$1" = "-f" || exit 5
test -f "$4/tiny.rs" || exit 6
test -f "$REPOSCOUT_GLOBAL_CONFIG" || exit 7
echo '{"diagnostics":{"analyzed_files":1}}'
"""
        info = tarfile.TarInfo("reposcout-test/reposcout")
        info.mode = mode
        info.size = len(executable)
        with tarfile.open(self.archive, "w:xz") as package:
            package.addfile(info, io.BytesIO(executable))

    def test_executes_packed_binary_on_one_file(self):
        self.package(0o755)
        SMOKE.smoke(self.archive, self.config)

    def test_rejects_non_executable_packed_binary(self):
        self.package(0o644)
        with self.assertRaisesRegex(ValueError, "not executable"):
            SMOKE.smoke(self.archive, self.config)


class CompletenessTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        checksums = []
        for name in ARTIFACTS.ARCHIVES:
            contents = f"packed {name}".encode()
            (self.root / name).write_bytes(contents)
            digest = hashlib.sha256(contents).hexdigest()
            (self.root / f"{name}.sha256").write_text(f"{digest} *{name}\n")
            checksums.append(f"{digest} *{name}\n")
        (self.root / "sha256.sum").write_text("".join(checksums))
        for name in ARTIFACTS.OTHER_FILES:
            (self.root / name).write_text("fixture")

    def test_requires_all_publish_files_and_matching_checksums(self):
        ARTIFACTS.verify(self.root)
        (self.root / ARTIFACTS.ARCHIVES[0]).write_bytes(b"corrupted")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            ARTIFACTS.verify(self.root)

    def test_requires_host_manifest_for_final_publication(self):
        with self.assertRaisesRegex(ValueError, "dist-manifest.json"):
            ARTIFACTS.verify(self.root, include_host_manifest=True)
        (self.root / "dist-manifest.json").write_text("{}\n")
        ARTIFACTS.verify(self.root, include_host_manifest=True)

    def test_rejects_missing_target_and_incomplete_unified_checksums(self):
        (self.root / ARTIFACTS.ARCHIVES[1]).unlink()
        with self.assertRaisesRegex(ValueError, "missing or empty"):
            ARTIFACTS.verify(self.root)
        (self.root / ARTIFACTS.ARCHIVES[1]).write_bytes(b"replacement")
        (self.root / "sha256.sum").write_text((self.root / "source.tar.gz.sha256").read_text())
        with self.assertRaisesRegex(ValueError, "unified checksums"):
            ARTIFACTS.verify(self.root)


if __name__ == "__main__":
    unittest.main()
