import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest

import adoption_eval
from adoption_eval import (
    ADOPTION_SCHEMA, ARMS, CASES, CURRENT, AdoptionError, build_prompt, classify, execute_run, install_description,
    load_phrasings, plan_runs, read_data, read_description, resolve_description, select_phrasing, summarize,
    text_sha256, wilson,
)
from claude_isolation import claude_arguments
from codex_isolation import directory_sha256
from review_campaign import CAPABILITIES
from test_claude_trace import assistant, init, result, tool_result, tool_use, usage

SKILL = adoption_eval.ROOT / "skills/reposcout"
DEV = ["Review this PR.", "Check this branch before merge.", "What could these commits break?"]
TUNING = ["Is this change safe to ship?", "Do a code review of base..head and list real defects.",
          "Walk me through what changed between these two revisions and flag risks.",
          "Audit the diff for regressions."]
HELD_OUT = ["Any concerns with merging this?", "Give me a quick rundown of these commits and anything that looks off.",
            "Find the bugs introduced between base and head.", "I'm about to deploy this. Anything I should know?"]


def stream(events):
    return "".join(json.dumps(event) + "\n" for event in events)


class PhrasingTests(unittest.TestCase):
    def test_split_is_explicit(self):
        data = read_data("phrasings.json")
        self.assertEqual([item["text"] for item in data["dev"]], DEV)
        self.assertEqual([item["text"] for item in data["tuning"]], TUNING)
        self.assertEqual([item["text"] for item in data["held_out"]], HELD_OUT)
        phrasings = load_phrasings()
        sets = {name: [item["id"] for item in phrasings.values() if item["set"] == name]
                for name in ("dev", "tuning", "held-out")}
        self.assertEqual(sets, {"dev": ["d1", "d2", "d3"], "tuning": ["h1", "h2", "h3", "h4"],
                                "held-out": ["h5", "h6", "h7", "h8"]})

    def test_held_out_requires_the_flag(self):
        phrasings = load_phrasings()
        self.assertEqual(select_phrasing(phrasings, "d1", held_out=False)["text"], DEV[0])
        self.assertEqual(select_phrasing(phrasings, "h1", held_out=False)["text"], TUNING[0])
        with self.assertRaises(AdoptionError) as caught:
            select_phrasing(phrasings, "h5", held_out=False)
        self.assertIn("--held-out", str(caught.exception))
        self.assertEqual(select_phrasing(phrasings, "h5", held_out=True)["text"], HELD_OUT[0])
        with self.assertRaises(AdoptionError):
            select_phrasing(phrasings, "missing", held_out=True)

    def test_run_command_refuses_held_out_before_touching_anything(self):
        with tempfile.TemporaryDirectory() as root:
            output = Path(root) / "out"
            argv = ["run", "--model", "claude-haiku-5-5", "--arm", "silent", "--description", CURRENT,
                    "--case", CASES[0], "--phrasing", "h5", "--repeat", "0", "--reposcout-binary", "/nonexistent",
                    "--auth-file", "/nonexistent", "--output-dir", str(output), "--claude-binary", "/nonexistent"]
            errors = io.StringIO()
            with contextlib.redirect_stderr(errors):
                self.assertEqual(adoption_eval.main(argv), 2)
            self.assertIn("held-out", errors.getvalue())
            self.assertFalse(output.exists())

    def test_plan_refuses_the_held_out_set_without_the_flag(self):
        argv = ["plan", "--seed", "1", "--phrasing-set", "held-out"]
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(adoption_eval.main(argv), 2)


