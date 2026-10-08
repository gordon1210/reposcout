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


def model_runner(runner):
    """These controller tests supply an explicit successful no-model environment attestation."""
    def execute(spec, *, preflight_only=False):
        if preflight_only:
            return {"status": "preflight-passed", "preflight": {"passed": True}, "process_tree_drained": True}
        return runner(spec)
    return execute


def traced_result(spec, *, status="completed", tokens=123):
    from test_codex_trace import records, turn, usage
    events = records(usage(tokens)) if status == "completed" else (
        [{"type": "thread.started", "thread_id": "thread"}] + turn("failed", usage(tokens), "failed"))
    stdout = spec.artifact_dir / "stdout.jsonl"
    stdout.write_text("".join(json.dumps(event) + "\n" for event in events))
    answer = spec.artifact_dir / "answer.json"
    campaign.write_new(answer, {"synthetic": True})
    return {"run_id": spec.run_id, "status": status, "thread_id": "thread",
            "resumed_thread_id": spec.resume_thread_id, "returncode": 0 if status == "completed" else 1,
            "stream_complete": True, "process_tree_drained": True, "stdout_truncated": False,
            "termination_reason": None, "stdout_path": str(stdout), "answer_path": str(answer),
            "stdout_bytes": stdout.stat().st_size, "stdout_sha256": campaign.file_hash(stdout),
            "cli_version": "codex-cli test", "usage_scope": "unknown"}


