import copy
import json
from pathlib import Path
import tempfile
import unittest

from accounting import InvalidLedger
import review_cases
import review_grading


def source_evidence(record, step, side, path):
    revision = record["steps"][step][side + "_revision"]
    source = review_cases.snapshots(record["case_id"])[revision][path]
    return {"path": path, "side": side, "snapshot": record["steps"][step][side + "_tree"],
            "start_line": 1, "end_line": len(source.splitlines()), "quote": source}


def answer(evidence, findings=None, retained=None):
    return {"conclusion": "issues" if findings else "no-issues", "findings": findings or [],
            "evidence": evidence, "retained_evidence": retained or [],
            "limitations": [], "validation": ["Inspected request assertions; did not execute them."]}


def refund_answer(record):
    evidence = [source_evidence(record, 0, side, path) for side, path in (
        ("base", "policy.py"), ("head", "policy.py"), ("head", "api.py"), ("head", "test_refund.py"))]
    finding = {"location": {"path": "policy.py", "side": "head", "snapshot": record["steps"][0]["head_tree"],
                            "start_line": 4, "end_line": 4},
               "trigger": {"input_json": '{"days":14}', "expected_json": "true", "actual_json": "false"},
               "cause": "The strict less-than comparison excludes the inclusive final refund day.",
               "impact": "Both registered refund flows deny valid day-14 requests.", "evidence_indices": [0, 1, 2, 3]}
    return answer(evidence, [finding])


def checkout_answer(record):
    evidence = [source_evidence(record, 0, side, path) for side, path in (
        ("base", "serializer.py"), ("head", "serializer.py"), ("head", "api.py"),
        ("head", "calculator.py"), ("head", "test_checkout.py"))]
    finding = {"location": {"path": "serializer.py", "side": "head", "snapshot": record["steps"][0]["head_tree"],
                            "start_line": 2, "end_line": 2},
               "trigger": {"input_json": '{"items":[{"quantity":2,"unit_price_cents":1250}]}',
                           "expected_json": "2500", "actual_json": "25"},
               "cause": "The serializer divides calculated cents by 100 before writing amount_cents.",
               "impact": "The checkout response reports 25 cents instead of the required 2500 cents.",
               "evidence_indices": list(range(len(evidence)))}
    return answer(evidence, [finding])


def adjudication(grade, passed=True):
    return {"schema": 1, "packet_id": grade["packet_id"], "answer_sha256": grade["answer_sha256"],
            "reviewer": "unit-test-semantic-reviewer", "blinded": True,
            "steps": [{"step_id": index, "semantic_passed": passed, "unsupported_finding_indices": [],
                       "missed_defect_ids": [], "missing_evidence": [],
                       "execution_claims": "none", "execution_evidence_indices": [],
                       "notes": "Fixture-only adjudication; not a real campaign judgment."}
                      for index in range(len(grade["adjudication_packet"]["tasks"]))]}


class ReviewGradingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def prepare(self, case_id):
        return review_cases.prepare_case(case_id, self.root / case_id)

    def test_correct_structural_answer_waits_for_blinded_semantic_review(self):
        case = self.prepare("refund-boundary")
        answers = [refund_answer(case)]
        grade = review_grading.grade_episode(case, answers)
        self.assertTrue(grade["automatic"]["passed"])
        self.assertFalse(grade["quality"]["passed"])
        self.assertEqual(grade["adjudication_status"], "pending")
        reviewed = review_grading.grade_episode(case, answers, adjudication(grade))
        self.assertTrue(reviewed["quality"]["passed"])
        self.assertFalse(reviewed["automatic"]["source_delivery_observed"])

    def test_fabricated_quote_range_snapshot_and_path_cannot_be_approved(self):
        case = self.prepare("refund-boundary")
        mutations = [("quote", "return True\n"), ("end_line", 999), ("snapshot", "0" * 40),
                     ("path", "../policy.py"), ("start_line", True)]
        for field, value in mutations:
            with self.subTest(field=field):
                supplied = refund_answer(case)
                supplied["evidence"][1][field] = value
                grade = review_grading.grade_episode(case, [supplied])
                self.assertFalse(grade["automatic"]["passed"])
                reviewed = review_grading.grade_episode(case, [supplied], adjudication(grade))
                self.assertFalse(reviewed["quality"]["passed"])

    def test_line_metadata_does_not_make_numbered_display_text_verbatim_source(self):
        case = self.prepare("refund-boundary")
        supplied = refund_answer(case)
        raw_line = supplied["evidence"][1]["quote"].splitlines(keepends=True)[3]
        supplied["evidence"][1].update({"start_line": 4, "end_line": 4, "quote": raw_line})
        self.assertTrue(review_grading.grade_episode(case, [supplied])["automatic"]["passed"])
        supplied["evidence"][1]["quote"] = raw_line.removesuffix("\n")
        self.assertTrue(review_grading.grade_episode(case, [supplied])["automatic"]["passed"])
        for prefix in ("4: ", "4\t", "+"):
            with self.subTest(prefix=prefix):
                supplied["evidence"][1]["quote"] = prefix + raw_line
                grade = review_grading.grade_episode(case, [supplied])
                self.assertIn("invalid-source-evidence", [error["code"] for error in grade["automatic"]["hard_errors"]])
                self.assertFalse(review_grading.grade_episode(case, [supplied], adjudication(grade))["quality"]["passed"])

    def test_known_literal_business_result_is_not_inferred_from_product_output(self):
        case = self.prepare("refund-boundary")
        supplied = refund_answer(case)
        supplied["findings"][0]["trigger"]["actual_json"] = "true"
        grade = review_grading.grade_episode(case, [supplied])
        self.assertTrue(any("known-domain" in item["detail"] for item in grade["automatic"]["hard_errors"]))
        self.assertFalse(review_grading.grade_episode(case, [supplied], adjudication(grade))["quality"]["passed"])

    def test_causal_prose_does_not_pass_because_locations_and_witnesses_match(self):
        case = self.prepare("refund-boundary")
        supplied = refund_answer(case)
        supplied["findings"][0]["cause"] = "Network packet loss makes the server reject the refund."
        grade = review_grading.grade_episode(case, [supplied])
        self.assertTrue(grade["automatic"]["passed"])
        self.assertFalse(grade["quality"]["passed"])
        decision = adjudication(grade, passed=False)
        decision["steps"][0]["notes"] = "Cited code does not support the claimed network cause."
        self.assertFalse(review_grading.grade_episode(case, [supplied], decision)["quality"]["passed"])

    def test_clean_case_false_positive_is_explicit(self):
        case = self.prepare("clean-refactor")
        evidence = [source_evidence(case, 0, side, path) for side, path in (
            ("base", "shipping.py"), ("head", "shipping.py"), ("head", "test_shipping.py"))]
        clean = answer(evidence)
        grade = review_grading.grade_episode(case, [clean])
        self.assertTrue(grade["automatic"]["passed"])
        self.assertTrue(review_grading.grade_episode(case, [clean], adjudication(grade))["quality"]["passed"])
        false_finding = {"location": {key: evidence[1][key] for key in ("path", "side", "snapshot", "start_line", "end_line")},
                         "trigger": {"input_json": '{"region":"domestic"}', "expected_json": "0", "actual_json": "499"},
                         "cause": "Domestic shipping should be free.", "impact": "Charges customers.", "evidence_indices": [1]}
        bad = answer(evidence, [false_finding])
        for conclusion in ("issues", "no-issues"):
            with self.subTest(conclusion=conclusion):
                bad["conclusion"] = conclusion
                grade = review_grading.grade_episode(case, [bad])
                self.assertIn("clean-case-false-positive", [item["code"] for item in grade["automatic"]["hard_errors"]])
                self.assertFalse(review_grading.grade_episode(case, [bad], adjudication(grade))["quality"]["passed"])

    def test_clean_answer_can_omit_final_quotes_after_semantic_review(self):
        case = self.prepare("clean-refactor")
        partial = [source_evidence(case, 0, "head", "shipping.py")]
        for evidence in ([], partial):
            with self.subTest(quoted_files=len(evidence)):
                supplied = answer(evidence)
                grade = review_grading.grade_episode(case, [supplied])
                self.assertFalse(grade["quality"]["passed"])
                self.assertEqual(grade["adjudication_status"], "pending")
                decision = adjudication(grade)
                decision["steps"][0]["resolutions"] = []
                reviewed = review_grading.grade_episode(case, [supplied], decision)
                self.assertTrue(reviewed["quality"]["passed"], reviewed["quality"])
                self.assertEqual(reviewed["quality"]["missing_evidence"], [])
                self.assertEqual(reviewed["automatic"]["hard_errors"], [])
                self.assertFalse(reviewed["automatic"]["source_delivery_observed"])

    def test_clean_adjudication_keeps_existing_alternative_evidence_resolutions(self):
        case = self.prepare("clean-refactor")
        supplied = answer([source_evidence(case, 0, "head", "shipping.py")])
        grade = review_grading.grade_episode(case, [supplied])
        decision = adjudication(grade)
        decision["steps"][0]["resolutions"] = [
            {"gap_id": "s0:evidence:" + identity, "evidence_indices": [0],
             "reason": "The reviewer checked both pinned implementations and request behavior; the accurate head quote need not reproduce every private anchor."}
            for identity in ("policy-preserved", "request-check")]
        reviewed = review_grading.grade_episode(case, [supplied], decision)
        self.assertTrue(reviewed["quality"]["passed"], reviewed["quality"])

    def test_clean_answer_still_rejects_inaccurate_supplied_evidence(self):
        case = self.prepare("clean-refactor")
        mutations = [("quote", "return 0\n"), ("end_line", 1), ("end_line", 999),
                     ("snapshot", "0" * 40), ("path", "../shipping.py")]
        for field, value in mutations:
            with self.subTest(field=field, value=value):
                item = source_evidence(case, 0, "head", "shipping.py")
                item[field] = value
                supplied = answer([item])
                grade = review_grading.grade_episode(case, [supplied])
                reviewed = review_grading.grade_episode(case, [supplied], adjudication(grade))
                self.assertFalse(reviewed["quality"]["passed"])
                self.assertIn("invalid-source-evidence", [item["code"] for item in reviewed["quality"]["regressions"]])
        supplied = answer([], retained=[{"step": 0, "index": 0, "side": "head",
                                         "snapshot": case["steps"][0]["head_tree"],
                                         "retention_proof": "The source was already inspected."}])
        grade = review_grading.grade_episode(case, [supplied])
        reviewed = review_grading.grade_episode(case, [supplied], adjudication(grade))
        self.assertFalse(reviewed["quality"]["passed"])
        self.assertIn("invalid-retained-evidence", [item["code"] for item in reviewed["quality"]["regressions"]])

    def test_clean_answer_without_quotes_still_obeys_the_schema(self):
        case = self.prepare("clean-refactor")
        missing_field = answer([])
        del missing_field["evidence"]
        wrong_type = answer([])
        wrong_type["evidence"] = "No issues."
        for supplied in (missing_field, wrong_type):
            with self.subTest(evidence=supplied.get("evidence")):
                grade = review_grading.grade_episode(case, [supplied])
                reviewed = review_grading.grade_episode(case, [supplied], adjudication(grade))
                self.assertFalse(reviewed["quality"]["passed"])
                self.assertIn("invalid-answer", [item["code"] for item in reviewed["quality"]["regressions"]])

    def test_clean_answer_without_quotes_needs_a_favorable_semantic_verdict(self):
        case = self.prepare("clean-refactor")
        supplied = answer([])
        grade = review_grading.grade_episode(case, [supplied])
        for verdict in ({"semantic_passed": False}, {"missing_evidence": ["The claimed production-path equivalence is unsupported."]}):
            with self.subTest(verdict=verdict):
                decision = adjudication(grade)
                decision["steps"][0].update(verdict)
                reviewed = review_grading.grade_episode(case, [supplied], decision)
                self.assertFalse(reviewed["quality"]["passed"])
                self.assertIn("semantic-review-failed", [item["code"] for item in reviewed["quality"]["regressions"]])

    def test_no_issues_answer_cannot_hide_a_real_defect(self):
        case = self.prepare("refund-boundary")
        supplied = answer([])
        grade = review_grading.grade_episode(case, [supplied])
        reviewed = review_grading.grade_episode(case, [supplied], adjudication(grade))
        self.assertFalse(reviewed["quality"]["passed"])
        self.assertIn("conclusion-mismatch", [item["code"] for item in reviewed["quality"]["regressions"]])
        self.assertTrue(any(gap.get("defect_id") == "inclusive-refund-day" for gap in reviewed["quality"]["missing_evidence"]))

    def test_clean_answer_without_quotes_must_substantiate_execution_claims(self):
        case = self.prepare("clean-refactor")
        supplied = answer([])
        supplied["validation"] = ["The shipping request assertions were run and passed."]
        grade = review_grading.grade_episode(case, [supplied])
        decision = adjudication(grade)
        decision["steps"][0]["execution_claims"] = "supported"
        with self.assertRaises(InvalidLedger):
            review_grading.grade_episode(case, [supplied], decision)
        decision["steps"][0]["execution_claims"] = "unsupported"
        rejected = review_grading.grade_episode(case, [supplied], decision)
        self.assertFalse(rejected["quality"]["passed"])
        self.assertIn("unsupported-test-execution-claim", [item["code"] for item in rejected["quality"]["regressions"]])
        executions = [{"step_id": 0, "command": ["python3", "-B", "test_shipping.py"], "exit_code": 0,
                       "trace_sha256": "1" * 64, "output_sha256": "2" * 64,
                       "output_excerpt": ""}]
        observed = review_grading.grade_episode(case, [supplied], execution_evidence=executions)
        decision = adjudication(observed)
        decision["steps"][0].update({"execution_claims": "supported", "execution_evidence_indices": [0],
                                     "resolutions": []})
        reviewed = review_grading.grade_episode(case, [supplied], decision, execution_evidence=executions)
        self.assertTrue(reviewed["quality"]["passed"], reviewed["quality"])
        self.assertTrue(reviewed["validation_execution_verified"])

    def test_missing_binding_is_distinct_from_correct_defect_identification(self):
        case = self.prepare("refund-boundary")
        supplied = refund_answer(case)
        supplied["evidence"].pop(2)
        supplied["findings"][0]["evidence_indices"] = [0, 1, 2]
        grade = review_grading.grade_episode(case, [supplied])
        self.assertEqual(len(grade["automatic"]["steps"][0]["known_true_findings"]), 1)
        self.assertTrue(any(gap.get("obligation_id") == "active-callers" for gap in grade["automatic"]["manual_gaps"]))
        self.assertFalse(review_grading.grade_episode(case, [supplied], adjudication(grade))["quality"]["passed"])

    def test_valid_alternative_witness_requires_explicit_semantic_resolution(self):
        case = self.prepare("refund-boundary")
        supplied = refund_answer(case)
        supplied["findings"][0]["trigger"]["input_json"] = '{"days":14,"channel":"web"}'
        grade = review_grading.grade_episode(case, [supplied])
        self.assertFalse(grade["automatic"]["passed"])
        self.assertEqual(grade["automatic"]["hard_errors"], [])
        decision = adjudication(grade)
        decision["steps"][0]["resolutions"] = [{"gap_id": gap["gap_id"], "reason": "The extra channel identifies the evidenced web entrypoint; day 14 still reaches the same policy.",
                                                "evidence_indices": [0, 1, 2]} for gap in grade["automatic"]["manual_gaps"]]
        self.assertTrue(review_grading.grade_episode(case, [supplied], decision)["quality"]["passed"])

    def test_field_projected_units_witness_requires_semantic_resolution(self):
        case = self.prepare("wire-units-noise")
        supplied = checkout_answer(case)
        grade = review_grading.grade_episode(case, [supplied])
        self.assertEqual(grade["automatic"]["hard_errors"], [])
        self.assertFalse(grade["automatic"]["passed"])
        self.assertFalse(grade["quality"]["passed"])
        decision = adjudication(grade)
        decision["steps"][0]["resolutions"] = [
            {"gap_id": gap["gap_id"], "reason": "The finding explicitly scopes both numeric results to amount_cents. The two-item route calculates 2500 cents; old serializer preserves it and new serializer writes 25. Currency remains EUR in both quoted serializers.",
             "evidence_indices": list(range(5))} for gap in grade["automatic"]["manual_gaps"]]
        self.assertTrue(review_grading.grade_episode(case, [supplied], decision)["quality"]["passed"])

    def test_wrong_units_results_remain_rejectable_for_projected_and_full_responses(self):
        case = self.prepare("wire-units-noise")
        supplied = checkout_answer(case)
        supplied["findings"][0]["trigger"]["actual_json"] = "250"
        grade = review_grading.grade_episode(case, [supplied])
        self.assertFalse(grade["quality"]["passed"])
        decision = adjudication(grade, passed=False)
        decision["steps"][0].update({"unsupported_finding_indices": [0],
                                     "notes": "The cited serializer yields amount_cents=25, not the claimed 250; a projected output still needs a true scoped result."})
        self.assertFalse(review_grading.grade_episode(case, [supplied], decision)["quality"]["passed"])
        supplied["findings"][0]["trigger"].update({"expected_json": '{"amount_cents":2500,"currency":"EUR"}',
                                                   "actual_json": '{"amount_cents":250,"currency":"EUR"}'})
        grade = review_grading.grade_episode(case, [supplied])
        self.assertTrue(any("known-domain" in error["detail"] for error in grade["automatic"]["hard_errors"]))
        self.assertFalse(review_grading.grade_episode(case, [supplied], adjudication(grade))["quality"]["passed"])

    def test_followup_preserves_valid_evidence_and_rejects_stale_source(self):
        case = self.prepare("review-followup")
        evidence = [source_evidence(case, 0, side, path) for side, path in (
            ("head", "api.py"), ("base", "percentage.py"), ("head", "percentage.py"),
            ("head", "policy.py"), ("head", "test_storage.py"))]
        finding = {"location": {"path": "percentage.py", "side": "head", "snapshot": case["steps"][0]["head_tree"], "start_line": 4, "end_line": 4},
                   "trigger": {"input_json": '{"used":895,"capacity":1000}', "expected_json": '"clear"', "actual_json": '"warning"'},
                   "cause": "Ceiling rounds 89.5 percent to 90 before comparison.", "impact": "Storage warnings appear early.", "evidence_indices": list(range(5))}
        first = answer(evidence, [finding])
        review_cases.activate_step(case, 1)
        retained = [{"step": 0, "index": item, "side": "head", "snapshot": case["steps"][1]["head_tree"],
                     "retention_proof": "The exact Git tree comparison changes only percentage.py; these files have identical blobs."}
                    for item in (0, 3, 4)]
        second = answer([source_evidence(case, 1, "head", "percentage.py")], retained=retained)
        grade = review_grading.grade_episode(case, [first, second])
        self.assertTrue(grade["automatic"]["passed"])
        self.assertTrue(review_grading.grade_episode(case, [first, second], adjudication(grade))["quality"]["passed"])
        one_based_origin = copy.deepcopy(second)
        for item in one_based_origin["retained_evidence"]:
            item["step"] = 1
        invalid = review_grading.grade_episode(case, [first, one_based_origin])
        self.assertIn("invalid-retained-evidence", [item["code"] for item in invalid["automatic"]["hard_errors"]])
        self.assertFalse(review_grading.grade_episode(case, [first, one_based_origin], adjudication(invalid))["quality"]["passed"])
        second["retained_evidence"].append({"step": 0, "index": 2, "side": "base", "snapshot": case["steps"][1]["base_tree"],
                                            "retention_proof": "The previous head tree is exactly the current base tree."})
        historical = review_grading.grade_episode(case, [first, second])
        self.assertTrue(historical["automatic"]["passed"])
        self.assertEqual(historical["automatic"]["steps"][1]["evidence"][-1]["side"], "base")
        self.assertEqual(historical["automatic"]["steps"][1]["evidence"][-1]["origin_snapshot"], case["steps"][0]["head_tree"])
        second["retained_evidence"].append({"step": 0, "index": 2, "side": "head", "snapshot": case["steps"][1]["head_tree"],
                                            "retention_proof": "Assume the old helper is still correct."})
        grade = review_grading.grade_episode(case, [first, second])
        self.assertIn("invalid-retained-evidence", [item["code"] for item in grade["automatic"]["hard_errors"]])
        incomplete = review_grading.grade_episode(case, [first])
        self.assertIn("incomplete-episode", [item["code"] for item in incomplete["automatic"]["hard_errors"]])

    def test_adjudication_and_oracle_are_bound_to_exact_answers(self):
        case = self.prepare("refund-boundary")
        answers = [refund_answer(case)]
        grade = review_grading.grade_episode(case, answers)
        decision = adjudication(grade)
        altered = copy.deepcopy(answers)
        altered[0]["findings"][0]["impact"] += " Added claim."
        with self.assertRaises(InvalidLedger):
            review_grading.grade_episode(case, altered, decision)
        oracle = Path(case["private_oracle_path"])
        data = json.loads(oracle.read_text())
        data["steps"][0]["conclusion"] = "no-issues"
        oracle.write_text(json.dumps(data))
        with self.assertRaises(InvalidLedger):
            review_grading.grade_episode(case, answers)

    def test_test_execution_claims_need_observed_bound_provenance(self):
        case = self.prepare("refund-boundary")
        supplied = refund_answer(case)
        supplied["validation"] = ["The request-level test was run and failed on day 14."]
        grade = review_grading.grade_episode(case, [supplied])
        decision = adjudication(grade)
        decision["steps"][0]["execution_claims"] = "supported"
        with self.assertRaises(InvalidLedger):
            review_grading.grade_episode(case, [supplied], decision)
        evidence = [{"step_id": 0, "command": ["python3", "-B", "test_refund.py"], "exit_code": 1,
                     "trace_sha256": "1" * 64, "output_sha256": "2" * 64,
                     "output_excerpt": "AssertionError: day-14 request returned allowed=false"}]
        observed = review_grading.grade_episode(case, [supplied], execution_evidence=evidence)
        self.assertNotEqual(observed["packet_id"], grade["packet_id"])
        decision = adjudication(observed)
        decision["steps"][0].update({"execution_claims": "supported", "execution_evidence_indices": [0]})
        verified = review_grading.grade_episode(case, [supplied], decision, execution_evidence=evidence)
        self.assertTrue(verified["quality"]["passed"])
        self.assertTrue(verified["validation_execution_verified"])
        decision["steps"][0]["execution_claims"] = "unsupported"
        self.assertFalse(review_grading.grade_episode(case, [supplied], decision, execution_evidence=evidence)["quality"]["passed"])


if __name__ == "__main__":
    unittest.main()
