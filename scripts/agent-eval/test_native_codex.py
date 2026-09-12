import copy
import json
import tempfile
import unittest
from pathlib import Path

import accounting
import native_codex as native


def usage(input_tokens, cached, written, output, reasoning):
    return dict(input_tokens=input_tokens, cached_input_tokens=cached,
                cache_write_input_tokens=written, output_tokens=output,
                reasoning_output_tokens=reasoning, total_tokens=input_tokens + output)


def records(session='fixture-root'):
    first = usage(100, 20, 30, 10, 3)
    second = usage(80, 60, 0, 12, 4)
    combined = {key: first[key] + second[key] for key in native.FIELDS}
    return [
        {'type': 'session_meta', 'payload': dict(id=session, cli_version='synthetic', model_provider='openai')},
        {'type': 'turn_context', 'payload': dict(turn_id='one', model='fixed-model', effort='medium')},
        {'type': 'token_count', 'timestamp': 'synthetic-1', 'payload': dict(total_token_usage=first, last_token_usage=first)},
        {'type': 'token_count', 'timestamp': 'synthetic-2', 'payload': dict(total_token_usage=first, last_token_usage=first)},
        {'type': 'token_count', 'timestamp': 'synthetic-3', 'payload': dict(total_token_usage=combined, last_token_usage=second)},
    ]