class PromptTests(unittest.TestCase):
    def test_silent_prompt_never_names_the_tool(self):
        for phrasing in (*DEV, *HELD_OUT):
            prompt = build_prompt("silent", phrasing, "a" * 40, "b" * 40)
            lowered = prompt.lower()
            for word in ("reposcout", "skill", "cli"):
                self.assertNotIn(word, lowered)
            lines = prompt.split("\n")
            self.assertEqual(lines[0], phrasing)
            self.assertEqual(lines[1], f"base={'a' * 40} head={'b' * 40}")
            self.assertEqual(len(lines), 3)

    def test_neutral_prompt_adds_exactly_the_campaign_sentence(self):
        silent = build_prompt("silent", DEV[0], "a" * 40, "b" * 40)
        neutral = build_prompt("neutral", DEV[0], "a" * 40, "b" * 40)
        self.assertEqual(neutral, silent + "\n\n" + CAPABILITIES["reposcout"])
        self.assertIn("RepoScout CLI and its bundled skill", neutral)

    def test_unknown_arm_is_refused(self):
        with self.assertRaises(AdoptionError):
            build_prompt("guided", DEV[0], "a", "b")

    def test_adoption_schema_accepts_the_preflight_answer(self):
        from claude_runner import MINIMAL_ANSWER
        self.assertTrue(set(ADOPTION_SCHEMA["required"]) <= set(MINIMAL_ANSWER))
        self.assertEqual(ARMS, ("silent", "neutral"))


class DescriptionTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def test_swap_changes_only_the_description_line(self):
        before = directory_sha256(SKILL)
        original = (SKILL / "SKILL.md").read_text()
        text = "Use when asked to review a PR: run `reposcout review-context` instead of `git diff`."
        copy = self.root / "reposcout"
        install_description(SKILL, copy, text)
        swapped = (copy / "SKILL.md").read_text()
        self.assertEqual(read_description(swapped), text)
        old, new = original.split("\n"), swapped.split("\n")
        self.assertEqual(len(old), len(new))
        differing = [index for index, (left, right) in enumerate(zip(old, new)) if left != right]
        self.assertEqual(len(differing), 1)
        self.assertTrue(new[differing[0]].startswith("description: "))
        self.assertEqual(sorted(path.relative_to(SKILL).as_posix() for path in SKILL.rglob("*") if path.is_file()),
                         sorted(path.relative_to(copy).as_posix() for path in copy.rglob("*") if path.is_file()))
        self.assertEqual(directory_sha256(SKILL), before)
        self.assertEqual(original, (SKILL / "SKILL.md").read_text())

    def test_unswapped_copy_is_byte_identical(self):
        copy = self.root / "reposcout"
        install_description(SKILL, copy, None)
        self.assertEqual(directory_sha256(copy), directory_sha256(SKILL))

    def test_current_is_read_from_the_skill(self):
        description = resolve_description(CURRENT, SKILL)
        self.assertIsNone(description["swap"])
        self.assertEqual(description["text"], read_description((SKILL / "SKILL.md").read_text()))
        self.assertIn("reposcout review-context", description["text"])
        self.assertEqual(len(text_sha256(description["text"])), 64)

    def test_candidates_are_bounded_and_round_trip(self):
        candidates = read_data("descriptions.json")["descriptions"]
        self.assertEqual(list(candidates), [CURRENT, "A", "B", "C", "D"])
        for name in ("A", "B", "C", "D"):
            description = resolve_description(name, SKILL)
            self.assertEqual(description["swap"], description["text"])
            self.assertLess(len(description["text"]), 1024)
            copy = self.root / name
            install_description(SKILL, copy, description["text"])
            self.assertEqual(read_description((copy / "SKILL.md").read_text()), description["text"])
        with self.assertRaises(AdoptionError):
            resolve_description("E", SKILL)

    def test_invalid_text_is_refused_before_copying(self):
        for text in ("", "two\nlines", "nul\x00byte"):
            with self.subTest(text=text), self.assertRaises(AdoptionError):
                install_description(SKILL, self.root / "bad", text)
        self.assertFalse((self.root / "bad").exists())


