"""Measure whether Claude Code models use the bundled RepoScout skill unprompted on review-like tasks.

Each run is one capped, ungraded Claude Code invocation inside the campaign isolation. The prompt is a
short review request over a development fixture; the `silent` arm never mentions RepoScout, the
`neutral` arm adds the campaigns' availability sentence. The skill description under test is swapped
into a private copy of the bundled skill, so the canonical skill stays untouched. Adoption is read from
raw tool_use events, independent of tool results.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import random
import re
import shlex
import shutil
import sys
import tempfile
import time

from accounting import fingerprint
from claude_isolation import claude_binary as bounded_claude_binary, claude_version, sandbox_tool_manifest
from claude_runner import MODEL_PATTERN, ClaudeTrialSpec, run_trial
from claude_trace import adoption_observations
from codex_isolation import (
    REQUIRED_RUNTIME_TOOLS, controller_ca_manifest, directory_sha256, runtime_tool_identities, runtime_tool_manifest,
)
from codex_trace import _read_trace
from review_campaign import CAPABILITIES, LIMITS, file_hash

SCHEMA = 1
KIND = "adoption-run"
SCRIPT_ROOT = Path(__file__).resolve().parent
ROOT = SCRIPT_ROOT.parents[1]
DATA_ROOT = SCRIPT_ROOT / "fixtures" / "adoption"
BUNDLE_ROOT = SCRIPT_ROOT / "fixtures/large-review/development"
CASES = ("status-extraction", "cancellation-change", "event-routing-change")
MODELS = ("claude-haiku-5-5", "claude-sonnet-5-5")
ARMS = ("silent", "neutral")
PHRASING_SETS = ("dev", "tuning", "held-out")
CURRENT = "current"
DEFAULT_MAX_TURNS = 6
DEFAULT_EFFORT = "medium"
DEFAULT_TIMEOUT_SECONDS = 300
REQUEST = "Reply with a short free-form answer."
TOKEN_FIELDS = ("uncached_input_tokens", "cache_read_input_tokens", "cache_write_input_tokens", "output_tokens")
# Unlike the review schema, one free-text field: adoption runs are never graded for answer quality.
ADOPTION_SCHEMA = {"type": "object", "properties": {"conclusion": {"type": "string"}}, "required": ["conclusion"]}
INVALID_STREAM_REASONS = ("runtime-isolation-violation", "credential-exposure", "isolation-preflight-failed",
                          "runner-error")
FRONTMATTER = re.compile(r"\A---\n(.*?)\n---\n", re.S)
Z95 = 1.959963984540054


class AdoptionError(ValueError):
    """An invalid request; nothing was started."""


def require(condition, message):
    if not condition:
        raise AdoptionError(message)


def read_data(name):
    return json.loads((DATA_ROOT / name).read_text(encoding="utf-8"))


def load_phrasings(path=None):
    """Map phrasing id to {id, text, set}; the set is `dev`, `tuning` or `held-out`."""
    data = json.loads(Path(path).read_text(encoding="utf-8")) if path else read_data("phrasings.json")
    result = {}
    for key, name in (("dev", "dev"), ("tuning", "tuning"), ("held_out", "held-out")):
        for item in data[key]:
            require(item["id"] not in result, "duplicate phrasing id")
            result[item["id"]] = {"id": item["id"], "text": item["text"], "set": name}
    return result


def select_phrasing(phrasings, phrasing_id, *, held_out):
    require(phrasing_id in phrasings, f"unknown phrasing: {phrasing_id}")
    chosen = phrasings[phrasing_id]
    require(chosen["set"] != "held-out" or held_out,
            "held-out phrasings are reserved for the decision run; pass --held-out to use them")
    return chosen


def build_prompt(arm, phrasing, base, head):
    require(arm in ARMS, f"unknown arm: {arm}")
    prompt = f"{phrasing}\nbase={base} head={head}\n{REQUEST}"
    if arm == "neutral":
        prompt += "\n\n" + CAPABILITIES["reposcout"]
    return prompt


def read_description(skill_md):
    """The frontmatter `description:` value of a SKILL.md text."""
    match = FRONTMATTER.match(skill_md)
    require(match is not None, "SKILL.md has no frontmatter")
    lines = [line for line in match.group(1).split("\n") if line.startswith("description:")]
    require(len(lines) == 1, "SKILL.md frontmatter needs exactly one description line")
    value = lines[0][len("description:"):].strip()
    return json.loads(value) if value.startswith('"') else value


def install_description(source, destination, text):
    """Copy the skill directory and replace only the frontmatter description line (`None` keeps it)."""
    require(text is None or isinstance(text, str) and text and "\n" not in text and "\x00" not in text,
            "invalid description text")
    shutil.copytree(source, destination)
    if text is None:
        return
    path = Path(destination) / "SKILL.md"
    content = path.read_text(encoding="utf-8")
    match = FRONTMATTER.match(content)
    require(match is not None, "SKILL.md has no frontmatter")
    lines = match.group(1).split("\n")
    indexes = [index for index, line in enumerate(lines) if line.startswith("description:")]
    require(len(indexes) == 1, "SKILL.md frontmatter needs exactly one description line")
    # A JSON string is a valid double-quoted YAML scalar, so colons and backticks need no further escaping.
    lines[indexes[0]] = "description: " + json.dumps(text, ensure_ascii=False)
    path.write_text("---\n" + "\n".join(lines) + content[match.end(1):], encoding="utf-8")
    require(read_description(path.read_text(encoding="utf-8")) == text, "description swap did not round-trip")


def resolve_description(name, skill_dir, descriptions=None):
    """The candidate's text and whether it replaces the skill's own description."""
    candidates = descriptions if descriptions is not None else read_data("descriptions.json")["descriptions"]
    require(name in candidates, f"unknown description: {name}")
    entry = candidates[name]
    if entry.get("source") == "skill":
        text = read_description((Path(skill_dir) / "SKILL.md").read_text(encoding="utf-8"))
        return {"name": name, "text": text, "swap": None, "source": "skill"}
    return {"name": name, "text": entry["text"], "swap": entry["text"], "source": "descriptions.json"}


def text_sha256(text):
    return hashlib.sha256(text.encode()).hexdigest()


def run_name(description, model, arm, case, phrasing, repeat):
    parts = [description, model.removeprefix("claude-"), arm, case, phrasing, f"r{repeat}"]
    name = "-".join(parts)
    require(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,180}", name) is not None, "invalid run name")
    return name


def validate_run_parameters(parameters, *, phrasings, held_out, descriptions=None, skill_dir=None):
    require(MODEL_PATTERN.fullmatch(parameters["model"]) is not None, "unsupported Claude model identity")
    require(parameters["arm"] in ARMS, f"unknown arm: {parameters['arm']}")
    require(parameters["case"] in CASES, f"case is not an adoption development case: {parameters['case']}")
    require(type(parameters["repeat"]) is int and parameters["repeat"] >= 0, "repeat must be a nonnegative integer")
    phrasing = select_phrasing(phrasings, parameters["phrasing"], held_out=held_out)
    description = resolve_description(parameters["description"], skill_dir or ROOT / "skills/reposcout", descriptions)
    return phrasing, description


def wilson(successes, total, z=Z95):
    """Wilson score interval for a proportion; (None, None) without observations."""
    if total == 0:
        return None, None
    p = successes / total
    denominator = 1 + z * z / total
    centre = p + z * z / (2 * total)
    margin = z * math.sqrt(p * (1 - p) / total + z * z / (4 * total * total))
    return max(0.0, (centre - margin) / denominator), min(1.0, (centre + margin) / denominator)


def plan_runs(*, models, arms, descriptions, cases, phrasing_set, runs_per_cell, seed, phrasings):
    """A deterministic run list: phrasing and case combinations cycle per cell, then all cells interleave."""
    require(phrasing_set in PHRASING_SETS, "unknown phrasing set")
    require(type(runs_per_cell) is int and runs_per_cell > 0, "runs per cell must be a positive integer")
    identifiers = [item["id"] for item in phrasings.values() if item["set"] == phrasing_set]
    combinations = [(phrasing, case) for phrasing in identifiers for case in cases]
    runs = []
    for description in descriptions:
        for model in models:
            for arm in arms:
                order = list(combinations)
                random.Random(f"{seed}:{description}:{model}:{arm}").shuffle(order)
                for index in range(runs_per_cell):
                    phrasing, case = order[index % len(order)]
                    repeat = index // len(order)
                    runs.append({"description": description, "model": model, "arm": arm, "case": case,
                                 "phrasing": phrasing, "repeat": repeat,
                                 "run_id": run_name(description, model, arm, case, phrasing, repeat)})
    random.Random(f"{seed}:order").shuffle(runs)
    for index, entry in enumerate(runs):
        entry["index"] = index
    return runs


def capture_runtime_pins(claude_path, reposcout_path, required_runtimes):
    """Hash the executables and shared inputs that the isolation copies, so drift fails the run."""
    claude = bounded_claude_binary(claude_path)
    reposcout = Path(reposcout_path).resolve(strict=True)
    require(reposcout.is_file(), "missing RepoScout binary")
    tools = runtime_tool_manifest(None, required=(*REQUIRED_RUNTIME_TOOLS, *required_runtimes))
    sandbox = sandbox_tool_manifest()
    authority = controller_ca_manifest(None)
    return {
        "claude_binary": claude, "claude_sha256": file_hash(claude), "claude_version": claude_version(claude),
        "reposcout_binary": reposcout, "reposcout_sha256": file_hash(reposcout),
        "controller_ca_file": Path(authority["path"]), "controller_ca_sha256": authority["sha256"],
        "runtime_tool_paths": {name: value["path"] for name, value in tools.items()},
        "runtime_tools_sha256": fingerprint(runtime_tool_identities(tools)),
        "sandbox_tool_paths": {name: value["path"] for name, value in sandbox.items()},
        "sandbox_tools_sha256": fingerprint(runtime_tool_identities(sandbox)),
    }


def trial_spec(parameters, *, run_id, record, run_dir, prompt, pins, skill_copy, skill_sha256):
    return ClaudeTrialSpec(
        run_id=run_id, workspace=Path(record["workspace"]), artifact_dir=run_dir / "invocation",
        controller_dir=run_dir / "controller", claude_binary=pins["claude_binary"],
        cli_version=pins["claude_version"], prompt=prompt, answer_schema=ADOPTION_SCHEMA,
        model=parameters["model"], effort=parameters["effort"], variant="reposcout",
        reposcout_binary=pins["reposcout_binary"], skill_dir=skill_copy,
        timeout_seconds=parameters["timeout_seconds"], memory_limit_bytes=LIMITS["memory_limit_bytes"],
        host_memory_reserve_bytes=LIMITS["host_memory_reserve_bytes"], max_output_bytes=LIMITS["max_output_bytes"],
        auth_file=Path(parameters["auth_file"]), expected_claude_sha256=pins["claude_sha256"],
        controller_ca_file=pins["controller_ca_file"], expected_controller_ca_sha256=pins["controller_ca_sha256"],
        expected_reposcout_sha256=pins["reposcout_sha256"], expected_skill_sha256=skill_sha256,
        expected_workspace_sha256=record["public_workspace_sha256"],
        runtime_tool_paths=pins["runtime_tool_paths"], expected_runtime_tools_sha256=pins["runtime_tools_sha256"],
        sandbox_tool_paths=pins["sandbox_tool_paths"], expected_sandbox_tools_sha256=pins["sandbox_tools_sha256"],
        required_runtimes=tuple(sorted({command[0] for command in record["check_commands"]})),
        max_turns=parameters["max_turns"], max_budget_usd=parameters.get("max_budget_usd"),
    )


def classify(result, observations):
    """`complete` when the stream closed cleanly and reached its final result; rates use only these runs."""
    if observations is None:
        return "none"
    closed = result.get("stream_complete") is True and result.get("process_tree_drained") is True
    if (observations["result_subtype"] is not None and closed
            and result.get("termination_reason") not in INVALID_STREAM_REASONS):
        return "complete"
    return "incomplete"


def write_json(path, value):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")


def execute_run(parameters, *, runner=run_trial, phrasings=None, descriptions=None, skill_dir=None,
                bundle_root=BUNDLE_ROOT, pin_capture=capture_runtime_pins):
    """Run one adoption invocation and write its outcome; never retries or grades."""
    from publication import private_destination
    from review_cases import create_signer, list_cases, prepare_case
    phrasings = phrasings if phrasings is not None else load_phrasings()
    skill_dir = Path(skill_dir) if skill_dir is not None else ROOT / "skills/reposcout"
    phrasing, description = validate_run_parameters(
        parameters, phrasings=phrasings, held_out=parameters.get("held_out", False),
        descriptions=descriptions, skill_dir=skill_dir)
    require(parameters["case"] in {case["case_id"] for case in list_cases(bundle_root=bundle_root)},
            "case missing from the development bundle")
    run_id = run_name(parameters["description"], parameters["model"], parameters["arm"], parameters["case"],
                      parameters["phrasing"], parameters["repeat"])
    output = private_destination(parameters["output_dir"])
    run_dir = output / run_id
    require(not run_dir.exists(), f"run already exists; adverse outcomes are never replaced: {run_id}")
    canonical_sha256 = directory_sha256(skill_dir)
    output.mkdir(mode=0o700, parents=True, exist_ok=True)
    run_dir.mkdir(mode=0o700)
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="reposcout-adoption-skill-") as scratch:
        skill_copy = Path(scratch) / "reposcout"
        install_description(skill_dir, skill_copy, description["swap"])
        skill_sha256 = directory_sha256(skill_copy)
        require(description["swap"] is not None or skill_sha256 == canonical_sha256,
                "the unswapped skill copy differs from the canonical skill")
        record = prepare_case(parameters["case"], run_dir / "workspace", private_directory=run_dir / "oracle",
                              signer=create_signer(run_dir / "signing"), bundle_root=bundle_root)
        (run_dir / "controller").mkdir(mode=0o700)
        (run_dir / "invocation").mkdir(mode=0o700)
        step = record["steps"][0]
        prompt = build_prompt(parameters["arm"], phrasing["text"], step["base_commit"], step["head_commit"])
        prompt_sha256 = text_sha256(prompt)
        pins = pin_capture(parameters["claude_binary"], parameters["reposcout_binary"],
                           sorted({command[0] for command in record["check_commands"]}))
        spec = trial_spec(parameters, run_id=run_id, record=record, run_dir=run_dir, prompt=prompt, pins=pins,
                          skill_copy=skill_copy, skill_sha256=skill_sha256)
        write_json(run_dir / "prompt.json", {"prompt": prompt, "prompt_sha256": prompt_sha256})
        result = runner(spec)
    stdout = result.get("stdout_path")
    observations = None
    trace_failures = []
    if stdout and Path(stdout).exists():
        records, _, _, trace_failures = _read_trace(stdout)
        observations = adoption_observations(records)
    measurement = classify(result, observations)
    outcome = {
        "schema": SCHEMA, "kind": KIND, "run_id": run_id, "model": parameters["model"], "arm": parameters["arm"],
        "case": parameters["case"], "repeat": parameters["repeat"],
        "phrasing": {"id": phrasing["id"], "set": phrasing["set"], "text": phrasing["text"]},
        "description": {"name": description["name"], "source": description["source"],
                        "text": description["text"], "text_sha256": text_sha256(description["text"])},
        "skill": {"canonical_dir_sha256": canonical_sha256, "installed_dir_sha256": skill_sha256,
                  "description_swapped": description["swap"] is not None},
        "prompt_sha256": prompt_sha256, "prompt": prompt,
        "limits": {"max_turns": parameters["max_turns"], "max_budget_usd": parameters.get("max_budget_usd"),
                   "timeout_seconds": parameters["timeout_seconds"], "effort": parameters["effort"]},
        "pins": {"claude_version": pins["claude_version"], "claude_sha256": pins["claude_sha256"],
                 "reposcout_sha256": pins["reposcout_sha256"]},
        "status": result.get("status"), "termination_reason": result.get("termination_reason"),
        "runner": {key: result.get(key) for key in (
            "process_status", "exit_code", "stream_complete", "process_tree_drained", "ownership_complete",
            "campaign_fatal", "failure_phase", "error_type", "serial_fence_written", "retained_isolation_root",
            "profile_sha256", "stdout_sha256", "peak_rss_bytes") if key in result},
        "measurement": measurement,
        "capped": bool(observations and observations["capped"]) or result.get("status") == "capped",
        "wall_seconds": round(time.monotonic() - started, 3),
        "runner_wall_seconds": result.get("wall_seconds"),
        "trace_failures": [failure.get("code") for failure in trace_failures][:16],
        "adoption": observations,
    }
    write_json(run_dir / "outcome.json", outcome)
    write_json(run_dir / "result.json", result)
    if result.get("process_tree_drained") is True:
        for name in ("workspace", "oracle", "signing", "controller"):
            path = run_dir / name
            if path.is_dir() and not path.is_symlink():
                shutil.rmtree(path)
    return outcome


def summarize(outcomes):
    """Adoption rate with a Wilson 95% interval per description x model x arm x phrasing set."""
    cells = {}
    for outcome in outcomes:
        key = (outcome["description"]["name"], outcome["description"]["text_sha256"][:12], outcome["model"],
               outcome["arm"], outcome["phrasing"]["set"])
        cell = cells.setdefault(key, {
            "description": key[0], "description_sha256": key[1], "model": key[2], "arm": key[3],
            "phrasing_set": key[4], "runs": 0, "excluded": 0, "adopted": 0, "invoked": 0, "adopted_before_diff": 0,
            "brief_called": 0, "capped": 0, "turns": 0, "adoption_turns": [], "list_price_usd": 0.0,
            **dict.fromkeys(TOKEN_FIELDS, 0)})
        if outcome["measurement"] != "complete":
            cell["excluded"] += 1
            continue
        adoption = outcome["adoption"]
        cell["runs"] += 1
        cell["adopted"] += adoption["adopted"]
        cell["invoked"] += adoption["invoked"]
        cell["adopted_before_diff"] += adoption["adopted_before_diff"]
        cell["brief_called"] += adoption["brief_called"]
        cell["capped"] += bool(outcome["capped"])
        cell["turns"] += adoption["num_turns"]
        cell["list_price_usd"] += adoption["list_price_usd"] or 0.0
        if adoption["first_adoption_turn"] is not None:
            cell["adoption_turns"].append(adoption["first_adoption_turn"])
        for field in TOKEN_FIELDS:
            value = adoption["tokens"].get(field)
            if cell[field] is not None:
                cell[field] = cell[field] + value if isinstance(value, int) else None
    rows = []
    for key in sorted(cells):
        cell = cells[key]
        cell["adoption_rate"] = cell["adopted"] / cell["runs"] if cell["runs"] else None
        cell["wilson_95"] = list(wilson(cell["adopted"], cell["runs"]))
        turns = cell.pop("adoption_turns")
        cell["mean_first_adoption_turn"] = sum(turns) / len(turns) if turns else None
        cell["mean_turns"] = cell["turns"] / cell["runs"] if cell["runs"] else None
        cell.pop("turns")
        rows.append(cell)
    return rows


def load_outcomes(directories):
    outcomes = []
    for directory in directories:
        for path in sorted(Path(directory).rglob("outcome.json")):
            value = json.loads(path.read_text(encoding="utf-8"))
            if isinstance(value, dict) and value.get("kind") == KIND:
                outcomes.append(value)
    return outcomes


def render_summary(rows):
    header = ("description", "model", "arm", "set", "adopted", "rate", "wilson95", "invoked", "pre-diff", "brief",
              "capped", "excl", "uncached", "cache-read", "cache-write", "output")
    lines = ["\t".join(header)]
    for row in rows:
        low, high = row["wilson_95"]
        rate = "-" if row["adoption_rate"] is None else f"{row['adoption_rate']:.2f}"
        interval = "-" if low is None else f"{low:.2f}-{high:.2f}"
        lines.append("\t".join(str(item) for item in (
            f"{row['description']}@{row['description_sha256']}", row["model"], row["arm"], row["phrasing_set"],
            f"{row['adopted']}/{row['runs']}", rate, interval, row["invoked"], row["adopted_before_diff"],
            row["brief_called"], row["capped"], row["excluded"],
            *("-" if row[field] is None else row[field] for field in TOKEN_FIELDS))))
    return "\n".join(lines)


def run_command_line(entry, common):
    command = [sys.executable, "-B", str(Path(__file__).resolve()), "run",
               "--model", entry["model"], "--arm", entry["arm"], "--description", entry["description"],
               "--case", entry["case"], "--phrasing", entry["phrasing"], "--repeat", str(entry["repeat"]),
               "--reposcout-binary", common["reposcout_binary"], "--auth-file", common["auth_file"],
               "--output-dir", common["output_dir"], "--max-turns", str(common["max_turns"])]
    if common.get("max_budget_usd") is not None:
        command += ["--max-budget-usd", str(common["max_budget_usd"])]
    if common.get("claude_binary"):
        command += ["--claude-binary", common["claude_binary"]]
    if common.get("held_out"):
        command.append("--held-out")
    return shlex.join(command)


def _csv(value):
    return [item for item in value.split(",") if item]


def build_parser():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run", help="execute exactly one run")
    run.add_argument("--model", required=True)
    run.add_argument("--arm", required=True, choices=ARMS)
    run.add_argument("--description", required=True)
    run.add_argument("--case", required=True, choices=CASES)
    run.add_argument("--phrasing", required=True)
    run.add_argument("--repeat", required=True, type=int)
    run.add_argument("--reposcout-binary", required=True)
    run.add_argument("--auth-file", required=True, help="path to the private credential file; only the path is read here")
    run.add_argument("--output-dir", required=True)
    run.add_argument("--claude-binary", default=shutil.which("claude"))
    run.add_argument("--held-out", action="store_true", help="allow held-out phrasings (decision runs only)")
    run.add_argument("--max-turns", type=int, default=DEFAULT_MAX_TURNS)
    run.add_argument("--max-budget-usd", type=float, default=None)
    run.add_argument("--effort", default=DEFAULT_EFFORT)
    run.add_argument("--timeout-seconds", type=float, default=DEFAULT_TIMEOUT_SECONDS)
    run.add_argument("--skill-dir", default=None)
    plan = commands.add_parser("plan", help="print a deterministic run list")
    plan.add_argument("--seed", required=True, type=int)
    plan.add_argument("--models", type=_csv, default=list(MODELS))
    plan.add_argument("--arms", type=_csv, default=list(ARMS))
    plan.add_argument("--descriptions", type=_csv, default=[CURRENT, "A", "B", "C"])
    plan.add_argument("--cases", type=_csv, default=list(CASES))
    plan.add_argument("--phrasing-set", choices=PHRASING_SETS, default="dev")
    plan.add_argument("--runs-per-cell", type=int, default=12)
    plan.add_argument("--held-out", action="store_true", help="required for the held-out phrasing set")
    plan.add_argument("--emit", choices=("json", "commands"), default="json")
    plan.add_argument("--reposcout-binary")
    plan.add_argument("--auth-file")
    plan.add_argument("--output-dir")
    plan.add_argument("--claude-binary")
    plan.add_argument("--max-turns", type=int, default=DEFAULT_MAX_TURNS)
    plan.add_argument("--max-budget-usd", type=float, default=None)
    summary = commands.add_parser("summary", help="adoption rates and token totals from run outcomes")
    summary.add_argument("--results", required=True, nargs="+")
    summary.add_argument("--json", action="store_true")
    return parser


def main(argv=None):
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if args.command == "run":
            require(args.claude_binary, "no Claude binary found; pass --claude-binary")
            require(args.max_turns > 0, "--max-turns must be positive")
            outcome = execute_run({
                "model": args.model, "arm": args.arm, "description": args.description, "case": args.case,
                "phrasing": args.phrasing, "repeat": args.repeat, "reposcout_binary": args.reposcout_binary,
                "auth_file": args.auth_file, "output_dir": args.output_dir, "claude_binary": args.claude_binary,
                "held_out": args.held_out, "max_turns": args.max_turns, "max_budget_usd": args.max_budget_usd,
                "effort": args.effort, "timeout_seconds": args.timeout_seconds}, skill_dir=args.skill_dir)
            adoption = outcome["adoption"] or {}
            print(json.dumps({"run_id": outcome["run_id"], "status": outcome["status"],
                              "measurement": outcome["measurement"], "capped": outcome["capped"],
                              "adopted": adoption.get("adopted"), "num_turns": adoption.get("num_turns"),
                              "tokens": adoption.get("tokens")}, sort_keys=True))
            if outcome["runner"].get("serial_fence_written") or outcome["runner"].get("retained_isolation_root"):
                return 3
            return 0 if outcome["measurement"] == "complete" else 1
        if args.command == "plan":
            phrasings = load_phrasings()
            require(args.phrasing_set != "held-out" or args.held_out,
                    "held-out phrasings are reserved for the decision run; pass --held-out to plan them")
            require(set(args.cases) <= set(CASES), "cases must come from the adoption development set")
            runs = plan_runs(models=args.models, arms=args.arms, descriptions=args.descriptions, cases=args.cases,
                             phrasing_set=args.phrasing_set, runs_per_cell=args.runs_per_cell, seed=args.seed,
                             phrasings=phrasings)
            if args.emit == "commands":
                require(args.reposcout_binary and args.auth_file and args.output_dir,
                        "--emit commands needs --reposcout-binary, --auth-file and --output-dir")
                common = {"reposcout_binary": args.reposcout_binary, "auth_file": args.auth_file,
                          "output_dir": args.output_dir, "claude_binary": args.claude_binary,
                          "max_turns": args.max_turns, "max_budget_usd": args.max_budget_usd,
                          "held_out": args.phrasing_set == "held-out"}
                print("\n".join(run_command_line(entry, common) for entry in runs))
            else:
                print(json.dumps({"schema": SCHEMA, "seed": args.seed, "phrasing_set": args.phrasing_set,
                                  "runs_per_cell": args.runs_per_cell, "runs": runs}, indent=2))
            return 0
        rows = summarize(load_outcomes(args.results))
        print(json.dumps(rows, indent=2, sort_keys=True) if args.json else render_summary(rows))
        return 0
    except (AdoptionError, OSError, ValueError, KeyError) as error:
        print(f"adoption_eval: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
