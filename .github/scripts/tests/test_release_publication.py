"""Exercise the release workflow's publish command without contacting GitHub."""

from itertools import takewhile
from pathlib import Path
import subprocess
import tempfile
from textwrap import dedent
import unittest


WORKFLOW = Path(__file__).resolve().parents[2] / "workflows" / "release.yml"


class ReleasePublicationTests(unittest.TestCase):
    def test_publishes_existing_tag_when_default_branch_workflows_have_changed(self):
        workflow = WORKFLOW.read_text(encoding="utf-8")
        _, step_marker, step = workflow.partition("\n      - name: Create GitHub Release\n")
        self.assertTrue(step_marker, "missing GitHub Release step")
        _, run_marker, body = step.partition("\n        run: |\n")
        self.assertTrue(run_marker, "missing release shell command")
        lines = takewhile(lambda line: not line.strip() or line.startswith("          "), body.splitlines())
        script = dedent("\n".join(lines))
        fake_gh = r"""
gh() {
  for arg in "$@"; do
    case "$arg" in
      --target|--target=*)
        echo 'Explicit target requires workflow write permission' >&2
        return 1
        ;;
    esac
  done
  printf '%s\n' "$@"
}
"""

        with tempfile.TemporaryDirectory() as directory:
            workdir = Path(directory)
            (workdir / "artifacts").mkdir()
            (workdir / "artifacts" / "reposcout.tar.xz").touch()
            result = subprocess.run(
                ["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", fake_gh + script],
                cwd=workdir,
                env={
                    "RUNNER_TEMP": directory,
                    "RELEASE_TAG": "v1.2.3",
                    "RELEASE_COMMIT": "a" * 40,
                    "ANNOUNCEMENT_IS_PRERELEASE": "false",
                    "ANNOUNCEMENT_TITLE": "RepoScout 1.2.3",
                    "ANNOUNCEMENT_BODY": "Example release notes",
                },
                capture_output=True,
                text=True,
                timeout=5,
            )

        self.assertEqual(result.returncode, 0, result.stderr)
        args = result.stdout.splitlines()
        self.assertEqual(args[:3], ["release", "create", "v1.2.3"])
        self.assertIn("--verify-tag", args)
        self.assertIn("artifacts/reposcout.tar.xz", args)
