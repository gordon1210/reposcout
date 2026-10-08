import io
import json
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

from accounting import InvalidLedger, fingerprint, read_json
import review_campaign as campaign


def catalog():
    return [{"case_id": name, "partition": "main", "title": name,
             "step_count": 2 if name == "review-followup" else 1,
             "fixture_sha256": fingerprint(name)} for name in campaign.MAIN_CASES]


def pins(timeout_seconds=campaign.LIMITS["timeout_seconds"]):
    return {"limits": {**campaign.LIMITS, "timeout_seconds": timeout_seconds}, "usage_scope": "unknown", "codex_version": "codex-cli test",
            "codex_binary": "/unexecuted/codex", "reposcout_binary": "/unexecuted/reposcout",
            "skill_dir": "/unexecuted/skill", "codex_sha256": fingerprint("codex"),
            "codex_runtime_sha256": fingerprint("runtime"), "reposcout_sha256": fingerprint("reposcout"),
            "skill": {"sha256": fingerprint("skill")}, "controller_ca_file": "/unexecuted/ca.pem",
            "controller_ca_sha256": fingerprint("public-ca"), "controller_ca_bytes": 123,
            "controller_ca_destination": "/etc/ssl/cert.pem"}


def prepared(parent, stage="smoke", ablation=False, timeout_seconds=campaign.LIMITS["timeout_seconds"]):
    """Make controller-owned synthetic records without Git, probes or model execution."""
    root, marker = campaign.create_campaign_root(Path(parent) / "campaign")
    plan = campaign.build_plan(stage, 917, pins(timeout_seconds), catalog(), ablation)
    for assignment in plan["assignments"]:
        directory = root / "runs" / assignment["run_id"]
        directory.mkdir(mode=0o700)
        identity = directory.stat()
        campaign.write_new(directory / "ownership.json", {"campaign_token": marker["token"],
                           "run_id": assignment["run_id"], "root": str(directory),
                           "device": identity.st_dev, "inode": identity.st_ino, "owner": os.getuid()})
        workspace = directory / "workspace"
        oracle = directory / "oracle"
        workspace.mkdir()
        oracle.mkdir()
        (directory / "controller").mkdir(mode=0o700)
        (workspace / "README.md").write_text("synthetic fixture\n")
        oracle_body = {"private": "synthetic", "snapshots": {"base": {}, "head": {"README.md": "synthetic fixture\n"}}}
        campaign.write_new(oracle / "oracle.json", oracle_body)
        steps = [{"step_id": index, "task": "Review the supplied comparison.", "base_tree": "a" * 40,
                  "head_tree": "b" * 40, "source_sha256": fingerprint(index), "head_revision": "head"}
                 for index in range(assignment["step_count"])]
        record = {"case_id": assignment["case_id"], "workspace": str(workspace),
                  "private_oracle_path": str(oracle / "oracle.json"), "steps": steps, "active_step": 0,
                  "public_workspace_sha256": campaign.directory_hash(workspace)["sha256"]}
        campaign.write_new(directory / "case.json", record)
        assignment.update({"run_dir": str(directory), "oracle_sha256": fingerprint(oracle_body),
                           "public_workspace_sha256": record["public_workspace_sha256"],
                           "disposable_identities": {name: {"device": (directory / name).stat().st_dev,
                                                            "inode": (directory / name).stat().st_ino}
                                                     for name in ("workspace", "controller")},
                           "initial_prompt": {"task_sha256": fingerprint("task")}})
    plan.pop("plan_sha256")
    plan["plan_sha256"] = fingerprint(plan)
    campaign.write_new(root / "plan.json", plan)
    return root, plan


def outcome(plan, assignment, status="failed", **values):
    return {"schema": 1, "plan_sha256": plan["plan_sha256"], "run_id": assignment["run_id"],
            "status": status, "accounting": None, "quality": None, "invocations": [],
            "answers": [], "errors": [], **values}


def fake_spec(plan, assignment, record, step, artifact_dir, resumed=None, timeout=None, usage_baseline=None):
    return SimpleNamespace(run_id=assignment["run_id"], prompt="public task", artifact_dir=Path(artifact_dir),
                           resume_thread_id=resumed, timeout_seconds=timeout, usage_scope=plan["pins"]["usage_scope"],
                           usage_baseline=usage_baseline), {"task_sha256": fingerprint(step["task"])}