class AssignmentTests(unittest.TestCase):
    def test_large_followup_has_twelve_balanced_single_step_assignments(self):
        names = ("publication-change-a", "publication-change-b", "publication-change-c")
        cases = [{"case_id": name, "partition": "holdout", "title": name,
                  "step_count": 1, "fixture_sha256": fingerprint(name)} for name in names]
        plan = campaign.build_plan("large-followup", 20261010, {}, cases)
        runs = plan["assignments"]
        self.assertEqual(plan["assignment_count"], 12)
        self.assertEqual(len(runs), 12)
        self.assertEqual(plan["repetitions"], 2)
        self.assertFalse(plan["ablation"])
        self.assertEqual(plan["cases"], cases)
        self.assertEqual({(run["case_id"], run["repeat_id"], run["variant"]) for run in runs},
                         {(name, repeat, arm) for name in names for repeat in (1, 2)
                          for arm in ("baseline", "reposcout")})
        self.assertEqual({run["step_count"] for run in runs}, {1})
        self.assertEqual([run["sequence"] for run in runs], list(range(12)))
        self.assertEqual([run["repeat_id"] for run in runs], [1] * 6 + [2] * 6)
        self.assertEqual(len({run["run_id"] for run in runs}), 12)
        self.assertEqual(len({run["pair_id"] for run in runs}), 6)
        self.assertEqual(plan, campaign.build_plan("large-followup", 20261010, {}, list(reversed(cases))))
        first_counts = []
        for repeat in (1, 2):
            selected = [run for run in runs if run["repeat_id"] == repeat]
            first_counts.append(sum(run["variant"] == "baseline" and run["pair_position"] == 0 for run in selected))
            for index in range(0, len(selected), 2):
                pair = selected[index:index + 2]
                self.assertEqual(pair[0]["pair_id"], pair[1]["pair_id"])
                self.assertEqual(pair[0]["case_id"], pair[1]["case_id"])
                self.assertEqual([run["pair_position"] for run in pair], [0, 1])
        self.assertEqual(sorted(first_counts), [1, 2])
        for name in names:
            self.assertEqual(sum(run["case_id"] == name and run["variant"] == "baseline"
                                 and run["pair_position"] == 0 for run in runs), 1)

    def test_large_followup_keeps_bundle_partition_and_invocation_boundaries(self):
        cases = [{"case_id": name, "partition": "holdout", "title": name,
                  "step_count": 1, "fixture_sha256": fingerprint(name)}
                 for name in ("publication-change-a", "publication-change-b", "publication-change-c")]
        for field, value, message in (("partition", "development", "partition"),
                                      ("step_count", 2, "one invocation")):
            with self.subTest(field=field), self.assertRaisesRegex(InvalidLedger, message):
                invalid = [dict(case) for case in cases]
                invalid[0][field] = value
                campaign.build_plan("large-followup", 20261010, {}, invalid)
        with self.assertRaisesRegex(InvalidLedger, "incomplete case catalog"):
            campaign.build_plan("large-followup", 20261010, {}, cases[:2])
        with self.assertRaisesRegex(InvalidLedger, "ablation belongs to exploratory"):
            campaign.build_plan("large-followup", 20261010, {}, cases, include_ablation=True)
        for bundle, original, message in ((None, None, "explicit separate fixture bundle"),
                                           ("/unexecuted/bundle", "/unexecuted/prior", "only the original holdout stage")):
            with self.subTest(bundle=bundle, original=original), self.assertRaisesRegex(InvalidLedger, message):
                campaign.prepare("/unexecuted/campaign", stage="large-followup", seed=20261010,
                                 codex_binary="/unexecuted/codex", codex_version="codex-cli test",
                                 reposcout_binary="/unexecuted/reposcout", skill_dir="/unexecuted/skill",
                                 bundle_root=bundle, original_campaign=original)

    def test_large_development_balances_an_odd_case_count_across_two_repeats(self):
        cases = [{"case_id": name, "partition": "development", "title": name,
                  "step_count": 1, "fixture_sha256": fingerprint(name)}
                 for name in ("cancellation-change", "event-routing-change", "status-extraction")]
        plan = campaign.build_plan("large-development", 20261008, {}, cases)
        self.assertEqual(plan["assignment_count"], 12)
        self.assertEqual(plan["repetitions"], 2)
        for case in cases:
            runs = [run for run in plan["assignments"] if run["case_id"] == case["case_id"]]
            self.assertEqual({(run["variant"], run["repeat_id"]) for run in runs},
                             {(variant, repeat) for variant in ("baseline", "reposcout") for repeat in (1, 2)})
            self.assertEqual(sum(run["variant"] == "baseline" and run["pair_position"] == 0 for run in runs), 1)
        self.assertEqual([run["sequence"] for run in plan["assignments"]], list(range(12)))
        self.assertEqual(len({run["run_id"] for run in plan["assignments"]}), 12)
        first_counts = []
        for repeat in (1, 2):
            selected = [run for run in plan["assignments"] if run["repeat_id"] == repeat]
            first_counts.append(sum(run["variant"] == "baseline" and run["pair_position"] == 0 for run in selected))
            for index in range(0, len(selected), 2):
                self.assertEqual(selected[index]["pair_id"], selected[index + 1]["pair_id"])
                self.assertEqual([selected[index]["pair_position"], selected[index + 1]["pair_position"]], [0, 1])
        self.assertEqual(sorted(first_counts), [1, 2])

    def test_historical_schedules_retain_their_reviewed_assignment_identities(self):
        # Captured from the reviewed pre-large-suite planner at 74b0959, not the new implementation.
        expected = {"smoke": "0a85035bdfe618abfdde88b75e56a5138be25613a37685981a5aec23567ef421",
                    "exploratory": "fdae4ec32c65f1c512ec45d8f8d862034310ee34a04ea164f33c113b0dd9a860"}
        for stage, digest in expected.items():
            self.assertEqual(fingerprint(campaign.build_plan(stage, 917, {}, catalog(), stage == "exploratory")["assignments"]), digest)

    def test_large_holdout_presets_assign_eight_paired_and_four_original_reviews(self):
        cases = [{"case_id": name, "partition": "holdout", "title": name,
                  "step_count": 1, "fixture_sha256": fingerprint(name)} for name in campaign.LARGE_HOLDOUT_CASES]
        paired = campaign.build_plan("large-holdout", 20261008, {}, cases)
        original = campaign.build_plan("large-holdout-original", 20261008, {}, cases)
        self.assertEqual(paired["assignment_count"], 8)
        self.assertEqual(original["assignment_count"], 4)
        self.assertEqual({run["variant"] for run in original["assignments"]}, {"reposcout"})
        self.assertEqual({(run["case_id"], run["repeat_id"]) for run in original["assignments"]},
                         {(name, repeat) for name in campaign.LARGE_HOLDOUT_CASES for repeat in (1, 2)})
        with self.assertRaises(InvalidLedger):
            campaign.build_plan("large-development", 20261008, {}, cases)

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
    def test_original_condition_remains_bound_to_prior_canonical_skill_and_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            binary = directory / "synthetic-binary"
            binary.write_bytes(b"frozen binary; not executed")
            skill = directory / "archived-skill"
            skill.mkdir()
            (skill / "SKILL.md").write_text("frozen canonical skill")
            archived = campaign.directory_hash(skill)
            original = {"stage": "large-development", "plan_sha256": fingerprint("original plan"),
                        "pins": {"skill": archived, "reposcout_sha256": campaign.file_hash(binary)}}
            directory_hash = campaign.directory_hash
            def changed_canonical(path):
                return {"sha256": fingerprint("new canonical"), "files": {}} if Path(path) == campaign.ROOT / "skills/reposcout" else directory_hash(path)
            ca = {"path": str(directory / "public-ca.pem"), "sha256": fingerprint("ca"),
                  "bytes": 10, "destination": "/etc/ssl/cert.pem"}
            with patch.object(campaign, "directory_hash", side_effect=changed_canonical), \
                    patch.object(campaign, "load_plan", return_value=original), \
                    patch.object(campaign, "binary_version", return_value="synthetic version"), \
                    patch.object(campaign.subprocess, "run", return_value=SimpleNamespace(stdout="synthetic revision")), \
                    patch("codex_isolation.codex_runtime_manifest", return_value={"bin/codex": fingerprint("runtime")}), \
                    patch("codex_isolation.controller_ca_manifest", return_value=ca), \
                    patch("codex_isolation.runtime_tool_manifest", return_value={
                        "node": {"path": "/unexecuted/node", "sha256": fingerprint("node")}}):
                with self.assertRaisesRegex(InvalidLedger, "exact canonical"):
                    campaign.capture_pins(binary, "synthetic version", binary, skill)
                captured = campaign.capture_pins(binary, "synthetic version", binary, skill,
                                                 original_campaign=directory / "prior")
                self.assertEqual(captured["skill"], archived)
                self.assertEqual(captured["original_plan_sha256"], original["plan_sha256"])
                binary.write_bytes(b"different binary")
                with self.assertRaisesRegex(InvalidLedger, "frozen canonical"):
                    campaign.capture_pins(binary, "synthetic version", binary, skill,
                                          original_campaign=directory / "prior")
                binary.write_bytes(b"frozen binary; not executed")
                (skill / "SKILL.md").write_text("different skill")
                with self.assertRaisesRegex(InvalidLedger, "frozen canonical"):
                    campaign.capture_pins(binary, "synthetic version", binary, skill,
                                          original_campaign=directory / "prior")

    def test_external_case_check_command_reaches_actual_runtime_attestation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            assignment = dict(plan["assignments"][0], case_id="publication-change-a")
            record = read_json(Path(assignment["run_dir"]) / "case.json")
            record["check_commands"] = [["node", "tests/run.mjs"]]
            spec, _ = campaign._trial_spec(plan, assignment, record, record["steps"][0], root / "planned")
            self.assertEqual(spec.required_runtimes, ("node",))

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
                    patch("codex_isolation.controller_ca_manifest", return_value=ca), \
                    patch("codex_isolation.runtime_tool_manifest", return_value={
                        "node": {"path": "/unexecuted/node", "sha256": fingerprint("node")}}):
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
    def test_explicit_assignment_selects_only_that_run_and_never_restarts_it(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            selected = plan["assignments"][5]["run_id"]
            auth = Path(temporary) / "synthetic-auth.json"
            auth.write_text("synthetic")
            calls = []
            @model_runner
            def runner(spec):
                calls.append(spec.run_id)
                raise RuntimeError("synthetic failure")
            with patch.object(campaign, "_trial_spec", side_effect=fake_spec):
                first = campaign.run(root, auth_file=auth, run_id=selected, runner=runner, check_pins=False)
                second = campaign.run(root, auth_file=auth, run_id=selected, runner=runner, check_pins=False)
                with self.assertRaisesRegex(InvalidLedger, "explicit assignment"):
                    campaign.run(root, auth_file=auth, run_id="unknown", runner=runner, check_pins=False)
                with self.assertRaisesRegex(InvalidLedger, "explicit assignment"):
                    campaign.run(root, auth_file=auth, run_id=selected, limit=1, runner=runner, check_pins=False)
            self.assertEqual(calls, [selected])
            self.assertEqual(first["runs"], [{"run_id": selected, "status": "failed"}])
            self.assertEqual(second["runs"], [])
            self.assertEqual([row["status"] for row in campaign.inventory(plan)],
                             ["notrun"] * 5 + ["failed"] + ["notrun"] * 2)

    def test_campaign_entry_prerequisite_failure_has_private_evidence_without_consuming_runs(self):
        for missing in (False, True):
            with self.subTest(missing=missing), tempfile.TemporaryDirectory() as temporary:
                root, plan = prepared(temporary)
                actual = {**plan["pins"], "codex_sha256": fingerprint("replacement")}
                options = {"side_effect": FileNotFoundError("/home/private/runtime disappeared")} if missing else {"return_value": actual}
                with patch.object(campaign, "capture_pins", **options), self.assertRaises(campaign.CampaignPrerequisiteError):
                    campaign.run(root, auth_file="unread-authentication", runner=lambda spec: self.fail("runner called"))
                self.assertTrue(all(run["status"] == "notrun" for run in campaign.inventory(plan)))
                files = list((root / "failures").glob("*.json"))
                self.assertEqual(len(files), 1)
                receipt = read_json(files[0])
                self.assertEqual(files[0].stem, fingerprint(receipt))
                self.assertEqual(receipt["phase"], "campaign-pins")
                self.assertEqual(receipt["category"], "runtime-pins-unavailable" if missing else "runtime-pin-drift")
                if missing:
                    self.assertEqual(receipt["evidence"], {"cause_type": "FileNotFoundError"})
                else:
                    self.assertEqual(receipt["evidence"]["changed_pins"]["codex_sha256"], {
                        "expected_sha256": fingerprint(plan["pins"]["codex_sha256"]),
                        "actual_sha256": fingerprint(actual["codex_sha256"])})
                self.assertNotIn("/home/private", json.dumps(receipt))

    def test_preflight_setup_failure_retains_original_phase_without_attribute_error(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            result = {"status": "not-started", "process_tree_drained": True, "preflight": None,
                      "failure_phase": "isolation-prepare", "error_type": "RuntimeUnavailableError"}
            with self.assertRaises(campaign.CampaignPrerequisiteError):
                campaign.run(root, preflight_only=True, runner=lambda spec, **options: result, check_pins=False)
            assignment = Path(plan["assignments"][0]["run_dir"])
            receipt = read_json(next((assignment / "failures").glob("*.json")))
            self.assertEqual(receipt["category"], "isolation-preflight-failed")
            self.assertEqual(receipt["evidence"], {"result_sha256": fingerprint(result), "failure_phase": "isolation-prepare"})
            self.assertEqual(receipt["error_type"], "CampaignPrerequisiteError")
            self.assertTrue(all(run["status"] == "notrun" for run in campaign.inventory(plan)))

    def test_runtime_preflight_failure_leaves_every_assignment_unconsumed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            calls = []

            def runner(spec, *, preflight_only=False):
                calls.append((spec.run_id, preflight_only))
                self.assertTrue(preflight_only)
                self.assertFalse((Path(plan["assignments"][0]["run_dir"]) / "started.json").exists())
                self.assertEqual(json.loads((spec.controller_dir / "auth.json").read_text()), {})
                return {"status": "not-started", "process_tree_drained": True,
                        "preflight": {"passed": False, "checks": {"required_runtimes": False}}}

            with self.assertRaisesRegex(campaign.CampaignPrerequisiteError, "runtime preflight failed"):
                campaign.run(root, auth_file="unread-credentials", runner=runner, check_pins=False)
            self.assertEqual(calls, [(plan["assignments"][0]["run_id"], True)])
            self.assertTrue(all(run["status"] == "notrun" for run in campaign.inventory(plan)))
            self.assertFalse(any(Path(item["run_dir"]).joinpath("started.json").exists() for item in plan["assignments"]))

    def test_pin_drift_after_a_result_preserves_costs_and_leaves_next_assignment_notrun(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            auth = Path(temporary) / "synthetic-auth.json"
            auth.write_text("synthetic")
            calls = []

            @model_runner
            def runner(spec):
                calls.append(spec.run_id)
                return traced_result(spec, status="failed", tokens=77)

            def capture(*args, **kwargs):
                return {**plan["pins"], **({"codex_sha256": fingerprint("replacement")} if calls else {})}

            with patch.object(campaign, "capture_pins", side_effect=capture):
                with self.assertRaisesRegex(InvalidLedger, "campaign conditions drifted"):
                    campaign.run(root, auth_file=auth, runner=runner)
            runs = campaign.inventory(plan)
            self.assertEqual(calls, [plan["assignments"][0]["run_id"]])
            self.assertEqual(runs[0]["status"], "failed")
            self.assertEqual(runs[0]["accounting"]["observed_usage"]["input_tokens"], 77)
            self.assertTrue(all(run["status"] == "notrun" for run in runs[1:]))
            self.assertFalse(any(Path(item["run_dir"]).joinpath("started.json").exists()
                                 for item in plan["assignments"][1:]))

    def test_mid_episode_pin_drift_stops_batch_and_retains_private_gate_evidence_and_costs(self):
        for receipt_failure in (False, True):
            with self.subTest(receipt_failure=receipt_failure), tempfile.TemporaryDirectory() as temporary:
                root, plan = prepared(temporary)
                assignments = plan["assignments"]
                first = next(item for item in assignments if item["case_id"] == "review-followup")
                assignments.remove(first)
                assignments.insert(0, first)
                plan.pop("plan_sha256")
                plan["plan_sha256"] = fingerprint(plan)
                (root / "plan.json").write_text(json.dumps(plan))
                auth = Path(temporary) / "synthetic-auth.json"
                auth.write_text("synthetic secret that must never enter failure evidence")
                calls = []

                @model_runner
                def runner(spec):
                    calls.append(spec.run_id)
                    return traced_result(spec)

                def capture(*args, **kwargs):
                    return {**plan["pins"], **({"codex_sha256": fingerprint("replacement")} if calls else {})}

                original_write = campaign.write_new
                def write(path, value):
                    if receipt_failure and Path(path).parent.name == "failures":
                        raise OSError("private storage message must not escape")
                    return original_write(path, value)

                with patch.object(campaign, "capture_pins", side_effect=capture), \
                        patch.object(campaign, "write_new", side_effect=write):
                    batch = campaign.run(root, auth_file=auth, runner=runner)
                self.assertEqual(batch["runs"], [{"run_id": first["run_id"], "status": "failed"}])
                self.assertEqual(calls, [first["run_id"]])
                runs = campaign.inventory(plan)
                self.assertTrue(runs[0]["campaign_fatal"])
                self.assertEqual(runs[0]["accounting"]["observed_usage"]["input_tokens"], 123)
                self.assertTrue(all(run["status"] == "notrun" for run in runs[1:]))
                failure = runs[0]["errors"][-1]
                self.assertEqual((failure["phase"], failure["step_id"], failure["category"]),
                                 ("runtime-pins", 1, "runtime-pin-drift"))
                if receipt_failure:
                    self.assertEqual(failure["private_evidence_status"], "unavailable")
                    self.assertNotIn("private storage message", json.dumps(runs[0]))
                else:
                    digest = failure["private_evidence_sha256"]
                    receipt = read_json(Path(first["run_dir"]) / "failures" / (digest + ".json"))
                    self.assertEqual(fingerprint(receipt), digest)
                    self.assertEqual(receipt["evidence"]["changed_pins"], {
                        "codex_sha256": {"expected_sha256": fingerprint(plan["pins"]["codex_sha256"]),
                                         "actual_sha256": fingerprint(fingerprint("replacement"))}})
                    self.assertNotIn("synthetic secret", json.dumps(receipt))

    def test_ordinary_episode_failure_does_not_stop_other_assignments(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, plan = prepared(temporary)
            auth = Path(temporary) / "synthetic-auth.json"
            auth.write_text("synthetic")
            calls = []

            @model_runner
            def runner(spec):
                calls.append(spec.run_id)
                return traced_result(spec, status="failed")

            result = campaign.run(root, auth_file=auth, limit=2, runner=runner, check_pins=False)
            self.assertEqual(calls, [item["run_id"] for item in plan["assignments"][:2]])
            self.assertEqual([item["status"] for item in result["runs"]], ["failed", "failed"])
            self.assertTrue(all(run["accounting"]["observed_usage"]["input_tokens"] == 123
                                for run in campaign.inventory(plan)[:2]))

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
                @model_runner
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
            @model_runner
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
            @model_runner
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
            @model_runner
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

    def test_followup_delivers_step_ids_and_resumes_exact_thread_once(self):
        with tempfile.TemporaryDirectory() as temporary:
            _, plan = prepared(temporary, "exploratory", ablation=True)
            auth = Path(temporary) / "synthetic-auth.json"
            auth.write_text("synthetic")
            shared_hashes = [set(), set()]
            for variant in ("baseline", "reposcout", "reposcout-cli"):
                with self.subTest(variant=variant):
                    assignment = next(run for run in plan["assignments"]
                                      if run["case_id"] == "review-followup" and run["variant"] == variant)
                    calls = []
                    thread = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
                    @model_runner
                    def runner(spec):
                        calls.append(spec)
                        answer = spec.artifact_dir / "answer.json"
                        campaign.write_new(answer, {"step": len(calls)})
                        stdout = spec.artifact_dir / "stdout.jsonl"
                        stdout.write_text("synthetic trace")
                        return {"status": "completed", "thread_id": thread,
                                "resumed_thread_id": spec.resume_thread_id,
                                "stdout_path": str(stdout), "answer_path": str(answer)}
                    def activate(record, index):
                        self.assertEqual(index, record["active_step"] + 1)
                        record["active_step"] = index
                    with patch("review_cases.activate_step", side_effect=activate), \
                            patch("codex_trace.parse_exec_trace", return_value={"synthetic": True}), \
                            patch("codex_trace.aggregate_episode", return_value={"invocation_count": 2}), \
                            patch("review_grading.grade_episode", return_value={"adjudication_status": "pending"}):
                        result = campaign.execute_assignment(plan, assignment, auth, runner=runner, check_pins=False)
                    self.assertEqual([spec.resume_thread_id for spec in calls], [None, thread])
                    self.assertEqual(result["status"], "completed")
                    self.assertEqual(result["invocation_count"], 2)
                    for step_id, spec in enumerate(calls):
                        self.assertIn(f"Current episode step_id: {step_id} (zero-based; the first review is step 0).",
                                      spec.prompt)
                        guidance = " ".join(spec.prompt.split())
                        self.assertIn("All evidence array indices are zero-based", guidance)
                        self.assertIn("use step=0 and index=0", guidance)
                        self.assertIn("index points into that answer's top-level evidence array", guidance)
                        retained = spec.answer_schema["properties"]["retained_evidence"]["items"]["properties"]
                        self.assertIn("Zero-based step_id of the earlier review answer", retained["step"]["description"])
                        self.assertIn("less than the current step_id", retained["step"]["description"])
                        self.assertIn("earlier answer's top-level evidence array", retained["index"]["description"])
                        self.assertIn("do not index its retained_evidence array", retained["index"]["description"])
                        delivered = read_json(Path(assignment["run_dir"]) / f"step-{step_id}/prompt.json")
                        self.assertEqual(delivered["prompt"], spec.prompt)
                        shared_hashes[step_id].add(delivered["shared_prompt_sha256"])
                    self.assertFalse((Path(assignment["run_dir"]) / "controller/auth.json").exists())
            self.assertEqual([len(values) for values in shared_hashes], [1, 1])

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
