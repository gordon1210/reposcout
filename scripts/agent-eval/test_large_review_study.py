from collections import Counter
import copy
import unittest

from accounting import InvalidLedger, fingerprint
from large_review_study import CAMPAIGNS, schedule, summarize
from review_campaign import EFFORT, MODEL, LARGE_DEVELOPMENT_CASES, LARGE_HOLDOUT_CASES
from review_export import project_run
from test_review_export import measured


def reports():
    common = {key: fingerprint(key) for key in (
        "codex_sha256", "codex_version", "codex_runtime_sha256", "runtime_tools_sha256", "controller_ca_sha256",
        "answer_schema_sha256", "limits", "usage_scope", "capabilities_sha256", "harness_sources")}
    common.update(model=MODEL, effort=EFFORT)
    result = {}
    for name in CAMPAIGNS:
        condition = name.split("-")[-1]
        result[name] = {"model": MODEL, "effort": EFFORT, "seed": 20261008,
                        "stage": "large-development" if name.startswith("development-") else (
                            "large-holdout-original" if name == "holdout-original" else "large-holdout"),
                        "plan_sha256": fingerprint(name), "runs": [],
                        "pins": {**common, "reposcout_sha256": fingerprint(condition), "skill": fingerprint(condition),
                                 "fixture_bundle": fingerprint(name.split("-")[0])}}
    result["holdout-original"]["pins"]["original_plan_sha256"] = result["development-original"]["plan_sha256"]
    for slot in schedule(20261008)["slots"]:
        name = slot["campaign"]
        identity = fingerprint(slot)
        assignment = {**slot, "run_id": identity, "pair_id": fingerprint((slot["case_id"], slot["repeat_id"])),
                      "pair_position": 0, "fixture_sha256": fingerprint(slot["case_id"]), "step_count": 1,
                      "oracle_sha256": fingerprint((slot["case_id"], "oracle")), "run_dir": "/unexecuted/fixture"}
        result[name]["runs"].append(project_run({"assignment": assignment, **measured(result[name], assignment)}))
    return result


class LargeStudyTests(unittest.TestCase):
    def test_order_contains_exactly_the_fixed_36_single_reviews_and_balanced_holdout_positions(self):
        value = schedule(20261008)
        self.assertEqual(value, schedule(20261008))
        self.assertEqual(value["assigned_invocations"], 36)
        slots = value["slots"]
        self.assertEqual([row["sequence"] for row in slots], list(range(36)))
        self.assertEqual(Counter(row["campaign"] for row in slots),
                         {"development-original": 12, "development-improved": 12, "holdout-improved": 8, "holdout-original": 4})
        self.assertEqual({row["case_id"] for row in slots[:24]}, set(LARGE_DEVELOPMENT_CASES))
        self.assertEqual({row["case_id"] for row in slots[24:]}, set(LARGE_HOLDOUT_CASES))
        for index in range(24, 36, 3):
            group = slots[index:index + 3]
            self.assertEqual(len({(row["case_id"], row["repeat_id"]) for row in group}), 1)
            self.assertEqual({row["condition"] for row in group}, {"native", "original", "improved"})
        for position in range(3):
            self.assertEqual(sorted(Counter(row["condition"] for row in slots[24 + position::3]).values()), [1, 1, 2])

    def test_matched_report_preserves_all_costs_quality_failures_and_unknown_cache_writes(self):
        inputs = reports()
        failed = inputs["development-original"]["runs"][0]
        failed["quality"]["quality"]["passed"] = False
        result = summarize(inputs)
        self.assertEqual(result["assignment_count"], 36)
        self.assertEqual(result["quality_counts"], {"failed": 1, "passed": 35})
        self.assertEqual(len(result["comparisons"]), 30)
        self.assertEqual({(group["campaign"], group["condition"], group["assigned_runs"])
                          for group in result["all_assigned_run_costs"]}, {
                              ("development-original", "native", 6), ("development-original", "original", 6),
                              ("development-improved", "native", 6), ("development-improved", "improved", 6),
                              ("holdout-improved", "native", 4), ("holdout-improved", "improved", 4),
                              ("holdout-original", "original", 4)})
        self.assertEqual([row["planned_sequence"] for row in result["runs"]], list(range(36)))
        self.assertEqual(sum(group["observed_token_known_sums"]["input_tokens"]
                             for group in result["all_assigned_run_costs"]), 3600)
        self.assertTrue(all(group["observed_token_known_sums"]["cache_write_input_tokens"] is None
                            for group in result["all_assigned_run_costs"]))
        self.assertTrue(all(set(group["observed_token_known_sums"]) == {
            "input_tokens", "cache_write_input_tokens", "cached_input_tokens", "output_tokens"}
            for group in result["all_assigned_run_costs"]))
        touching = [pair for pair in result["comparisons"] if failed["run_id"] in (pair["reference_run"], pair["candidate_run"])]
        self.assertTrue(touching)
        self.assertTrue(all(not pair["eligible"] for pair in touching))
        self.assertTrue(all(pair["field_comparisons"]["cache_write_input_tokens"]["delta"] is None
                            for pair in result["comparisons"]))

    def test_missing_identities_in_both_rows_never_qualify_a_comparison(self):
        for field in ("fixture_sha256", "oracle_sha256", "task_sha256", "step_count"):
            with self.subTest(field=field):
                inputs = reports()
                for report in inputs.values():
                    for row in report["runs"]:
                        if field == "task_sha256":
                            row["invocations"][0]["prompt_hashes"].pop(field)
                        elif field == "step_count":
                            row[field] = None
                        else:
                            row.pop(field)
                result = summarize(inputs)
                self.assertEqual(result["assignment_count"], 36)
                self.assertTrue(all(not pair["eligible"] and "missing-or-invalid-comparison-identity"
                                    in pair["ineligibility_reasons"] for pair in result["comparisons"]))
                self.assertTrue(all(pair["token_deltas"] is None for pair in result["comparisons"]))

    def test_drift_extra_assignments_and_changed_signed_task_identity_cannot_form_valid_pairs(self):
        original = reports()
        for mutate in (
            lambda rows: rows["holdout-original"]["pins"].update(reposcout_sha256=fingerprint("foreign")),
            lambda rows: rows["development-improved"]["pins"].update(harness_sources=fingerprint("foreign")),
            lambda rows: rows["holdout-original"]["runs"].append(copy.deepcopy(rows["holdout-original"]["runs"][0])),
        ):
            changed = copy.deepcopy(original)
            mutate(changed)
            with self.assertRaises(InvalidLedger):
                summarize(changed)
        changed = copy.deepcopy(original)
        row = changed["holdout-original"]["runs"][0]
        row["oracle_sha256"] = fingerprint("different signer")
        row["invocations"][0]["prompt_hashes"]["task_sha256"] = fingerprint("different signed commits")
        pairs = summarize(changed)["comparisons"]
        affected = [pair for pair in pairs if row["run_id"] in (pair["reference_run"], pair["candidate_run"])]
        self.assertTrue(all(not pair["eligible"] and "task-or-step-inventory-differs" in pair["ineligibility_reasons"]
                            for pair in affected))


if __name__ == "__main__":
    unittest.main()
