import json
from contextlib import contextmanager
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from accounting import InvalidLedger, fingerprint, read_json
import review_campaign as campaign
import review_export as exporting
from test_review_campaign import outcome, prepared


def measured(plan, assignment, status="completed", *, tokens=100, quality_pass=True, pending=False,
             basis="exec-emitted-initial-turn-usage", comparable=True):
    usage = {"input_tokens": tokens, "cached_input_tokens": 10, "output_tokens": 20,
             "reasoning_output_tokens": None, "total_tokens": None}
    packet = fingerprint(assignment["run_id"])
    grade = {"schema": 1, "packet_id": packet, "answer_sha256": fingerprint([{"synthetic": True}]),
             "automatic": {"passed": quality_pass, "regressions": [], "missing_evidence": []},
             "adjudication_status": "pending" if pending else "accepted",
             "quality": {"passed": quality_pass and not pending,
                         "regressions": [] if quality_pass else ["observed-review-failure"],
                         "missing_evidence": [], "evidence_sha256": fingerprint("quality")},
             "adjudication_packet": {"packet_id": packet, "answer_sha256": fingerprint([{"synthetic": True}]),
                                     "tasks": ["Public review task"], "oracle_rubrics": [], "answers": [{"synthetic": True}]}}
    accounting = {"schema": 1, "adapter": "codex-exec-jsonl-v1", "kind": "codex-exec-episode",
                  "invocation_count": assignment["step_count"], "usage_basis": basis, "observed_usage": usage,
                  "observed_input_plus_output_tokens": tokens + 20, "comparable_usage": comparable,
                  "comparable_fields": ["input_tokens", "cached_input_tokens", "output_tokens"] if comparable else [],
                  "usage_complete": False, "unknown_fields": ["provider_call_ids"], "episode_errors": [],
                  "money": {"provider_charge": None}, "invocations": []}
    invocation = {"step_id": 0, "prompt_hashes": {"task_sha256": fingerprint("shared-task")},
                  "result": {"status": status, "cli_version": "codex-cli test",
                             "process_tree_drained": True,
                             "stdout_path": str(Path(assignment["run_dir"]) / "step-0/invocation/stdout.jsonl"),
                             "answer_path": str(Path(assignment["run_dir"]) / "step-0/invocation/answer.json"),
                             "thread_id": "private-session-" + assignment["run_id"]}}
    invocations = []
    for index in range(assignment["step_count"]):
        result = {**invocation["result"],
                  "stdout_path": str(Path(assignment["run_dir"]) / f"step-{index}/invocation/stdout.jsonl"),
                  "answer_path": str(Path(assignment["run_dir"]) / f"step-{index}/invocation/answer.json")}
        invocations.append({**invocation, "step_id": index, "result": result})
    return outcome(plan, assignment, status, accounting=accounting, quality=grade,
                   invocations=invocations, wall_seconds=3.0)


