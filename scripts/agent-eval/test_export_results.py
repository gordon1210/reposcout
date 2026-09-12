import base64
import unittest

import accounting
import export_results
import prompt_delivery


class ExportTests(unittest.TestCase):
    def test_prompt_opacity_is_unknown_not_mismatch_or_match(self):
        token = base64.urlsafe_b64encode(bytes([128]) + b'x' * 96).decode()
        opaque = prompt_delivery.compare_payload(token, 'public fixture prompt\n')
        self.assertIsNone(opaque['exact_plaintext_equal'])
        self.assertEqual(opaque['independent_plaintext_verification'], 'unavailable-encrypted-record')
        self.assertNotIn(token, str(opaque))
        mismatch = prompt_delivery.compare_payload('changed task', 'public fixture prompt\n')
        self.assertFalse(mismatch['exact_plaintext_equal'])
        equal = prompt_delivery.compare_payload('public fixture prompt\n', 'public fixture prompt\n')
        self.assertTrue(equal['exact_plaintext_equal'])

    def test_export_rejects_nested_payloads_and_private_paths(self):
        for data in ({'runs': [{'content': 'source'}]}, {'native_log': 'private'},
                     {'runs': [{'ref': '/Users/example/private'}]}, {'path': '/tmp/task/file'}):
            with self.assertRaises(accounting.InvalidLedger):
                export_results.safe_projection(data)
        export_results.safe_projection({'runs': [{'run_id': 'example', 'path': 'src/lib.rs',
                                                 'source_sha256': 'a' * 64, 'bytes': 82}]})

    def test_latency_export_keeps_actual_failures_and_removes_local_paths(self):
        matrix = {'complete': False, 'results': [{'command': ['/private/tmp/tools/reposcout', 'read', '.'],
            'workspace': '/private/tmp/work', 'stdout_path': '/tmp/out', 'stderr_path': '/tmp/err',
            'pid': 123, 'exit_code': 2, 'stopped': 'rss-limit', 'elapsed_ns': 100, 'cache_hits': None}]}
        result = export_results.project_latency(matrix)
        self.assertFalse(result['complete'])
        self.assertEqual(result['results'][0]['exit_code'], 2)
        self.assertEqual(result['results'][0]['command'][0], 'reposcout')
        self.assertEqual(result['results'][0]['stopped'], 'rss-limit')
        self.assertNotIn('pid', result['results'][0])


if __name__ == '__main__':
    unittest.main()
