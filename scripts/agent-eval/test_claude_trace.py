import json
from pathlib import Path
import tempfile
import unittest

from accounting import InvalidLedger
from claude_trace import (
    ADAPTER, aggregate_episode, extract_execution_evidence, map_usage, parse_exec_trace, structured_output,
)

SESSION = "7776f03d-0967-4787-86a5-37407d0be1ed"
TOOLS = ["Bash", "Glob", "Grep", "Read", "Skill", "StructuredOutput"]
ANSWER = {"conclusion": "insufficient-evidence", "findings": [], "evidence": [],
          "retained_evidence": [], "limitations": [], "validation": []}


def init(**extra):
    return {"type": "system", "subtype": "init", "session_id": SESSION, "tools": TOOLS, "mcp_servers": [],
            "skills": ["reposcout", "doctor", "plugin-authoring"], "permissionMode": "dontAsk",
            "plugins": [{"name": "cc-plugin-agents-md"}, {"name": "cc-plugin-plugin-authoring"}],
            "model": "claude-haiku-5-5", "cwd": "/workspace", "claude_code_version": "2.1.295", **extra}


def usage(input_tokens=5, creation=7, read=11, output=1):
    return {"input_tokens": input_tokens, "cache_creation_input_tokens": creation,
            "cache_read_input_tokens": read, "output_tokens": output}


def assistant(message_id, blocks, message_usage=None, timestamp="2026-10-09T18:55:09.868Z"):
    return {"type": "assistant", "session_id": SESSION, "parent_tool_use_id": None, "timestamp": timestamp,
            "message": {"id": message_id, "model": "claude-haiku-5-5", "content": blocks,
                        "usage": message_usage or usage()}}


def tool_use(tool_id, name, arguments):
    return {"type": "tool_use", "id": tool_id, "name": name, "input": arguments}


def tool_result(tool_id, content, *, error=False, details=None, decision="accept",
                timestamp="2026-10-09T18:55:10.000Z"):
    block = {"type": "tool_result", "tool_use_id": tool_id, "content": content}
    if error:
        block["is_error"] = True
    record = {"type": "user", "session_id": SESSION, "parent_tool_use_id": None, "timestamp": timestamp,
              "message": {"role": "user", "content": [block]},
              "tool_result_meta": [{"id": tool_id, "permission_decision": {"decision": decision}}]}
    if details is not None:
        record["tool_use_result"] = details
    return record


def result(total=None, **extra):
    total = total or usage(10, 14, 22, 30)
    return {"type": "result", "subtype": "success", "is_error": False, "session_id": SESSION, "usage": total,
            "modelUsage": {"claude-haiku-5-5": {
                "inputTokens": total["input_tokens"], "outputTokens": total["output_tokens"],
                "cacheReadInputTokens": total["cache_read_input_tokens"],
                "cacheCreationInputTokens": total["cache_creation_input_tokens"]}},
            "subagent_stats": {"spawned": 0}, "permission_denials": [], "num_turns": 2,
            "structured_output": ANSWER, "total_cost_usd": 0.0001, **extra}


def standard_events():
    return [
        init(),
        assistant("msg_1", [tool_use("toolu_1", "Bash", {"command": "git diff --stat"})]),
        tool_result("toolu_1", "1 file changed", details={"stdout": "1 file changed", "interrupted": False}),
        assistant("msg_2", [{"type": "text", "text": "done"}]),
        assistant("msg_2", [tool_use("toolu_2", "StructuredOutput", ANSWER)]),
        tool_result("toolu_2", "Structured output provided successfully"),
        result(),
    ]


