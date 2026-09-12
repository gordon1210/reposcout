import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import accounting
import fixtures
import pilot


class PilotTests(unittest.TestCase):
    def test_preparation_separates_oracles_and_arms_and_checks_snapshot(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            binary = root / 'synthetic-binary'
            binary.write_bytes(b'fixture-only; never executed')
            with patch.object(pilot, 'git', return_value='c' * 40):
                result = pilot.prepare(root / 'campaign', binary, fixtures.ROOT / 'prompt-templates.json')
            self.assertEqual(result['trial_count'], 24)
            campaign = accounting.read_json(root / 'campaign/campaign.json')
            self.assertEqual(campaign['execution'], 'not-run')
            by_case = {}
            for trial in campaign['trials']:
                by_case.setdefault(trial['task_id'], []).append(trial)
                prompt = Path(trial['prompt_path']).read_text()
                self.assertNotIn('known_selectors', prompt)
                self.assertNotIn('{{', prompt)
                self.assertFalse(trial['children_allowed'])
                self.assertEqual(trial['fork_turns'], 'none')
                if trial['feature'] in ('F3', 'F4', 'F6'):
                    self.assertNotIn('"definitions"', prompt)
                case = next(case for case in fixtures.cases() if case['task_id'] == trial['task_id'])
                self.assertTrue(pilot.verify_trial(trial, case['oracle'])['passed'])
            for trials in by_case.values():
                self.assertEqual(len(trials), 2)
                self.assertEqual(trials[0]['start_context_sha256'], trials[1]['start_context_sha256'])
                self.assertEqual(trials[0]['fixture_sha256'], trials[1]['fixture_sha256'])
                self.assertNotEqual(trials[0]['prompt_sha256'], trials[1]['prompt_sha256'])
            trial = campaign['trials'][0]
            (Path(trial['workspace']) / 'src/inventory.rs').write_text('changed')
            with self.assertRaises(accounting.InvalidLedger):
                pilot.verify_trial(trial, {})
            with patch.object(pilot, 'git', return_value='c' * 40), self.assertRaises(FileExistsError):
                pilot.prepare(root / 'campaign', binary, fixtures.ROOT / 'prompt-templates.json')

    def test_routing_revision_removes_fixed_entry_and_composed_stays_separate(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            binary = root / 'synthetic-binary'
            binary.write_bytes(b'never executed')
            policy = fixtures.ROOT / 'routing-policy.json'
            templates = fixtures.ROOT / 'prompt-templates.json'
            with patch.object(pilot, 'git', return_value='c' * 40):
                pilot.prepare(root / 'routing', binary, templates, ['F1:known-definition:isolated'], 'routing-v1', policy)
                pilot.prepare(root / 'composed', binary, templates, None, 'composed-v1', policy, True)
            for name, kind in [('routing', 'routing-remeasure'), ('composed', 'composed-workflow')]:
                campaign = accounting.read_json(root / name / 'campaign.json')
                self.assertEqual(campaign['trial_count'], 2)
                self.assertEqual(campaign['trials'][0]['start_context_sha256'], campaign['trials'][1]['start_context_sha256'])
                for trial in campaign['trials']:
                    self.assertEqual(trial['experiment_kind'], kind)
                    self.assertNotIn('"entry_commands"', Path(trial['prompt_path']).read_text())
                    self.assertIsNotNone(trial['routing_policy_sha256'])
                    self.assertEqual(trial['execution'], 'not-run')
                    explicit = accounting.read_json(policy)['composed_variant_guidance']
                    present = explicit in Path(trial['prompt_path']).read_text()
                    self.assertEqual(present, name == 'composed' and trial['variant'] == 'reposcout')

    def test_composed_task_requires_changed_consumer_and_calculation_evidence(self):
        case = fixtures.composed_case()
        self.assertTrue(fixtures.verify_answer(case, case['oracle'])['passed'])
        public = json.dumps(case['task'])
        for symbol in ('cap_quantity', 'invoice_total', 'tax_for'):
            self.assertNotIn(symbol, public)
        answer = dict(case['oracle'])
        answer['definitions'] = ['cap_quantity', 'invoice_total']
        self.assertFalse(fixtures.verify_answer(case, answer)['passed'])
        answer = dict(case['oracle'])
        answer['values'] = [0, 750]
        self.assertFalse(fixtures.verify_answer(case, answer)['passed'])

    def test_malformed_answers_are_retained_as_failed_quality(self):
        case = fixtures.cases()[0]
        for answer in (None, [], {'definitions': [1]}, {'definitions': [], 'evidence': None}):
            self.assertFalse(fixtures.verify_answer(case, answer)['passed'])


if __name__ == '__main__':
    unittest.main()
