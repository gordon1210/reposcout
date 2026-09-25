"""Behavior fixtures for the cargo-dist post-processing helpers."""

import hashlib
import json
import lzma
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]
FIXTURE = Path(__file__).parent / "fixtures/cargo-dist-0.33-installer-fragment.sh"
HASH = "0" * 64


class InstallerHardeningTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.installer = self.root / "installer.sh"
        self.installer.write_text(FIXTURE.read_text())

    def harden(self):
        return subprocess.run(
            [sys.executable, SCRIPTS / "harden-cargo-dist-installer.py", self.installer],
            capture_output=True,
            text=True,
        )

    def test_hardens_generated_blocks_and_refuses_missing_checker(self):
        self.assertEqual(self.harden().returncode, 0)
        self.assertEqual(subprocess.run(["sh", "-n", self.installer]).returncode, 0)
        test_script = (
            self.installer.read_text()
            + '\ncheck_cmd() { return 1; }\nsay() { :; }\nerr() { printf "%s\\n" "$1" >&2; exit 1; }\n'
            + f'verify_checksum /dev/null sha256 {HASH}\n'
        )
        output = subprocess.run(["bash", "-c", test_script], capture_output=True, text=True)
        self.assertNotEqual(output.returncode, 0)
        self.assertIn("cannot verify the sha256 checksum", output.stderr)
        self.assertIn('chmod 600 "$RECEIPT_HOME/$APP_NAME-receipt.json"', self.installer.read_text())

    def test_rejects_missing_and_repeated_template_blocks_without_writing(self):
        source = FIXTURE.read_text()
        for contents in (source.replace("skipping sha256", "ignoring sha256"), source + source):
            with self.subTest(contents=contents[:40]):
                self.installer.write_text(contents)
                self.assertNotEqual(self.harden().returncode, 0)
                self.assertEqual(self.installer.read_text(), contents)


class ArchivePreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.archive = self.root / "reposcout-x86_64-unknown-linux-gnu.tar.xz"
        self.sidecar = self.root / f"{self.archive.name}.sha256"
        self.manifest = self.root / "dist-manifest.json"
        self.payload = b"release content\n" * 512
        self.archive.write_bytes(lzma.compress(self.payload, preset=9))
        self.sidecar.write_text("old checksum\n")
        self.manifest.write_text(
            json.dumps(
                {"artifacts": {self.archive.name: {
                    "kind": "executable-zip",
                    "checksum": self.sidecar.name,
                    "checksums": {"sha256": "old"},
                }}}
            )
        )

    def prepare(self):
        return subprocess.run(
            [
                sys.executable,
                SCRIPTS / "prepare-cargo-dist-archives.py",
                "--manifest", self.manifest,
                "--artifacts-dir", self.root,
                "--max-decoder-memory-mib", "64",
            ],
            capture_output=True,
            text=True,
        )

    def test_preserves_archive_contents_and_updates_both_checksums(self):
        self.assertEqual(self.prepare().returncode, 0)
        decoder = lzma.LZMADecompressor(memlimit=64 * 1024 * 1024)
        self.assertEqual(decoder.decompress(self.archive.read_bytes()), self.payload)
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.assertEqual(self.sidecar.read_text(), f"{digest} *{self.archive.name}\n")
        manifest = json.loads(self.manifest.read_text())
        self.assertEqual(manifest["artifacts"][self.archive.name]["checksums"]["sha256"], digest)

    def test_damaged_archive_fails_without_overwriting_artifact_or_manifest(self):
        self.archive.write_bytes(b"not an xz archive")
        before_manifest = self.manifest.read_bytes()
        self.assertNotEqual(self.prepare().returncode, 0)
        self.assertEqual(self.archive.read_bytes(), b"not an xz archive")
        self.assertEqual(self.manifest.read_bytes(), before_manifest)
        self.assertEqual(self.sidecar.read_text(), "old checksum\n")


class ChecksumNormalizationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.path = Path(self.temporary.name) / "sha256.sum"

    def normalize(self):
        return subprocess.run(
            [sys.executable, SCRIPTS / "normalize-cargo-dist-checksums.py", self.path],
            capture_output=True,
            text=True,
        )

    def test_removes_only_one_extra_final_newline(self):
        self.path.write_text(f"{HASH} *source.tar.gz\n\n")
        self.assertEqual(self.normalize().returncode, 0)
        self.assertEqual(self.path.read_text(), f"{HASH} *source.tar.gz\n")

    def test_rejects_crlf_and_internal_blank_lines(self):
        for malformed in (
            f"{HASH} *source.tar.gz\r\n",
            f"{HASH} *source.tar.gz\n\n{HASH} *other.tar.xz\n",
        ):
            with self.subTest(malformed=malformed[:20]):
                self.path.write_bytes(malformed.encode())
                self.assertNotEqual(self.normalize().returncode, 0)
                self.assertEqual(self.path.read_bytes(), malformed.encode())


if __name__ == "__main__":
    unittest.main()