class NativeTests(unittest.TestCase):
    def test_cumulative_repetitions_reasoning_and_explicit_window(self):
        result = native.audit(records(), 'fixture-root')
        self.assertEqual(result['model_tokens']['total_tokens'], 202)
        self.assertEqual(result['model_tokens']['reasoning_output_tokens'], 7)
        self.assertEqual(result['model_tokens']['uncached_input_tokens'], 70)
        self.assertEqual(result['repeated_updates'], 1)
        self.assertEqual(result['advancing_updates'], 2)
        self.assertFalse(result['usage_complete'])
        window = native.audit(records(), 'fixture-root', start_after=2, end_at=3)
        self.assertEqual(window['model_tokens']['total_tokens'], 92)

    def test_real_response_ids_use_child_thread_and_crosscheck_totals(self):
        raw = records()
        for index, identity in ((4, 'response-two'), (2, 'response-one')):
            payload = raw[index]['payload']
            raw.insert(index, {'type': 'token_usage_record', 'payload': {
                'thread_id': 'fixture-root', 'response_id': identity,
                'usage': copy.deepcopy(payload['last_token_usage']),
                'thread_token_usage': copy.deepcopy(payload['total_token_usage'])}})
        result = native.audit(raw, 'fixture-root')
        self.assertTrue(result['provider_call_ids_available'])
        self.assertEqual([item['response_id'] for item in result['response_usage']], ['response-one', 'response-two'])
        raw[2]['payload']['thread_id'] = 'different-root-session'
        with self.assertRaises(accounting.InvalidLedger):
            native.audit(raw, 'fixture-root')

    def test_resets_gaps_conflicting_repeats_and_missing_fields_fail(self):
        variants = []
        reset = records()
        reset[-1]['payload']['total_token_usage'] = usage(5, 0, 0, 1, 0)
        variants.append(reset)
        gap = records()
        gap[-1]['payload']['last_token_usage'] = usage(79, 60, 0, 12, 4)
        variants.append(gap)
        repeated = records()
        repeated[3]['payload']['last_token_usage'] = usage(1, 0, 0, 1, 0)
        variants.append(repeated)
        missing = records()
        del missing[-1]['payload']['last_token_usage']['cache_write_input_tokens']
        variants.append(missing)
        null = records()
        null[-1]['payload'] = None
        variants.append(null)
        for variant in variants:
            with self.subTest(variant=variants.index(variant)), self.assertRaises(accounting.InvalidLedger):
                native.audit(variant, 'fixture-root')

    def test_missing_start_end_metadata_and_model_drift_fail(self):
        for args in [(records(), 'wrong'), (records()[2:], 'fixture-root'),
                     (records(), 'fixture-root', 0, 4), (records(), 'fixture-root', 1, 1)]:
            with self.assertRaises(accounting.InvalidLedger):
                native.audit(*args)
        changed = records()
        changed.insert(4, {'type': 'turn_context', 'payload': dict(model='different', effort='medium')})
        with self.assertRaises(accounting.InvalidLedger):
            native.audit(changed, 'fixture-root')
        inherited = records()
        inherited[2]['payload']['last_token_usage'] = usage(10, 0, 0, 1, 0)
        with self.assertRaises(accounting.InvalidLedger):
            native.audit(inherited, 'fixture-root')

    def test_privacy_projection_omits_prompts_and_unknown_metadata(self):
        raw = records()
        raw[0]['payload']['instructions'] = 'DO_NOT_EXPORT'
        raw[1]['payload']['developer_instructions'] = 'DO_NOT_EXPORT'
        for item in raw[2:]:
            item['type'] = 'event_msg'
            item['payload'] = {'type': 'token_count', 'info': item['payload'], 'private': 'DO_NOT_EXPORT'}
        raw.append({'type': 'response_item', 'payload': {'content': 'DO_NOT_EXPORT'}})
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'synthetic.jsonl'
            path.write_text(''.join(json.dumps(item) + '\n' for item in raw))
            sanitized = list(native.sanitized_records(path))
        self.assertNotIn('DO_NOT_EXPORT', json.dumps(sanitized))
        self.assertEqual(native.audit(sanitized, 'fixture-root')['model_tokens']['total_tokens'], 202)

    def inputs(self):
        h = 'a' * 64
        manifest = dict.fromkeys(accounting.CONDITIONS, 'fixed')
        manifest.update(schema=1, run_id='pilot', variant='baseline', mode='end-to-end',
                        provider='openai', model='fixed-model', effort='medium', evidence_kind='synthetic')
        for key in ('fixture_sha256', 'task_sha256', 'oracle_sha256', 'start_context_sha256'):
            manifest[key] = h
        audits = [native.audit(records(), 'fixture-root'), native.audit(records('fixture-child'), 'fixture-child')]
        entries = [dict(session_id=item['session_id'], parent_session_id=None if i == 0 else 'fixture-root',
                        end_at=item['end_at'], projection_sha256=item['projection_sha256'], status='success')
                   for i, item in enumerate(audits)]
        attestation = dict(run_id='pilot', closed=True, inventory_complete=True, evidence_sha256=h,
                           evidence_reference='synthetic lifecycle fixture', status='success', sessions=entries)
        quality = dict(passed=True, regressions=[], missing_evidence=[], evidence_sha256=h)
        return manifest, audits, attestation, quality

    def test_closed_inventory_counts_children_and_keeps_failures(self):
        args = self.inputs()
        result = native.close_window(*args)
        self.assertEqual(result['model_tokens']['total_tokens'], 404)
        self.assertEqual(result['agents'], 2)
        candidate = copy.deepcopy(result)
        candidate['variant'] = 'reposcout'
        self.assertFalse(native.compare_windows(result, candidate)['eligible'])
        args[2]['status'] = 'aborted'
        args[3]['passed'] = False
        args[3]['missing_evidence'] = ['missing answer']
        failed = native.close_window(*args)
        self.assertEqual(failed['model_tokens']['total_tokens'], 404)
        self.assertEqual(failed['status'], 'aborted')

    def test_closed_windows_reject_missing_children_boundaries_or_attestation(self):
        for mutate in (lambda args: args[2].update(closed=False),
                       lambda args: args[2]['sessions'].pop(),
                       lambda args: args[2]['sessions'][0].update(end_at=99),
                       lambda args: args[1][0]['model_tokens'].update(total_tokens=999),
                       lambda args: args[1][0].update(start_after=1)):
            args = self.inputs()
            mutate(args)
            with self.assertRaises(accounting.InvalidLedger):
                native.close_window(*args)


if __name__ == '__main__':
    unittest.main()
