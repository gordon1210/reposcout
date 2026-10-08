"""Explicit, typed public summaries; detailed campaign evidence remains private."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from accounting import InvalidLedger, read_json, require

TOKEN_FIELDS = ("input_tokens", "cache_write_input_tokens", "cached_input_tokens", "output_tokens")
CASES = frozenset(("clean-refactor", "refund-boundary", "import-wiring", "package-wiring", "wire-units-noise",
                   "sparse-evidence", "review-followup", "staff-only", "renewal-holdout", "cancellation-change",
                   "event-routing-change", "status-extraction", "publication-change-a", "publication-change-b",
                   "publication-change-c"))
VARIANTS = frozenset(("baseline", "reposcout", "reposcout-cli"))
STATUSES = frozenset(("completed", "failed", "aborted", "notrun"))
QUALITY = frozenset(("passed", "failed", "pending-adjudication", "unavailable"))
BASES = frozenset(("exec-emitted-initial-turn-usage", "exec-thread-cumulative-usage",
                   "exec-invocation-cumulative-usage", "unavailable"))
LEGACY = {
    'scripts/agent-eval/results/2026-09-12-native-pilot/integrity.json': '06cd5679916b2ca0bcd2bad9cb20c4156849cc69cc3ea2c33c57d21cf57a9668',
    'scripts/agent-eval/results/2026-09-12-native-pilot/results.json': 'a242aeb2a1ae16b958998fdbc97724c7a98eaa4b3883c37f5e3e0cfa76a87427',
}


def private_destination(destination):
    """Reject source-checkout destinations and aliases before creating private artifacts."""
    path = Path(os.path.abspath(destination))
    require(not path.is_symlink() and all(not parent.is_symlink() for parent in path.parents),
            "private output path traverses a symlink")
    def git_metadata(parent):
        metadata = parent / ".git"
        return metadata.is_file() or metadata.is_symlink() or (metadata.is_dir() and (metadata / "HEAD").exists())
    require(all(not git_metadata(parent) for parent in (path, *path.parents)),
            "private evaluation output must be outside Git checkouts")
    return path


def _integer(value, label, maximum=2 ** 63 - 1):
    require(type(value) is int and 0 <= value <= maximum, "invalid public " + label)
    return value


def _enum(value, choices, label):
    require(type(value) is str and value in choices, "unsupported public " + label)
    return value


def validate_publication(value):
    require(type(value) is dict and set(value) == {"schema", "kind", "synthetic_fixtures", "runs"},
            "unsupported publication fields")
    require(type(value["schema"]) is int and value["schema"] == 1
            and value["kind"] == "codex-review-public-summary" and value["synthetic_fixtures"] is True,
            "invalid publication identity")
    runs = value["runs"]
    require(type(runs) is list and 0 < len(runs) <= 60, "invalid publication inventory")
    identities = set()
    for row in runs:
        require(type(row) is dict and set(row) == {"case_id", "repeat_id", "variant", "status", "quality",
                                                 "usage_basis", "comparable_usage", "tokens"},
                "unsupported public run fields")
        _enum(row["case_id"], CASES, "case")
        _enum(row["variant"], VARIANTS, "variant")
        _enum(row["status"], STATUSES, "status")
        _enum(row["quality"], QUALITY, "quality")
        _enum(row["usage_basis"], BASES, "usage basis")
        require(0 < _integer(row["repeat_id"], "repeat", 3), "invalid public repeat")
        require(type(row["comparable_usage"]) is bool, "invalid public comparability")
        identity = (row["case_id"], row["repeat_id"], row["variant"])
        require(identity not in identities, "duplicate publication assignment")
        identities.add(identity)
        tokens = row["tokens"]
        require(type(tokens) is dict and set(tokens) == set(TOKEN_FIELDS), "unsupported public token fields")
        for field in TOKEN_FIELDS:
            if tokens[field] is not None:
                _integer(tokens[field], "token count")
        if row["comparable_usage"]:
            require(tokens["input_tokens"] is not None and tokens["output_tokens"] is not None,
                    "comparable accounting requires observed input and output")
        if row["usage_basis"] == "unavailable":
            require(not row["comparable_usage"] and all(value is None for value in tokens.values()),
                    "unavailable accounting cannot invent token observations")
    return value


def project_publication(report):
    """Copy selected typed facts, never arbitrary text, IDs, hashes, pins or nested metadata."""
    from review_export import quality_state
    require(report.get("kind") == "codex-review-campaign-report" and report.get("synthetic_fixtures") is True,
            "publication requires a synthetic review campaign")
    runs = report.get("runs")
    require(type(runs) is list and report.get("assignment_count") == len(runs),
            "publication must include every assigned outcome")
    rows = []
    for run in runs:
        accounting = run.get("accounting") or {}
        observed = accounting.get("observed_usage") or {}
        rows.append({"case_id": run["case_id"], "repeat_id": run["repeat_id"], "variant": run["variant"],
                     "status": run["status"], "quality": quality_state(run),
                     "usage_basis": accounting.get("usage_basis") or "unavailable",
                     "comparable_usage": accounting.get("comparable_usage") is True,
                     "tokens": {field: observed.get(field) for field in TOKEN_FIELDS}})
    return validate_publication({"schema": 1, "kind": "codex-review-public-summary",
                                 "synthetic_fixtures": True, "runs": rows})


def publish(root, destination, adjudications=None):
    from review_export import report
    from review_campaign import write_new
    value = project_publication(report(root, adjudications))
    destination = Path(destination)
    require(not destination.exists() and not destination.is_symlink(), "publication destination already exists")
    require(all(not parent.is_symlink() for parent in destination.absolute().parents),
            "publication path traverses a symlink")
    destination.mkdir(mode=0o700, parents=True)
    write_new(destination / "publication.json", value)
    return {"assignments": len(value["runs"]), "kind": value["kind"]}


def check_path(path, data):
    if path.startswith("scripts/agent-eval/results/"):
        require(path in LEGACY and hashlib.sha256(data).hexdigest() == LEGACY[path],
                "raw evaluation results cannot be published")
    if path.startswith("scripts/agent-eval/publications/"):
        require(re.fullmatch(r"scripts/agent-eval/publications/[a-z0-9-]{1,64}/publication\.json", path),
                "only publication.json belongs in the public summary directory")
        require(len(data) <= 65536, "publication exceeds byte limit")
        validate_publication(_strict_json(data))
    if path.startswith("scripts/agent-eval/"):
        require(not path.endswith((".gz", ".xz", ".zip", ".tar", ".zst", ".bz2", ".7z")),
                "raw evaluation archives cannot be published")
        relative = path.removeprefix("scripts/agent-eval/")
        require(("/" not in relative and (relative.endswith(".py") or relative == "README.md"))
                or relative.split("/")[0] in {"fixtures", "results", "publications"},
                "unexpected evaluation artifact directory")
    if path.startswith("scripts/agent-eval/fixtures/"):
        require(Path(path).name not in {"fixture_design.json", "freeze-manifest.json", "ARCHIVE_INDEX.json"}
                and not Path(path).name.endswith("-handoff.json"), "private fixture authoring evidence cannot be published")


def _strict_json(data):
    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate publication JSON field")
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique_pairs)


def _git(root, *arguments):
    return subprocess.run(["git", "-C", str(root), *arguments], check=True, capture_output=True,
                          timeout=10).stdout


def _check_entries(root, records, *, index=False):
    for record in records.split(b"\0"):
        if not record:
            continue
        metadata, raw_name = record.split(b"\t", 1)
        name = os.fsdecode(raw_name)
        if not name.startswith("scripts/agent-eval/"):
            continue
        mode, kind_or_oid, oid_or_stage = metadata.decode("ascii").split()
        require(mode in {"100644", "100755"}, "evaluation publication requires regular files")
        if index:
            require(oid_or_stage == "0", "unmerged evaluation publication input")
            oid = kind_or_oid
        else:
            require(kind_or_oid == "blob", "evaluation publication requires blobs")
            oid = oid_or_stage
        if name in LEGACY or name.startswith("scripts/agent-eval/publications/"):
            require(int(_git(root, "cat-file", "-s", oid)) <= 16 * 1024 * 1024,
                    "publication input exceeds byte limit")
            check_path(name, _git(root, "cat-file", "blob", oid))
        else:
            check_path(name, b"")


def check_repository(root, commit=None):
    """Check exact index or commit objects, never mutable working-tree file contents."""
    if commit is None:
        _check_entries(root, _git(root, "ls-files", "--stage", "-z", "scripts/agent-eval"), index=True)
    else:
        _check_entries(root, _git(root, "ls-tree", "-r", "-z", commit, "scripts/agent-eval"))


def check_outgoing(root, updates, destination_remote=None):
    """Git pre-push stdin inventory; examine every locally known outgoing commit version."""
    commits = set()
    for line in updates:
        fields = line.split()
        require(len(fields) == 4, "invalid pre-push ref inventory")
        _, new, _, old = fields
        require(re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", new) and len(new) == len(old)
                and re.fullmatch(r"[0-9a-f]+", old), "invalid pre-push object identity")
        if set(new) == {"0"}:
            continue
        tip = _git(root, "rev-parse", new + "^{commit}").decode("ascii").strip()
        exclusions = []
        if set(old) != {"0"}:
            # A known lease target defines the previously published history. Missing objects fail closed.
            _git(root, "cat-file", "-e", old + "^{commit}")
            exclusions.append(old)
        else:
            require(isinstance(destination_remote, str)
                    and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._/-]*", destination_remote)
                    and ".." not in destination_remote,
                    "new refs require a configured destination remote")
            remotes = _git(root, "remote").decode().splitlines()
            require(destination_remote in remotes, "unknown publication destination remote")
            references = _git(root, "for-each-ref", "--format=%(objectname)",
                              "refs/remotes/" + destination_remote + "/").decode("ascii").splitlines()
            require(references, "new refs require known destination history")
            exclusions.extend(references)
        arguments = ["rev-list", tip, "--not", *exclusions]
        commits.update(_git(root, *arguments).decode("ascii").splitlines())
        commits.add(tip)
    for commit in sorted(commits):
        check_repository(root, commit)
    return len(commits)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repository", nargs="?", default=str(Path(__file__).resolve().parents[2]))
    parser.add_argument("--remote", help="Git pre-push destination remote for new refs")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--commit", help="Validate an exact committed tree instead of the Git index")
    mode.add_argument("--pre-push", action="store_true", help="Validate all outgoing commit versions from Git stdin")
    args = parser.parse_args()
    try:
        if args.pre_push:
            check_outgoing(args.repository, sys.stdin, args.remote)
        else:
            check_repository(args.repository, args.commit)
    except (InvalidLedger, OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(2, "Evaluation publication policy rejected the Git objects: " + type(error).__name__ + "\n")
    print("Evaluation publication policy passed.")


if __name__ == "__main__":
    main()