class ReportingTests(unittest.TestCase):
    def test_real_input_receipt_and_planned_identities_export_on_network_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            assignment = plan["assignments"][0]
            digest = fingerprint("pinned input")
            identities = {"workspace": digest, "codex": digest, "codex_runtime": digest, "controller_ca": digest,
                          "reposcout": None, "skill": None}
            assignment["configuration"] = {"expected_inputs": identities}
            assignment["configuration_sha256"] = fingerprint(assignment["configuration"])
            plan.pop("plan_sha256")
            plan["plan_sha256"] = fingerprint(plan)
            (root / "plan.json").write_text(json.dumps(plan))
            failed = outcome(plan, assignment, "failed", invocations=[{"step_id": 0, "prompt_hashes": {}, "result": {
                "status": "failed", "termination_reason": "network-before-task", "process_tree_drained": True,
                "input_receipt": {name: {"sha256": digest, "expected_sha256": digest, "matched": True}
                                  for name in ("workspace", "codex", "codex_runtime", "controller_ca")}
                                 | {"codex_runtime_assets": {"bin/codex": digest, "bin/code-mode-host": digest}}
            }}])
            campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", failed)
            destination = Path(temporary) / "export"
            exporting.export(root, destination)
            stored = read_json(destination / "results.json")
            run = stored["runs"][0]
            self.assertEqual(stored["status_counts"], {"failed": 1, "notrun": 7})
            self.assertEqual(run["invocations"][0]["result"]["input_receipt"]["workspace_identity"]["sha256"], digest)
            self.assertEqual(run["configuration"]["expected_inputs"]["workspace_identity"], digest)
            self.assertEqual(run["invocations"][0]["result"]["input_receipt"]["controller_ca"]["sha256"], digest)
            self.assertNotIn("controller_ca_file", stored["pins"])
            self.assertNotIn("workspace", run["invocations"][0]["result"]["input_receipt"])
            self.assertNotIn("workspace", run["configuration"]["expected_inputs"])
            self.assertIsNone(run["accounting"])
            exporting.safe_projection(stored)

    def test_input_identity_projection_cannot_hide_raw_receipt_fields(self):
        for value in ({"workspace": "/tmp/private/source"}, {"workspace": {"sha256": "0" * 64, "expected_sha256": None,
                      "matched": True, "command": "raw"}}, {"stdout": "raw"},
                      {"codex_runtime_assets": {"/home/person/private": "0" * 64}}):
            with self.assertRaises(InvalidLedger):
                exporting.project_input_receipt(value)

    def test_all_assigned_outcomes_and_failures_survive_export(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            assignment = plan["assignments"][0]
            campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", measured(
                plan, assignment, "failed", tokens=77, quality_pass=False, comparable=False))
            report = exporting.report(root)
            self.assertEqual(report["assignment_count"], 8)
            self.assertEqual(report["status_counts"], {"failed": 1, "notrun": 7})
            self.assertEqual(len(report["pair_inventory"]), 4)
            self.assertEqual(report["conditional_quality_matched_pairs"], [])
            costs = next(group for group in report["all_assigned_run_costs"] if group["usage_basis"] != "unavailable")
            self.assertEqual(costs["observed_token_known_sums"]["input_tokens"], 77)
            self.assertEqual(costs["status_counts"], {"failed": 1})
            self.assertIsNone(costs["provider_charge"])
            destination = Path(temporary) / "export"
            exporting.export(root, destination)
            stored = read_json(destination / "results.json")
            self.assertEqual(stored["status_counts"], report["status_counts"])
            self.assertNotIn("/tmp/private", json.dumps(stored))
            self.assertNotIn("private-session", json.dumps(stored))
            integrity = read_json(destination / "integrity.json")
            self.assertEqual(integrity["results_canonical_json_sha256"], fingerprint(stored))

    def test_failed_episode_exports_known_partial_costs_without_complete_totals(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            assignment = next(item for item in plan["assignments"] if item["step_count"] == 2)
            failed = measured(plan, assignment, "failed", tokens=77, quality_pass=False, comparable=False)
            accounting = failed["accounting"]
            accounting["observed_usage"] = dict.fromkeys(exporting.USAGE_FIELDS)
            accounting["known_usage_prefix"] = {
                "basis": "normalized-nonoverlapping-invocations; not-a-complete-episode-total",
                "invocation_count": 1, "stopped_at_invocation": 1,
                "stop_reasons": ["episode-thread-or-run-mismatch"],
                "observed_usage": {**dict.fromkeys(exporting.USAGE_FIELDS), "input_tokens": 77,
                                   "cached_input_tokens": 10, "cache_write_input_tokens": 0, "output_tokens": 20},
                "field_invocation_indices": {field: [0] if field in exporting.REQUESTED_TOKEN_FIELDS else []
                                             for field in exporting.USAGE_FIELDS},
            }
            campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", failed)
            stored = exporting.report(root)
            costs = next(group for group in stored["all_assigned_run_costs"] if group["usage_basis"] != "unavailable")
            self.assertIsNone(costs["observed_token_known_sums"]["input_tokens"])
            self.assertEqual(costs.get("observed_partial_token_known_sums", {}).get("input_tokens"), 77)
            self.assertEqual(costs["observed_partial_token_known_sums"]["cache_write_input_tokens"], 0)
            self.assertIsNone(costs["observed_partial_token_known_sums"]["total_tokens"])
            self.assertEqual(costs["partial_token_known_run_counts"]["input_tokens"], 1)
            self.assertEqual(costs["token_known_run_counts"]["input_tokens"], 0)
            exported = next(item for item in stored["runs"] if item["run_id"] == assignment["run_id"])
            self.assertEqual(exported["accounting"]["known_usage_prefix"], accounting["known_usage_prefix"])
            self.assertEqual(stored["conditional_quality_matched_pairs"], [])
            # A field already represented by a whole episode must not also enter its partial sum.
            exported["accounting"]["observed_usage"]["input_tokens"] = 77
            costs = exporting.all_run_costs([exported])[0]
            self.assertEqual(costs["observed_token_known_sums"]["input_tokens"], 77)
            self.assertIsNone(costs["observed_partial_token_known_sums"]["input_tokens"])
            self.assertEqual(costs["partial_token_known_run_counts"]["input_tokens"], 0)

    def test_incomplete_pairs_are_visible_and_not_eligible(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            assignment = plan["assignments"][0]
            campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", measured(plan, assignment))
            report = exporting.report(root)
            self.assertEqual(report["incomplete_main_pairs"], 4)
            comparison = next(item for item in report["pair_inventory"] if item["pair_id"] == assignment["pair_id"])
            self.assertFalse(comparison["eligible"])
            self.assertTrue(any("notrun" in reason for reason in comparison["ineligibility_reasons"]))

    def test_failed_main_arm_is_incomplete_while_cli_only_failure_is_separate(self):
        for failed_variant in ("baseline", "reposcout", "reposcout-cli"):
            with self.subTest(failed_variant=failed_variant), tempfile.TemporaryDirectory() as temporary:
                root, plan = prepared(temporary, "exploratory", ablation=True)
                failed = next(item for item in plan["assignments"]
                              if item["case_id"] == "review-followup" and item["variant"] == failed_variant)
                for assignment in plan["assignments"]:
                    value = measured(plan, assignment, "failed" if assignment == failed else "completed")
                    if assignment == failed:
                        value["invocations"] = value["invocations"][:1]
                    campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", value)
                report = exporting.report(root)
                self.assertEqual(report["status_counts"], {"completed": 59, "failed": 1})
                self.assertEqual(report["incomplete_main_pairs"], 0 if failed_variant == "reposcout-cli" else 1)
                comparison = next(item for item in report["pair_inventory"]
                                  if item["pair_id"] == failed["pair_id"] and item["candidate_variant"] == "reposcout")
                if failed_variant != "reposcout-cli":
                    self.assertFalse(comparison["eligible"])
                    self.assertIn(failed_variant + ":failed", comparison["ineligibility_reasons"])

    def test_runtime_hashes_and_private_failure_digest_export_without_runtime_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            assignment = plan["assignments"][0]
            asset = fingerprint("Node executable")
            plan["pins"].update(runtime_tool_paths={"node": "/home/private/selected/node"},
                                runtime_tool_assets={"node": asset}, runtime_tools_sha256=fingerprint({"node": asset}))
            plan.pop("plan_sha256")
            plan["plan_sha256"] = fingerprint(plan)
            (root / "plan.json").write_text(json.dumps(plan))
            error = campaign._safe_error(ValueError("private exception text /home/private/credential"),
                                         phase="runner", step_id=0, private_directory=Path(assignment["run_dir"]))
            value = measured(plan, assignment, "failed")
            value["errors"] = [error]
            value["invocations"][0]["result"]["input_receipt"] = {
                "runtime_tools": {"sha256": plan["pins"]["runtime_tools_sha256"],
                                  "expected_sha256": plan["pins"]["runtime_tools_sha256"], "matched": True},
                "runtime_tool_assets": {"node": asset}}
            campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", value)
            report = exporting.report(root)
            self.assertEqual(report["pins"]["runtime_tool_assets"], {"node": asset})
            self.assertEqual(report["runs"][0]["errors"][0]["private_evidence_sha256"], error["private_evidence_sha256"])
            self.assertNotIn("runtime_tool_paths", report["pins"])
            self.assertNotIn("/home/private", json.dumps(report))
            self.assertNotIn("private exception text", json.dumps(report))
            private = read_json(Path(assignment["run_dir"]) / "failures" / (error["private_evidence_sha256"] + ".json"))
            self.assertEqual((private["phase"], private["error_type"]), ("runner", "ValueError"))
            self.assertNotIn("private exception text", json.dumps(private))

    def test_only_adjudicated_quality_matched_comparable_pairs_get_deltas(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            pair = plan["assignments"][0]["pair_id"]
            for assignment in (run for run in plan["assignments"] if run["pair_id"] == pair):
                tokens = 140 if assignment["variant"] == "baseline" else 100
                campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", measured(plan, assignment, tokens=tokens))
            report = exporting.report(root)
            selected = report["conditional_quality_matched_pairs"]
            self.assertEqual(len(selected), 1)
            self.assertEqual(selected[0]["token_deltas"]["input_tokens"], -40)
            self.assertEqual(selected[0]["token_deltas"]["output_tokens"], 0)
            self.assertNotIn("input_plus_output_tokens", selected[0]["token_deltas"])
            self.assertNotIn("reasoning_output_tokens", selected[0]["token_deltas"])
            self.assertFalse(selected[0]["all_requested_token_fields_comparable"])
            fields = selected[0]["field_comparisons"]
            self.assertTrue(fields["cached_input_tokens"]["eligible"])
            self.assertFalse(fields["cache_write_input_tokens"]["eligible"])
            self.assertIsNone(fields["cache_write_input_tokens"]["delta"])

    def test_reports_and_exports_refuse_active_or_stale_campaign_lease(self):
        for stale in (False, True):
            with self.subTest(stale=stale), tempfile.TemporaryDirectory() as temporary:
                root, plan = prepared(temporary)
                assignment = plan["assignments"][0]
                campaign.write_new(Path(assignment["run_dir"]) / "started.json", {
                    "run_id": assignment["run_id"], "plan_sha256": plan["plan_sha256"]})
                if stale:
                    (root / "controller.lock").write_text('{"pid":123,"created_unix":0}')
                    lease = None
                else:
                    lease = campaign.campaign_lock(root)
                    lease.__enter__()
                original = (root / "controller.lock").read_bytes()
                try:
                    with self.assertRaises(FileExistsError):
                        exporting.report(root)
                    destination = Path(temporary) / "export"
                    with self.assertRaises(FileExistsError):
                        exporting.export(root, destination)
                    self.assertFalse(destination.exists())
                    self.assertEqual((root / "controller.lock").read_bytes(), original)
                finally:
                    if lease is not None:
                        lease.__exit__(None, None, None)

    def test_cache_write_zero_is_comparable_but_one_missing_arm_remains_unknown(self):
        for both_known in (False, True):
            with self.subTest(both_known=both_known), tempfile.TemporaryDirectory() as temporary:
                root, plan = prepared(temporary)
                pair = plan["assignments"][0]["pair_id"]
                for assignment in (item for item in plan["assignments"] if item["pair_id"] == pair):
                    value = measured(plan, assignment)
                    if both_known or assignment["variant"] == "baseline":
                        value["accounting"]["observed_usage"]["cache_write_input_tokens"] = 0
                        value["accounting"]["comparable_fields"].append("cache_write_input_tokens")
                    campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", value)
                comparison = exporting.report(root)["conditional_quality_matched_pairs"][0]
                field = comparison["field_comparisons"]["cache_write_input_tokens"]
                self.assertEqual(field["eligible"], both_known)
                self.assertEqual(field["delta"], 0 if both_known else None)
                self.assertEqual(comparison["all_requested_token_fields_comparable"], both_known)

    def test_public_report_contains_no_duration_outcome_or_aggregate(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            assignment = plan["assignments"][0]
            value = measured(plan, assignment)
            value["invocations"][0]["result"].update(wall_seconds=3, elapsed_seconds=4, duration_ms=3000)
            campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", value)
            report = exporting.report(root)
            encoded = json.dumps(report)
            for field in ("wall_seconds", "elapsed_seconds", "duration_ms", "wall_seconds_known_sum"):
                self.assertNotIn('"' + field + '"', encoded)

    def test_pending_adjudication_and_usage_basis_drift_exclude_pairs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            pair = plan["assignments"][0]["pair_id"]
            for assignment in (run for run in plan["assignments"] if run["pair_id"] == pair):
                campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", measured(
                    plan, assignment, pending=assignment["variant"] == "baseline",
                    basis="exec-turn-usage" if assignment["variant"] == "reposcout" else "exec-emitted-initial-turn-usage"))
            report = exporting.report(root)
            comparison = next(item for item in report["pair_inventory"] if item["pair_id"] == pair)
            self.assertFalse(comparison["eligible"])
            self.assertIn("usage-basis-differs", comparison["ineligibility_reasons"])
            self.assertIn("baseline:quality-pending-adjudication", comparison["ineligibility_reasons"])
            self.assertEqual(report["quality_counts"]["pending-adjudication"], 1)

    def test_unknown_adjudication_cannot_attach_to_another_answer(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, _ = prepared(temporary)
            path = Path(temporary) / "adjudications.json"
            path.write_text(json.dumps([{"packet_id": "foreign", "answer_sha256": "0" * 64}]))
            with self.assertRaises(InvalidLedger):
                exporting.report(root, path)

    def test_sanitizer_refuses_raw_fields_and_host_paths(self):
        for value in ({"stdout": "raw"}, {"error": "/home/person/private.log"}, {"nested": [{"prompt": "raw"}]}):
            with self.assertRaises(InvalidLedger):
                exporting.safe_projection(value)
        exporting.safe_projection({"sandbox_home": "/home/eval/.config", "unknown_cost": None})

    def test_human_packets_have_no_arm_or_cost_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            for assignment in plan["assignments"][:2]:
                campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", measured(plan, assignment))
            destination = Path(temporary) / "packets"
            result = exporting.packets(root, destination)
            self.assertEqual(result["packet_count"], 2)
            for path in destination.glob("*.json"):
                packet = read_json(path)
                self.assertNotIn("variant", packet)
                self.assertNotIn("run_id", packet)
                self.assertNotIn("cost", packet)
                self.assertNotIn("usage", packet)
                self.assertIn("answers", packet)

    def test_post_export_cleanup_preserves_private_evidence_and_unrelated_data(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            unrelated = Path(temporary) / "user-data.txt"
            unrelated.write_text("preserve")
            exported = Path(temporary) / "export"
            exporting.export(root, exported)
            results_hash = read_json(exported / "integrity.json")["results_canonical_json_sha256"]
            result = campaign.cleanup_public(root, exported, results_hash)
            self.assertEqual(result["removed_public_directories"], 16)
            self.assertEqual(unrelated.read_text(), "preserve")
            for assignment in plan["assignments"]:
                directory = Path(assignment["run_dir"])
                self.assertFalse((directory / "workspace").exists())
                self.assertFalse((directory / "controller").exists())
                self.assertTrue((directory / "oracle/oracle.json").exists())
            with self.assertRaises(InvalidLedger):
                campaign.run(root, preflight_only=True, check_pins=False, runner=lambda spec: self.fail("retired inputs used"))

    def test_cleanup_refuses_replaced_target_before_removing_any_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            exported = Path(temporary) / "export"
            exporting.export(root, exported)
            results_hash = read_json(exported / "integrity.json")["results_canonical_json_sha256"]
            controller = Path(plan["assignments"][0]["run_dir"]) / "controller"
            controller.rename(controller.with_name("original-controller"))
            controller.mkdir(mode=0o700)
            sentinel = controller / "unrelated.txt"
            sentinel.write_text("replacement data")
            with self.assertRaises(InvalidLedger):
                campaign.cleanup_public(root, exported, results_hash)
            self.assertEqual(sentinel.read_text(), "replacement data")
            self.assertTrue((Path(plan["assignments"][0]["run_dir"]) / "workspace").exists())
            self.assertFalse((root / "cleanup.json").exists())

    def test_cleanup_refuses_undrained_unknown_or_missing_preflight_receipt(self):
        for drained in (False, None, "missing"):
            with self.subTest(drained=drained), tempfile.TemporaryDirectory() as temporary:
                root, plan = prepared(temporary)
                run = Path(plan["assignments"][0]["run_dir"])
                probe = run / "preflight-synthetic"
                probe.mkdir()
                if drained != "missing":
                    campaign.write_new(probe / "result.json", {"process_tree_drained": drained})
                destination = Path(temporary) / "export"
                receipt = exporting.export(root, destination)
                with self.assertRaisesRegex(InvalidLedger, "preflight"):
                    campaign.cleanup_public(root, destination, receipt["results_canonical_json_sha256"])
                self.assertTrue((run / "controller").is_dir())
                self.assertTrue((run / "workspace").is_dir())
                self.assertFalse((root / "cleanup.json").exists())

    def test_cleanup_compares_export_inventory_after_acquiring_lease(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            assignment = plan["assignments"][0]
            destination = Path(temporary) / "export"
            receipt = exporting.export(root, destination)
            original = campaign.campaign_lock

            @contextmanager
            def completed_before_acquisition(path):
                campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", outcome(plan, assignment))
                with original(path):
                    yield

            with patch("review_campaign.campaign_lock", new=completed_before_acquisition):
                with self.assertRaisesRegex(InvalidLedger, "outcomes changed"):
                    campaign.cleanup_public(root, destination, receipt["results_canonical_json_sha256"])
            self.assertTrue((Path(assignment["run_dir"]) / "controller").is_dir())
            self.assertFalse((root / "cleanup.json").exists())

    def test_cleanup_unlinks_owned_native_links_without_following_external_targets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            outside = Path(temporary) / "external-data"
            outside.mkdir()
            sentinel = outside / "valuable.txt"
            sentinel.write_text("preserve external target")
            controller = Path(plan["assignments"][0]["run_dir"]) / "controller"
            aliases = controller / "tmp/arg0/native"
            aliases.mkdir(parents=True)
            for name in ("codex-execve-wrapper", "codex-linux-sandbox", "applypatch", "apply_patch"):
                (aliases / name).symlink_to(sentinel)
            (aliases / "external-directory").symlink_to(outside, target_is_directory=True)
            exported = Path(temporary) / "export"
            exporting.export(root, exported)
            digest = read_json(exported / "integrity.json")["results_canonical_json_sha256"]
            campaign.cleanup_public(root, exported, digest)
            self.assertFalse(controller.exists())
            self.assertEqual(sentinel.read_text(), "preserve external target")
            self.assertEqual(list(outside.iterdir()), [sentinel])

    def test_cleanup_refuses_root_alias_and_mounted_directory(self):
        for alias in (True, False):
            with self.subTest(alias=alias), tempfile.TemporaryDirectory() as temporary:
                root, plan = prepared(temporary)
                controller = Path(plan["assignments"][0]["run_dir"]) / "controller"
                external = Path(temporary) / "external-data"
                external.mkdir()
                sentinel = external / "valuable.txt"
                sentinel.write_text("preserve")
                exported = Path(temporary) / "export"
                exporting.export(root, exported)
                digest = read_json(exported / "integrity.json")["results_canonical_json_sha256"]
                if alias:
                    controller.rename(controller.with_name("original-controller"))
                    controller.symlink_to(external, target_is_directory=True)
                    with self.assertRaises(InvalidLedger):
                        campaign.cleanup_public(root, exported, digest)
                else:
                    mounted = controller / "mounted-data"
                    mounted.mkdir()
                    with patch.object(campaign.os.path, "ismount", side_effect=lambda path: Path(path) == mounted):
                        with self.assertRaises(InvalidLedger):
                            campaign.cleanup_public(root, exported, digest)
                self.assertEqual(sentinel.read_text(), "preserve")
                self.assertTrue((Path(plan["assignments"][0]["run_dir"]) / "workspace").exists())
                self.assertFalse((root / "cleanup.json").exists())


if __name__ == "__main__":
    unittest.main()
