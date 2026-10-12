import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from accounting import InvalidLedger, fingerprint, read_json
import review_campaign as campaign
from review_export import project_input_receipt
from test_claude_trace import ANSWER, assistant, init, result, tool_result, tool_use, usage


def catalog():
    return [{"case_id": name, "partition": "development", "title": name, "step_count": 1,
             "fixture_sha256": fingerprint(name)} for name in campaign.LARGE_DEVELOPMENT_CASES]


def claude_pins(effort="medium"):
    return {"runtime": "claude", "model": "claude-haiku-5-5", "effort": effort,
            "limits": dict(campaign.LIMITS), "usage_scope": "invocation", "usage_scope_calibration": None,
            "claude_binary": "/unexecuted/claude", "claude_sha256": fingerprint("claude"),
            "claude_version": "2.1.295 (Claude Code)", "reposcout_binary": "/unexecuted/reposcout",
            "reposcout_sha256": fingerprint("reposcout"), "skill_dir": "/unexecuted/skill",
            "skill": {"sha256": fingerprint("skill")}, "controller_ca_file": "/unexecuted/ca.pem",
            "controller_ca_sha256": fingerprint("public-ca"), "runtime_tool_paths": {"bash": "/usr/bin/bash"},
            "runtime_tools_sha256": fingerprint("tools"), "sandbox_tool_paths": {"bwrap": "/usr/bin/bwrap"},
            "sandbox_tools_sha256": fingerprint("sandbox")}


def prepared(parent, pins=None, treatment=campaign.DEFAULT_TREATMENT):
    """Controller-owned synthetic Claude records without probes or model execution."""
    pins = claude_pins() if pins is None else pins
    root, marker = campaign.create_campaign_root(Path(parent) / "campaign")
    plan = campaign.build_plan("large-development", 917, pins, catalog(), effort=pins["effort"], treatment=treatment)
    for assignment in plan["assignments"]:
        directory = root / "runs" / assignment["run_id"]
        directory.mkdir(mode=0o700)
        identity = directory.stat()
        campaign.write_new(directory / "ownership.json", {"campaign_token": marker["token"],
                           "run_id": assignment["run_id"], "root": str(directory),
                           "device": identity.st_dev, "inode": identity.st_ino, "owner": os.getuid()})
        workspace, oracle = directory / "workspace", directory / "oracle"
        workspace.mkdir()
        oracle.mkdir()
        (directory / "controller").mkdir(mode=0o700)
        (workspace / "README.md").write_text("synthetic fixture\n")
        oracle_body = {"snapshots": {"head": {"README.md": "synthetic fixture\n"}}}
        campaign.write_new(oracle / "oracle.json", oracle_body)
        record = {"case_id": assignment["case_id"], "workspace": str(workspace),
                  "private_oracle_path": str(oracle / "oracle.json"), "active_step": 0,
                  "steps": [{"step_id": 0, "task": "Review the supplied comparison.", "base_tree": "a" * 40,
                             "head_tree": "b" * 40, "source_sha256": fingerprint(0), "head_revision": "head"}],
                  "check_commands": [["python3", "-m", "unittest"]],
                  "public_workspace_sha256": campaign.directory_hash(workspace)["sha256"]}
        campaign.write_new(directory / "case.json", record)
        assignment.update({"run_dir": str(directory), "oracle_sha256": fingerprint(oracle_body),
                           "public_workspace_sha256": record["public_workspace_sha256"]})
    plan.pop("plan_sha256")
    plan["plan_sha256"] = fingerprint(plan)
    campaign.write_new(root / "plan.json", plan)
    return root, plan


def claude_result(spec, *, answer=ANSWER):
    events = [init(skills=["doctor", "plugin-authoring"] + (["reposcout"] if spec.skill_dir else [])),
              assistant("msg_1", [tool_use("toolu_1", "Bash", {"command": "git diff --stat"})], usage(5, 7, 11, 1)),
              tool_result("toolu_1", "1 file changed", details={"stdout": "1 file changed", "interrupted": False}),
              assistant("msg_2", [tool_use("toolu_2", "StructuredOutput", answer)], usage(5, 7, 11, 29)),
              tool_result("toolu_2", "Structured output provided successfully"),
              result(usage(10, 14, 22, 30), structured_output=answer)]
    stdout = spec.artifact_dir / "stdout.jsonl"
    stdout.write_text("".join(json.dumps(event) + "\n" for event in events))
    answer_path = spec.artifact_dir / "answer.json"
    campaign.write_new(answer_path, answer)
    return {"run_id": spec.run_id, "runtime": "claude", "status": "completed", "process_status": "completed",
            "thread_id": events[0]["session_id"], "resumed_thread_id": None, "returncode": 0, "exit_code": 0,
            "stream_complete": True, "process_tree_drained": True, "stdout_truncated": False,
            "termination_reason": None, "stdout_path": str(stdout), "answer_path": str(answer_path),
            "stdout_bytes": stdout.stat().st_size, "stdout_sha256": campaign.file_hash(stdout),
            "cli_version": spec.cli_version, "usage_scope": "invocation", "usage_baseline": None,
            "answer_validation": {"valid": True}, "preflight": {"passed": True}}


class ClaudeCampaignTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.parent = Path(self.temporary.name)

    def test_plan_records_runtime_model_and_effort_and_loads(self):
        root, plan = prepared(self.parent)
        self.assertEqual((plan["runtime"], plan["model"], plan["effort"]), ("claude", "claude-haiku-5-5", "medium"))
        self.assertEqual(plan["kind"], "codex-review-campaign-plan")
        self.assertEqual(campaign.load_plan(root)["plan_sha256"], plan["plan_sha256"])

    def test_codex_plans_carry_no_runtime_field(self):
        from test_review_campaign import catalog as codex_catalog, pins as codex_pins
        plan = campaign.build_plan("smoke", 917, codex_pins(), codex_catalog())
        self.assertNotIn("runtime", plan)
        self.assertEqual(plan["model"], campaign.MODEL)

    def test_resumed_episodes_are_refused_for_claude(self):
        from test_review_campaign import catalog as codex_catalog
        with self.assertRaisesRegex(InvalidLedger, "never resumed"):
            campaign.build_plan("smoke", 917, claude_pins(), codex_catalog(), effort="medium")

    def test_runtime_and_model_tampering_are_refused(self):
        for change in ({"runtime": "codex"}, {"model": campaign.MODEL}):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as temporary:
                root, plan = prepared(temporary)
                plan.update(change)
                plan.pop("plan_sha256")
                plan["plan_sha256"] = fingerprint(plan)
                (root / "plan.json").write_text(json.dumps(plan))
                with self.assertRaises(InvalidLedger):
                    campaign.load_plan(root)

    def test_trial_spec_pins_claude_inputs_and_carries_only_the_credential_path(self):
        _, plan = prepared(self.parent)
        assignment = next(item for item in plan["assignments"] if item["variant"] == "reposcout")
        record = read_json(Path(assignment["run_dir"]) / "case.json")
        auth = self.parent / "credentials.json"
        spec, hashes = campaign._trial_spec(plan, assignment, record, record["steps"][0], self.parent / "artifacts",
                                            auth_file=auth)
        self.assertEqual((type(spec).__name__, spec.model, spec.effort), ("ClaudeTrialSpec", "claude-haiku-5-5", "medium"))
        self.assertEqual(spec.auth_file, auth)
        self.assertEqual(spec.expected_claude_sha256, plan["pins"]["claude_sha256"])
        self.assertEqual(spec.expected_sandbox_tools_sha256, plan["pins"]["sandbox_tools_sha256"])
        self.assertEqual(spec.required_runtimes, ("python3",))
        self.assertEqual(hashes["capabilities_sha256"], assignment["capabilities_sha256"])
        with self.assertRaises(InvalidLedger):
            campaign._trial_spec(plan, assignment, record, record["steps"][0], self.parent / "artifacts", "thread")

    def test_guided_trial_spec_receives_the_pinned_skill_and_directed_prompt(self):
        _, plan = prepared(self.parent, treatment="reposcout-guided")
        assignment = next(item for item in plan["assignments"] if item["variant"] != "baseline")
        self.assertEqual(assignment["variant"], "reposcout-guided")
        record = read_json(Path(assignment["run_dir"]) / "case.json")
        spec, hashes = campaign._trial_spec(plan, assignment, record, record["steps"][0], self.parent / "artifacts")
        self.assertEqual(spec.skill_dir, Path(plan["pins"]["skill_dir"]))
        self.assertEqual(spec.expected_skill_sha256, plan["pins"]["skill"]["sha256"])
        self.assertEqual(spec.expected_reposcout_sha256, plan["pins"]["reposcout_sha256"])
        self.assertTrue(spec.prompt.endswith(campaign.CAPABILITIES["reposcout-guided"]))
        self.assertEqual(hashes["capabilities_sha256"], assignment["capabilities_sha256"])

    def test_cli_forwards_the_selected_treatment(self):
        base = ["review_campaign.py", "prepare", str(self.parent / "never"), "--stage", "large-development",
                "--seed", "1", "--reposcout-binary", "/unexecuted/reposcout", "--runtime", "claude",
                "--claude-binary", "/unexecuted/claude", "--model", "claude-haiku-5-5"]
        for extra, expected in (([], "reposcout"), (["--treatment", "reposcout-guided"], "reposcout-guided")):
            with self.subTest(expected=expected), patch("sys.argv", base + extra), \
                    patch.object(campaign, "prepare", return_value={"plan_sha256": "0" * 64, "assignment_count": 12}) as prepare, \
                    patch("sys.stdout"):
                campaign.main()
            self.assertEqual(prepare.call_args.kwargs["treatment"], expected)
        with patch("sys.argv", base + ["--treatment", "reposcout-cli"]), patch.object(campaign, "prepare") as prepare, \
                patch("sys.stderr"), self.assertRaises(SystemExit):
            campaign.main()
        prepare.assert_not_called()

    def test_execution_uses_claude_accounting_without_staging_credentials(self):
        root, plan = prepared(self.parent)
        auth = self.parent / "credentials.json"
        auth.write_text("synthetic credential path; read only by the runner")
        seen = []

        def runner(spec, *, preflight_only=False):
            controller = Path(spec.controller_dir)
            seen.append((preflight_only, sorted(item.name for item in controller.iterdir()), spec.auth_file))
            if preflight_only:
                return {"status": "preflight-passed", "preflight": {"passed": True}, "process_tree_drained": True}
            return claude_result(spec)

        with patch("review_grading.grade_episode", return_value={"synthetic": True}):
            summary = campaign.run(root, auth_file=auth, limit=1, runner=runner, check_pins=False)
        self.assertEqual([item["status"] for item in summary["runs"]], ["completed"])
        self.assertEqual(seen, [(True, [], None), (False, [], auth)])
        outcome = read_json(Path(plan["assignments"][0]["run_dir"]) / "outcome.json")
        self.assertEqual(outcome["accounting"]["kind"], "claude-stream-json-episode")
        self.assertEqual(outcome["accounting"]["observed_usage"],
                         {"input_tokens": 46, "cached_input_tokens": 22, "cache_write_input_tokens": 14,
                          "output_tokens": 30, "reasoning_output_tokens": None, "total_tokens": None})
        self.assertTrue(outcome["accounting_inventory_complete"])
        self.assertEqual(outcome["execution_evidence"][0]["exit_code"], 0)

    def test_claude_input_receipt_projects_to_hash_identities(self):
        identity = {"sha256": "1" * 64, "expected_sha256": "1" * 64, "matched": True}
        receipt = {"workspace": identity, "claude": identity, "controller_ca": identity, "reposcout": identity,
                   "skill": identity, "runtime_tools": identity, "sandbox_tools": identity,
                   "runtime_tool_assets": {"bash": "2" * 64}, "sandbox_tool_assets": {"bwrap": "3" * 64}}
        projected = project_input_receipt(receipt)
        self.assertEqual(projected["sandbox_tool_assets"], {"bwrap": "3" * 64})
        self.assertEqual(projected["claude"], identity)

    def test_cli_requires_runtime_specific_pins(self):
        cases = [["--runtime", "claude", "--claude-binary", "/unexecuted/claude"],
                 ["--runtime", "claude", "--claude-binary", "/unexecuted/claude", "--model", "claude-haiku-5-5",
                  "--codex-binary", "/unexecuted/codex"],
                 ["--codex-binary", "/unexecuted/codex", "--codex-version", "v", "--model", "claude-haiku-5-5"]]
        for arguments in cases:
            argv = ["review_campaign.py", "prepare", str(self.parent / "never"), "--stage", "large-development",
                    "--seed", "1", "--reposcout-binary", "/unexecuted/reposcout", *arguments]
            with self.subTest(arguments=arguments), patch("sys.argv", argv), \
                    patch.object(campaign, "prepare") as prepare, patch("sys.stderr"), self.assertRaises(SystemExit):
                campaign.main()
            prepare.assert_not_called()


if __name__ == "__main__":
    unittest.main()
