"""Fixture tests for the exact-commit GitHub Actions release gate."""

import copy
import importlib.util
from pathlib import Path
import re
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("release_ci_gate", SCRIPTS / "check-release-ci.py")
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)
REPO = "gordon1210/reposcout"
SHA = "a" * 40


def run(workflow, run_id):
    return {
        "id": run_id,
        "run_attempt": 1,
        "head_sha": SHA,
        "head_branch": "main",
        "event": "push",
        "repository": {"full_name": REPO},
        "head_repository": {"full_name": REPO},
        "path": f".github/workflows/{workflow}@refs/heads/main",
        "status": "completed",
        "conclusion": "success",
    }


def job(name, run_id):
    return {"name": name, "run_id": run_id, "status": "completed", "conclusion": "success"}


class FakeAPI:
    def __init__(self):
        self.runs = {
            "rust.yml": [run("rust.yml", 10)],
            "release-helpers.yml": [run("release-helpers.yml", 20)],
        }
        self.jobs = {
            (10, 1): [job("Rust", 10), job("Native macOS", 10)],
            (20, 1): [job("Release helpers", 20)],
        }

    def __call__(self, path):
        workflow = re.search(r"/workflows/([^/]+)/runs", path)
        if workflow:
            entries = self.runs[workflow[1]]
            return {"total_count": len(entries), "workflow_runs": copy.deepcopy(entries)}
        attempt = re.search(r"/runs/(\d+)/attempts/(\d+)/jobs", path)
        if attempt:
            entries = self.jobs.get((int(attempt[1]), int(attempt[2])), [])
            return {"total_count": len(entries), "jobs": copy.deepcopy(entries)}
        raise AssertionError(f"unexpected API request: {path}")


class ReleaseGateTests(unittest.TestCase):
    def setUp(self):
        self.api = FakeAPI()

    def check(self):
        return GATE.check_release_ci(self.api, REPO, SHA)

    def test_accepts_exact_successful_main_runs_with_all_required_jobs(self):
        self.assertEqual(self.check(), {"rust.yml": 10, "release-helpers.yml": 20})

    def test_accepts_manual_main_verification_for_exact_commit(self):
        self.api.runs["rust.yml"][0]["event"] = "workflow_dispatch"
        self.assertEqual(self.check(), {"rust.yml": 10, "release-helpers.yml": 20})

    def test_rejects_missing_helper_run(self):
        self.api.runs["release-helpers.yml"] = []
        with self.assertRaisesRegex(ValueError, "no trusted main run"):
            self.check()

    def test_rejects_pr_fork_other_branch_and_other_commit_results(self):
        for change in (
            {"event": "pull_request"},
            {"head_branch": "feature"},
            {"head_repository": {"full_name": "fork/reposcout"}},
            {"head_sha": "b" * 40},
        ):
            with self.subTest(change=change):
                self.api.runs["rust.yml"][0].update(change)
                with self.assertRaisesRegex(ValueError, "no trusted main run"):
                    self.check()
                self.api = FakeAPI()

    def test_rejects_latest_failure_even_when_older_run_succeeded(self):
        newest = run("rust.yml", 11)
        newest["conclusion"] = "failure"
        self.api.runs["rust.yml"].append(newest)
        with self.assertRaisesRegex(ValueError, "latest run is not successful"):
            self.check()

    def test_rejects_in_progress_run(self):
        self.api.runs["rust.yml"][0].update(status="in_progress", conclusion=None)
        with self.assertRaisesRegex(ValueError, "latest run is not successful"):
            self.check()

    def test_rejects_skipped_required_job_even_if_run_succeeded(self):
        self.api.jobs[(10, 1)][1]["conclusion"] = "skipped"
        with self.assertRaisesRegex(ValueError, "did not succeed"):
            self.check()

    def test_rejects_partial_rerun_attempt(self):
        self.api.runs["rust.yml"][0]["run_attempt"] = 2
        self.api.jobs[(10, 2)] = [job("Rust", 10)]
        with self.assertRaisesRegex(ValueError, "re-run all jobs"):
            self.check()

    def test_rejects_api_error(self):
        def failing_fetch(_path):
            raise OSError("API unavailable")

        with self.assertRaisesRegex(OSError, "API unavailable"):
            GATE.check_release_ci(failing_fetch, REPO, SHA)

    def test_pagination_requires_every_record(self):
        entries = [{"id": index} for index in range(101)]

        def paged(path):
            page = int(re.search(r"[?&]page=(\d+)", path)[1])
            return {"total_count": 101, "jobs": entries[(page - 1) * 100 : page * 100]}

        self.assertEqual(len(GATE.all_pages(paged, "/jobs", "jobs")), 101)
        with self.assertRaisesRegex(ValueError, "incomplete"):
            GATE.all_pages(lambda _path: {"total_count": 2, "jobs": []}, "/jobs", "jobs")


if __name__ == "__main__":
    unittest.main()
