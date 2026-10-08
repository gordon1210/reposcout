"""Synthetic current exec streams; no model sessions or repository scans."""

import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from accounting import InvalidLedger
import codex_trace


def usage(input_tokens=100, cached=70, output=20, reasoning=None):
    value = {'input_tokens': input_tokens, 'cached_input_tokens': cached, 'output_tokens': output}
    if reasoning is not None:
        value['reasoning_output_tokens'] = reasoning
    return value


def turn(identity, value=None, status='completed'):
    return [
        {'type': 'turn.started', 'turn_id': identity},
        {'type': 'item.completed', 'item': {'id': identity + '-answer', 'type': 'agent_message', 'text': 'PRIVATE'}},
        {'type': 'turn.' + status, 'turn_id': identity, **({'usage': value} if value is not None else {})},
    ]


def records(value=None):
    return [{'type': 'thread.started', 'thread_id': 'thread'}] + turn('t1', value or usage())


def command(identity, text, output='PRIVATE', exit_code=0, status='completed'):
    return [
        {'type': 'item.started', 'timestamp': '2026-10-07T10:00:00Z',
         'item': {'id': identity, 'type': 'command_execution', 'command': text, 'status': 'in_progress'}},
        {'type': 'item.completed', 'timestamp': '2026-10-07T10:00:00.125Z',
         'item': {'id': identity, 'type': 'command_execution', 'command': text,
                  'aggregated_output': output, 'exit_code': exit_code, 'status': status}},
    ]