class TraceCase(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)

    def trace(self, events, name="stdout.jsonl"):
        path = Path(self.directory.name) / name
        path.write_text("".join(json.dumps(event) + "\n" for event in events))
        return path

    def controller(self, path, **extra):
        from codex_trace import _read_trace
        _, digest, size, _ = _read_trace(path)
        return {"run_id": "run-1", "invocation_id": "invocation-1", "status": "completed", "process_status": "completed",
                "returncode": 0, "stdout_sha256": digest, "stdout_bytes": size, "thread_id": SESSION,
                "resumed_thread_id": None, "stream_complete": True, "process_tree_drained": True,
                "stdout_truncated": False, "termination_reason": None, "model": "claude-haiku-5-5",
                "cli_version": "2.1.295 (Claude Code)", **extra}


class UsageMappingTest(unittest.TestCase):
    def test_input_includes_both_cache_partitions(self):
        self.assertEqual(map_usage(usage(5, 7, 11, 3)), {
            "input_tokens": 23, "cached_input_tokens": 11, "cache_write_input_tokens": 7,
            "output_tokens": 3, "reasoning_output_tokens": None, "total_tokens": None})

    def test_unknown_partition_keeps_input_unknown(self):
        mapped = map_usage({"input_tokens": 5, "cache_creation_input_tokens": None,
                            "cache_read_input_tokens": 11, "output_tokens": 3})
        self.assertIsNone(mapped["input_tokens"])
        self.assertIsNone(mapped["cache_write_input_tokens"])
        self.assertEqual(mapped["cached_input_tokens"], 11)


