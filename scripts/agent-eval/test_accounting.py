import copy
import json
import unittest
from pathlib import Path

import accounting
import fixtures

ROOT = Path(__file__).parent / 'fixtures'


class AccountingTests(unittest.TestCase):
    def setUp(self):
        self.manifest = accounting.read_json(ROOT / 'usage-manifest.json')
        self.events = list(accounting.read_events(ROOT / 'usage-events.jsonl'))

    def result(self):
        return accounting.summarize(self.manifest, self.events)

    def test_cache_retry_child_and_reasoning_partition(self):
        result = self.result()
        self.assertEqual(result['model_tokens'], dict(input_tokens=220, input_uncached_tokens=110,
            input_cache_read_tokens=80, input_cache_creation_tokens=30, output_tokens=30,
            total_tokens=250, reasoning_tokens_subset=7, reasoning_reported_calls=2))
        self.assertEqual((result['agents'], result['attempts'], result['model_calls']), (2, 3, 3))
        self.assertEqual(result['provider_cost']['amounts'], {'USD': '0.03'})
        self.assertEqual(result['explanatory']['tool_output_bytes'], 999)
        self.assertEqual(result['explanatory']['duplicate_source_bytes'], 50)
        self.assertEqual(result['explanatory']['unique_source_bytes'], 250)
        self.assertEqual(result['explanatory']['followup_reads'], 1)
        self.assertEqual(len(result['explanatory']['context_events']), 1)

    def test_duplicate_event_and_conflicting_provider_call(self):
        for same_event_id in (True, False):
            with self.subTest(same_event_id=same_event_id):
                events = copy.deepcopy(self.events)
                extra = copy.deepcopy(events[2])
                if not same_event_id:
                    extra['event_id'] = 'conflict'
                    extra['output_tokens'] = 100
                events.insert(3, extra)
                with self.assertRaises(accounting.InvalidLedger):
                    accounting.summarize(self.manifest, events)

    def test_reject_invalid_partition_reasoning_and_streaming(self):
        for field, value in [('input_tokens', 101), ('reasoning_tokens', 11), ('final', False),
                             ('input_uncached_tokens', True), ('output_tokens', -1)]:
            with self.subTest(field=field):
                events = copy.deepcopy(self.events)
                events[2][field] = value
                with self.assertRaises(accounting.InvalidLedger):
                    accounting.summarize(self.manifest, events)

    def test_failed_aborted_and_incomplete_keep_consumption(self):
        for status in ('failed', 'aborted', 'success'):
            events = copy.deepcopy(self.events)
            events[-1]['status'] = status
            events[-1]['usage_complete'] = False
            if status != 'success':
                events[-2]['passed'] = False
                events[-2]['missing_evidence'] = ['answer unavailable']
            result = accounting.summarize(self.manifest, events)
            self.assertEqual(result['model_tokens']['total_tokens'], 250)
            candidate = copy.deepcopy(result)
            candidate['variant'] = 'reposcout'
            compared = accounting.compare(result, candidate)
            self.assertFalse(compared['eligible'])
            self.assertIsNone(compared['token_delta'])

    def test_inventory_requires_all_children_and_calls(self):
        for key, value in [('agent_ids', ['root']), ('provider_call_ids', ['one', 'two'])]:
            events = copy.deepcopy(self.events)
            events[-1][key] = value
            with self.assertRaises(accounting.InvalidLedger):
                accounting.summarize(self.manifest, events)
        self.events[1]['parent_agent_id'] = 'missing'
        with self.assertRaises(accounting.InvalidLedger):
            self.result()

    def test_comparability_and_synthetic_savings_gate(self):
        baseline = self.result()
        candidate = copy.deepcopy(baseline)
        candidate['variant'] = 'reposcout'
        self.assertFalse(accounting.compare(baseline, candidate)['eligible'])
        for field in accounting.CONDITIONS:
            changed = copy.deepcopy(candidate)
            changed['conditions'][field] += '-different'
            with self.subTest(field=field), self.assertRaises(accounting.InvalidLedger):
                accounting.compare(baseline, changed)
        baseline['conditions']['evidence_kind'] = 'observed'
        candidate['conditions']['evidence_kind'] = 'observed'
        self.assertEqual(accounting.compare(baseline, candidate)['token_delta'], 0)

    def test_missing_quality_and_event_after_end_rejected(self):
        for events in (self.events[:-2] + self.events[-1:], self.events + [self.events[0]]):
            with self.assertRaises(accounting.InvalidLedger):
                accounting.summarize(self.manifest, events)

    def test_duplicate_json_fields_and_invalid_hash(self):
        with self.assertRaises(accounting.InvalidLedger):
            json.loads('{"schema":1,"schema":2}', object_pairs_hook=accounting.object_pairs)
        self.manifest['fixture_sha256'] = 'unknown'
        with self.assertRaises(accounting.InvalidLedger):
            self.result()


class FixtureTests(unittest.TestCase):
    def test_all_features_have_both_modes_and_pinned_sources(self):
        suite = fixtures.cases()
        self.assertEqual(json.loads((ROOT / "tasks.json").read_text())["cases"], suite)
        self.assertEqual(len(suite), 12)
        self.assertEqual({(case['feature'], case['mode']) for case in suite},
                         {(f'F{i}', mode) for i in range(1, 7) for mode in ('isolated', 'end-to-end')})
        for case in suite:
            self.assertEqual(case['fixture_sha256'], fixtures.files_digest(case['fixture_files']))
            self.assertEqual(case['execution'], 'not-run')
            self.assertEqual(case['task_sha256'], fixtures.files_digest(case['task']))
            self.assertTrue(case['baseline']['targeted_reads'])
            self.assertNotIn('known_selectors', case['task'])
            if case["feature"] == "F1" and case["mode"] == "end-to-end":
                self.assertNotIn("selector", case["task"])
                self.assertNotIn("cap_quantity", json.dumps(case["task"]))

    def test_oracles_require_exact_set_behavior_and_evidence(self):
        for case in fixtures.cases():
            answer = copy.deepcopy(case['oracle'])
            self.assertTrue(fixtures.verify_answer(case, answer)['passed'])
            answer['definitions'].append('unrelated')
            self.assertFalse(fixtures.verify_answer(case, answer)['passed'])
            answer = copy.deepcopy(case['oracle'])
            answer['evidence'] = []
            self.assertFalse(fixtures.verify_answer(case, answer)['passed'])
            if case['oracle']['values'] is not None:
                answer = copy.deepcopy(case['oracle'])
                answer['values'] = [999]
                self.assertFalse(fixtures.verify_answer(case, answer)['passed'])

    def test_diagnostics_positions_match_snapshot_bytes(self):
        for line in (ROOT / 'diagnostics.jsonl').read_text().splitlines():
            record = json.loads(line)
            self.assertEqual(record['code']['code'], 'synthetic_eval_warning')
            span = record['spans'][0]
            lines = (ROOT / 'repository' / span['file_name']).read_bytes().splitlines(keepends=True)
            self.assertEqual(span['byte_start'], sum(map(len, lines[:span['line_start'] - 1])))


if __name__ == '__main__':
    unittest.main()