class CodexTraceTests(unittest.TestCase):
    def parse(self, events=None, *, raw=None, controller_changes=None, controller=True, expected_thread_id=None):
        if raw is None:
            raw = ''.join(json.dumps(event) + '\n' for event in events).encode()
        closed = {
            'run_id': 'run', 'invocation_id': 'invocation-1', 'thread_id': 'thread',
            'resumed_thread_id': None, 'status': 'completed', 'returncode': 0,
            'stream_complete': True, 'process_tree_drained': True, 'stdout_truncated': False,
            'termination_reason': None, 'stdout_sha256': hashlib.sha256(raw).hexdigest(),
            'stdout_bytes': len(raw), 'duration_ms': 250, 'cli_version': '0.160.1',
            'usage_scope': 'unknown',
        }
        closed.update(controller_changes or {})
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'trace.jsonl'
            path.write_bytes(raw)
            return codex_trace.parse_exec_trace(path, controller=closed if controller else None,
                                                expected_thread_id=expected_thread_id)

    def codes(self, result):
        return {error['code'] for error in result['validation_errors']}

    def execution_evidence(self, events=None, *, raw=None, expected_sha256=None):
        if raw is None:
            raw = ''.join(json.dumps(event) + '\n' for event in events).encode()
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'trace.jsonl'
            path.write_bytes(raw)
            return codex_trace.extract_execution_evidence(path, step_id=0, expected_sha256=expected_sha256)

    def test_core_observation_comparable_without_inventing_missing_fields(self):
        result = self.parse(records())
        self.assertTrue(result['comparable_usage'])
        self.assertEqual(result['usage_basis'], 'exec-emitted-initial-turn-usage')
        self.assertEqual(result['observed_input_plus_output_tokens'], 120)
        self.assertEqual(result['observed_usage']['cached_input_tokens'], 70)
        self.assertIsNone(result['observed_usage']['reasoning_output_tokens'])
        self.assertIsNone(result['observed_usage']['total_tokens'])
        self.assertFalse(result['usage_complete'])
        self.assertFalse(result['provider_call_ids_available'])
        self.assertIn('cache_write_input_tokens', result['unknown_fields'])
        self.assertIsNone(result['money']['subscription_charge'])
        self.assertIsNone(result['skill_evidence']['actual_activation'])
        self.assertIsNone(result['skill_evidence']['automatic_context_injection'])
        self.assertIsNone(result['command_statistics']['command_elapsed_ms_known_sum'])
        self.assertNotIn('PRIVATE', json.dumps(result))

    def test_cached_input_and_reasoning_are_subsets_not_addends(self):
        result = self.parse(records(usage(reasoning=7)))
        self.assertEqual(result['observed_input_plus_output_tokens'], 120)
        self.assertEqual(result['observed_usage']['reasoning_output_tokens'], 7)
        self.assertEqual(result['observed_usage']['input_tokens'], 100)

    def test_observed_codex_01601_usage_shape_supports_cache_write_without_double_count(self):
        # Sanitized usage shape from the authorized calibration trace. Counts
        # are observations; no source, command or response payload is retained.
        value = {'input_tokens': 80467, 'cached_input_tokens': 62848,
                 'cache_write_input_tokens': 0, 'output_tokens': 4680,
                 'reasoning_output_tokens': 2007}
        result = self.parse(records(value))
        self.assertTrue(result['comparable_usage'])
        self.assertEqual(result['observed_input_plus_output_tokens'], 85147)
        self.assertEqual(result['observed_usage']['cache_write_input_tokens'], 0)
        self.assertIn('cache_write_input_tokens', result['comparable_fields'])
        self.assertEqual(result['observed_usage']['reasoning_output_tokens'], 2007)
        self.assertNotIn('cache_write_input_tokens', result['unknown_fields'])
        self.assertFalse(result['usage_complete'])
        missing = self.parse(records(usage()))
        self.assertIsNone(missing['observed_usage']['cache_write_input_tokens'])
        self.assertNotIn('cache_write_input_tokens', missing['comparable_fields'])
        self.assertIn('cache_write_input_tokens', missing['unknown_fields'])
        nonzero = self.parse(records({**usage(), 'cache_write_input_tokens': 10}))
        self.assertTrue(nonzero['comparable_usage'])
        self.assertEqual(nonzero['observed_input_plus_output_tokens'], 120)
        unestablished = self.parse(records({**usage(), 'cache_write_input_tokens': 31}))
        self.assertEqual(unestablished['observed_input_plus_output_tokens'], 120)
        self.assertIn('cache_write_partition_relationship', unestablished['unknown_fields'])
        invalid = self.parse(records({**usage(), 'cache_write_input_tokens': -1}))
        self.assertFalse(invalid['comparable_usage'])
        self.assertIn('invalid-usage-number:cache_write_input_tokens', self.codes(invalid))

    def test_controller_is_required_and_hash_bound(self):
        result = self.parse(records(), controller=False)
        self.assertFalse(result['comparable_usage'])
        self.assertEqual(result['closure_errors'], ['missing-controller-closure'])
        for changes, expected in [
            ({'stdout_sha256': '0' * 64}, 'controller-trace-hash-mismatch'),
            ({'stdout_bytes': 0}, 'controller-trace-size-mismatch'),
            ({'thread_id': 'other'}, 'controller-thread-mismatch'),
            ({'returncode': 1}, 'controller-terminal-status-mismatch'),
            ({'process_tree_drained': False}, 'controller-stream-not-closed'),
            ({'stdout_truncated': True}, 'controller-stream-not-closed'),
            ({'termination_reason': 'timeout'}, 'controller-stream-not-closed'),
        ]:
            with self.subTest(changes=changes):
                result = self.parse(records(), controller_changes=changes)
                self.assertFalse(result['comparable_usage'])
                self.assertIn(expected, result['closure_errors'])

    def test_completed_controller_does_not_forge_stream_terminal(self):
        events = records()[:-1]
        result = self.parse(events)
        self.assertFalse(result['comparable_usage'])
        self.assertIn('missing-turn-terminal', self.codes(result))
        self.assertIn('controller-terminal-status-mismatch', result['closure_errors'])
        self.assertIsNone(result['observed_input_plus_output_tokens'])

    def test_failed_stream_usage_is_visible_but_not_comparable(self):
        events = [{'type': 'thread.started', 'thread_id': 'thread'}] + turn('failed', usage(30, 10, 4), 'failed')
        result = self.parse(events, controller_changes={'status': 'failed', 'returncode': 1})
        self.assertEqual(result['status'], 'failed')
        self.assertEqual(result['observed_input_plus_output_tokens'], 34)
        self.assertFalse(result['comparable_usage'])

    def test_missing_failed_turn_usage_does_not_turn_into_zero(self):
        events = [{'type': 'thread.started', 'thread_id': 'thread'}] + turn('failed', None, 'failed')
        result = self.parse(events, controller_changes={'status': 'failed', 'returncode': 1})
        self.assertIsNone(result['observed_usage']['input_tokens'])
        self.assertIsNone(result['emitted_usage_known_sum']['input_tokens'])
        self.assertFalse(result['comparable_usage'])

    def test_failed_attempt_and_retry_success_preserve_both_turns(self):
        events = [{'type': 'thread.started', 'thread_id': 'thread'}]
        events += turn('failed', usage(30, 10, 4), 'failed')
        events += turn('retry', usage(50, 20, 6))
        result = self.parse(events, controller_changes={'usage_scope': 'turn'})
        self.assertEqual(result['observed_input_plus_output_tokens'], 90)
        self.assertEqual([item['status'] for item in result['turns']], ['failed', 'completed'])
        self.assertFalse(result['comparable_usage'])

    def test_unreported_failed_attempt_makes_per_turn_total_unknown(self):
        events = [{'type': 'thread.started', 'thread_id': 'thread'}]
        events += turn('failed', None, 'failed') + turn('retry', usage())
        result = self.parse(events, controller_changes={'usage_scope': 'turn'})
        self.assertIsNone(result['observed_usage']['input_tokens'])
        self.assertEqual(result['emitted_usage_known_sum']['input_tokens'], 100)

    def test_unknown_usage_format_keeps_observations_but_fails_closed(self):
        value = {**usage(), 'future_total': 500}
        result = self.parse(records(value))
        self.assertEqual(result['observed_usage']['input_tokens'], 100)
        self.assertFalse(result['comparable_usage'])
        self.assertIn('unknown-usage-format', self.codes(result))
        self.assertIn('unsupported:future_total', result['unknown_fields'])
        result = self.parse(records({'input_tokens_details': {'cached_tokens': 70}}))
        self.assertIsNone(result['observed_usage']['input_tokens'])

    def test_invalid_numbers_and_subset_constraints_fail_closed(self):
        for invalid in (True, -1, 1.25, '100', 2 ** 63, float('nan')):
            with self.subTest(invalid=invalid):
                result = self.parse(records({**usage(), 'input_tokens': invalid}))
                self.assertFalse(result['comparable_usage'])
                self.assertIsNone(result['observed_usage']['input_tokens'])
        for value, expected in [
            (usage(cached=101), 'cached-input-exceeds-input'),
            (usage(reasoning=21), 'reasoning-exceeds-output'),
            ({**usage(), 'total_tokens': 190}, 'reported-total-disagrees'),
        ]:
            result = self.parse(records(value))
            self.assertFalse(result['comparable_usage'])
            self.assertIn(expected, self.codes(result))

    def test_missing_core_field_is_unknown_not_zero(self):
        result = self.parse(records({'input_tokens': 100, 'output_tokens': 20}))
        self.assertIsNone(result['observed_usage']['cached_input_tokens'])
        self.assertFalse(result['comparable_usage'])
        self.assertIn('cached_input_tokens', result['unknown_fields'])

    def test_duplicate_json_fields_events_and_completions_never_double_count(self):
        raw = b'{"type":"thread.started","thread_id":"thread","thread_id":"other"}\n'
        result = self.parse(raw=raw)
        self.assertIn('invalid-json-record', self.codes(result))
        events = records()
        events[-1]['event_id'] = 'finish'
        events.append(copy.deepcopy(events[-1]))
        result = self.parse(events)
        self.assertEqual(result['observed_input_plus_output_tokens'], 120)
        self.assertIn('duplicate-or-invalid-event-id', self.codes(result))
        events[-1].pop('event_id')
        result = self.parse(events)
        self.assertEqual(result['observed_input_plus_output_tokens'], 120)
        self.assertIn('terminal-event-without-open-turn', self.codes(result))

    def test_multiple_turns_require_a_declared_scope(self):
        events = records() + turn('t2', usage(20, 10, 5))
        result = self.parse(events)
        self.assertFalse(result['comparable_usage'])
        self.assertIsNone(result['observed_input_plus_output_tokens'])
        self.assertEqual(result['emitted_usage_known_sum']['input_tokens'], 120)
        result = self.parse(events, controller_changes={'usage_scope': 'turn'})
        self.assertTrue(result['comparable_usage'])
        self.assertEqual(result['observed_input_plus_output_tokens'], 145)

    def test_repeated_cumulative_updates_count_once_and_reset_fails(self):
        events = records(usage(reasoning=7))
        events += turn('t2', usage(reasoning=7))
        events += turn('t3', usage(150, 90, 30, 10))
        result = self.parse(events, controller_changes={'usage_scope': 'invocation'})
        self.assertTrue(result['comparable_usage'])
        self.assertEqual(result['observed_input_plus_output_tokens'], 180)
        self.assertEqual(result['observed_usage']['reasoning_output_tokens'], 10)
        self.assertEqual(result['normalized_usage_records'][1]['input_tokens'], 0)
        events += turn('t4', usage(120, 80, 25, 8))
        result = self.parse(events, controller_changes={'usage_scope': 'invocation'})
        self.assertFalse(result['comparable_usage'])
        self.assertIn('cumulative-usage-reset:input_tokens', self.codes(result))

    def test_retry_error_marker_blocks_unproved_complete_usage(self):
        events = records()
        events.insert(2, {'type': 'error', 'message': 'Reconnecting... PRIVATE 1/5'})
        result = self.parse(events)
        self.assertEqual(result['retry_message_markers'], 1)
        self.assertEqual(result['observed_input_plus_output_tokens'], 120)
        self.assertFalse(result['comparable_usage'])
        self.assertNotIn('PRIVATE', json.dumps(result))

    def test_foreign_or_misordered_lifecycle_cannot_compare(self):
        cases = []
        events = records()
        events[-1]['thread_id'] = 'foreign'
        cases.append((events, 'foreign-thread-event'))
        events = records()
        events[-1]['turn_id'] = 'foreign'
        cases.append((events, 'terminal-turn-id-mismatch'))
        events = records()
        events.insert(2, {'type': 'turn.started', 'turn_id': 'second'})
        cases.append((events, 'turn-start-without-thread-or-during-turn'))
        for events, expected in cases:
            with self.subTest(expected=expected):
                result = self.parse(events)
                self.assertFalse(result['comparable_usage'])
                self.assertIn(expected, self.codes(result))
        result = self.parse(records(), expected_thread_id='other')
        self.assertIn('unexpected-thread-id', self.codes(result))

    def test_truncated_json_preserves_prior_usage_as_partial_evidence(self):
        raw = ''.join(json.dumps(event) + '\n' for event in records()).encode() + b'{"type":"turn.started"'
        result = self.parse(raw=raw)
        self.assertEqual(result['observed_input_plus_output_tokens'], 120)
        self.assertFalse(result['comparable_usage'])
        self.assertIn('invalid-json-record', self.codes(result))

    def test_limits_are_bounded_and_not_success(self):
        for limit, value in [('MAX_TRACE_BYTES', 32), ('MAX_LINE_BYTES', 32), ('MAX_EVENTS', 2)]:
            with self.subTest(limit=limit), patch.object(codex_trace, limit, value):
                result = self.parse(records())
                self.assertIn('trace-limit-exceeded', self.codes(result))
                self.assertFalse(result['comparable_usage'])

    def test_commands_capture_help_failure_skill_output_and_timing_without_bodies(self):
        events = records()
        events[2:2] = (command('help', "/bin/bash -lc 'reposcout read --help'", 'äPRIVATE') +
                       command('read', 'cat .agents/skills/reposcout/SKILL.md') +
                       command('fail', 'reposcout read --invalid', exit_code=2, status='failed'))
        result = self.parse(events)
        stats = result['command_statistics']
        self.assertEqual(stats['completed_command_items'], 3)
        self.assertEqual(stats['recognized_reposcout_invocations'], 2)
        self.assertEqual(stats['recognized_reposcout_help_invocations'], 1)
        self.assertEqual(stats['recognized_skill_read_invocations'], 1)
        self.assertEqual(stats['failed_command_items'], 1)
        self.assertEqual(result['commands'][0]['reported_output_utf8_bytes'], 9)
        self.assertEqual(result['commands'][0]['elapsed_ms'], 125)
        self.assertIsNone(result['source_metrics']['attributed_source_bytes'])
        self.assertIsNone(result['source_metrics']['model_context_fullness'])
        self.assertNotIn('PRIVATE', json.dumps(result))

    def test_command_strings_do_not_prove_conditional_or_mentioned_invocation(self):
        events = records()
        events[2:2] = (command('echo', "echo 'reposcout read --help'") +
                       command('conditional', 'false && reposcout read --help') +
                       command('unresolved', '$(echo reposcout) read --help'))
        result = self.parse(events)
        stats = result['command_statistics']
        self.assertEqual(stats['recognized_reposcout_invocations'], 0)
        self.assertEqual(stats['reposcout_command_markers'], 1)
        self.assertEqual(stats['unparsed_command_items'], 1)

    def test_unavailable_command_and_failed_skill_read_are_attempts_not_activation(self):
        events = records()
        events[2:2] = (command('missing', 'reposcout --help', exit_code=127, status='failed') +
                       command('denied', 'reposcout read --help', exit_code=126, status='failed') +
                       command('skill', 'cat .agents/skills/reposcout/SKILL.md', exit_code=1, status='failed') +
                       command('echo', "echo '&&' 'reposcout' '--help'") +
                       command('background', 'reposcout --help &') +
                       command('syntax', 'reposcout --help &&', exit_code=2, status='failed'))
        result = self.parse(events)
        stats = result['command_statistics']
        self.assertEqual(stats['recognized_reposcout_invocations'], 0)
        self.assertEqual(stats['recognized_reposcout_help_invocations'], 0)
        self.assertEqual(stats['recognized_reposcout_unavailable_attempts'], 2)
        self.assertEqual(stats['recognized_skill_read_invocations'], 0)
        self.assertEqual(stats['skill_read_command_markers'], 1)
        self.assertIsNone(result['skill_evidence']['actual_activation'])

    def test_duplicate_command_completion_and_open_command_are_visible(self):
        events = records()
        started, completed = command('command', 'reposcout read --help')
        events[2:2] = [started, completed, copy.deepcopy(completed)]
        result = self.parse(events)
        self.assertEqual(result['command_statistics']['recognized_reposcout_invocations'], 1)
        self.assertIn('duplicate-item-completion', self.codes(result))
        events = records()
        events[2:2] = [started]
        result = self.parse(events)
        self.assertIn('terminal-turn-with-open-items', self.codes(result))
        self.assertFalse(result['comparable_usage'])
        self.assertEqual(len(result['incomplete_commands']), 1)
        self.assertIsNone(result['incomplete_commands'][0]['reported_output_utf8_bytes'])

    def test_unknown_event_and_item_format_remain_visible(self):
        events = records()
        events.insert(2, {'type': 'future.event', 'text': 'PRIVATE'})
        result = self.parse(events)
        self.assertEqual(result['unknown_event_types'], ['future.event'])
        self.assertFalse(result['comparable_usage'])
        events = records()
        events.insert(2, {'type': 'item.completed', 'item': {'id': 'new', 'type': 'future_item', 'text': 'PRIVATE'}})
        result = self.parse(events)
        self.assertEqual(result['unknown_item_types'], ['future_item'])
        self.assertFalse(result['comparable_usage'])

    def test_resume_per_turn_episode_keeps_both_invocations(self):
        first = self.parse(records(), controller_changes={'usage_scope': 'turn'})
        second = self.parse([{'type': 'thread.started', 'thread_id': 'thread'}] + turn('t2', usage(40, 10, 8)),
                            controller_changes={'invocation_id': 'invocation-2', 'resumed_thread_id': 'thread', 'usage_scope': 'turn'})
        result = codex_trace.aggregate_episode([first, second])
        self.assertTrue(result['comparable_usage'])
        self.assertEqual(result['observed_input_plus_output_tokens'], 168)
        self.assertEqual(result['invocation_count'], 2)
        self.assertFalse(result['usage_complete'])

    def test_cumulative_resume_subtracts_verified_baseline(self):
        first = self.parse(records(usage(reasoning=5)), controller_changes={'usage_scope': 'thread-cumulative'})
        second = self.parse([{'type': 'thread.started', 'thread_id': 'thread'}] + turn('t2', usage(150, 90, 30, 9)),
                            controller_changes={'invocation_id': 'invocation-2', 'resumed_thread_id': 'thread',
                                                'usage_scope': 'thread-cumulative', 'usage_baseline': usage(reasoning=5)})
        result = codex_trace.aggregate_episode([first, second])
        self.assertTrue(result['comparable_usage'])
        self.assertEqual(result['observed_input_plus_output_tokens'], 180)
        self.assertEqual(result['observed_usage']['reasoning_output_tokens'], 9)
        second = self.parse([{'type': 'thread.started', 'thread_id': 'thread'}] + turn('t2', usage(150, 90, 30, 9)),
                            controller_changes={'invocation_id': 'invocation-2', 'resumed_thread_id': 'thread',
                                                'usage_scope': 'thread-cumulative', 'usage_baseline': usage(80, 60, 15, 4)})
        result = codex_trace.aggregate_episode([first, second])
        self.assertFalse(result['comparable_usage'])
        self.assertIsNone(result['observed_input_plus_output_tokens'])
        self.assertIn('cumulative-resume-baseline-mismatch', result['episode_errors'])

    def test_unknown_resume_does_not_guess_cumulative_or_turn_sum(self):
        first = self.parse(records())
        second = self.parse([{'type': 'thread.started', 'thread_id': 'thread'}] + turn('t2', usage(150, 90, 30)),
                            controller_changes={'invocation_id': 'invocation-2', 'resumed_thread_id': 'thread'})
        result = codex_trace.aggregate_episode([first, second])
        self.assertFalse(result['comparable_usage'])
        self.assertIsNone(result['observed_input_plus_output_tokens'])
        self.assertEqual(result['invocations'][1]['turns'][0]['reported_usage']['input_tokens'], 150)
        self.assertIn('episode-resume-usage-scope-unknown', result['episode_errors'])

    def test_resumed_cache_write_comparison_requires_observations_in_both_invocations(self):
        initial = {**usage(), 'cache_write_input_tokens': 0}
        first = self.parse(records(initial), controller_changes={'usage_scope': 'thread-cumulative'})
        for known in (False, True):
            with self.subTest(resume_cache_write_known=known):
                terminal = usage(150, 90, 30)
                if known:
                    terminal['cache_write_input_tokens'] = 0
                second = self.parse([{'type': 'thread.started', 'thread_id': 'thread'}] + turn('t2', terminal),
                                    controller_changes={'invocation_id': 'invocation-2', 'resumed_thread_id': 'thread',
                                                        'usage_scope': 'thread-cumulative', 'usage_baseline': initial})
                result = codex_trace.aggregate_episode([first, second])
                self.assertTrue(result['comparable_usage'])
                self.assertEqual('cache_write_input_tokens' in second['comparable_fields'], known)
                self.assertEqual('cache_write_input_tokens' in result['comparable_fields'], known)
                self.assertEqual(result['observed_usage']['cache_write_input_tokens'], 0 if known else None)
                self.assertIn('cache_write_partition_relationship', result['unknown_fields'])

    def test_episode_rejects_duplicate_identity_capture_and_projection_tampering(self):
        first = self.parse(records(), controller_changes={'usage_scope': 'turn'})
        result = codex_trace.aggregate_episode([first, first])
        self.assertFalse(result['comparable_usage'])
        self.assertIn('duplicate-or-missing-invocation-id', result['episode_errors'])
        self.assertIn('duplicate-captured-trace', result['episode_errors'])
        forged = copy.deepcopy(first)
        forged['observed_usage']['input_tokens'] = 0
        with self.assertRaises(InvalidLedger):
            codex_trace.aggregate_episode([forged])

    def test_private_execution_receipts_preserve_actual_command_and_bounded_output(self):
        events = records()
        output = 'x' * 4095 + 'äPRIVATE'
        command_text = "echo 'python3 checks/private_fixture.py'"
        events[2:2] = command('check', command_text, output)
        receipts = self.execution_evidence(events)
        self.assertEqual(len(receipts), 1)
        receipt = receipts[0]
        self.assertEqual(set(receipt), {'step_id', 'command', 'exit_code', 'trace_sha256',
                                       'output_sha256', 'output_excerpt'})
        self.assertEqual(receipt['command'], command_text)
        self.assertEqual(receipt['exit_code'], 0)
        self.assertEqual(receipt['output_sha256'], hashlib.sha256(output.encode()).hexdigest())
        self.assertLessEqual(len(receipt['output_excerpt'].encode()), 4096)
        self.assertEqual(receipt['output_excerpt'], 'x' * 4095)
        # The receipt retains shell semantics; it does not turn an echo into a
        # verified fixture check or enter sanitized public accounting.
        public = self.parse(events)
        self.assertNotIn('private_fixture', json.dumps(public))

    def test_execution_receipts_exclude_unmatched_changed_and_duplicate_commands(self):
        events = records()
        first_start, first_end = command('duplicate', 'python3 checks/fixture.py')
        second_start, second_end = command('changed', 'python3 checks/fixture.py')
        second_end['item']['command'] = 'echo checked'
        _, missing_start = command('missing', 'python3 checks/fixture.py')
        events[2:2] = [first_start, first_end, copy.deepcopy(first_end), second_start, second_end, missing_start]
        self.assertEqual(self.execution_evidence(events), [])

    def test_execution_receipts_bind_partial_capture_and_reject_limits(self):
        events = records()
        events[2:2] = command('check', 'python3 checks/fixture.py', 'passed')
        raw = ''.join(json.dumps(event) + '\n' for event in events).encode() + b'{"type":'
        digest = hashlib.sha256(raw).hexdigest()
        with self.assertRaises(InvalidLedger):
            self.execution_evidence(raw=raw)
        receipts = self.execution_evidence(raw=raw, expected_sha256=digest)
        self.assertEqual(len(receipts), 1)
        self.assertEqual(receipts[0]['trace_sha256'], digest)
        with self.assertRaises(InvalidLedger):
            self.execution_evidence(raw=raw, expected_sha256='0' * 64)
        with patch.object(codex_trace, 'MAX_TRACE_BYTES', 32), self.assertRaises(InvalidLedger):
            self.execution_evidence(events)

if __name__ == '__main__':
    unittest.main()
