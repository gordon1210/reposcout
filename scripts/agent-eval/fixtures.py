"""Prepare bounded offline snapshots and quality oracles without executing models."""

import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent / "fixtures"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def files_digest(files):
    return digest(json.dumps(files, sort_keys=True, separators=(",", ":")).encode())


def snapshot():
    return {str(path.relative_to(ROOT / "repository")): digest(path.read_bytes())
            for path in sorted((ROOT / "repository").rglob("*.rs"))}


def declaration(path, symbol):
    lines = (ROOT / "repository" / path).read_text().splitlines()
    needle = "pub fn " + symbol + "("
    starts = [index for index, line in enumerate(lines) if line.startswith(needle)]
    if len(starts) != 1:
        raise ValueError("fixture declaration must be unique: " + symbol)
    start = starts[0]
    end = next(index for index in range(start + 1, len(lines)) if lines[index] == "}")
    return {"path": path, "symbol": symbol, "start_line": start + 1, "end_line": end + 1}


def baseline_steps(targets, discovery):
    searches = [["rg", "-n", "--max-count", "8", "--", pattern, "src"] for pattern in discovery]
    reads = [["sed", "-n", f'{target["start_line"]},{target["end_line"]}p', target["path"]]
             for target in targets]
    return {"search": searches, "targeted_reads": reads, "allow_refinement": True,
            "allow_whole_file_when_justified": True}


def cases():
    selectors = {
        "cap_quantity": declaration("src/inventory.rs", "cap_quantity"),
        "invoice_total": declaration("src/billing.rs", "invoice_total"),
        "tax_for": declaration("src/billing.rs", "tax_for"),
        "checkout_total": declaration("src/checkout.rs", "checkout_total"),
        "preview_total": declaration("src/checkout.rs", "preview_total"),
        "retry_delay": declaration("src/payments.rs", "retry_delay"),
    }
    definitions = [
        ("F1", "known-definition", ["cap_quantity"], ["pub fn cap_quantity"],
         {"operation": "evaluate", "selector": selectors["cap_quantity"],
          "inputs": [{"value": -3, "limit": 10}, {"value": 14, "limit": 10}]}, [0, 10]),
        ("F2", "changed-definition", ["cap_quantity"], ["pub fn cap_quantity"],
         {"operation": "identify-changed-definition-and-evaluate", "before": "before/src/inventory.rs",
          "after": "src/inventory.rs", "inputs": [{"value": -3, "limit": 10}]}, [0]),
        ("F3", "unknown-entry", ["retry_delay"], ["retry|duplicate_payment", "pub fn retry_delay"],
         {"operation": "find", "query": "retry delay duplicate payment"}, None),
        ("F4", "multi-location-diagnostics", ["invoice_total", "checkout_total"], ["pub fn invoice_total|pub fn checkout_total"],
         {"operation": "identify-diagnostic-definitions", "diagnostics": "diagnostics.jsonl"}, None),
        ("F5", "definition-plan", ["invoice_total", "cap_quantity", "tax_for"], ["pub fn invoice_total", "cap_quantity|tax_for"],
         {"operation": "plan-direct-local-dependencies", "selector": selectors["invoice_total"]}, None),
        ("F6", "consumers", ["checkout_total", "preview_total"], ["invoice_total\\("],
         {"operation": "identify-direct-callers", "selector": selectors["invoice_total"]}, None),
    ]
    result = []
    file_hashes = snapshot()
    file_hashes["@before/src/inventory.rs"] = digest((ROOT / "before/src/inventory.rs").read_bytes())
    file_hashes["@diagnostics.jsonl"] = digest((ROOT / "diagnostics.jsonl").read_bytes())
    for feature, name, names, discovery, task, values in definitions:
        expected = [selectors[symbol] for symbol in names]
        oracle = {"definitions": sorted(names), "values": values, "evidence": expected}
        for mode in ("isolated", "end-to-end"):
            payload = dict(task)
            if feature == "F1" and mode == "end-to-end":
                payload.pop("selector")
                payload["operation"] = "find-and-evaluate"
                payload["query"] = json.loads((ROOT / "prompt-templates.json").read_text())["cases"]["F1"]["end_to_end_query"]
            case_discovery = ["cap|quantity|limit"] if feature == "F1" and mode == "end-to-end" else discovery
            result.append({"schema": 1, "task_id": feature + ":" + name + ":" + mode,
                           "feature": feature, "mode": mode, "task": payload,
                           "task_sha256": files_digest(payload), "fixture_files": file_hashes,
                           "fixture_sha256": files_digest(file_hashes), "oracle_sha256": files_digest(oracle),
                           "oracle": oracle, "baseline": baseline_steps(expected, [] if mode == "isolated" else case_discovery),
                           "execution": "not-run", "required_external": ["agent model usage export", "pinned model and harness", "quality answer"]})
    return result



def composed_case():
    base = next(case for case in cases() if case['feature'] == 'F2' and case['mode'] == 'end-to-end')
    task = {'operation': 'changed-definition-consumers-and-behavior',
            'before': 'before/src/inventory.rs', 'after': 'src/inventory.rs',
            'inputs': [{'quantity': -3, 'unit_price': 250}, {'quantity': 3, 'unit_price': 250}]}
    evidence = [declaration('src/inventory.rs', 'cap_quantity'), declaration('src/billing.rs', 'invoice_total'),
                declaration('src/billing.rs', 'tax_for')]
    oracle = {'definitions': ['cap_quantity', 'invoice_total', 'tax_for'], 'values': [0, 900], 'evidence': evidence}
    return {**base, 'task_id': 'COMPOSED:changed-consumers-source:end-to-end', 'feature': 'COMPOSED',
            'task': task, 'task_sha256': files_digest(task), 'oracle': oracle, 'oracle_sha256': files_digest(oracle)}


def verify_answer(case, answer):
    expected = case["oracle"]
    if not isinstance(answer, dict):
        return {"passed": False, "regressions": ["answer-must-be-object"],
                "missing_evidence": expected["evidence"], "evidence_sha256": files_digest(answer)}
    definitions = answer.get("definitions")
    regressions = []
    missing = []
    if not isinstance(definitions, list) or not all(isinstance(item, str) for item in definitions) or sorted(definitions) != expected["definitions"]:
        regressions.append("definition-set-mismatch")
    if expected["values"] is not None and answer.get("values") != expected["values"]:
        regressions.append("behavior-answer-mismatch")
    evidence = answer.get("evidence", [])
    if not isinstance(evidence, list):
        evidence = []
    for required in expected["evidence"]:
        if not any(item == required for item in evidence):
            missing.append(required)
    return {"passed": not regressions and not missing, "regressions": regressions,
            "missing_evidence": missing, "evidence_sha256": files_digest(answer)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("manifest", "task", "verify"))
    parser.add_argument("--task")
    parser.add_argument("--answer")
    args = parser.parse_args()
    suite = cases()
    if args.command == "manifest":
        output = {"schema": 1, "evidence_kind": "synthetic", "cases": suite}
    else:
        case = next(case for case in suite if case["task_id"] == args.task)
        if args.command == "task":
            output = {key: case[key] for key in ("schema", "task_id", "feature", "mode", "task", "task_sha256", "fixture_sha256")}
        else:
            with Path(args.answer).open("rb") as stream:
                data = stream.read(1024 * 1024 + 1)
            if len(data) > 1024 * 1024:
                parser.error("answer exceeds 1 MiB")
            output = verify_answer(case, json.loads(data))
    print(json.dumps(output, sort_keys=True, indent=2))


if __name__ == "__main__":
    main()
