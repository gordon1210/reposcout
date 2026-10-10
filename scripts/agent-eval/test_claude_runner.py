from dataclasses import replace
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from unittest import mock

from claude_isolation import CLAUDE_DESTINATION, LAUNCHER_SOURCE, prepare_isolation
from claude_runner import (
    MINIMAL_ANSWER, ClaudeAuthenticationError, ClaudeTrialSpec, load_oauth_token, run_trial,
)
from codex_isolation import REQUIRED_RUNTIME_TOOLS
from codex_runner import SerialLease
from review_cases import answer_schema
from test_claude_trace import SESSION, assistant, init, result, tool_result, tool_use, usage

TOKEN = "sk-ant-oat01-synthetic-access-token-0123456789"


def claude_spec(root, *, variant="baseline"):
    workspace = root / "public"
    workspace.mkdir()
    (workspace / "main.py").write_text("def total(value):\n    return value\n")
    (workspace / ".git/info").mkdir(parents=True)
    controller = root / "controller"
    controller.mkdir(mode=0o700)
    binary = root / "runtime/claude"
    binary.parent.mkdir(parents=True)
    binary.write_text("synthetic executable; never executed by these tests\n")
    binary.chmod(0o700)
    ca_bundle = root / "system-ca.pem"
    ca_bundle.write_text("synthetic public CA bundle; no TLS calls in these tests\n")
    auth = root / "credentials.json"
    auth.write_text(json.dumps({"claudeAiOauth": {"accessToken": TOKEN, "refreshToken": "refresh-never-used",
                                                  "expiresAt": int((time.time() + 7200) * 1000)}}))
    auth.chmod(0o600)
    reposcout = root / "tools/reposcout"
    reposcout.parent.mkdir()
    reposcout.write_text("synthetic\n")
    reposcout.chmod(0o700)
    skill = root / "skill"
    skill.mkdir()
    (skill / "SKILL.md").write_text("---\nname: reposcout\n---\n")
    return ClaudeTrialSpec(
        run_id="case-1-" + variant, workspace=workspace, artifact_dir=root / "artifacts",
        controller_dir=controller, claude_binary=binary, cli_version="2.1.295 (Claude Code)",
        prompt="Review the two supplied trees.", answer_schema=answer_schema(), model="claude-haiku-5-5",
        effort="low", variant=variant, auth_file=auth, controller_ca_file=ca_bundle,
        reposcout_binary=None if variant == "baseline" else reposcout,
        skill_dir=skill if variant == "reposcout" else None,
        runtime_tool_paths={name: str((Path("/usr/bin") / name).resolve()) for name in REQUIRED_RUNTIME_TOOLS},
    )


def model_events(answer=MINIMAL_ANSWER, *, tools=None, final=True):
    events = [
        init(skills=["doctor", "plugin-authoring"], **({"tools": tools} if tools else {})),
        assistant("msg_1", [tool_use("toolu_1", "Bash", {"command": "git status"})]),
        tool_result("toolu_1", "clean", details={"stdout": "clean", "interrupted": False}),
        assistant("msg_2", [tool_use("toolu_2", "StructuredOutput", answer)]),
        tool_result("toolu_2", "Structured output provided successfully"),
    ]
    if final:
        events.append(result(usage(10, 14, 22, 30), structured_output=answer))
    return events


