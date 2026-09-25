#!/usr/bin/env python3
"""Require completed main CI for the exact commit before a tag release starts."""

from __future__ import annotations

import json
import os
import re
import sys
from urllib.parse import urlencode
from urllib.request import Request, urlopen


REQUIRED_WORKFLOWS = {
    "rust.yml": {"Rust", "Native macOS"},
    "release-helpers.yml": {"Release helpers"},
}
MAX_RESULTS = 1000


def api_get(path: str, token: str) -> dict:
    request = Request(
        f"https://api.github.com{path}",
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    with urlopen(request, timeout=15) as response:
        return json.load(response)


def all_pages(fetch, path: str, key: str) -> list[dict]:
    values: list[dict] = []
    for page in range(1, MAX_RESULTS // 100 + 1):
        separator = "&" if "?" in path else "?"
        payload = fetch(f"{path}{separator}per_page=100&page={page}")
        total = payload.get("total_count")
        entries = payload.get(key)
        if not isinstance(total, int) or total < 0 or total > MAX_RESULTS:
            raise ValueError(f"invalid or excessive {key} count: {total}")
        if not isinstance(entries, list) or len(entries) > 100:
            raise ValueError(f"invalid {key} response")
        values.extend(entries)
        if len(values) == total:
            return values
        if len(values) > total or not entries:
            raise ValueError(f"incomplete {key} response")
    raise ValueError(f"{key} exceeds pagination limit")


def check_workflow(fetch, repo: str, sha: str, workflow: str, required_jobs: set[str]) -> int:
    query = urlencode({"branch": "main", "head_sha": sha})
    runs = all_pages(
        fetch,
        f"/repos/{repo}/actions/workflows/{workflow}/runs?{query}",
        "workflow_runs",
    )
    trusted = [
        run
        for run in runs
        if run.get("head_sha") == sha
        and run.get("head_branch") == "main"
        and run.get("event") in {"push", "workflow_dispatch"}
        and isinstance(run.get("repository"), dict)
        and run["repository"].get("full_name") == repo
        and isinstance(run.get("head_repository"), dict)
        and run["head_repository"].get("full_name") == repo
        and run.get("path", "").split("@", 1)[0] == f".github/workflows/{workflow}"
    ]
    if not trusted:
        raise ValueError(f"{workflow}: no trusted main run for {sha}")

    if any(not isinstance(run.get("id"), int) for run in trusted):
        raise ValueError(f"{workflow}: run response has no valid id")
    latest = max(trusted, key=lambda run: run["id"])
    run_id = latest["id"]
    attempt = latest["run_attempt"]
    if (
        not isinstance(run_id, int)
        or not isinstance(attempt, int)
        or attempt < 1
        or latest.get("status") != "completed"
        or latest.get("conclusion") != "success"
    ):
        raise ValueError(f"{workflow}: latest run is not successful and complete")

    jobs = all_pages(
        fetch,
        f"/repos/{repo}/actions/runs/{run_id}/attempts/{attempt}/jobs",
        "jobs",
    )
    for name in required_jobs:
        matching = [job for job in jobs if job.get("name") == name]
        if len(matching) != 1 or matching[0].get("run_id") != run_id:
            raise ValueError(
                f"{workflow}: required job {name!r} is missing or ambiguous; "
                "re-run all jobs for a complete attempt"
            )
        job = matching[0]
        if job.get("status") != "completed" or job.get("conclusion") != "success":
            raise ValueError(f"{workflow}: required job {name!r} did not succeed")
    return run_id


def check_release_ci(fetch, repo: str, sha: str) -> dict[str, int]:
    return {
        workflow: check_workflow(fetch, repo, sha, workflow, jobs)
        for workflow, jobs in REQUIRED_WORKFLOWS.items()
    }


def main() -> None:
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    sha = os.environ.get("RELEASE_COMMIT", "")
    token = os.environ.get("GH_TOKEN", "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        raise SystemExit("invalid GITHUB_REPOSITORY")
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise SystemExit("invalid RELEASE_COMMIT")
    if not token:
        raise SystemExit("GH_TOKEN is required to inspect CI runs")

    try:
        runs = check_release_ci(lambda path: api_get(path, token), repo, sha)
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f"release CI gate failed: {error}") from error
    for workflow, run_id in runs.items():
        print(f"{workflow}: required jobs passed in main run {run_id} for {sha}")


if __name__ == "__main__":
    main()