class PlanTests(unittest.TestCase):
    def runs(self, **options):
        arguments = {"models": ["claude-haiku-5-5"], "arms": ["silent"], "descriptions": [CURRENT, "A"],
                     "cases": list(CASES), "phrasing_set": "dev", "runs_per_cell": 12, "seed": 7,
                     "phrasings": load_phrasings()}
        return plan_runs(**{**arguments, **options})

    def test_plan_is_deterministic_and_seed_dependent(self):
        self.assertEqual(self.runs(), self.runs())
        self.assertNotEqual([run["run_id"] for run in self.runs()], [run["run_id"] for run in self.runs(seed=8)])
        self.assertEqual([run["index"] for run in self.runs()], list(range(24)))

    def test_cells_cover_every_combination_before_repeating(self):
        runs = [run for run in self.runs() if run["description"] == CURRENT]
        self.assertEqual(len(runs), 12)
        combinations = {(run["phrasing"], run["case"]) for run in runs}
        self.assertEqual(len(combinations), 9)
        self.assertEqual({run["repeat"] for run in runs}, {0, 1})
        self.assertEqual(len({run["run_id"] for run in self.runs()}), 24)

    def test_held_out_plan_uses_only_held_out_phrasings(self):
        runs = self.runs(phrasing_set="held-out", runs_per_cell=36)
        self.assertEqual({run["phrasing"] for run in runs}, {"h5", "h6", "h7", "h8"})
        self.assertEqual(len(runs), 72)
        self.assertEqual({run["phrasing"] for run in self.runs()}, {"d1", "d2", "d3"})
        self.assertEqual({run["phrasing"] for run in self.runs(phrasing_set="tuning")}, {"h1", "h2", "h3", "h4"})

    def test_commands_carry_the_explicit_parameters(self):
        entry = self.runs()[0]
        line = adoption_eval.run_command_line(entry, {
            "reposcout_binary": "/b/reposcout", "auth_file": "/a/creds.json", "output_dir": "/o", "max_turns": 6,
            "max_budget_usd": 0.05, "claude_binary": None, "held_out": False})
        for part in ("--model " + entry["model"], "--phrasing " + entry["phrasing"], "--max-turns 6",
                     "--max-budget-usd 0.05", "--auth-file /a/creds.json"):
            self.assertIn(part, line)
        self.assertNotIn("--held-out", line)


class SummaryTests(unittest.TestCase):
    def test_wilson_matches_the_published_table(self):
        low, high = wilson(0, 12)
        self.assertEqual((round(low, 2), round(high, 2)), (0.0, 0.24))
        low, high = wilson(10, 12)
        self.assertEqual((round(low, 2), round(high, 2)), (0.55, 0.95))
        low, high = wilson(28, 36)
        self.assertEqual((round(low, 2), round(high, 2)), (0.62, 0.88))
        self.assertEqual(wilson(0, 0), (None, None))

    def outcome(self, adopted, *, measurement="complete", capped=False, description="A", model="claude-haiku-5-5",
                arm="silent", phrasing="d1", turn=2):
        tokens = {"uncached_input_tokens": 3, "cache_read_input_tokens": 5, "cache_write_input_tokens": 7,
                  "output_tokens": 11, "source": "result"}
        return {"kind": "adoption-run", "model": model, "arm": arm, "measurement": measurement, "capped": capped,
                "phrasing": {"id": phrasing, "set": "dev" if phrasing.startswith("d") else "held-out"},
                "description": {"name": description, "text_sha256": text_sha256(description)},
                "adoption": {"adopted": adopted, "invoked": adopted, "adopted_before_diff": adopted,
                             "brief_called": adopted, "num_turns": 4, "tokens": tokens, "list_price_usd": 0.01,
                             "first_adoption_turn": turn if adopted else None}}

    def test_rates_intervals_and_token_totals(self):
        outcomes = [self.outcome(True), self.outcome(False, capped=True), self.outcome(True, turn=4),
                    self.outcome(False, measurement="incomplete"),
                    self.outcome(True, arm="neutral"), self.outcome(True, phrasing="h1")]
        rows = {(row["arm"], row["phrasing_set"]): row for row in summarize(outcomes)}
        self.assertEqual(len(rows), 3)
        row = rows[("silent", "dev")]
        self.assertEqual((row["runs"], row["adopted"], row["excluded"], row["capped"]), (3, 2, 1, 1))
        self.assertAlmostEqual(row["adoption_rate"], 2 / 3)
        self.assertEqual(row["wilson_95"], list(wilson(2, 3)))
        self.assertEqual((row["uncached_input_tokens"], row["cache_read_input_tokens"],
                          row["cache_write_input_tokens"], row["output_tokens"]), (9, 15, 21, 33))
        self.assertEqual(row["mean_first_adoption_turn"], 3)
        self.assertEqual(row["mean_turns"], 4)
        self.assertIn("2/3", adoption_eval.render_summary(list(rows.values())))

    def test_distinct_description_texts_are_not_pooled(self):
        changed = self.outcome(True)
        changed["description"]["text_sha256"] = text_sha256("another text")
        self.assertEqual(len(summarize([self.outcome(True), changed])), 2)