class AssignmentTests(unittest.TestCase):
    def test_smoke_has_exact_preselected_four_cases_and_eight_runs(self):
        plan = campaign.build_plan("smoke", 4, {}, catalog())
        self.assertEqual(plan["assignment_count"], 8)
        self.assertEqual(sum(run["step_count"] for run in plan["assignments"]), 10)
        self.assertEqual({run["case_id"] for run in plan["assignments"]}, set(campaign.SMOKE_CASES))
        self.assertEqual(sum(run["variant"] == "baseline" and run["pair_position"] == 0
                             for run in plan["assignments"]), 2)

    def test_repeat_and_stratum_balance_with_interleaved_ablation(self):
        plan = campaign.build_plan("exploratory", 94, {}, catalog(), True)
        runs = plan["assignments"]
        self.assertEqual(len(runs), 60)
        self.assertEqual(sum(run["variant"] == "reposcout-cli" for run in runs), 12)
        for repeat in range(1, 4):
            selected = [run for run in runs if run["repeat_id"] == repeat]
            pairs = {run["pair_id"] for run in selected}
            self.assertEqual(len(pairs), 8)
            baseline_first = 0
            for pair in pairs:
                main = [run for run in selected if run["pair_id"] == pair and run["variant"] != "reposcout-cli"]
                baseline_first += main[0]["variant"] == "baseline"
            self.assertEqual(baseline_first, 4)
        for name in campaign.MAIN_CASES:
            pairs = [run for run in runs if run["case_id"] == name and run["variant"] == "baseline"]
            self.assertEqual({run["repeat_id"] for run in pairs}, {1, 2, 3})
            first = 0
            for run in pairs:
                main = [item for item in runs if item["pair_id"] == run["pair_id"] and item["variant"] != "reposcout-cli"]
                first += main[0]["variant"] == "baseline"
            self.assertIn(first, (1, 2))
        self.assertEqual({run["case_id"] for run in runs if run["variant"] == "reposcout-cli"}, set(campaign.ABLATION_CASES))

    def test_seed_is_stable_and_ids_do_not_reveal_treatment_or_case(self):
        first = campaign.build_plan("exploratory", 11, {}, catalog())
        self.assertEqual(first, campaign.build_plan("exploratory", 11, {}, catalog()))
        self.assertNotEqual(first["assignments"], campaign.build_plan("exploratory", 12, {}, catalog())["assignments"])
        for run in first["assignments"]:
            self.assertNotIn(run["case_id"], run["run_id"])
            self.assertNotIn(run["variant"], run["run_id"])

    def test_holdout_cannot_replace_a_main_partition(self):
        cases = catalog()
        cases[0]["partition"] = "holdout"
        with self.assertRaises(InvalidLedger):
            campaign.build_plan("exploratory", 1, {}, cases)

    def test_every_arm_receives_the_same_serial_resource_policy(self):
        step = {"step_id": 0, "task": "Review this change.", "base_tree": "a" * 40,
                "head_tree": "b" * 40, "source_sha256": fingerprint("shared")}
        shared = set()
        for variant in ("baseline", "reposcout", "reposcout-cli"):
            prompt, hashes = campaign.make_prompt(step, variant)
            self.assertIn("at most one build, test, benchmark, or RepoScout invocation at a time", prompt)
            self.assertIn("including source-query commands", prompt)
            shared.add(hashes["shared_prompt_sha256"])
        self.assertEqual(len(shared), 1)