class ParseTraceTest(TraceCase):
    def test_duplicate_message_usage_is_counted_once(self):
        path = self.trace(standard_events())
        evidence = parse_exec_trace(path, controller=self.controller(path))
        self.assertEqual(evidence["adapter"], ADAPTER)
        self.assertEqual(evidence["provider_message_count"], 2)
        self.assertEqual(evidence["observed_usage"]["input_tokens"], 46)
        self.assertEqual(evidence["observed_usage"]["cached_input_tokens"], 22)
        self.assertEqual(evidence["observed_usage"]["cache_write_input_tokens"], 14)
        self.assertEqual(evidence["observed_usage"]["output_tokens"], 30)
        self.assertTrue(all(item["matched"] for item in evidence["usage_cross_check"].values()))
        self.assertEqual(evidence["validation_errors"], [])
        self.assertTrue(evidence["controller_closed"])
        self.assertTrue(evidence["comparable_usage"])
        self.assertEqual(evidence["thread_id"], SESSION)
        self.assertEqual(evidence["structured_output_attempts"], 1)
        self.assertEqual(evidence["command_statistics"]["completed_command_items"], 1)

    def test_changed_usage_for_one_message_is_invalid(self):
        events = standard_events()
        events[4] = assistant("msg_2", [tool_use("toolu_2", "StructuredOutput", ANSWER)], usage(6, 7, 11, 1))
        path = self.trace(events)
        evidence = parse_exec_trace(path, controller=self.controller(path))
        self.assertIn("message-usage-changed", [error["code"] for error in evidence["validation_errors"]])
        self.assertFalse(evidence["comparable_usage"])

    def test_assistant_sum_disagreeing_with_result_is_not_comparable(self):
        events = standard_events()
        events[-1] = result(usage(99, 14, 22, 30))
        path = self.trace(events)
        evidence = parse_exec_trace(path, controller=self.controller(path))
        codes = [error["code"] for error in evidence["validation_errors"]]
        self.assertIn("assistant-usage-disagrees-with-result:input_tokens", codes)
        self.assertFalse(evidence["comparable_usage"])

    def test_tool_errors_project_exit_codes_and_file_tools(self):
        events = [
            init(),
            assistant("msg_1", [tool_use("toolu_1", "Bash", {"command": "cargo test"})]),
            tool_result("toolu_1", "Exit code 101\nfailed", error=True, details="Error: Exit code 101\nfailed"),
            assistant("msg_2", [tool_use("toolu_2", "Bash", {"command": "cat /controller/x"})]),
            tool_result("toolu_2", "Permission denied", error=True, decision="reject"),
            assistant("msg_3", [tool_use("toolu_3", "Read", {"file_path": "/workspace/src/a.py", "offset": 4, "limit": 2})]),
            tool_result("toolu_3", "4\tline\n5\tline"),
            assistant("msg_4", [tool_use("toolu_4", "Read", {"file_path": "/workspace/missing.py"})]),
            tool_result("toolu_4", "File does not exist.", error=True),
            assistant("msg_5", [tool_use("toolu_5", "Skill", {"skill": "reposcout"})]),
            tool_result("toolu_5", "Launching skill: reposcout"),
            assistant("msg_6", [tool_use("toolu_6", "Bash", {"command": "reposcout review-context --brief"})]),
            tool_result("toolu_6", "brief", details={"stdout": "brief", "interrupted": False}),
            result(usage(30, 42, 66, 30)),
        ]
        path = self.trace(events)
        evidence = parse_exec_trace(path, controller=self.controller(path))
        commands = {command["item_id"]: command for command in evidence["commands"]}
        self.assertEqual(commands["toolu_1"]["exit_code"], 101)
        self.assertTrue(commands["toolu_1"]["failed_command"])
        self.assertIsNone(commands["toolu_2"]["exit_code"])
        self.assertEqual(commands["toolu_2"]["permission_decision"], "reject")
        self.assertEqual(commands["toolu_3"]["kind"], "file-tool")
        self.assertEqual(commands["toolu_3"]["path"], "/workspace/src/a.py")
        self.assertEqual(commands["toolu_3"]["range"], {"offset": 4, "limit": 2})
        self.assertTrue(commands["toolu_3"]["contains_source_read_command_marker"])
        self.assertTrue(commands["toolu_4"]["failed_command"])
        self.assertEqual(commands["toolu_5"]["recognized_skill_read_invocations"], 1)
        self.assertEqual(commands["toolu_6"]["recognized_reposcout_invocations"], 1)
        statistics = evidence["command_statistics"]
        self.assertEqual(statistics["failed_command_items"], 3)
        self.assertEqual(statistics["permission_denied_items"], 1)
        self.assertEqual(statistics["tool_call_counts"], {"Bash": 3, "Read": 2, "Skill": 1})
        receipts = extract_execution_evidence(path, step_id=0)
        self.assertEqual([(item["command"], item["exit_code"]) for item in receipts],
                         [("cargo test", 101), ("reposcout review-context --brief", 0)])

    def test_missing_final_result_is_incomplete_and_unknown(self):
        path = self.trace(standard_events()[:-1])
        evidence = parse_exec_trace(path, controller=self.controller(path, status="failed", process_status="failed",
                                                                      returncode=1))
        codes = [error["code"] for error in evidence["validation_errors"]]
        self.assertIn("missing-final-result", codes)
        self.assertTrue(all(value is None for value in evidence["observed_usage"].values()))
        self.assertFalse(evidence["comparable_usage"])
        self.assertIsNone(structured_output(path))

    def test_unclosed_tool_use_is_reported(self):
        events = standard_events()[:2]
        path = self.trace(events)
        evidence = parse_exec_trace(path)
        codes = [error["code"] for error in evidence["validation_errors"]]
        self.assertIn("unclosed-tool-uses", codes)
        self.assertEqual(len(evidence["incomplete_commands"]), 1)

    def test_subagent_and_unavailable_tool_events_are_errors(self):
        events = standard_events()
        side = assistant("msg_side", [{"type": "text", "text": "x"}])
        side["parent_tool_use_id"] = "toolu_1"
        events.insert(3, side)
        events.insert(3, assistant("msg_agent", [tool_use("toolu_agent", "Agent", {"prompt": "x"})]))
        events.insert(4, tool_result("toolu_agent", "No such tool available: Agent", error=True))
        events[-1] = result(usage(25, 35, 55, 30), subagent_stats={"spawned": 1})
        path = self.trace(events)
        evidence = parse_exec_trace(path, controller=self.controller(path))
        codes = [error["code"] for error in evidence["validation_errors"]]
        self.assertIn("subagent-event", codes)
        self.assertIn("unavailable-tool-use:Agent", codes)
        self.assertIn("subagent-spawned-or-unreported", codes)
        self.assertFalse(evidence["comparable_usage"])

    def test_foreign_session_and_duplicate_tool_identity_are_errors(self):
        events = standard_events()
        foreign = dict(events[1], session_id="other")
        events.insert(2, foreign)
        events.insert(2, assistant("msg_dup", [tool_use("toolu_1", "Bash", {"command": "ls"})]))
        path = self.trace(events)
        codes = [error["code"] for error in parse_exec_trace(path)["validation_errors"]]
        self.assertIn("foreign-session-event", codes)
        self.assertIn("duplicate-or-invalid-tool-use-id", codes)

    def test_answer_failure_keeps_closure_but_not_comparability(self):
        path = self.trace(standard_events())
        evidence = parse_exec_trace(path, controller=self.controller(
            path, status="failed", termination_reason="answer-schema-invalid"))
        self.assertTrue(evidence["controller_closed"])
        self.assertFalse(evidence["comparable_usage"])

    def test_thinking_estimates_and_permission_denials_keep_usage_comparable(self):
        events = standard_events()
        events[2:2] = [
            {"type": "system", "subtype": "thinking_tokens", "session_id": SESSION, "uuid": "u1",
             "estimated_tokens": 120, "estimated_tokens_delta": 120},
            {"type": "system", "subtype": "permission_denied", "session_id": SESSION, "tool_name": "Bash",
             "tool_use_id": "toolu_x", "decision_reason_type": "mode", "message": "denied"},
            {"type": "system", "subtype": "unannounced", "session_id": SESSION},
        ]
        path = self.trace(events)
        evidence = parse_exec_trace(path, controller=self.controller(path))
        self.assertEqual(evidence["unknown_event_types"], ["system:unannounced"])
        self.assertFalse(evidence["comparable_usage"])
        del events[4]
        path = self.trace(events, "known.jsonl")
        evidence = parse_exec_trace(path, controller=self.controller(path))
        self.assertEqual(evidence["unknown_event_types"], [])
        self.assertTrue(evidence["comparable_usage"])
        self.assertEqual((evidence["thinking_estimate_events"], evidence["permission_denied_events"]), (1, 1))
        self.assertEqual(evidence["observed_usage"]["output_tokens"], 30)

    def test_unexpected_model_usage_is_invalid(self):
        events = standard_events()
        total = usage(10, 14, 22, 30)
        events[-1] = result(total, modelUsage={
            "claude-haiku-5-5": {"inputTokens": 5, "outputTokens": 15, "cacheReadInputTokens": 11,
                                 "cacheCreationInputTokens": 7},
            "claude-sonnet-5-5": {"inputTokens": 5, "outputTokens": 15, "cacheReadInputTokens": 11,
                                  "cacheCreationInputTokens": 7}})
        path = self.trace(events)
        codes = [error["code"] for error in parse_exec_trace(path, controller=self.controller(path))["validation_errors"]]
        self.assertIn("unexpected-model-usage", codes)

    def test_episode_aggregates_single_invocation_and_rejects_resume(self):
        path = self.trace(standard_events())
        trace = parse_exec_trace(path, controller=self.controller(path))
        episode = aggregate_episode([trace])
        self.assertEqual(episode["kind"], "claude-stream-json-episode")
        self.assertEqual(episode["observed_usage"]["input_tokens"], 46)
        self.assertTrue(episode["comparable_usage"])
        second = self.trace(standard_events(), "second.jsonl")
        other = parse_exec_trace(second, controller=self.controller(second, invocation_id="invocation-2"))
        self.assertIn("claude-resume-unsupported", aggregate_episode([trace, other])["episode_errors"])
        with self.assertRaises(InvalidLedger):
            aggregate_episode([dict(trace, adapter="codex-exec-jsonl-v1")])


if __name__ == "__main__":
    unittest.main()
