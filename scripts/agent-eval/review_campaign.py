"""Freeze, execute and export bounded Codex PR-review campaigns.

Plans and terminal outcomes are append-only. Raw answers, credentials and traces stay
in the private campaign directory; the export is an explicit metadata projection.
"""

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import random
import shutil
import stat
import subprocess
import tempfile
import time

from accounting import InvalidLedger, fingerprint, read_json, require
from codex_isolation import REPOSCOUT_CHILD_TIMEOUT_SECONDS

SCHEMA = 1
MODEL = "gpt-6.1-sol"
EFFORT = "max"
MAIN_CASES = ("clean-refactor", "refund-boundary", "import-wiring", "package-wiring",
              "wire-units-noise", "sparse-evidence", "review-followup", "staff-only")
SMOKE_CASES = ("clean-refactor", "refund-boundary", "import-wiring", "sparse-evidence")
ABLATION_CASES = ("refund-boundary", "import-wiring", "sparse-evidence", "review-followup")
VARIANTS = ("baseline", "reposcout", "reposcout-cli")
TERMINAL_STATUSES = ("completed", "failed", "aborted")
SOURCE_FILES = ("accounting.py", "review_campaign.py", "review_export.py", "review_cases.py", "review_grading.py",
                "codex_runner.py", "codex_isolation.py", "codex_trace.py")
ROOT = Path(__file__).resolve().parents[2]
SCRIPT_ROOT = Path(__file__).resolve().parent
LIMITS = {"timeout_seconds": 180, "memory_limit_bytes": 1024 ** 3,
          "host_memory_reserve_bytes": 12 * 1024 ** 3, "max_output_bytes": 16 * 1024 ** 2,
          "reposcout_child_timeout_seconds": REPOSCOUT_CHILD_TIMEOUT_SECONDS}
CAPABILITIES = {
    "baseline": "Git, bounded shell searches, targeted source reads, and the supplied one-shot application checks are available.",
    "reposcout": "Git, bounded shell searches, targeted source reads, the supplied one-shot application checks, and the RepoScout CLI and its bundled skill are available.",
    "reposcout-cli": "Git, bounded shell searches, targeted source reads, the supplied one-shot application checks, and the RepoScout CLI are available.",
}


def write_new(path, value):
    """Publish complete JSON atomically, refusing an existing immutable artifact."""
    path = Path(path)
    require(not path.is_symlink(), "artifact cannot be a symlink")
    descriptor, temporary = tempfile.mkstemp(prefix=".publish-", dir=path.parent)
    temporary = Path(temporary)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.link(temporary, path)
        directory_descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory_descriptor)
        finally:
            os.close(directory_descriptor)
    finally:
        temporary.unlink(missing_ok=True)


def file_hash(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        while block := stream.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def directory_hash(path):
    path = Path(path).resolve()
    require(path.is_dir(), "missing pinned directory")
    files = {}
    total = 0
    for item in sorted(path.rglob("*")):
        require(not item.is_symlink(), "pinned directory contains a symlink")
        if item.is_file():
            total += item.stat().st_size
            require(total <= 16 * 1024 ** 2 and len(files) < 2048, "pinned directory exceeds bounds")
            files[item.relative_to(path).as_posix()] = file_hash(item)
    require(files, "empty pinned directory")
    return {"sha256": fingerprint(files), "files": files}


def binary_version(binary):
    result = subprocess.run([str(binary), "--version"], check=True, capture_output=True,
                            text=True, timeout=10)
    require(0 < len(result.stdout) <= 4096, "invalid binary version output")
    return result.stdout.strip()


def validated_timeout_seconds(value):
    require(type(value) in (int, float) and 0 < value <= 600,
            "timeout must be positive and at most 600 seconds")
    return value


def calibrated_usage_scope(cli_version, codex_sha256):
    # Runtime-specific calibration evidence is private; absent evidence stays unknown.
    return "unknown", None


def capture_pins(codex_binary, codex_version, reposcout_binary, skill_dir, controller_ca_file=None,
                 *, timeout_seconds=LIMITS["timeout_seconds"]):
    limits = {**LIMITS, "timeout_seconds": validated_timeout_seconds(timeout_seconds)}
    codex_binary, reposcout_binary, skill_dir = map(Path, (codex_binary, reposcout_binary, skill_dir))
    codex_binary, reposcout_binary, skill_dir = (path.resolve() for path in
                                                (codex_binary, reposcout_binary, skill_dir))
    require(codex_binary.is_file() and reposcout_binary.is_file(), "missing evaluation binary")
    require(directory_hash(skill_dir) == directory_hash(ROOT / "skills/reposcout"),
            "treatment requires the exact canonical RepoScout skill bundle")
    actual_version = binary_version(codex_binary)
    require(actual_version == codex_version, "declared Codex version differs from pinned binary")
    codex_sha256 = file_hash(codex_binary)
    usage_scope, calibration = calibrated_usage_scope(actual_version, codex_sha256)
    from review_cases import answer_schema
    from codex_isolation import codex_runtime_manifest, controller_ca_manifest
    runtime_assets = codex_runtime_manifest(codex_binary)
    controller_ca = controller_ca_manifest(controller_ca_file)
    revision = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "HEAD"], check=True,
                              capture_output=True, text=True, timeout=10).stdout.strip()
    return {"codex_binary": str(codex_binary), "codex_sha256": codex_sha256,
            "codex_runtime_assets": runtime_assets, "codex_runtime_sha256": fingerprint(runtime_assets),
            "controller_ca_file": controller_ca["path"], "controller_ca_sha256": controller_ca["sha256"],
            "controller_ca_bytes": controller_ca["bytes"], "controller_ca_destination": controller_ca["destination"],
            "codex_version": actual_version, "reposcout_binary": str(reposcout_binary),
            "reposcout_sha256": file_hash(reposcout_binary),
            "reposcout_version": binary_version(reposcout_binary), "skill_dir": str(skill_dir),
            "skill": directory_hash(skill_dir), "fixture_bundle": directory_hash(SCRIPT_ROOT / "fixtures/pr-review"),
            "harness_sources": {name: file_hash(SCRIPT_ROOT / name) for name in SOURCE_FILES},
            "repository_revision": revision, "model": MODEL, "effort": EFFORT,
            "answer_schema_sha256": fingerprint(answer_schema()), "limits": limits,
            "usage_scope": usage_scope, "usage_scope_calibration": calibration,
            "capabilities_sha256": fingerprint(CAPABILITIES)}


