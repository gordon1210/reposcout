import json
import tempfile
import unittest
from pathlib import Path

import accounting
import native_evidence


class EvidenceTests(unittest.TestCase):
    def records(self):
        return [
            {'type': 'session_meta', 'payload': {'id': 'synthetic-session', 'cli_version': 'synthetic',
             'model_provider': 'openai', 'base_instructions': 'PRIVATE',
             'source': {'subagent': {'thread_spawn': {'parent_thread_id': 'parent'}}}}},
            {'type': 'event_msg', 'timestamp': '2026-01-01T00:00:00Z',
             'payload': {'type': 'task_started', 'turn_id': 'turn'}},
            {'type': 'response_item', 'timestamp': '2026-01-01T00:00:01Z',
             'payload': {'type': 'custom_tool_call', 'call_id': 'call', 'name': 'exec', 'input': 'PRIVATE'}},
            {'type': 'event_msg', 'payload': {'type': 'item_completed', 'item': {
                'type': 'CommandExecution', 'id': 'command', 'duration': {'secs': 0, 'nanos': 12},
                'status': 'completed', 'exit_code': 0, 'stdout': 'ä', 'stderr': '', 'command': ['PRIVATE']}}},
            {'type': 'response_item', 'timestamp': '2026-01-01T00:00:01.250Z',
             'payload': {'type': 'custom_tool_call_output', 'call_id': 'call',
                         'output': [{'type': 'input_text', 'text': 'äPRIVATE'}]}},
            {'type': 'compacted', 'payload': {'message': 'PRIVATE'}},
            {'type': 'event_msg', 'timestamp': '2026-01-01T00:00:02Z',
             'payload': {'type': 'task_complete', 'turn_id': 'turn', 'duration_ms': 2000,
                         'time_to_first_token_ms': 500, 'last_agent_message': 'PRIVATE'}},
        ]

    def collect(self, records):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'synthetic.jsonl'
            path.write_text(''.join(json.dumps(record) + '\n' for record in records))
            return native_evidence.collect(path, 'synthetic-session')

    def test_sanitized_bytes_latency_compaction_and_missing_source_metrics(self):
        result = self.collect(self.records())
        self.assertNotIn('PRIVATE', json.dumps(result))
        self.assertEqual(result['command_stdout_bytes'], 2)
        self.assertEqual(result['tool_returned_text_utf8_bytes'], 9)
        self.assertEqual(result['tool_calls'][0]['tool_roundtrip_elapsed_ms'], 250)
        self.assertEqual(result['compaction_events'], 1)
        self.assertIsNone(result['source_overlap'])

    def test_unmatched_duplicate_and_child_markers(self):
        records = self.records()
        records.pop(4)
        with self.assertRaises(accounting.InvalidLedger):
            self.collect(records)
        records = self.records()
        records.insert(4, records[3])
        with self.assertRaises(accounting.InvalidLedger):
            self.collect(records)
        records = self.records()
        records[2]['payload']['input'] = 'spawn_agent({})'
        self.assertEqual(self.collect(records)['child_activity_markers'], 1)


if __name__ == '__main__':
    unittest.main()
