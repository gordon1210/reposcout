"""Close an evaluated trial from explicit quality and lifecycle evidence without inferring missing usage."""

import argparse
from pathlib import Path

import accounting
import native_codex
import native_evidence
import pilot


def close_trial(trial_root, log, session_id, agent_path, parent_id):
    root = Path(trial_root)
    trial = accounting.read_json(root / 'trial.json')
    audit = native_codex.audit(native_codex.sanitized_records(log), session_id)
    evidence = native_evidence.collect(log, session_id)
    accounting.require(evidence['metadata']['parent_thread_id'] == parent_id, 'unexpected native parent')
    accounting.require(evidence['metadata']['agent_path'] == agent_path, 'unexpected native agent path')
    accounting.require(evidence['metadata']['agent_role'] == 'astra_specialist', 'unexpected native profile')
    accounting.require(evidence['child_activity_markers'] == 0, 'child activity requires expanded inventory')
    lifecycle = evidence['lifecycle']
    accounting.require(len(lifecycle) == 2 and [item['type'] for item in lifecycle] == ['task_started', 'task_complete'],
                       'expected one completed fresh native turn')
    accounting.require(lifecycle[0]['turn_id'] == lifecycle[1]['turn_id'], 'native completion turn mismatch')
    accounting.require(evidence['token_count_records'] == audit['end_at'], 'native usage boundary mismatch')
    accounting.require(set(evidence['response_ids']) == {item['response_id'] for item in audit['response_usage']},
                       'response usage coverage mismatch')
    quality = pilot.verify_trial(trial, accounting.read_json(root / 'answer.json'))
    manifest = {**trial, 'model': audit['model'], 'effort': audit['effort'], 'provider': 'openai',
                'harness': 'native-codex/astra_specialist/fork-none', 'retention_policy': 'native-default-unchanged'}
    completion = {'schema': 1, 'run_id': trial['run_id'], 'agent_path': agent_path, 'session_id': session_id,
                  'parent_thread_id': parent_id, 'main_attested_complete': True,
                  'main_attested_fresh_exact_prompt': True, 'main_attested_source': 'authorized-main-task-handoff',
                  'prompt_sha256': trial['prompt_sha256'], 'native_evidence_sha256': evidence['evidence_sha256'],
                  'observed_completion': lifecycle[-1], 'closed': True, 'inventory_complete': True, 'children': []}
    status = 'success' if quality['passed'] and not evidence['error_markers'] else 'failed'
    attestation = {'run_id': trial['run_id'], 'closed': True, 'inventory_complete': True, 'status': status,
                   'evidence_sha256': accounting.fingerprint(completion), 'evidence_reference': str(root / 'lifecycle-evidence.json'),
                   'sessions': [{'session_id': session_id, 'parent_session_id': None, 'end_at': audit['end_at'],
                                 'projection_sha256': audit['projection_sha256'], 'status': status}]}
    closed = native_codex.close_window(manifest, [audit], attestation, quality)
    artifacts = {'usage-audit.json': audit, 'native-evidence.json': evidence, 'quality.json': quality,
                 'native-manifest.json': manifest, 'lifecycle-evidence.json': completion,
                 'attestation.json': attestation, 'closed-window.json': closed,
                 'final.json': {'schema': 1, 'run_id': trial['run_id'], 'agent_id': agent_path,
                                'native_session_id': session_id, 'native_log': str(log), 'quality': quality,
                                'lifecycle_evidence_sha256': attestation['evidence_sha256'],
                                'usage_status': 'closed-native-window'}}
    accounting.require(not any((root / name).exists() for name in artifacts), 'trial closure artifacts already exist')
    for name, value in artifacts.items():
        pilot.write_new(root / name, value)
    return {'run_id': trial['run_id'], 'status': status, 'total_tokens': audit['model_tokens']['total_tokens'],
            'input_tokens': audit['model_tokens']['input_tokens'], 'output_tokens': audit['model_tokens']['output_tokens'],
            'cached_input_tokens': audit['model_tokens']['cached_input_tokens'], 'end_at': audit['end_at'],
            'projection_sha256': audit['projection_sha256'], 'tool_calls': len(evidence['tool_calls']),
            'tool_text_bytes': evidence['tool_returned_text_utf8_bytes'], 'duration_ms': lifecycle[-1]['duration_ms'],
            'compactions': evidence['compaction_events'], 'closed_window': str(root / 'closed-window.json')}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('trial')
    parser.add_argument('--log', required=True)
    parser.add_argument('--session-id', required=True)
    parser.add_argument('--agent-path', required=True)
    parser.add_argument('--parent-id', required=True)
    parser.add_argument('--main-attested-complete', action='store_true', required=True)
    args = parser.parse_args()
    try:
        import json
        print(json.dumps(close_trial(args.trial, args.log, args.session_id, args.agent_path, args.parent_id), sort_keys=True))
    except (accounting.InvalidLedger, OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(2, f'{error}\n')


if __name__ == '__main__':
    main()
