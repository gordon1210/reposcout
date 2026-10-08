"""Fixed 36-review order and matched reporting for the local large-fixture study.

Execution remains in review_campaign. This module selects no workloads or retries
from observed outcomes and never starts a model.
"""

import argparse
from collections import Counter
import itertools
import json
import random
import re

from accounting import fingerprint, read_json, require
from review_campaign import EFFORT, MODEL, LARGE_DEVELOPMENT_CASES, LARGE_HOLDOUT_CASES, build_plan
from review_export import REQUESTED_TOKEN_FIELDS, _comparison, all_run_costs, quality_state, safe_projection

CAMPAIGNS = ("development-original", "development-improved", "holdout-improved", "holdout-original")


def schedule(seed):
    require(type(seed) is int, "study seed must be an integer")
    cases = [{"case_id": name, "title": name, "partition": "development", "step_count": 1,
              "fixture_sha256": fingerprint(name)} for name in LARGE_DEVELOPMENT_CASES]
    development = build_plan("large-development", seed, {}, cases)["assignments"]
    slots = []
    for condition in ("original", "improved"):
        for row in development:
            slots.append({"campaign": "development-" + condition, "case_id": row["case_id"],
                          "repeat_id": row["repeat_id"], "variant": row["variant"],
                          "condition": "native" if row["variant"] == "baseline" else condition})
    generator = random.Random(seed)
    permutations = list(itertools.permutations(("native", "original", "improved")))
    # Four triplets cannot balance six permutations exactly; every condition still
    # occupies each position once or twice, without depending on any result.
    balanced = [orders for orders in itertools.combinations(permutations, 4)
                if all(sorted(Counter(order[position] for order in orders).values()) == [1, 1, 2]
                       for position in range(3))]
    orders = list(generator.choice(balanced))
    generator.shuffle(orders)
    blocks = []
    for repeat in (1, 2):
        names = list(LARGE_HOLDOUT_CASES)
        generator.shuffle(names)
        blocks.extend((name, repeat) for name in names)
    for (case_id, repeat), order in zip(blocks, orders):
        for condition in order:
            slots.append({"campaign": "holdout-original" if condition == "original" else "holdout-improved",
                          "case_id": case_id, "repeat_id": repeat,
                          "variant": "baseline" if condition == "native" else "reposcout", "condition": condition})
    for index, row in enumerate(slots):
        row["sequence"] = index
    result = {"schema": 1, "kind": "large-review-study-order", "seed": seed,
              "assigned_invocations": 36, "slots": slots, "statistical_inference": False}
    result["order_sha256"] = fingerprint(result)
    return result


def _indexed_reports(reports):
    require(set(reports) == set(CAMPAIGNS), "all four frozen stage reports are required")
    reference = reports["development-original"]
    common = ("codex_sha256", "codex_version", "codex_runtime_sha256", "runtime_tools_sha256",
              "controller_ca_sha256", "model", "effort", "answer_schema_sha256", "limits",
              "usage_scope", "capabilities_sha256", "harness_sources")
    indexed = {}
    for name, report in reports.items():
        require(report["model"] == MODEL and report["effort"] == EFFORT,
                "study model or effort differs")
        require(report["seed"] == reference["seed"], "study seeds differ")
        require(all(key in report["pins"] and report["pins"][key] == reference["pins"].get(key) for key in common),
                "shared runtime, harness, task or safety condition differs")
        expected_stage = "large-development" if name.startswith("development-") else (
            "large-holdout-original" if name == "holdout-original" else "large-holdout")
        require(report["stage"] == expected_stage, "report has the wrong study stage")
        rows = {(row["case_id"], row["repeat_id"], row["variant"]): row for row in report["runs"]}
        require(len(rows) == len(report["runs"]), "duplicate stage assignment")
        indexed[name] = rows
    for condition in ("original", "improved"):
        before, holdout = reports["development-" + condition], reports["holdout-" + condition]
        require(all(before["pins"].get(key) == holdout["pins"].get(key) for key in ("reposcout_sha256", "skill")),
                "heldout treatment differs from its frozen development condition")
    for names in (("development-original", "development-improved"), ("holdout-original", "holdout-improved")):
        require(reports[names[0]]["pins"].get("fixture_bundle") == reports[names[1]]["pins"].get("fixture_bundle"),
                "matched stage fixture bundles differ")
    require(reports["holdout-original"]["pins"].get("original_plan_sha256") == reference["plan_sha256"],
            "original holdout lacks the original canonical development pin")
    return indexed


