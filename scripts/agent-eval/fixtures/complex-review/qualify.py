"""Controller-only sequential zero-model qualification of a materialized complex-review bundle."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from publication import private_destination  # noqa: E402
import review_cases  # noqa: E402

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--bundle", type=Path, required=True)
parser.add_argument("--output", type=Path)
parser.add_argument("--case", action="append", dest="selected_cases")
args = parser.parse_args()
bundle = Path(args.bundle).resolve()
if args.output:
    output = private_destination(args.output)
    output.mkdir(mode=0o700, exist_ok=False)
else:
    output = Path(tempfile.mkdtemp(prefix="fieldline-qualification-", dir=private_destination(tempfile.gettempdir())))
catalog = json.loads((bundle / "catalog.json").read_text())
cases = [case for case in catalog["cases"] if not args.selected_cases or case["case_id"] in args.selected_cases]
if args.selected_cases and len(cases) != len(set(args.selected_cases)):
    raise ValueError("unknown case selected")
report = {"schema": 1, "bundle": str(bundle), "checks": [], "output_directory": str(output), "passed": False}


def persist():
    (output / "qualification.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")


def check(name, condition, **details):
    report["checks"].append({"name": name, "passed": bool(condition), **details})
    persist()
    if not condition:
        raise AssertionError(name)


def install(files, name):
    workspace = output / name
    for path, text in files.items():
        target = workspace / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
    return workspace


def run(name, command, workspace, timeout):
    completed = subprocess.run(command, cwd=workspace, text=True, capture_output=True, timeout=timeout)
    (output / f"{name}.stdout").write_text(completed.stdout)
    (output / f"{name}.stderr").write_text(completed.stderr)
    check(name + ":execution", completed.returncode == 0, command=command, returncode=completed.returncode)
    return completed.stdout


def public_tests(name, case_id, workspace):
    summary = run(name, review_cases.check_command(case_id, bundle), workspace, 300).strip().splitlines()[-1]
    counts = {key: int(value) for value, key in re.findall(r"(\d+) (passed|failed|skipped)", summary)}
    check(name + ":all-passed", counts.get("passed", 0) > 0 and counts.get("failed", 0) == 0, summary=summary)
    return counts


def probe(name, case_id, workspace):
    source = (bundle / case_id / "probe.py").read_text()
    return json.loads(run(name, [sys.executable, "-I", "-B", "-c", source, str(workspace)], workspace, 10))


try:
    base_counts = None
    for case in cases:
        case_id = case["case_id"]
        oracle = json.loads((bundle / case_id / "oracle.json").read_text())
        step = oracle["steps"][0]
        snapshots = review_cases.snapshots(case_id, bundle_root=bundle)
        check(case_id + ":source-limits", True, files={revision: len(files) for revision, files in snapshots.items()},
              bytes={revision: sum(len(text.encode()) for text in files.values()) for revision, files in snapshots.items()})
        review_cases._resolve_obligations(oracle, snapshots, [{"base_revision": "base", "head_revision": "head"}])
        check(case_id + ":unique-oracle-anchors", True)
        private_text = json.dumps(oracle) + (bundle / case_id / "probe.py").read_text()
        check(case_id + ":oracle-independent-of-reposcout", "reposcout" not in private_text.lower())
        if base_counts is None:
            base_counts = public_tests("base-public-tests", case_id, install(snapshots["base"], "base"))
        base = output / "base"
        check(case_id + ":base-domain", probe(case_id + "-base-domain", case_id, base) == oracle["domain_expected"]["base"])
        head = install(snapshots["head"], case_id + "-head")
        head_counts = public_tests(case_id + "-head-public-tests", case_id, head)
        check(case_id + ":head-domain", probe(case_id + "-head-domain", case_id, head) == oracle["domain_expected"]["head"],
              base_tests=base_counts, head_tests=head_counts)
        same = oracle["domain_expected"]["base"] == oracle["domain_expected"]["head"]
        check(case_id + ":domain-relation", same == (step["conclusion"] == "no-issues") and same == (not step["defects"]),
              conclusion=step["conclusion"], base_equals_head=same)
        harness = review_cases.verify_domain(case_id, bundle_root=bundle)
        check(case_id + ":harness-verify-domain", harness["passed"])
    selected = {case["case_id"] for case in cases}
    for index, mutation in enumerate(json.loads((bundle / "counterfactuals.json").read_text())["mutations"]):
        case_id = mutation["case_id"]
        if case_id not in selected:
            continue
        files = dict(review_cases.snapshots(case_id, bundle_root=bundle)["head"])
        original = files[mutation["file"]]
        check(f"{case_id}:counterfactual-{index}:single-target", original.count(mutation["from"]) == 1)
        files[mutation["file"]] = original.replace(mutation["from"], mutation["to"])
        workspace = install(files, f"{case_id}-counterfactual-{index}")
        actual = probe(f"{case_id}-counterfactual-{index}-domain", case_id, workspace)
        expected = json.loads((bundle / case_id / "oracle.json").read_text())["domain_expected"][mutation["expected_revision"]]
        differs = actual != expected
        check(f"{case_id}:counterfactual-{index}", differs if mutation.get("must_differ") else not differs,
              purpose=mutation["purpose"], expected_relation="different" if mutation.get("must_differ") else "equal")
    report["passed"] = True
    persist()
    print(json.dumps({"passed": True, "report": str(output / "qualification.json"), "checks": len(report["checks"])}))
except Exception as error:
    report["error"] = {"type": type(error).__name__, "message": str(error)}
    persist()
    print(json.dumps({"passed": False, "report": str(output / "qualification.json"), "error": report["error"]}))
    raise
