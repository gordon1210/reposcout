"""Check bounded prompt-delivery metadata, preserving opaque payload hashes and explicit plaintext-verification gaps without exporting task text."""

import argparse
import base64
import hashlib
import json
from pathlib import Path

from accounting import InvalidLedger, object_pairs, read_json, require
from native_codex import MAX_LINE_BYTES, MAX_LOG_BYTES, MAX_RECORDS
from pilot import write_new


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


def compare_payload(message, prepared):
    exact = message == prepared
    newline_only = message.rstrip('\n') == prepared.rstrip('\n')
    opaque = False
    try:
        decoded = base64.b64decode(message, altchars=b'-_', validate=True)
        opaque = len(decoded) > 64 and decoded[0] == 128
    except (ValueError, UnicodeEncodeError):
        pass
    return {'prepared_prompt_sha256': digest(prepared), 'recorded_payload_sha256': digest(message),
            'recorded_payload_kind': 'opaque-fernet-shaped' if opaque else 'plaintext',
            'exact_plaintext_equal': None if opaque else exact,
            'equal_ignoring_final_newlines': None if opaque else newline_only,
            'independent_plaintext_verification': 'unavailable-encrypted-record' if opaque else 'performed'}


def audit(root_log, campaign, expected_parent, main_attested=False):
    require(main_attested, 'explicit main prompt-delivery attestation required')
    registry = {}
    for final_path in sorted(Path(campaign).rglob('final.json')):
        final = read_json(final_path)
        trial = read_json(final_path.parent / 'trial.json')
        agent = final['agent_id']
        require(agent.startswith('/root/m2_') and agent.count('/') == 2, 'non-benchmark agent in registry')
        name = agent.rsplit('/', 1)[-1]
        require(name not in registry, 'duplicate benchmark agent identity')
        prepared = Path(trial['prompt_path']).read_text()
        require(digest(prepared) == trial['prompt_sha256'], 'prepared prompt changed')
        registry[name] = (trial, prepared)
    require(0 < len(registry) <= 100, 'invalid bounded benchmark registry')
    found = {}
    total = 0
    parent_verified = False
    with Path(root_log).open('rb') as stream:
        for ordinal, line in enumerate(iter(lambda: stream.readline(MAX_LINE_BYTES + 1), b''), 1):
            total += len(line)
            require(total <= MAX_LOG_BYTES and len(line) <= MAX_LINE_BYTES and ordinal <= MAX_RECORDS,
                    'root prompt audit exceeds limits')
            row = json.loads(line, object_pairs_hook=object_pairs)
            payload = row.get('payload', {})
            if row.get('type') == 'session_meta':
                require(payload.get('id') == expected_parent and not parent_verified, 'unexpected parent session')
                parent_verified = True
            if row.get('type') != 'response_item' or payload.get('name') != 'spawn_agent':
                continue
            arguments = json.loads(payload.get('arguments', payload.get('input', '{}')), object_pairs_hook=object_pairs)
            name = arguments.get('task_name')
            if name not in registry:
                continue
            require(name not in found, 'duplicate measured spawn call')
            require(arguments.get('agent_type') == 'astra_specialist' and arguments.get('fork_turns') == 'none',
                    'benchmark spawn profile differs')
            trial, prepared = registry[name]
            message = arguments.get('message')
            require(isinstance(message, str), 'missing native spawn message')
            found[name] = {'run_id': trial['run_id'], 'task_name': name, 'parent_session_id': expected_parent,
                           'spawn_call_id': payload.get('call_id'), 'profile': 'astra_specialist', 'fork_turns': 'none',
                           'main_attests_exact_prepared_prompt': True,
                           **compare_payload(message, prepared)}
    require(parent_verified and set(found) == set(registry), 'incomplete benchmark spawn coverage')
    return {'schema': 1, 'runs': sorted(found.values(), key=lambda item: item['run_id']),
            'raw_payloads_exported': False, 'decryption_attempted': False}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('root_log')
    parser.add_argument('campaign')
    parser.add_argument('output')
    parser.add_argument('--parent-session-id', required=True)
    parser.add_argument('--main-attested-exact-prompts', action='store_true', required=True)
    args = parser.parse_args()
    try:
        result = audit(args.root_log, args.campaign, args.parent_session_id, args.main_attested_exact_prompts)
        write_new(Path(args.output), result)
        print(json.dumps({'runs': len(result['runs']), 'plaintext_equal': sum(
            item['exact_plaintext_equal'] is True for item in result['runs']), 'opaque': sum(
            item['exact_plaintext_equal'] is None for item in result['runs'])}))
    except (InvalidLedger, OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(2, f'{error}\n')


if __name__ == '__main__':
    main()