def build_plan(stage, seed, pins, cases, include_ablation=False):
    """Pure assignment planner: balanced main-arm order and fixed replicate identities."""
    require(stage in ("smoke", "exploratory"), "unknown campaign stage")
    require(type(seed) is int, "seed must be an integer")
    require(not include_ablation or stage == "exploratory", "ablation belongs to exploratory stage")
    selected = SMOKE_CASES if stage == "smoke" else MAIN_CASES
    catalog = {item["case_id"]: item for item in cases}
    require(len(catalog) == len(cases) and all(name in catalog for name in selected), "incomplete case catalog")
    require(all(catalog[name]["partition"] == "main" for name in selected), "held-out cases are excluded")
    generator = random.Random(seed)
    first_orders = [0] * (len(selected) // 2) + [1] * (len(selected) // 2)
    generator.shuffle(first_orders)
    orientations = dict(zip(selected, first_orders))
    assignments = []
    repetitions = 1 if stage == "smoke" else 3
    for repeat in range(1, repetitions + 1):
        order = list(selected)
        generator.shuffle(order)
        for case_id in order:
            pair_id = "pair-" + fingerprint({"stage": stage, "seed": seed, "case": case_id, "repeat": repeat})[:24]
            arms = ["baseline", "reposcout"]
            if (orientations[case_id] + repeat - 1) % 2:
                arms.reverse()
            if include_ablation and case_id in ABLATION_CASES:
                arms.insert(generator.randrange(3), "reposcout-cli")
            for position, variant in enumerate(arms):
                run_id = "run-" + fingerprint({"pair": pair_id, "variant": variant})[:24]
                assignments.append({"run_id": run_id, "pair_id": pair_id,
                                    "repeat_id": repeat, "case_id": case_id, "variant": variant,
                                    "pair_position": position, "sequence": len(assignments),
                                    "fixture_sha256": catalog[case_id]["fixture_sha256"],
                                    "step_count": catalog[case_id]["step_count"],
                                    "capabilities_sha256": fingerprint(CAPABILITIES[variant])})
    plan = {"schema": SCHEMA, "kind": "codex-review-campaign-plan", "stage": stage,
            "seed": seed, "repetitions": repetitions, "ablation": include_ablation,
            "model": MODEL, "effort": EFFORT, "pins": pins,
            "cases": [catalog[name] for name in selected], "assignments": assignments,
            "assignment_count": len(assignments), "statistical_inference": False}
    plan["plan_sha256"] = fingerprint(plan)
    return plan


def _no_symlinks(path):
    path = Path(path).absolute()
    require(all(not component.is_symlink() for component in (path, *path.parents)),
            "campaign paths cannot traverse symlinks")
    return path


def create_campaign_root(destination):
    destination = _no_symlinks(destination)
    require(not destination.exists(), "campaign destination already exists")
    require(not destination.is_relative_to(ROOT), "campaign must be outside the source checkout")
    require(destination.parent.is_dir(), "campaign parent must already exist")
    destination.mkdir(mode=0o700)
    identity = destination.stat()
    marker = {"schema": SCHEMA, "root": str(destination), "device": identity.st_dev,
              "inode": identity.st_ino, "owner": os.getuid(), "token": os.urandom(32).hex()}
    write_new(destination / "ownership.json", marker)
    (destination / "runs").mkdir(mode=0o700)
    return destination, marker


def verify_owned(root):
    root = _no_symlinks(root)
    marker = read_json(root / "ownership.json")
    actual = root.stat()
    require(marker.get("root") == str(root) and marker.get("device") == actual.st_dev
            and marker.get("inode") == actual.st_ino and marker.get("owner") == os.getuid()
            and actual.st_uid == os.getuid() and stat.S_IMODE(actual.st_mode) == 0o700,
            "campaign ownership changed")
    return root, marker


def make_prompt(step, variant):
    from review_cases import ANSWER_GUIDANCE
    task = {key: step.get(key) for key in ("step_id", "task", "base_commit", "head_commit", "base_tree", "head_tree", "source_sha256")}
    common = (step["task"] + "\n\nComparison revisions: base=" + step.get("base_commit", step["base_tree"])
              + "; head=" + step.get("head_commit", step["head_tree"])
              + ". Evidence tree identities: base=" + step["base_tree"] + "; head=" + step["head_tree"]
              + ".\n\n" + ANSWER_GUIDANCE)
    prompt = common + "\n\n" + CAPABILITIES[variant]
    return prompt, {"task_sha256": fingerprint(task), "shared_prompt_sha256": fingerprint(common),
                    "capabilities_sha256": fingerprint(CAPABILITIES[variant]),
                    "prompt_sha256": fingerprint(prompt)}


def prepare(destination, *, stage, seed, codex_binary, codex_version, reposcout_binary,
            skill_dir, include_ablation=False, smoke_campaign=None, controller_ca_file=None,
            timeout_seconds=LIMITS["timeout_seconds"]):
    from review_cases import create_signer, list_cases, prepare_case
    pins = capture_pins(codex_binary, codex_version, reposcout_binary, skill_dir, controller_ca_file,
                        timeout_seconds=timeout_seconds)
    plan = build_plan(stage, seed, pins, list_cases(), include_ablation)
    root, marker = create_campaign_root(destination)
    signer = create_signer(root / "signing")
    write_new(root / "signer.json", signer)
    plan["fixture_signer"] = {key: signer[key] for key in ("kind", "public_key_sha256", "principal")}
    if smoke_campaign:
        smoke_root, _ = verify_owned(smoke_campaign)
        smoke = load_plan(smoke_root)
        require(stage == "exploratory" and smoke["stage"] == "smoke" and smoke["pins"] == pins,
                "smoke and exploratory conditions differ")
        plan["smoke_campaign"] = str(smoke_root)
        plan["smoke_plan_sha256"] = smoke["plan_sha256"]
    for assignment in plan["assignments"]:
        run_dir = Path(tempfile.mkdtemp(prefix=assignment["run_id"] + "-", dir=root / "runs"))
        identity = run_dir.stat()
        write_new(run_dir / "ownership.json", {"campaign_token": marker["token"],
                                               "run_id": assignment["run_id"], "root": str(run_dir),
                                               "device": identity.st_dev, "inode": identity.st_ino,
                                               "owner": os.getuid()})
        record = prepare_case(assignment["case_id"], run_dir / "workspace", private_directory=run_dir / "oracle", signer=signer)
        require(record["fixture_sha256"] == assignment["fixture_sha256"], "fixture changed during preparation")
        write_new(run_dir / "case.json", record)
        (run_dir / "controller").mkdir(mode=0o700)
        assignment["run_dir"] = str(run_dir)
        assignment["oracle_sha256"] = fingerprint(read_json(record["private_oracle_path"]))
        assignment["public_workspace_sha256"] = record["public_workspace_sha256"]
        assignment["disposable_identities"] = {
            name: {"device": (run_dir / name).stat().st_dev, "inode": (run_dir / name).stat().st_ino}
            for name in ("workspace", "controller")}
        assignment["steps"] = [{key: step[key] for key in ("step_id", "task", "source_sha256", "base_commit", "head_commit", "base_tree", "head_tree")}
                               for step in record["steps"]]
        assignment["step_prompts"] = [make_prompt(step, assignment["variant"])[1] for step in record["steps"]]
        _, assignment["initial_prompt"] = make_prompt(record["steps"][0], assignment["variant"])
        from codex_isolation import configuration_manifest
        spec, _ = _trial_spec(plan, assignment, record, record["steps"][0], run_dir / "planned-artifacts")
        assignment["configuration"] = configuration_manifest(spec)
        assignment["configuration_sha256"] = fingerprint(assignment["configuration"])
    plan.pop("plan_sha256")
    plan["plan_sha256"] = fingerprint(plan)
    write_new(root / "plan.json", plan)
    return plan


def load_plan(root):
    root, marker = verify_owned(root)
    plan = read_json(root / "plan.json")
    require(plan.get("schema") == SCHEMA and plan.get("kind") == "codex-review-campaign-plan", "invalid campaign plan")
    require(plan.get("plan_sha256") == fingerprint({key: value for key, value in plan.items() if key != "plan_sha256"}),
            "immutable campaign plan hash mismatch")
    require(plan.get("model") == MODEL and plan.get("effort") == EFFORT, "unauthorized model or effort")
    assignments = plan["assignments"]
    require(0 < len(assignments) <= 60 and plan["assignment_count"] == len(assignments), "invalid assignment inventory")
    require(len({item["run_id"] for item in assignments}) == len(assignments), "duplicate assignment")
    require(len({item["run_dir"] for item in assignments}) == len(assignments), "assigned runs share a directory")
    for assignment in assignments:
        run_dir = _no_symlinks(assignment["run_dir"])
        require(run_dir.parent == root / "runs", "run directory escapes campaign")
        ownership = read_json(run_dir / "ownership.json")
        identity = run_dir.stat()
        require(ownership == {"campaign_token": marker["token"], "run_id": assignment["run_id"], "root": str(run_dir),
                              "device": identity.st_dev, "inode": identity.st_ino, "owner": os.getuid()}
                and identity.st_uid == os.getuid() and stat.S_IMODE(identity.st_mode) == 0o700,
                "run ownership changed")
    return plan


def verify_runtime_pins(plan):
    pins = plan["pins"]
    actual = capture_pins(pins["codex_binary"], pins["codex_version"], pins["reposcout_binary"], pins["skill_dir"],
                          pins.get("controller_ca_file"), timeout_seconds=pins["limits"]["timeout_seconds"])
    require(actual == pins, "campaign conditions drifted; prepare a separate campaign")


@contextmanager
def campaign_lock(root):
    """A crash leaves the lease for explicit inspection rather than concurrent reruns."""
    path = Path(root) / "controller.lock"
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    identity = os.fstat(descriptor)
    try:
        with os.fdopen(descriptor, "w") as stream:
            json.dump({"pid": os.getpid(), "created_unix": time.time()}, stream)
            stream.flush()
            os.fsync(stream.fileno())
        yield
    finally:
        if path.exists() and not path.is_symlink():
            current = path.stat()
            if (current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino):
                path.unlink()


@contextmanager
def private_auth_copy(source, controller):
    controller = Path(controller)
    require(not controller.is_symlink(), "private authentication controller cannot be a symlink")
    controller_identity = controller.stat()
    require(stat.S_ISDIR(controller_identity.st_mode) and controller_identity.st_uid == os.getuid()
            and stat.S_IMODE(controller_identity.st_mode) == 0o700, "private authentication controller is not owned")
    destination = controller / "auth.json"
    require(not destination.exists() and not destination.is_symlink(), "private auth destination already exists")
    source_descriptor = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
    copied = False
    try:
        source_stat = os.fstat(source_descriptor)
        require(stat.S_ISREG(source_stat.st_mode) and source_stat.st_size <= 1024 ** 2,
                "invalid authentication source")
        target_descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        copied = True
        with os.fdopen(target_descriptor, "wb") as target, os.fdopen(source_descriptor, "rb") as source_stream:
            source_descriptor = None
            remaining = 1024 ** 2
            while block := source_stream.read(min(65536, remaining + 1)):
                remaining -= len(block)
                require(remaining >= 0, "authentication source exceeds bounds")
                target.write(block)
            target.flush()
            os.fsync(target.fileno())
        yield
    finally:
        if source_descriptor is not None:
            os.close(source_descriptor)
        if copied:
            require(not controller.is_symlink(), "private authentication controller changed type during cleanup")
            current_controller = controller.stat()
            require((current_controller.st_dev, current_controller.st_ino) ==
                    (controller_identity.st_dev, controller_identity.st_ino)
                    and current_controller.st_uid == os.getuid()
                    and stat.S_IMODE(current_controller.st_mode) == 0o700,
                    "private authentication controller changed identity during cleanup")
            require(not destination.is_symlink(), "private auth copy changed type during cleanup")
            if destination.exists():
                current = destination.stat()
                require(stat.S_ISREG(current.st_mode) and current.st_uid == os.getuid()
                        and stat.S_IMODE(current.st_mode) == 0o600 and current.st_nlink == 1,
                        "private auth copy is not an owned regular credential during cleanup")
                # Native refresh may atomically replace this file inside the owned controller.
                destination.unlink()


@contextmanager
def private_probe_auth(controller):
    """A no-model probe needs file shape only; it must not read host credentials."""
    destination = Path(controller) / "auth.json"
    descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    identity = os.fstat(descriptor)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(b"{}\n")
            stream.flush()
            os.fsync(stream.fileno())
        yield
    finally:
        require(not destination.is_symlink(), "probe credential changed type during cleanup")
        current = destination.stat()
        require((current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino),
                "probe credential changed identity during cleanup")
        destination.unlink()


def _trial_spec(plan, assignment, record, step, artifact_dir, resumed=None, timeout=None, usage_baseline=None):
    from codex_runner import TrialSpec
    from review_cases import answer_schema
    pins = plan["pins"]
    variant = assignment["variant"]
    prompt, hashes = make_prompt(step, variant)
    spec = TrialSpec(run_id=assignment["run_id"], workspace=Path(record["workspace"]),
                     artifact_dir=Path(artifact_dir), controller_dir=Path(assignment["run_dir"]) / "controller",
                     codex_binary=Path(pins["codex_binary"]), cli_version=pins["codex_version"],
                     prompt=prompt, answer_schema=answer_schema(), model=MODEL, effort=EFFORT,
                     variant=variant, reposcout_binary=None if variant == "baseline" else Path(pins["reposcout_binary"]),
                     skill_dir=Path(pins["skill_dir"]) if variant == "reposcout" else None,
                     resume_thread_id=resumed, timeout_seconds=pins["limits"]["timeout_seconds"] if timeout is None else timeout,
                     memory_limit_bytes=pins["limits"]["memory_limit_bytes"],
                     host_memory_reserve_bytes=pins["limits"]["host_memory_reserve_bytes"],
                     max_output_bytes=pins["limits"]["max_output_bytes"], usage_scope=pins["usage_scope"],
                     usage_baseline=usage_baseline)
    # The runner validates the actual private copies, closing the staging race.
    from dataclasses import replace
    spec = replace(spec, expected_codex_sha256=pins["codex_sha256"],
                   expected_codex_runtime_sha256=pins["codex_runtime_sha256"],
                   controller_ca_file=Path(pins["controller_ca_file"]),
                   expected_controller_ca_sha256=pins["controller_ca_sha256"],
                   expected_reposcout_sha256=pins["reposcout_sha256"] if variant != "baseline" else None,
                   expected_skill_sha256=pins["skill"]["sha256"] if variant == "reposcout" else None,
                   expected_workspace_sha256=record.get("public_workspace_sha256"))
    return spec, hashes


def _safe_error(error):
    # Raw exception strings may contain prompts, credentials or host paths.
    return {"error_type": type(error).__name__, "reason": "controller-exception; inspect private artifacts"}


def _resume_usage_baseline(trace, thread_id, cli_version):
    require(isinstance(trace, dict) and thread_id is not None and trace.get("thread_id") == thread_id
            and trace.get("cli_version") == cli_version and trace.get("status") == "completed"
            and trace.get("usage_scope") == "thread-cumulative" and trace.get("controller_closed") is True
            and trace.get("comparable_usage") is True,
            "cumulative resume requires a verified successful prior thread")
    turns = trace.get("turns", [])
    require(turns and turns[-1].get("status") == "completed" and isinstance(turns[-1].get("reported_usage"), dict),
            "cumulative resume requires prior terminal counters")
    # These are the raw cumulative counters, including unknown optional fields.
    return dict(turns[-1]["reported_usage"])


def verify_public_workspace(record, expected_hash):
    workspace = Path(record["workspace"])
    actual_hash = directory_hash(workspace)["sha256"]
    require(actual_hash == expected_hash, "prepared public workspace changed before staging")
    oracle = read_json(record["private_oracle_path"])
    expected = oracle["snapshots"][record["steps"][record["active_step"]]["head_revision"]]
    files = {}
    for item in workspace.rglob("*"):
        relative = item.relative_to(workspace)
        if relative.parts[0] == ".git":
            continue
        require(not item.is_symlink(), "public fixture contains a symlink")
        if item.is_file():
            files[relative.as_posix()] = item.read_text(encoding="utf-8")
    require(files == expected, "public fixture differs from frozen source snapshot")
    record["public_workspace_sha256"] = actual_hash


def recover_attempts(assignment, outcome):
    """Recover charged attempts even when the runner did not return to the controller."""
    from codex_trace import aggregate_episode, parse_exec_trace
    directory = Path(assignment["run_dir"])
    known = {item["step_id"]: item for item in outcome.get("invocations", [])}
    invocations, traces = [], []
    recovery_errors = []
    for index in range(assignment["step_count"]):
        step = directory / f"step-{index}"
        marker_path = step / "attempt.json"
        lifecycle_path = step / "invocation/lifecycle.json"
        result_path = step / "result.json"
        if not marker_path.exists() and not lifecycle_path.exists() and not result_path.exists() and index not in known:
            continue
        marker = read_json(marker_path) if marker_path.exists() else {}
        require(not marker or marker.get("run_id") == assignment["run_id"] and marker.get("step_id") == index,
                "foreign invocation attempt marker")
        if result_path.exists():
            result = read_json(result_path)
        elif lifecycle_path.exists():
            result = read_json(lifecycle_path)
        else:
            result = dict(known.get(index, {}).get("result") or {})
        require(result.get("run_id", assignment["run_id"]) == assignment["run_id"], "foreign recovered lifecycle")
        result.setdefault("run_id", assignment["run_id"])
        result.setdefault("invocation_id", marker.get("attempt_id", f"{assignment['run_id']}-step-{index}"))
        if result.get("status") not in TERMINAL_STATUSES:
            result["status"] = "aborted"
        result.setdefault("stream_complete", False)
        result.setdefault("process_tree_drained", False)
        result.setdefault("resumed_thread_id", marker.get("resumed_thread_id"))
        result.setdefault("usage_scope", marker.get("usage_scope", "unknown"))
        result.setdefault("usage_baseline", marker.get("usage_baseline"))
        hashes = marker.get("prompt_hashes") or known.get(index, {}).get("prompt_hashes") or {}
        if not hashes and (step / "prompt.json").exists():
            hashes = {key: value for key, value in read_json(step / "prompt.json").items() if key != "prompt"}
        invocation = {"step_id": index, "result": result, "prompt_hashes": hashes}
        invocations.append(invocation)
        stdout = Path(result["stdout_path"]) if result.get("stdout_path") else step / "invocation/stdout.jsonl"
        require(stdout == step / "invocation/stdout.jsonl",
                "recovered trace path escapes assigned invocation")
        if stdout.is_file() and not stdout.is_symlink():
            try:
                result["stdout_path"] = str(stdout)
                # Preserve a returned receipt's hashes; incomplete receipts gain only observed bytes.
                if result.get("stdout_sha256") is None:
                    result["stdout_bytes"] = stdout.stat().st_size
                    result["stdout_sha256"] = file_hash(stdout)
                else:
                    result.setdefault("stdout_bytes", stdout.stat().st_size)
                if result.get("trace_sha256") is None:
                    result["trace_sha256"] = result["stdout_sha256"]
                trace = parse_exec_trace(stdout, controller=result,
                                         expected_thread_id=result.get("thread_id") or result.get("resumed_thread_id"))
                traces.append(trace)
            except (InvalidLedger, OSError, ValueError, KeyError, TypeError) as error:
                recovery_errors.append(_safe_error(error))
        elif (step / "trace-accounting.json").is_file():
            traces.append(read_json(step / "trace-accounting.json"))
    outcome["invocations"] = invocations
    outcome["invocation_count"] = len(invocations)
    outcome["accounting_inventory_complete"] = len(traces) == len(invocations)
    if traces:
        try:
            outcome["accounting"] = aggregate_episode(traces)
        except (InvalidLedger, ValueError, KeyError, TypeError) as error:
            recovery_errors.append(_safe_error(error))
    outcome["recovery_errors"] = recovery_errors
    return outcome


def execute_assignment(plan, assignment, auth_file, *, runner=None, check_pins=True):
    from codex_trace import aggregate_episode, extract_execution_evidence, parse_exec_trace
    from review_cases import activate_step
    from review_grading import grade_episode
    if runner is None:
        from codex_runner import run_trial
        runner = run_trial
    run_dir = Path(assignment["run_dir"])
    require(not (run_dir / "started.json").exists() and not (run_dir / "outcome.json").exists(),
            "assigned run has already started; adverse outcomes cannot be replaced")
    record = read_json(run_dir / "case.json")
    require(record["workspace"] == str(run_dir / "workspace")
            and record["private_oracle_path"] == str(run_dir / "oracle/oracle.json"),
            "private fixture record escapes its assigned run")
    require(fingerprint(read_json(record["private_oracle_path"])) == assignment["oracle_sha256"], "private oracle drift")
    verify_public_workspace(record, assignment["public_workspace_sha256"])
    write_new(run_dir / "started.json", {"schema": SCHEMA, "run_id": assignment["run_id"],
                                        "plan_sha256": plan["plan_sha256"], "started_unix": time.time()})
    outcome = {"schema": SCHEMA, "run_id": assignment["run_id"], "plan_sha256": plan["plan_sha256"],
               "status": "failed", "invocations": [], "accounting": None, "quality": None,
               "answers": [], "errors": []}
    traces = []
    answers = []
    execution_evidence = []
    episode_start = time.monotonic()
    try:
        with private_auth_copy(auth_file, run_dir / "controller"):
            thread = None
            usage_baseline = None
            for index in range(len(record["steps"])):
                if check_pins:
                    verify_runtime_pins(plan)
                verify_public_workspace(record, record["public_workspace_sha256"])
                if index:
                    require(thread is not None and len(record["steps"]) == 2 and assignment["case_id"] == "review-followup",
                            "exact resume is only permitted for the two-step followup")
                    activate_step(record, index)
                    verify_public_workspace(record, record["public_workspace_sha256"])
                remaining = plan["pins"]["limits"]["timeout_seconds"] - (time.monotonic() - episode_start)
                if remaining <= 0:
                    outcome["status"] = "aborted"
                    outcome["errors"].append({"reason": "episode-time-limit-before-next-invocation"})
                    break
                directory = run_dir / f"step-{index}"
                directory.mkdir(mode=0o700)
                artifacts = directory / "invocation"
                artifacts.mkdir(mode=0o700)
                step = record["steps"][index]
                spec, hashes = _trial_spec(plan, assignment, record, step, artifacts, thread, remaining,
                                           usage_baseline=usage_baseline)
                if "step_prompts" in assignment:
                    require(hashes == assignment["step_prompts"][index], "assigned prompt drifted after planning")
                write_new(directory / "prompt.json", {"prompt": spec.prompt, **hashes})
                write_new(directory / "case.json", record)
                marker = {"schema": SCHEMA, "run_id": assignment["run_id"], "step_id": index,
                          "attempt_id": f"{assignment['run_id']}-step-{index}", "prompt_hashes": hashes,
                          "resumed_thread_id": thread, "plan_sha256": plan["plan_sha256"],
                          "usage_scope": plan["pins"]["usage_scope"], "usage_baseline": usage_baseline,
                          "started_unix": time.time()}
                write_new(directory / "attempt.json", marker)
                invocation = {"step_id": index, "result": {"run_id": assignment["run_id"],
                              "invocation_id": marker["attempt_id"], "status": "aborted"}, "prompt_hashes": hashes}
                outcome["invocations"].append(invocation)
                result = runner(spec)
                result.setdefault("invocation_id", f"{assignment['run_id']}-step-{index}")
                result.setdefault("run_id", assignment["run_id"])
                require(result["run_id"] == assignment["run_id"], "foreign runner result")
                write_new(directory / "result.json", result)
                invocation["result"] = result
                trace = None
                try:
                    trace = parse_exec_trace(result["stdout_path"], controller=result,
                                             expected_thread_id=thread or result.get("thread_id"))
                    traces.append(trace)
                    write_new(directory / "trace-accounting.json", trace)
                    receipts = extract_execution_evidence(result["stdout_path"], step_id=index,
                                                          expected_sha256=result.get("stdout_sha256"))
                    write_new(directory / "execution-evidence.json", receipts)
                    execution_evidence.extend(receipts)
                except (InvalidLedger, OSError, ValueError, KeyError, TypeError) as error:
                    outcome["errors"].append(_safe_error(error))
                status = result.get("status")
                if result.get("process_tree_drained") is False:
                    outcome["status"] = "aborted"
                    outcome["errors"].append({"reason": "owned-process-tree-not-drained"})
                    break
                if status != "completed":
                    outcome["status"] = "aborted" if status == "aborted" else "failed"
                    break
                answer = read_json(result["answer_path"])
                answers.append(answer)
                outcome["answers"].append({"step_id": index, "answer_sha256": fingerprint(answer)})
                if index:
                    require(result.get("resumed_thread_id") == thread and result.get("thread_id") == thread,
                            "runner did not resume the exact prior thread")
                thread = result.get("thread_id")
                if index + 1 < len(record["steps"]) and plan["pins"]["usage_scope"] == "thread-cumulative":
                    usage_baseline = _resume_usage_baseline(trace, thread, plan["pins"]["codex_version"])
                outcome["status"] = "completed" if len(answers) == len(record["steps"]) else "failed"
            if len(answers) == len(record["steps"]):
                outcome["quality"] = grade_episode(record, answers, execution_evidence=execution_evidence)
            outcome["execution_evidence"] = execution_evidence
    except KeyboardInterrupt:
        outcome["status"] = "aborted"
        outcome["errors"].append({"reason": "controller-interrupted"})
    except Exception as error:
        outcome["status"] = "failed"
        outcome["errors"].append(_safe_error(error))
    if traces:
        try:
            outcome["accounting"] = aggregate_episode(traces)
        except (InvalidLedger, ValueError, KeyError, TypeError) as error:
            outcome["errors"].append(_safe_error(error))
    outcome["wall_seconds"] = round(time.monotonic() - episode_start, 6)
    recover_attempts(assignment, outcome)
    write_new(run_dir / "outcome.json", outcome)
    return outcome


def smoke_qualified(plan):
    require(plan.get("smoke_campaign"), "exploratory execution requires a qualified smoke campaign")
    smoke = load_plan(plan["smoke_campaign"])
    qualification = read_json(Path(plan["smoke_campaign"]) / "qualification.json")
    require(smoke["stage"] == "smoke" and smoke["pins"] == plan["pins"]
            and smoke["plan_sha256"] == plan["smoke_plan_sha256"] == qualification.get("plan_sha256"),
            "smoke qualification conditions differ")
    require(all(qualification.get(field) is True for field in
                ("isolation_passed", "structurally_consumable", "accounting_basis_understood")),
            "smoke harness is not qualified")


def qualify_smoke(root, evidence):
    plan = load_plan(root)
    require(plan["stage"] == "smoke", "only smoke campaigns can qualify the harness")
    runs = inventory(plan)
    require(all(run["status"] == "completed" for run in runs), "smoke has missing or failed protocol outcomes")
    for run in runs:
        invocations = run.get("invocations", [])
        accounting = run.get("accounting") or {}
        require(len(invocations) == run["assignment"]["step_count"]
                and accounting.get("invocation_count") == len(invocations)
                and run.get("quality") is not None, "smoke measurement inventory is incomplete")
        require(all(item["result"].get("preflight", {}).get("passed") is True
                    and item["result"].get("stream_complete") is True
                    and item["result"].get("process_tree_drained") is True
                    and item["result"].get("stdout_truncated") is not True
                    for item in invocations), "smoke isolation or lifecycle closure failed")
    require(evidence.get("plan_sha256") == plan["plan_sha256"], "foreign qualification evidence")
    require(all(evidence.get(field) is True for field in
                ("isolation_passed", "structurally_consumable", "accounting_basis_understood")),
            "smoke harness qualification checks failed")
    require(isinstance(evidence.get("reason"), str) and evidence["reason"], "qualification requires an explicit reason")
    value = {**evidence, "evidence_sha256": fingerprint(evidence)}
    write_new(Path(root) / "qualification.json", value)
    return value


def run(root, *, auth_file=None, limit=None, preflight_only=False, runner=None, check_pins=True):
    plan = load_plan(root)
    require(not (Path(root) / "cleanup.json").exists(), "campaign public inputs have been retired")
    require(limit is None or type(limit) is int and limit > 0, "limit must be positive")
    if check_pins:
        verify_runtime_pins(plan)
    if not preflight_only:
        require(auth_file is not None, "live execution requires a private authentication source")
        if plan["stage"] == "exploratory":
            smoke_qualified(plan)
    if runner is None:
        from codex_runner import run_trial
        runner = run_trial
    results = []
    with campaign_lock(root):
        for assignment in plan["assignments"]:
            directory = Path(assignment["run_dir"])
            if (directory / "started.json").exists() or (directory / "outcome.json").exists():
                continue
            if limit is not None and len(results) >= limit:
                break
            if preflight_only:
                record = read_json(directory / "case.json")
                artifacts = Path(tempfile.mkdtemp(prefix="preflight-", dir=directory))
                spec, _ = _trial_spec(plan, assignment, record, record["steps"][0], artifacts)
                with private_probe_auth(directory / "controller"):
                    result = runner(spec, preflight_only=True)
                write_new(artifacts / "result.json", result)
                results.append({"run_id": assignment["run_id"], "status": result.get("status"), "preflight": result.get("preflight")})
                require(result.get("preflight", {}).get("passed") is True, "isolation preflight failed")
            else:
                outcome = execute_assignment(plan, assignment, auth_file, runner=runner, check_pins=check_pins)
                results.append({"run_id": assignment["run_id"], "status": outcome["status"]})
                if outcome["status"] == "aborted":
                    break
    return {"plan_sha256": plan["plan_sha256"], "preflight_only": preflight_only, "runs": results}


def _verify_disposable_tree(path, expected):
    path = _no_symlinks(path)
    require(path.is_dir(), "disposable target changed type")
    identity = path.stat()
    require({"device": identity.st_dev, "inode": identity.st_ino} == expected,
            "disposable target identity changed")
    require(shutil.rmtree.avoids_symlink_attacks, "cleanup requires directory-descriptor traversal")
    directories = [path]
    while directories:
        directory = directories.pop()
        require(not os.path.ismount(directory), "cleanup cannot traverse mounted directories")
        with os.scandir(directory) as entries:
            for entry in entries:
                metadata = entry.stat(follow_symlinks=False)
                require(metadata.st_dev == identity.st_dev, "cleanup cannot cross a filesystem boundary")
                if stat.S_ISLNK(metadata.st_mode):
                    # rmtree unlinks this owned entry; its target is never visited.
                    require(metadata.st_uid == os.getuid(), "disposable symlink is not owned")
                elif stat.S_ISDIR(metadata.st_mode):
                    directories.append(Path(entry.path))
                else:
                    require(stat.S_ISREG(metadata.st_mode), "disposable entry changed type")


def cleanup_public(root, exported, expected_results_sha256):
    """Retire only recorded public fixtures/controller copies after a verified export."""
    plan = load_plan(root)
    exported = Path(exported)
    results = read_json(exported / "results.json")
    integrity = read_json(exported / "integrity.json")
    require(results.get("plan_sha256") == plan["plan_sha256"] == integrity.get("plan_sha256")
            and fingerprint(results) == integrity.get("results_canonical_json_sha256") == expected_results_sha256,
            "cleanup requires the exact verified campaign export")
    require({item["run_id"]: item["status"] for item in results["runs"]} ==
            {item["assignment"]["run_id"]: item["status"] for item in inventory(plan)},
            "run outcomes changed since export")
    targets = []
    for assignment in plan["assignments"]:
        run_dir = Path(assignment["run_dir"])
        for name in ("workspace", "controller"):
            path = run_dir / name
            expected = assignment["disposable_identities"][name]
            _verify_disposable_tree(path, expected)
            targets.append(path)
    with campaign_lock(root):
        for run in inventory(plan):
            require(all(invocation["result"].get("process_tree_drained") is not False
                        for invocation in run.get("invocations", [])), "cleanup cannot race an undrained process tree")
        write_new(Path(root) / "cleanup.json", {"schema": SCHEMA, "plan_sha256": plan["plan_sha256"],
                  "results_canonical_json_sha256": expected_results_sha256,
                  "targets": [str(path) for path in targets], "private_evidence_retained": True})
        for path in targets:
            assignment = next(item for item in plan["assignments"] if Path(item["run_dir"]) == path.parent)
            _verify_disposable_tree(path, assignment["disposable_identities"][path.name])
            shutil.rmtree(path)
    return {"removed_public_directories": len(targets), "private_evidence_retained": True}


def inventory(plan):
    runs = []
    for assignment in plan["assignments"]:
        directory = Path(assignment["run_dir"])
        path = directory / "outcome.json"
        if path.exists():
            outcome = read_json(path)
            require(outcome.get("run_id") == assignment["run_id"] and outcome.get("plan_sha256") == plan["plan_sha256"],
                    "foreign immutable run outcome")
            require(outcome.get("status") in TERMINAL_STATUSES, "invalid terminal run status")
        elif (directory / "started.json").exists():
            started = read_json(directory / "started.json")
            require(started.get("run_id") == assignment["run_id"] and started.get("plan_sha256") == plan["plan_sha256"],
                    "foreign started marker")
            outcome = {"status": "aborted", "errors": [{"reason": "interrupted-without-terminal-outcome"}],
                       "accounting": None, "quality": None, "invocations": [], "answers": []}
        else:
            outcome = {"status": "notrun", "accounting": None, "quality": None, "invocations": [], "answers": [], "errors": []}
        if outcome["status"] != "notrun":
            recover_attempts(assignment, outcome)
        runs.append({"assignment": assignment, **outcome})
    return runs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    create = commands.add_parser("prepare")
    create.add_argument("campaign")
    create.add_argument("--stage", choices=("smoke", "exploratory"), required=True)
    create.add_argument("--seed", type=int, required=True)
    create.add_argument("--codex-binary", required=True)
    create.add_argument("--codex-version", required=True)
    create.add_argument("--reposcout-binary", required=True)
    create.add_argument("--skill-dir", default=str(ROOT / "skills/reposcout"))
    create.add_argument("--ablation", action="store_true")
    create.add_argument("--smoke-campaign")
    create.add_argument("--controller-ca-file")
    create.add_argument("--timeout-seconds", type=float, default=LIMITS["timeout_seconds"])
    execute = commands.add_parser("run")
    execute.add_argument("campaign")
    execute.add_argument("--auth-file")
    execute.add_argument("--limit", type=int)
    execute.add_argument("--preflight-only", action="store_true")
    qualify = commands.add_parser("qualify-smoke")
    qualify.add_argument("campaign")
    qualify.add_argument("evidence")
    packets = commands.add_parser("packets")
    packets.add_argument("campaign")
    packets.add_argument("destination")
    cleanup = commands.add_parser("cleanup-public")
    cleanup.add_argument("campaign")
    cleanup.add_argument("exported")
    cleanup.add_argument("--expected-results-sha256", required=True)
    for name in ("report", "export"):
        view = commands.add_parser(name)
        view.add_argument("campaign")
        if name == "export":
            view.add_argument("destination")
        view.add_argument("--adjudications")
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            plan = prepare(args.campaign, stage=args.stage, seed=args.seed, codex_binary=args.codex_binary,
                           codex_version=args.codex_version, reposcout_binary=args.reposcout_binary,
                           skill_dir=args.skill_dir, include_ablation=args.ablation, smoke_campaign=args.smoke_campaign,
                           controller_ca_file=args.controller_ca_file, timeout_seconds=args.timeout_seconds)
            result = {"plan_sha256": plan["plan_sha256"], "assignment_count": plan["assignment_count"]}
        elif args.command == "run":
            result = run(args.campaign, auth_file=args.auth_file, limit=args.limit, preflight_only=args.preflight_only)
        elif args.command == "qualify-smoke":
            result = qualify_smoke(args.campaign, read_json(args.evidence))
        elif args.command == "packets":
            from review_export import packets
            result = packets(args.campaign, args.destination)
        elif args.command == "cleanup-public":
            result = cleanup_public(args.campaign, args.exported, args.expected_results_sha256)
        else:
            from review_export import export, report
            result = (export(args.campaign, args.destination, args.adjudications) if args.command == "export"
                      else report(args.campaign, args.adjudications))
        print(json.dumps(result, sort_keys=True, indent=2))
    except InvalidLedger as error:
        parser.exit(2, f"{error}\n")
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        parser.exit(2, f"{type(error).__name__}: campaign operation failed; inspect private controller artifacts\n")


if __name__ == "__main__":
    main()