class TimeoutTests(unittest.TestCase):
    def test_capture_refuses_nonpositive_nonfinite_or_excessive_timeout_before_reading_inputs(self):
        for value in (0, -1, 601, float("inf"), float("nan"), True, "600"):
            with self.subTest(timeout=value), self.assertRaises(InvalidLedger):
                campaign.capture_pins("unused", "unused", "unused", "unused", timeout_seconds=value)

    def test_custom_timeout_is_captured_and_frozen_for_both_arms(self):
        from codex_isolation import configuration_manifest
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            binary = directory / "synthetic-binary"
            binary.write_bytes(b"not executed")
            ca = {"path": str(directory / "public-ca.pem"), "sha256": fingerprint("ca"),
                  "bytes": 10, "destination": "/etc/ssl/cert.pem"}
            with patch.object(campaign, "directory_hash", return_value={"sha256": fingerprint("files"), "files": {}}), \
                    patch.object(campaign, "binary_version", return_value="synthetic version"), \
                    patch.object(campaign, "file_hash", return_value=fingerprint("file")), \
                    patch.object(campaign.subprocess, "run", return_value=SimpleNamespace(stdout="synthetic revision")), \
                    patch("codex_isolation.codex_runtime_manifest", return_value={"bin/codex": fingerprint("runtime")}), \
                    patch("codex_isolation.controller_ca_manifest", return_value=ca):
                captured = campaign.capture_pins(binary, "synthetic version", binary, directory, timeout_seconds=600)
            self.assertEqual(captured["limits"], {**campaign.LIMITS, "timeout_seconds": 600})
            self.assertEqual(campaign.LIMITS["timeout_seconds"], 180)
            root, plan = prepared(temporary, timeout_seconds=600)
            plan["pins"] = captured
            for variant in ("baseline", "reposcout"):
                assignment = next(item for item in plan["assignments"] if item["variant"] == variant)
                record = read_json(Path(assignment["run_dir"]) / "case.json")
                spec, _ = campaign._trial_spec(plan, assignment, record, record["steps"][0], root / "planned")
                self.assertEqual(configuration_manifest(spec)["limits"], captured["limits"])
            with patch.object(campaign, "capture_pins", return_value=captured) as recapture:
                campaign.verify_runtime_pins(plan)
                self.assertEqual(recapture.call_args.kwargs["timeout_seconds"], 600)
            drifted = {**captured, "limits": {**captured["limits"], "timeout_seconds": 599}}
            with patch.object(campaign, "capture_pins", return_value=drifted), self.assertRaises(InvalidLedger):
                campaign.verify_runtime_pins(plan)

    def test_prepare_library_and_cli_forward_selected_timeout(self):
        arguments = {"stage": "smoke", "seed": 1, "codex_binary": "codex", "codex_version": "version",
                     "reposcout_binary": "reposcout", "skill_dir": "skill", "timeout_seconds": 600}
        with patch.object(campaign, "capture_pins", side_effect=InvalidLedger("synthetic stop")) as capture:
            with self.assertRaises(InvalidLedger):
                campaign.prepare("unused", **arguments)
            self.assertEqual(capture.call_args.kwargs["timeout_seconds"], 600)
        argv = ["review_campaign.py", "prepare", "unused", "--stage", "smoke", "--seed", "1",
                "--codex-binary", "codex", "--codex-version", "version", "--reposcout-binary", "reposcout",
                "--timeout-seconds", "600"]
        with patch("sys.argv", argv), patch("sys.stdout", new=io.StringIO()), \
                patch.object(campaign, "prepare", return_value={"plan_sha256": "synthetic", "assignment_count": 8}) as create:
            campaign.main()
            self.assertEqual(create.call_args.kwargs["timeout_seconds"], 600)