def _matched_comparison(reference, candidate):
    def known_hash(value):
        return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None

    identity_known = all(
        all(known_hash(row.get(key)) for key in ("fixture_sha256", "oracle_sha256"))
        and type(row.get("step_count")) is int and row["step_count"] == 1
        and len(row["invocations"]) == 1
        and known_hash((row["invocations"][0].get("prompt_hashes") or {}).get("task_sha256"))
        for row in (reference, candidate))
    normalized = [{**row, "step_count": row.get("step_count"), "invocations": [
        {**item, "prompt_hashes": item.get("prompt_hashes") or {}} for item in row["invocations"]]}
        for row in (reference, candidate)]
    comparison = _comparison(*normalized)
    if not identity_known:
        comparison.update(eligible=False, usage_basis=None, token_deltas=None,
                          all_requested_token_fields_comparable=False)
        comparison["ineligibility_reasons"].append("missing-or-invalid-comparison-identity")
        for field in comparison["field_comparisons"].values():
            field.update(eligible=False, delta=None)
            if "pair-ineligible" not in field["ineligibility_reasons"]:
                field["ineligibility_reasons"].append("pair-ineligible")
    return comparison


def summarize(reports):
    indexed = _indexed_reports(reports)
    order = schedule(reports["development-original"]["seed"])
    expected = {name: set() for name in CAMPAIGNS}
    rows, by_case = [], {}
    for slot in order["slots"]:
        name = slot["campaign"]
        key = (slot["case_id"], slot["repeat_id"], slot["variant"])
        expected[name].add(key)
        require(key in indexed[name], "a frozen assignment is absent from its report")
        row = {**indexed[name][key], "planned_sequence": slot["sequence"], "campaign": name,
               "campaign_plan_sha256": reports[name]["plan_sha256"], "condition": slot["condition"]}
        rows.append(row)
        by_case.setdefault((slot["case_id"], slot["repeat_id"]), []).append(row)
    require(all(set(indexed[name]) == expected[name] for name in CAMPAIGNS), "study report contains extra assignments")
    comparisons = []
    for (case_id, repeat), candidates in by_case.items():
        named = {(row["campaign"], row["condition"]): row for row in candidates}
        if case_id in LARGE_DEVELOPMENT_CASES:
            pairs = ((("development-original", "native"), ("development-original", "original")),
                     (("development-improved", "native"), ("development-improved", "improved")),
                     (("development-original", "original"), ("development-improved", "improved")))
        else:
            pairs = ((("holdout-improved", "native"), ("holdout-original", "original")),
                     (("holdout-improved", "native"), ("holdout-improved", "improved")),
                     (("holdout-original", "original"), ("holdout-improved", "improved")))
        for left_key, right_key in pairs:
            left, right = named[left_key], named[right_key]
            comparison = _matched_comparison(left, right)
            comparisons.append({"case_id": case_id, "repeat_id": repeat,
                                "reference_campaign": left["campaign"], "candidate_campaign": right["campaign"],
                                "reference_condition": left["condition"], "candidate_condition": right["condition"],
                                **comparison})
    costs = []
    for campaign, condition in sorted({(row["campaign"], row["condition"]) for row in rows}):
        selected = [row for row in rows if (row["campaign"], row["condition"]) == (campaign, condition)]
        for group in all_run_costs(selected):
            measured = [row for row in selected if ((row.get("accounting") or {}).get("usage_basis")
                                                    or "unavailable") == group["usage_basis"]]
            for key in ("observed_token_known_sums", "token_known_run_counts",
                        "observed_partial_token_known_sums", "partial_token_known_run_counts"):
                group[key] = {field: group[key][field] for field in REQUESTED_TOKEN_FIELDS}
            costs.append({"campaign": campaign, "condition": condition,
                          "quality_counts": dict(Counter(quality_state(row) for row in measured)), **group})
    result = {"schema": 1, "kind": "large-review-study-report", "order_sha256": order["order_sha256"],
              "assignment_count": len(rows), "status_counts": dict(Counter(row["status"] for row in rows)),
              "quality_counts": dict(Counter(quality_state(row) for row in rows)), "runs": rows,
              "all_assigned_run_costs": costs, "comparisons": comparisons,
              "statistical_inference": False,
              "limits": ["sequence is planned order; actual execution order requires the separate bound controller attestation",
                         "two repeats per case do not establish population benefit or equivalence",
                         "development before/after is exploratory; holdout runs only after the single improvement freeze",
                         "input includes cache-read; unknown cache-write stays unknown; duration is not an efficacy metric"]}
    safe_projection(result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    order = commands.add_parser("schedule")
    order.add_argument("--seed", type=int, required=True)
    report = commands.add_parser("report")
    for name in CAMPAIGNS:
        report.add_argument("--" + name, required=True)
    args = parser.parse_args()
    result = schedule(args.seed) if args.command == "schedule" else summarize(
        {name: read_json(getattr(args, name.replace("-", "_"))) for name in CAMPAIGNS})
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