class FakeExecutor:
    def __init__(self, events, *, leak=None):
        self.events = events
        self.leak = leak
        self.calls = []

    def __call__(self, command, **kwargs):
        self.calls.append((command, kwargs))
        stdout, stderr = Path(kwargs["stdout_path"]), Path(kwargs["stderr_path"])
        stdout.write_text("".join(json.dumps(event) + "\n" for event in self.events))
        stderr.write_bytes(b"")
        if self.leak is not None:
            self.leak(command)
        return {"status": "completed", "returncode": 0, "duration_ms": 25, "peak_rss_bytes": 4096,
                "termination_reason": None, "stream_complete": True, "process_tree_drained": True,
                "stdout_truncated": False, "stderr_truncated": False,
                "stdout_total_bytes": stdout.stat().st_size, "stderr_total_bytes": 0,
                "ownership_complete": True, "owned_processes": []}


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.spec = claude_spec(self.root)
        self.spec.artifact_dir.mkdir(mode=0o700)
        self.plans = []

        def prepare(spec):
            plan = prepare_isolation(spec)
            self.plans.append(plan)
            return plan

        def preflight(spec, executor, artifacts, outcome):
            plan = prepare(spec)
            receipt = {"schema": 1, "passed": self.preflight_passes, "profile_sha256": plan.profile_sha256,
                       "input_receipt": plan.input_receipt, "checks": {}}
            return plan, receipt

        self.preflight_passes = True
        lease = self.root / "serial.lock"
        patches = [mock.patch("claude_runner.SerialLease", side_effect=lambda: SerialLease(lease)),
                   mock.patch("claude_runner.prepare_isolation", side_effect=prepare),
                   mock.patch("claude_runner._run_preflight", side_effect=preflight)]
        for patcher in patches:
            patcher.start()
            self.addCleanup(patcher.stop)

    def tearDown(self):
        for plan in self.plans:
            plan.cleanup()
        self.temporary.cleanup()

    def run_model(self, events, **options):
        fake = FakeExecutor(events, **options)
        return run_trial(self.spec, executor=fake), fake

    def test_completed_invocation_validates_answer_and_passes_token_only_on_stdin(self):
        outcome, fake = self.run_model(model_events())
        self.assertEqual(outcome["status"], "completed", outcome.get("termination_reason"))
        self.assertEqual(outcome["thread_id"], SESSION)
        self.assertEqual(json.loads(Path(outcome["answer_path"]).read_text()), MINIMAL_ANSWER)
        self.assertEqual(outcome["answer_validation"], {"valid": True})
        command, options = fake.calls[0]
        self.assertEqual(command[:5], ["/usr/bin/python3", "-I", "-S", "-c", LAUNCHER_SOURCE])
        self.assertIn(CLAUDE_DESTINATION, json.loads(command[5]))
        self.assertNotIn(TOKEN, json.dumps(command))
        self.assertNotIn("ANTHROPIC_BASE_URL", json.loads(command[5]))
        self.assertTrue(options["input_bytes"].startswith(TOKEN.encode() + b"\n"))
        self.assertNotIn(TOKEN, json.dumps(outcome))
        self.assertNotIn(TOKEN, (self.spec.artifact_dir / "lifecycle.json").read_text())

    def test_sandbox_placeholders_stay_out_of_fixture_git_status(self):
        from claude_isolation import SANDBOX_PLACEHOLDERS
        plan = prepare_isolation(self.spec)
        self.plans.append(plan)
        exclude = (plan.root / "workspace/.git/info/exclude").read_text().splitlines()
        self.assertTrue({"/.claude/", "/.zshrc", "/.mcp.json"} <= set(exclude))
        self.assertEqual({line for line in exclude if line} - {"/.claude/"},
                         {"/" + name for name in SANDBOX_PLACEHOLDERS})

    def test_schema_failure_is_a_retained_failed_outcome(self):
        invalid = dict(MINIMAL_ANSWER, conclusion="maybe")
        outcome, _ = self.run_model(model_events(invalid))
        self.assertEqual(outcome["status"], "failed")
        self.assertEqual(outcome["termination_reason"], "answer-schema-invalid")
        self.assertIsNone(outcome["answer_path"])
        self.assertEqual(json.loads(Path(outcome["retained_answer_path"]).read_text()), invalid)
        self.assertFalse(outcome["answer_validation"]["valid"])
        self.assertNotIn("campaign_fatal", outcome)

    def test_missing_final_result_fails_without_answer(self):
        outcome, _ = self.run_model(model_events(final=False))
        self.assertEqual(outcome["status"], "failed")
        self.assertEqual(outcome["termination_reason"], "claude-result-error")
        self.assertIsNone(outcome["answer_path"])

    def test_unexpected_runtime_inventory_is_campaign_fatal(self):
        outcome, _ = self.run_model(model_events(tools=["Agent", "Bash", "Glob", "Grep", "Read", "Skill",
                                                        "StructuredOutput"]))
        self.assertEqual(outcome["termination_reason"], "runtime-isolation-violation")
        self.assertTrue(outcome["campaign_fatal"])
        self.assertFalse(outcome["runtime_isolation_checks"]["session_tools"])

    def test_token_persisted_by_the_runtime_is_campaign_fatal(self):
        def leak(command):
            plan = self.plans[-1]
            (plan.root / "home/leaked.txt").write_text(TOKEN)

        outcome, _ = self.run_model(model_events(), leak=leak)
        self.assertEqual(outcome["termination_reason"], "credential-exposure")
        self.assertTrue(outcome["campaign_fatal"])
        self.assertEqual(outcome["exposure_locations"], ["home/leaked.txt"])
        self.assertIsNone(outcome["answer_path"])

    def test_failed_preflight_never_reads_credentials_or_starts_a_model(self):
        self.preflight_passes = False
        with mock.patch("claude_runner.load_oauth_token") as loaded:
            outcome, fake = self.run_model(model_events())
        self.assertEqual(outcome["termination_reason"], "isolation-preflight-failed")
        self.assertTrue(outcome["campaign_fatal"])
        self.assertEqual(fake.calls, [])
        loaded.assert_not_called()

    def test_preflight_only_needs_no_credentials(self):
        spec = replace(self.spec, auth_file=None)
        fake = FakeExecutor(model_events())
        outcome = run_trial(spec, preflight_only=True, executor=fake)
        self.assertEqual(outcome["status"], "preflight-passed")
        self.assertEqual(fake.calls, [])

    def test_resume_is_rejected_before_execution(self):
        with self.assertRaises(ValueError):
            run_trial(replace(self.spec, resume_thread_id=SESSION), executor=FakeExecutor([]))


class TokenTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.path = Path(self.temporary.name) / "credentials.json"

    def write(self, value, mode=0o600):
        self.path.write_text(value if isinstance(value, str) else json.dumps(value))
        self.path.chmod(mode)

    def test_reads_only_the_unexpired_access_token(self):
        self.write({"claudeAiOauth": {"accessToken": TOKEN, "refreshToken": "never-used",
                                      "expiresAt": int((time.time() + 3600) * 1000)}})
        self.assertEqual(load_oauth_token(self.path, valid_for_seconds=600), TOKEN.encode())

    def test_failures_never_echo_contents(self):
        cases = [
            ({"claudeAiOauth": {"accessToken": TOKEN, "expiresAt": int((time.time() + 60) * 1000)}}, 0o600),
            ({"claudeAiOauth": {"accessToken": TOKEN, "expiresAt": int((time.time() + 3600) * 1000)}}, 0o640),
            ({"other": TOKEN}, 0o600),
            ("not a token\n" + TOKEN, 0o600),
        ]
        for value, mode in cases:
            with self.subTest(mode=mode):
                self.write(value, mode)
                with self.assertRaises(ClaudeAuthenticationError) as caught:
                    load_oauth_token(self.path, valid_for_seconds=600)
                self.assertNotIn(TOKEN, str(caught.exception))

    def test_symlinked_source_is_refused(self):
        self.write({"claudeAiOauth": {"accessToken": TOKEN, "expiresAt": int((time.time() + 3600) * 1000)}})
        link = Path(self.temporary.name) / "link.json"
        os.symlink(self.path, link)
        with self.assertRaises(ClaudeAuthenticationError):
            load_oauth_token(link, valid_for_seconds=600)


if __name__ == "__main__":
    unittest.main()