class OwnershipTests(unittest.TestCase):
    def test_native_atomic_auth_refresh_is_cleaned_in_owned_controller(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            source = directory / "synthetic-auth.json"
            source.write_text("synthetic original")
            controller = directory / "controller"
            controller.mkdir(mode=0o700)
            with campaign.private_auth_copy(source, controller):
                replacement = controller / "refreshed-auth.json"
                replacement.write_text("synthetic refreshed")
                replacement.chmod(0o600)
                os.replace(replacement, controller / "auth.json")
            self.assertFalse((controller / "auth.json").exists())
            self.assertEqual(source.read_text(), "synthetic original")

    def test_auth_cleanup_rejects_foreign_controller_replacement(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            source = directory / "synthetic-auth.json"
            source.write_text("synthetic")
            controller = directory / "controller"
            controller.mkdir(mode=0o700)
            with self.assertRaises(InvalidLedger):
                with campaign.private_auth_copy(source, controller):
                    controller.rename(directory / "original-controller")
                    controller.mkdir(mode=0o700)
                    (controller / "auth.json").write_text("unrelated replacement data")
            self.assertEqual((controller / "auth.json").read_text(), "unrelated replacement data")

    def test_auth_cleanup_rejects_a_symlink_without_touching_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            source = directory / "synthetic-auth.json"
            source.write_text("synthetic")
            controller = directory / "controller"
            controller.mkdir(mode=0o700)
            target = directory / "unrelated.json"
            target.write_text("unrelated")
            with self.assertRaises(InvalidLedger):
                with campaign.private_auth_copy(source, controller):
                    (controller / "auth.json").unlink()
                    (controller / "auth.json").symlink_to(target)
            self.assertEqual(target.read_text(), "unrelated")

    def test_existing_directory_is_preserved(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "existing"
            path.mkdir()
            sentinel = path / "valuable.txt"
            sentinel.write_text("user data")
            with self.assertRaises(InvalidLedger):
                campaign.create_campaign_root(path)
            self.assertEqual(sentinel.read_text(), "user data")

    def test_symlink_parent_and_checkout_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)
            (path / "link").symlink_to(path, target_is_directory=True)
            with self.assertRaises(InvalidLedger):
                campaign.create_campaign_root(path / "link/new")
            with self.assertRaises(InvalidLedger):
                campaign.create_campaign_root(campaign.ROOT / "uncreated-eval-test")

    def test_atomic_outcome_cannot_replace_an_adverse_result(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "outcome.json"
            campaign.write_new(path, {"status": "failed"})
            with self.assertRaises(FileExistsError):
                campaign.write_new(path, {"status": "completed"})
            self.assertEqual(read_json(path), {"status": "failed"})

    def test_exclusive_lock_keeps_first_controller_lease(self):
        with tempfile.TemporaryDirectory() as temporary:
            with campaign.campaign_lock(temporary):
                with self.assertRaises(FileExistsError):
                    with campaign.campaign_lock(temporary):
                        self.fail("second controller acquired lock")
                self.assertTrue((Path(temporary) / "controller.lock").exists())
            self.assertFalse((Path(temporary) / "controller.lock").exists())

    def test_plan_and_run_ownership_tampering_are_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            self.assertEqual(campaign.load_plan(root)["plan_sha256"], plan["plan_sha256"])
            plan["seed"] += 1
            (root / "plan.json").write_text(json.dumps(plan))
            with self.assertRaises(InvalidLedger):
                campaign.load_plan(root)

    def test_runtime_drift_is_refused(self):
        plan = {"pins": pins()}
        with patch.object(campaign, "capture_pins", return_value={**pins(), "codex_sha256": "changed"}):
            with self.assertRaises(InvalidLedger):
                campaign.verify_runtime_pins(plan)


class ExecutionTests(unittest.TestCase):
    def test_uncalibrated_scope_is_unknown(self):
        self.assertEqual(campaign.calibrated_usage_scope("codex-cli synthetic", "0" * 64), ("unknown", None))

    def test_cumulative_followup_uses_raw_prior_counters_and_counts_full_episode_once(self):
        from test_codex_trace import records
        baseline = {"input_tokens": 80467, "cached_input_tokens": 62848, "cache_write_input_tokens": 0,
                    "output_tokens": 4680, "reasoning_output_tokens": 2007}
        terminal = {"input_tokens": 99101, "cached_input_tokens": 62848, "cache_write_input_tokens": 0,
                    "output_tokens": 4713, "reasoning_output_tokens": 2023}
        for optional_missing in (False, True):
            with self.subTest(optional_missing=optional_missing), tempfile.TemporaryDirectory() as temporary:
                _, plan = prepared(temporary, "exploratory", timeout_seconds=600)
                plan["pins"]["usage_scope"] = "thread-cumulative"
                assignment = next(run for run in plan["assignments"] if run["case_id"] == "review-followup")
                auth = Path(temporary) / "synthetic-auth.json"
                auth.write_text("synthetic")
                first = {key: value for key, value in baseline.items()
                         if not optional_missing or key not in ("cache_write_input_tokens", "reasoning_output_tokens")}
                calls = []
                def runner(spec):
                    index = len(calls)
                    calls.append(spec)
                    raw = "".join(json.dumps(event) + "\n" for event in records(first if index == 0 else terminal)).encode()
                    stdout = spec.artifact_dir / "stdout.jsonl"
                    stdout.write_bytes(raw)
                    answer = spec.artifact_dir / "answer.json"
                    campaign.write_new(answer, {"step": index})
                    return {"run_id": spec.run_id, "invocation_id": f"attempt-{index}", "status": "completed",
                            "thread_id": "thread", "resumed_thread_id": spec.resume_thread_id, "returncode": 0,
                            "stream_complete": True, "process_tree_drained": True, "stdout_truncated": False,
                            "termination_reason": None, "stdout_path": str(stdout), "answer_path": str(answer),
                            "stdout_bytes": len(raw), "stdout_sha256": campaign.file_hash(stdout),
                            "cli_version": spec.cli_version, "usage_scope": spec.usage_scope,
                            "usage_baseline": spec.usage_baseline}
                def activate(record, index):
                    record["active_step"] = index
                with patch("review_cases.activate_step", side_effect=activate), \
                        patch("review_grading.grade_episode", return_value={"adjudication_status": "pending"}):
                    result = campaign.execute_assignment(plan, assignment, auth, runner=runner, check_pins=False)
                self.assertEqual(result["status"], "completed")
                self.assertEqual([spec.resume_thread_id for spec in calls], [None, "thread"])
                self.assertEqual([spec.usage_scope for spec in calls], ["thread-cumulative"] * 2)
                self.assertIsNone(calls[0].usage_baseline)
                self.assertEqual(calls[1].usage_baseline["input_tokens"], 80467)
                self.assertIsNone(calls[1].usage_baseline["total_tokens"])
                accounting = result["accounting"]
                self.assertTrue(accounting["comparable_usage"])
                self.assertEqual(accounting["observed_input_plus_output_tokens"], 103814)
                self.assertNotEqual(accounting["observed_input_plus_output_tokens"], 188961)
                self.assertEqual([item["observed_input_plus_output_tokens"] for item in accounting["invocations"]], [85147, 18667])
                self.assertIsNone(accounting["observed_usage"]["total_tokens"])
                if optional_missing:
                    self.assertIsNone(calls[1].usage_baseline["cache_write_input_tokens"])
                    self.assertIsNone(accounting["observed_usage"]["reasoning_output_tokens"])
                else:
                    self.assertEqual(accounting["observed_usage"]["cache_write_input_tokens"], 0)
                    self.assertEqual(accounting["observed_usage"]["reasoning_output_tokens"], 2023)
                marker = read_json(Path(assignment["run_dir"]) / "step-1/attempt.json")
                self.assertEqual(marker["usage_baseline"], calls[1].usage_baseline)

    def test_cumulative_baseline_refuses_foreign_or_unverified_prior_trace(self):
        trace = {"thread_id": "thread", "cli_version": "version", "status": "completed",
                 "usage_scope": "thread-cumulative", "controller_closed": True, "comparable_usage": True,
                 "observed_usage": {"input_tokens": 1},
                 "turns": [{"status": "completed", "reported_usage": {"input_tokens": 80467, "total_tokens": None}}]}
        self.assertEqual(campaign._resume_usage_baseline(trace, "thread", "version")["input_tokens"], 80467)
        for change in ({"thread_id": "other"}, {"status": "failed"}, {"controller_closed": False},
                       {"comparable_usage": False}, {"cli_version": "other"}, {"usage_scope": "unknown"}, {"turns": []}):
            with self.subTest(change=change), self.assertRaises(InvalidLedger):
                campaign._resume_usage_baseline({**trace, **change}, "thread", "version")

    def test_trial_spec_passes_frozen_public_ca_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            _, plan = prepared(temporary)
            assignment = plan["assignments"][0]
            record = read_json(Path(assignment["run_dir"]) / "case.json")
            spec, _ = campaign._trial_spec(plan, assignment, record, record["steps"][0], Path(temporary) / "artifacts")
            self.assertEqual(spec.controller_ca_file, Path(plan["pins"]["controller_ca_file"]))
            self.assertEqual(spec.expected_controller_ca_sha256, plan["pins"]["controller_ca_sha256"])

    def test_interrupted_return_recovers_recorded_attempt_and_usage(self):
        from test_codex_trace import records, usage
        with tempfile.TemporaryDirectory() as temporary:
            _, plan = prepared(temporary)
            assignment = plan["assignments"][0]
            auth = Path(temporary) / "synthetic-auth.json"
            auth.write_text("synthetic")
            def runner(spec):
                stdout = spec.artifact_dir / "stdout.jsonl"
                raw = "".join(json.dumps(event) + "\n" for event in records(usage(123))).encode()
                stdout.write_bytes(raw)
                receipt = {"run_id": spec.run_id, "invocation_id": "recorded-attempt", "status": "completed",
                           "thread_id": "thread", "resumed_thread_id": None, "returncode": 0,
                           "stream_complete": True, "process_tree_drained": True, "stdout_truncated": False,
                           "termination_reason": None, "stdout_path": str(stdout), "stdout_bytes": len(raw),
                           "stdout_sha256": campaign.file_hash(stdout), "cli_version": "codex-cli test", "usage_scope": "unknown"}
                campaign.write_new(spec.artifact_dir / "lifecycle.json", receipt)
                raise KeyboardInterrupt
            with patch.object(campaign, "_trial_spec", side_effect=fake_spec):
                result = campaign.execute_assignment(plan, assignment, auth, runner=runner, check_pins=False)
            self.assertEqual(result["status"], "aborted")
            self.assertEqual(result["invocation_count"], 1)
            self.assertEqual(result["accounting"]["observed_input_plus_output_tokens"], 143)
            inventoried = campaign.inventory(plan)[0]
            self.assertEqual(inventoried["status"], "aborted")
            self.assertEqual(inventoried["accounting"]["observed_usage"]["input_tokens"], 123)
            self.assertFalse((Path(assignment["run_dir"]) / "controller/auth.json").exists())

    def test_followup_interruption_retains_both_invocation_observations(self):
        from test_codex_trace import records, usage
        with tempfile.TemporaryDirectory() as temporary:
            _, plan = prepared(temporary, "exploratory")
            assignment = next(run for run in plan["assignments"] if run["case_id"] == "review-followup")
            auth = Path(temporary) / "synthetic-auth.json"
            auth.write_text("synthetic")
            calls = []
            def runner(spec):
                index = len(calls)
                calls.append(spec.resume_thread_id)
                stdout = spec.artifact_dir / "stdout.jsonl"
                raw = "".join(json.dumps(event) + "\n" for event in records(usage(100 + index))).encode()
                stdout.write_bytes(raw)
                answer = spec.artifact_dir / "answer.json"
                campaign.write_new(answer, {"synthetic": True})
                receipt = {"run_id": spec.run_id, "invocation_id": f"attempt-{index}", "status": "completed",
                           "thread_id": "thread", "resumed_thread_id": spec.resume_thread_id, "returncode": 0,
                           "stream_complete": True, "process_tree_drained": True, "stdout_truncated": False,
                           "termination_reason": None, "stdout_path": str(stdout), "answer_path": str(answer),
                           "stdout_bytes": len(raw), "stdout_sha256": campaign.file_hash(stdout),
                           "cli_version": "codex-cli test", "usage_scope": "unknown"}
                campaign.write_new(spec.artifact_dir / "lifecycle.json", receipt)
                if index:
                    raise KeyboardInterrupt
                return receipt
            def activate(record, index):
                record["active_step"] = index
            with patch.object(campaign, "_trial_spec", side_effect=fake_spec), patch("review_cases.activate_step", side_effect=activate):
                result = campaign.execute_assignment(plan, assignment, auth, runner=runner, check_pins=False)
            self.assertEqual(calls, [None, "thread"])
            self.assertEqual(result["status"], "aborted")
            self.assertEqual(result["invocation_count"], 2)
            self.assertEqual([item["observed_usage"]["input_tokens"] for item in result["accounting"]["invocations"]], [100, None])
            self.assertEqual(result["accounting"]["invocations"][1]["emitted_usage_known_sum"]["input_tokens"], 101)
            self.assertIsNone(result["accounting"]["observed_input_plus_output_tokens"])
            self.assertIn("episode-resume-usage-scope-unknown", result["accounting"]["episode_errors"])

    def test_crash_marker_recovers_lifecycle_without_result_or_outcome(self):
        from test_codex_trace import records
        with tempfile.TemporaryDirectory() as temporary:
            _, plan = prepared(temporary)
            assignment = plan["assignments"][0]
            directory = Path(assignment["run_dir"])
            campaign.write_new(directory / "started.json", {"run_id": assignment["run_id"], "plan_sha256": plan["plan_sha256"]})
            step = directory / "step-0"
            artifacts = step / "invocation"
            artifacts.mkdir(parents=True)
            campaign.write_new(step / "attempt.json", {"run_id": assignment["run_id"], "step_id": 0,
                                                       "attempt_id": "crashed-attempt", "prompt_hashes": {}})
            stdout = artifacts / "stdout.jsonl"
            stdout.write_text("".join(json.dumps(event) + "\n" for event in records()))
            campaign.write_new(artifacts / "lifecycle.json", {"run_id": assignment["run_id"],
                "invocation_id": "crashed-attempt", "status": "aborted", "thread_id": "thread", "usage_scope": "unknown",
                "stdout_path": str(stdout), "stdout_sha256": campaign.file_hash(stdout), "stdout_bytes": stdout.stat().st_size,
                "stream_complete": False, "process_tree_drained": True})
            result = campaign.inventory(plan)[0]
            self.assertEqual(result["status"], "aborted")
            self.assertEqual(result["invocation_count"], 1)
            self.assertEqual(result["accounting"]["observed_usage"]["input_tokens"], 100)
            self.assertFalse(result["accounting"]["comparable_usage"])

    def test_prepared_source_mutation_is_refused_before_model(self):
        with tempfile.TemporaryDirectory() as temporary:
            _, plan = prepared(temporary)
            assignment = plan["assignments"][0]
            directory = Path(assignment["run_dir"])
            (directory / "workspace/README.md").write_text("changed test source")
            with self.assertRaises(InvalidLedger):
                campaign.execute_assignment(plan, assignment, Path(temporary) / "unread-auth", runner=lambda spec: self.fail("model submitted"), check_pins=False)

    def test_preflight_does_not_consume_any_assignment(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            calls = []
            def runner(spec, *, preflight_only=False):
                calls.append(preflight_only)
                return {"status": "preflight-passed", "preflight": {"passed": True}, "process_tree_drained": True}
            with patch.object(campaign, "_trial_spec", side_effect=fake_spec):
                campaign.run(root, limit=2, preflight_only=True, runner=runner, check_pins=False)
            self.assertEqual(calls, [True, True])
            self.assertTrue(all(run["status"] == "notrun" for run in campaign.inventory(plan)))

    def test_unconfirmed_preflight_drain_stops_before_another_probe(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            calls = []

            def runner(spec, *, preflight_only=False):
                calls.append(spec.run_id)
                return {"status": "failed", "preflight": {"passed": False}, "process_tree_drained": False}

            with patch.object(campaign, "_trial_spec", side_effect=fake_spec):
                with self.assertRaisesRegex(InvalidLedger, "cleanup is unconfirmed"):
                    campaign.run(root, preflight_only=True, runner=runner, check_pins=False)
            self.assertEqual(calls, [plan["assignments"][0]["run_id"]])
            receipts = list(Path(plan["assignments"][0]["run_dir"]).glob("preflight-*/result.json"))
            self.assertEqual(len(receipts), 1)
            self.assertFalse(read_json(receipts[0])["process_tree_drained"])

    def test_started_without_outcome_is_aborted_and_not_retried(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            first = plan["assignments"][0]
            campaign.write_new(Path(first["run_dir"]) / "started.json", {
                "run_id": first["run_id"], "plan_sha256": plan["plan_sha256"]})
            self.assertEqual(campaign.inventory(plan)[0]["status"], "aborted")
            calls = []
            def runner(spec, *, preflight_only=False):
                calls.append(spec.run_id)
                return {"status": "preflight-passed", "preflight": {"passed": True}, "process_tree_drained": True}
            with patch.object(campaign, "_trial_spec", side_effect=fake_spec):
                campaign.run(root, limit=1, preflight_only=True, runner=runner, check_pins=False)
            self.assertEqual(calls, [plan["assignments"][1]["run_id"]])

    def test_failure_is_permanent_and_private_auth_is_cleaned(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            auth = Path(temporary) / "synthetic-auth.json"
            auth.write_text("synthetic credential; not host authentication")
            calls = []
            def runner(spec):
                calls.append(spec.run_id)
                raise ValueError("private error contents must not escape")
            with patch.object(campaign, "_trial_spec", side_effect=fake_spec):
                campaign.run(root, auth_file=auth, limit=1, runner=runner, check_pins=False)
                campaign.run(root, auth_file=auth, limit=1, runner=runner, check_pins=False)
            self.assertEqual(calls, [run["run_id"] for run in plan["assignments"][:2]])
            self.assertEqual([run["status"] for run in campaign.inventory(plan)[:2]], ["failed", "failed"])
            self.assertEqual(auth.read_text(), "synthetic credential; not host authentication")
            for assignment in plan["assignments"][:2]:
                self.assertFalse((Path(assignment["run_dir"]) / "controller/auth.json").exists())
                stored = read_json(Path(assignment["run_dir"]) / "outcome.json")
                self.assertNotIn("private error contents", json.dumps(stored))
                with self.assertRaises(InvalidLedger):
                    campaign.execute_assignment(plan, assignment, auth, runner=runner)

    def test_followup_resumes_exact_thread_once(self):
        with tempfile.TemporaryDirectory() as temporary:
            _, plan = prepared(temporary, "exploratory")
            assignment = next(run for run in plan["assignments"] if run["case_id"] == "review-followup")
            auth = Path(temporary) / "synthetic-auth.json"
            auth.write_text("synthetic")
            resumes = []
            thread = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
            def runner(spec):
                resumes.append(spec.resume_thread_id)
                answer = spec.artifact_dir / "answer.json"
                campaign.write_new(answer, {"step": len(resumes)})
                stdout = spec.artifact_dir / "stdout.jsonl"
                stdout.write_text("synthetic trace")
                return {"status": "completed", "thread_id": thread, "resumed_thread_id": spec.resume_thread_id,
                        "stdout_path": str(stdout), "answer_path": str(answer)}
            def activate(record, index):
                self.assertEqual(index, record["active_step"] + 1)
                record["active_step"] = index
            with patch.object(campaign, "_trial_spec", side_effect=fake_spec), \
                 patch("review_cases.activate_step", side_effect=activate), \
                 patch("codex_trace.parse_exec_trace", return_value={"synthetic": True}), \
                 patch("codex_trace.aggregate_episode", return_value={"invocation_count": 2}), \
                 patch("review_grading.grade_episode", return_value={"adjudication_status": "pending"}):
                result = campaign.execute_assignment(plan, assignment, auth, runner=runner, check_pins=False)
            self.assertEqual(resumes, [None, thread])
            self.assertEqual(result["status"], "completed")
            self.assertEqual(result["invocation_count"], 2)
            self.assertFalse((Path(assignment["run_dir"]) / "controller/auth.json").exists())

    def test_smoke_quality_failure_does_not_block_harness_qualification(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            for assignment in plan["assignments"]:
                campaign.write_new(Path(assignment["run_dir"]) / "outcome.json", outcome(
                    plan, assignment, "completed", quality={"adjudication_status": "accepted", "quality": {"passed": False}},
                    accounting={"invocation_count": assignment["step_count"]}, invocations=[{"step_id": index, "result": {
                        "preflight": {"passed": True}, "stream_complete": True,
                        "process_tree_drained": True, "stdout_truncated": False}}
                        for index in range(assignment["step_count"])]))
            value = campaign.qualify_smoke(root, {"plan_sha256": plan["plan_sha256"], "isolation_passed": True,
                "structurally_consumable": True, "accounting_basis_understood": True,
                "reason": "Protocol checks pass; negative model quality is an observed product outcome."})
            self.assertTrue(value["structurally_consumable"])


if __name__ == "__main__":
    unittest.main()