class ClassificationTests(unittest.TestCase):
    OBSERVED = {"result_subtype": "error_max_turns"}

    def test_a_capped_run_is_a_complete_measurement(self):
        capped = {"status": "capped", "termination_reason": "max-turns", "stream_complete": True,
                  "process_tree_drained": True}
        self.assertEqual(classify(capped, self.OBSERVED), "complete")

    def test_failures_without_a_closed_final_result_are_excluded(self):
        base = {"status": "failed", "stream_complete": True, "process_tree_drained": True}
        self.assertEqual(classify(base, {"result_subtype": None}), "incomplete")
        self.assertEqual(classify({**base, "stream_complete": False}, self.OBSERVED), "incomplete")
        self.assertEqual(classify({**base, "termination_reason": "runtime-isolation-violation"}, self.OBSERVED),
                         "incomplete")
        self.assertEqual(classify(base, None), "none")
        self.assertEqual(classify({**base, "termination_reason": "answer-schema-invalid"}, self.OBSERVED), "complete")


class ExecuteRunTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.specs = []
        self.binary = self.root / "reposcout"
        self.binary.write_text("synthetic\n")

    def parameters(self, **extra):
        return {"model": "claude-haiku-5-5", "arm": "silent", "description": "A", "case": CASES[0],
                "phrasing": "d1", "repeat": 0, "reposcout_binary": str(self.binary),
                "auth_file": str(self.root / "credentials.json"), "output_dir": str(self.root / "out"),
                "claude_binary": str(self.root / "claude"), "max_turns": 6, "max_budget_usd": None,
                "effort": "medium", "timeout_seconds": 60, **extra}

    def pins(self, claude, reposcout, runtimes):
        return {"claude_binary": Path(claude), "claude_sha256": "0" * 64, "claude_version": "2.1.295 (Claude Code)",
                "reposcout_binary": Path(reposcout), "reposcout_sha256": "1" * 64,
                "controller_ca_file": Path("/etc/ssl/cert.pem"), "controller_ca_sha256": "2" * 64,
                "runtime_tool_paths": {}, "runtime_tools_sha256": "3" * 64,
                "sandbox_tool_paths": {}, "sandbox_tools_sha256": "4" * 64}

    def runner(self, events, **overrides):
        def run(spec):
            self.specs.append(spec)
            self.copy = (Path(spec.skill_dir) / "SKILL.md").read_text()
            self.copy_exists = Path(spec.skill_dir)
            stdout = Path(spec.artifact_dir) / "stdout.jsonl"
            stdout.write_text(stream(events))
            return {"status": "capped", "termination_reason": "max-turns", "stream_complete": True,
                    "process_tree_drained": True, "stdout_path": str(stdout), "wall_seconds": 1.5,
                    "returncode": 1, **overrides}
        return run

    def capped_events(self):
        final = result(usage(3, 5, 8, 13), subtype="error_max_turns", is_error=True, num_turns=6)
        final.pop("structured_output")
        return [init(), assistant("msg_1", [tool_use("toolu_1", "Skill", {"skill": "reposcout"})]),
                tool_result("toolu_1", "Launching skill"),
                assistant("msg_2", [tool_use("toolu_2", "Bash", {"command": "reposcout review-context . --brief"})]),
                final]

    def test_one_run_records_prompt_description_skill_and_cap(self):
        before = directory_sha256(SKILL)
        outcome = execute_run(self.parameters(), runner=self.runner(self.capped_events()), pin_capture=self.pins)
        spec = self.specs[0]
        self.assertEqual(spec.max_turns, 6)
        arguments = claude_arguments(spec)
        self.assertEqual(arguments[arguments.index("--max-turns") + 1], "6")
        self.assertEqual((spec.variant, spec.model, spec.answer_schema), ("reposcout", "claude-haiku-5-5",
                                                                           ADOPTION_SCHEMA))
        self.assertEqual(read_description(self.copy), resolve_description("A", SKILL)["text"])
        self.assertFalse(self.copy_exists.exists())
        self.assertNotIn("reposcout", spec.prompt.lower())
        self.assertEqual(outcome["prompt_sha256"], text_sha256(spec.prompt))
        self.assertEqual(outcome["description"]["text_sha256"], text_sha256(resolve_description("A", SKILL)["text"]))
        self.assertTrue(outcome["skill"]["description_swapped"])
        self.assertEqual(outcome["skill"]["canonical_dir_sha256"], before)
        self.assertNotEqual(outcome["skill"]["installed_dir_sha256"], before)
        self.assertEqual(directory_sha256(SKILL), before)
        self.assertEqual(outcome["measurement"], "complete")
        self.assertTrue(outcome["capped"])
        adoption = outcome["adoption"]
        self.assertTrue(adoption["adopted"])
        self.assertTrue(adoption["brief_called"])
        self.assertEqual((adoption["first_adoption_turn"], adoption["num_turns"]), (1, 6))
        self.assertEqual(adoption["tokens"]["cache_write_input_tokens"], 5)
        saved = json.loads((self.root / "out" / outcome["run_id"] / "outcome.json").read_text())
        self.assertEqual(saved["run_id"], outcome["run_id"])
        self.assertFalse((self.root / "out" / outcome["run_id"] / "workspace").exists())

    def test_current_description_keeps_the_skill_bytes(self):
        outcome = execute_run(self.parameters(description=CURRENT), runner=self.runner(self.capped_events()),
                              pin_capture=self.pins)
        self.assertFalse(outcome["skill"]["description_swapped"])
        self.assertEqual(outcome["skill"]["installed_dir_sha256"], outcome["skill"]["canonical_dir_sha256"])
        self.assertEqual(outcome["description"]["text"], read_description((SKILL / "SKILL.md").read_text()))

    def test_neutral_arm_prompt_reaches_the_runner(self):
        execute_run(self.parameters(arm="neutral"), runner=self.runner(self.capped_events()), pin_capture=self.pins)
        self.assertTrue(self.specs[0].prompt.endswith(CAPABILITIES["reposcout"]))

    def test_held_out_and_foreign_cases_are_refused_before_any_work(self):
        for options in ({"phrasing": "h6"}, {"case": "config-defaults"}, {"case": "behavior-preserving-refactor"},
                        {"arm": "guided"}, {"repeat": -1}):
            with self.subTest(options=options), self.assertRaises(AdoptionError):
                execute_run(self.parameters(**options), runner=self.runner([]), pin_capture=self.pins)
        self.assertEqual(self.specs, [])
        self.assertFalse((self.root / "out").exists())
        outcome = execute_run(self.parameters(phrasing="h6", held_out=True), runner=self.runner(self.capped_events()),
                              pin_capture=self.pins)
        self.assertEqual(outcome["phrasing"]["set"], "held-out")

    def test_a_run_is_never_replaced(self):
        execute_run(self.parameters(), runner=self.runner(self.capped_events()), pin_capture=self.pins)
        with self.assertRaises(AdoptionError):
            execute_run(self.parameters(), runner=self.runner(self.capped_events()), pin_capture=self.pins)

    def test_an_aborted_run_without_a_final_result_is_incomplete(self):
        events = self.capped_events()[:-1]
        outcome = execute_run(self.parameters(), runner=self.runner(
            events, status="aborted", termination_reason="timeout", stream_complete=False), pin_capture=self.pins)
        self.assertEqual(outcome["measurement"], "incomplete")
        self.assertFalse(outcome["capped"])
        self.assertTrue(outcome["adoption"]["adopted"])


if __name__ == "__main__":
    unittest.main()
