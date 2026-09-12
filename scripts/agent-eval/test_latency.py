import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import accounting
import fixtures
import latency


class LatencyTests(unittest.TestCase):
    def test_matrix_is_serial_bounded_and_retains_real_phase_metadata(self):
        calls = []

        def measured(command, workspace):
            calls.append((command, workspace))
            return ({'pid': 1, 'exit_code': 0, 'elapsed_ns': 10, 'sampled_peak_rss_kib': None,
                     'stdout_bytes': 2, 'stderr_bytes': 0, 'stopped': None}, b'{}', b'')

        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            binary = root / 'synthetic-binary'
            binary.write_bytes(b'never executed')
            with patch.object(latency, 'measured_command', side_effect=measured), patch.object(latency.pilot, 'git', return_value='a' * 40):
                result = latency.matrix(root / 'matrix', binary, fixtures.digest(binary.read_bytes()))
            self.assertEqual(len(calls), 18)
            matrix = accounting.read_json(result['matrix'])
            self.assertFalse(matrix['model_tokens_measured'])
            for index, entry in enumerate(matrix['results']):
                self.assertEqual(entry['phase'], ('cold-requested-fresh-root', 'warm-requested-repeat', 'after-edit')[index % 3])
                self.assertIsNone(entry['cache_hits'])
                self.assertEqual(entry['edit'] is not None, index % 3 == 2)
                if entry['edit']:
                    self.assertNotEqual(entry['edit']['before_sha256'], entry['edit']['after_sha256'])


if __name__ == '__main__':
    unittest.main()
