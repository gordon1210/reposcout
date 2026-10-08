"""Controller-only sequential fixture qualification; never copy into agent sources."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from publication import private_destination

parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path)
parser.add_argument("--skip-base-tests", action="store_true")
parser.add_argument("--base-test-receipt", default=None)
parser.add_argument("--case", action="append", dest="selected_cases")
args = parser.parse_args()
bundle = Path(__file__).resolve().parent
if args.output:
    output = private_destination(args.output)
    output.mkdir(mode=0o700, exist_ok=False)
else:
    temp_parent = private_destination(tempfile.gettempdir())
    output = Path(tempfile.mkdtemp(prefix="meridian-qualification-", dir=temp_parent))
catalog = json.loads((bundle / "catalog.json").read_text())
if args.selected_cases:
    unknown = set(args.selected_cases) - {case["case_id"] for case in catalog["cases"]}
    if unknown:
        raise ValueError(f"Unknown cases: {sorted(unknown)}")
    catalog["cases"] = [case for case in catalog["cases"] if case["case_id"] in args.selected_cases]
report = {"schema": 1, "checks": [], "output_directory": str(output), "passed": False}


def persist():
    (output / "qualification.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")


def check(name, condition, **details):
    report["checks"].append({"name": name, "passed": bool(condition), **details})
    persist()
    if not condition:
        raise AssertionError(name)


def copied_source(source, target):
    for path in source.rglob("*"):
        if path.is_symlink():
            raise ValueError(f"Fixture symlink is forbidden: {path}")
    shutil.copytree(source, target)
    return target


def run(name, command, workspace):
    completed = subprocess.run(command, cwd=workspace, text=True, capture_output=True, timeout=90)
    (output / f"{name}.stdout").write_text(completed.stdout)
    (output / f"{name}.stderr").write_text(completed.stderr)
    check(name + ":execution", completed.returncode == 0,
          command=command, cwd=str(workspace), returncode=completed.returncode,
          stdout_file=f"{name}.stdout", stderr_file=f"{name}.stderr")
    return completed.stdout


def public_tests(name, workspace):
    return run(name, [sys.executable, "-B", "-m", "unittest", "discover", "-s", "tests"], workspace)


def domain_probe(name, case_id, workspace):
    source = (bundle / case_id / "probe.py").read_text()
    observed = run(name, [sys.executable, "-I", "-B", "-c", source, str(workspace)], workspace)
    return json.loads(observed)


def differences(expected, actual, prefix=""):
    if type(expected) != type(actual):
        return [prefix or "/"]
    if isinstance(expected, dict):
        paths = []
        for key in sorted(set(expected) | set(actual)):
            child = prefix + "/" + key
            if key not in expected or key not in actual:
                paths.append(child)
            else:
                paths.extend(differences(expected[key], actual[key], child))
        return paths
    if isinstance(expected, list):
        if len(expected) != len(actual):
            return [prefix]
        return [path for index, (left, right) in enumerate(zip(expected, actual))
                for path in differences(left, right, prefix + "/" + str(index))]
    return [] if expected == actual else [prefix]


try:
    public_files = [path for path in (bundle / "application").rglob("*") if path.is_file()]
    check("source-limits", len(public_files) + 2 <= catalog["source_limits"]["max_files"]
          and sum(path.stat().st_size for path in public_files) < catalog["source_limits"]["max_bytes"],
          application_files=len(public_files), injected_files_reserved=2,
          application_bytes=sum(path.stat().st_size for path in public_files))
    base = copied_source(bundle / "application", output / "base")
    if args.skip_base_tests:
        report["checks"].append({"name": "base-public-tests", "status": "previously-run-external",
                                 "receipt": args.base_test_receipt, "executed_by_this_command": False})
        persist()
    else:
        public_tests("base-public-tests", base)
    heads = {}
    oracles = {}
    for case in catalog["cases"]:
        case_id = case["case_id"]
        oracle = json.loads((bundle / case_id / "oracle.json").read_text())
        oracles[case_id] = oracle
        actual = domain_probe(case_id + "-base-domain", case_id, base)
        check(case_id + ":base-domain", actual == oracle["domain_expected"]["base"],
              differences=differences(oracle["domain_expected"]["base"], actual))
        head = copied_source(bundle / "application", output / (case_id + "-head"))
        for overlay in (bundle / case_id / "head").rglob("*"):
            if overlay.is_file():
                target = head / overlay.relative_to(bundle / case_id / "head")
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(overlay.read_bytes())
        heads[case_id] = head
        public_tests(case_id + "-head-public-tests", head)
        actual = domain_probe(case_id + "-head-domain", case_id, head)
        check(case_id + ":head-domain", actual == oracle["domain_expected"]["head"],
              differences=differences(oracle["domain_expected"]["head"], actual))
    counterfactuals = json.loads((bundle / "counterfactuals.json").read_text())["mutations"]
    for mutation in counterfactuals:
        case_id = mutation["case_id"]
        if case_id not in heads:
            continue
        workspace = copied_source(heads[case_id], output / (case_id + "-counterfactual"))
        path = workspace / mutation["file"]
        original = path.read_text()
        check(case_id + ":single-counterfactual-target", original.count(mutation["from"]) == 1)
        path.write_text(original.replace(mutation["from"], mutation["to"]))
        actual = domain_probe(case_id + "-counterfactual-domain", case_id, workspace)
        expected = oracles[case_id]["domain_expected"][mutation["expected_revision"]]
        difference = differences(expected, actual)
        check(case_id + ":counterfactual-domain", bool(difference) if mutation.get("must_differ") else not difference,
              purpose=mutation["purpose"], differences=difference,
              expected_relation="different" if mutation.get("must_differ") else "equal")
        if mutation.get("must_differ"):
            path.write_text(original)
            restored = domain_probe(case_id + "-restored-domain", case_id, workspace)
            check(case_id + ":restored-domain", restored == expected,
                  differences=differences(expected, restored))
    report["passed"] = True
    persist()
    print(json.dumps({"passed": True, "report": str(output / "qualification.json"), "checks": len(report["checks"])}))
except Exception as error:
    report["error"] = {"type": type(error).__name__, "message": str(error)}
    persist()
    print(json.dumps({"passed": False, "report": str(output / "qualification.json"), "error": report["error"]}))
    raise
