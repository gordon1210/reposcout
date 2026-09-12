"""Audit native Codex usage windows and externally attested session lifecycles without inventing provider-call identities."""

import argparse
import json
from pathlib import Path

from accounting import CONDITIONS, InvalidLedger, fingerprint, integer, object_pairs, read_json, require, sha256, text

FIELDS = ('input_tokens', 'cached_input_tokens', 'cache_write_input_tokens',
          'output_tokens', 'reasoning_output_tokens', 'total_tokens')
MAX_LOG_BYTES = 256 * 1024 * 1024
MAX_LINE_BYTES = 16 * 1024 * 1024
MAX_RECORDS = 1_000_000


def usage(value):
    require(isinstance(value, dict), 'missing native usage object')
    result = {key: integer(value.get(key), key) for key in FIELDS}
    require(result['input_tokens'] + result['output_tokens'] == result['total_tokens'],
            'native total differs from input plus output')
    require(result['cached_input_tokens'] + result['cache_write_input_tokens'] <= result['input_tokens'],
            'native cache exceeds input')
    require(result['reasoning_output_tokens'] <= result['output_tokens'], 'native reasoning exceeds output')
    return result


def sanitized_records(path):
    total = 0
    count = 0
    with Path(path).open('rb') as stream:
        while line := stream.readline(MAX_LINE_BYTES + 1):
            total += len(line)
            count += 1
            require(total <= MAX_LOG_BYTES and len(line) <= MAX_LINE_BYTES and count <= MAX_RECORDS,
                    'native log exceeds bounded adapter limits')
            try:
                record = json.loads(line, object_pairs_hook=object_pairs)
            except ValueError:
                raise InvalidLedger(f'invalid JSON at native record {count}') from None
            require(isinstance(record, dict), 'native record must be an object')
            kind = record.get('type')
            payload = record.get('payload')
            if not isinstance(payload, dict):
                continue
            if kind == 'session_meta':
                yield {'type': kind, 'payload': {key: payload.get(key) for key in
                       ('id', 'cli_version', 'model_provider', 'forked_from_id')}}
            elif kind == 'token_usage_record':
                yield {'type': kind, 'payload': {key: payload.get(key) for key in
                       ('thread_id', 'turn_id', 'response_id', 'usage', 'thread_token_usage')}}
            elif kind == 'turn_context':
                yield {'type': kind, 'payload': {key: payload.get(key) for key in
                       ('turn_id', 'model', 'effort')}}
            elif kind == 'event_msg' and payload.get('type') == 'token_count':
                info = payload.get('info')
                yield {'type': 'token_count', 'timestamp': record.get('timestamp'),
                       'payload': {key: info.get(key) for key in ('total_token_usage', 'last_token_usage')}
                       if isinstance(info, dict) else None}


def audit(records, session_id, start_after=0, end_at=None):
    integer(start_after, 'start_after')
    if end_at is not None:
        integer(end_at, 'end_at')
        require(end_at > start_after, 'empty or reversed native window')
    metadata = None
    context = None
    previous_last = None
    previous = dict.fromkeys(FIELDS, 0)
    baseline = dict(previous) if start_after == 0 else None
    final = None
    ordinal = 0
    repeated = 0
    advances = []
    responses = []
    response_ids = set()
    contexts = set()
    for record in records:
        kind = record['type']
        payload = record['payload']
        if kind == 'session_meta':
            require(metadata is None or metadata == payload, 'conflicting session metadata')
            require(payload.get('id') == session_id, 'unexpected native session')
            require(payload.get('model_provider') == 'openai', 'unsupported native provider')
            metadata = payload
        elif kind == 'token_usage_record':
            require(payload.get('thread_id') == session_id, 'foreign response usage thread')
            response_id = text(payload.get('response_id'), 'response_id')
            require(response_id not in response_ids, 'duplicate or conflicting response usage')
            response_ids.add(response_id)
            responses.append({'response_id': response_id, 'usage': usage(payload.get('usage')),
                              'thread_token_usage': usage(payload.get('thread_token_usage'))})
        elif kind == 'turn_context':
            context = payload
        elif kind == 'token_count':
            ordinal += 1
            if payload is None:
                require(ordinal <= start_after, 'missing usage in requested native window')
                continue
            current = usage(payload['total_token_usage'])
            last = usage(payload['last_token_usage'])
            if ordinal <= start_after:
                previous = current
                previous_last = last
                if ordinal == start_after:
                    baseline = current
                continue
            require(metadata is not None and context is not None, 'missing native session/model metadata')
            require(baseline is not None, 'missing start boundary usage')
            model = text(context.get('model'), 'native model')
            effort = text(context.get('effort'), 'native effort')
            contexts.add((model, effort))
            require(len(contexts) == 1, 'model or effort changed within native window')
            delta = {key: current[key] - previous[key] for key in FIELDS}
            require(all(value >= 0 for value in delta.values()), f'cumulative usage reset at token event {ordinal}')
            if any(delta.values()):
                require(delta == last, f'usage delta differs from last call at token event {ordinal}')
                advances.append({'token_event': ordinal, 'timestamp': record.get('timestamp'),
                                 'turn_id': context.get('turn_id'), 'usage': delta,
                                 'projection_sha256': fingerprint(record)})
            else:
                require(previous_last is None or last == previous_last, "conflicting repeated usage update")
                repeated += 1
            previous = current
            previous_last = last
            final = current
            if end_at == ordinal:
                break
    require(final is not None and advances, 'native window contains no measured usage advances')
    require(end_at is None or ordinal == end_at, 'requested native end boundary missing')
    totals = {key: final[key] - baseline[key] for key in FIELDS}
    totals['uncached_input_tokens'] = totals['input_tokens'] - totals['cached_input_tokens'] - totals['cache_write_input_tokens']
    model, effort = next(iter(contexts))
    selected_responses = [item for item in responses
                          if item['thread_token_usage']['total_tokens'] > baseline['total_tokens']]
    if selected_responses:
        require([item['usage'] for item in selected_responses] == [item['usage'] for item in advances],
                'response usage differs from cumulative window updates')
        accumulated = dict(baseline)
        for item in selected_responses:
            accumulated = {key: accumulated[key] + item['usage'][key] for key in FIELDS}
            require(accumulated == item['thread_token_usage'], 'response thread total differs from accumulated usage')
    return {'schema': 1, 'kind': 'native-codex-usage-audit', 'session_id': session_id,
            'cli_version': metadata.get('cli_version'), 'forked_from_id': metadata.get('forked_from_id'),
            'model': model, 'effort': effort, 'start_after': start_after, 'end_at': ordinal,
            'model_tokens': totals, 'advancing_updates': len(advances), 'repeated_updates': repeated,
            'updates': advances, 'usage_complete': False, 'provider_call_ids_available': bool(selected_responses),
            'response_usage': selected_responses,
            'coverage': 'unverified-provider-call-and-child-completeness',
            'projection_sha256': fingerprint(advances)}



def close_window(manifest, audits, attestation, quality):
    require(manifest.get('schema') == 1, 'invalid native manifest schema')
    require(manifest.get('variant') in ('baseline', 'reposcout'), 'invalid native variant')
    require(manifest.get('mode') in ('isolated', 'end-to-end'), 'invalid native mode')
    require(manifest.get('evidence_kind') in ('synthetic', 'observed'), 'invalid native evidence kind')
    for field in CONDITIONS + ('run_id', 'effort'):
        text(manifest.get(field), field)
    for field in ('fixture_sha256', 'task_sha256', 'oracle_sha256', 'start_context_sha256'):
        sha256(manifest[field], field)
    require(attestation.get('closed') is True and attestation.get('inventory_complete') is True,
            'native lifecycle and agent inventory attestation required')
    sha256(attestation.get('evidence_sha256'), 'attestation evidence')
    text(attestation.get('evidence_reference'), 'attestation reference')
    require(attestation.get('run_id') == manifest['run_id'], 'foreign native attestation')
    status = attestation.get('status')
    require(status in ('success', 'failed', 'aborted'), 'invalid native outcome')
    require(type(quality.get('passed')) is bool, 'quality outcome required')
    require(isinstance(quality.get('regressions'), list) and isinstance(quality.get('missing_evidence'), list),
            'quality evidence lists required')
    sha256(quality.get('evidence_sha256'), 'quality evidence')
    require(status != 'success' or quality['passed'], 'success requires quality pass')
    entries = attestation.get('sessions')
    require(isinstance(entries, list) and entries, 'native session inventory required')
    by_id = {}
    for item in audits:
        require(item.get('kind') == 'native-codex-usage-audit' and item.get('schema') == 1,
                'invalid native audit')
        identity = text(item.get('session_id'), 'native session id')
        require(identity not in by_id, 'duplicate native session audit')
        require(item.get('start_after') == 0, 'closed pilot requires fresh session accounting')
        require(item.get('model') == manifest['model'] and item.get('effort') == manifest['effort'],
                'native model or effort differs from manifest')
        require(manifest['provider'] == 'openai', 'unsupported native provider')
        require(item.get('projection_sha256') == fingerprint(item.get('updates')), 'native projection hash mismatch')
        summed = dict.fromkeys(FIELDS, 0)
        for update in item['updates']:
            values = usage(update['usage'])
            for key in FIELDS:
                summed[key] += values[key]
        require(all(item['model_tokens'].get(key) == summed[key] for key in FIELDS), 'native audit total mismatch')
        by_id[identity] = item
    registered = set()
    roots = 0
    for entry in entries:
        identity = text(entry.get('session_id'), 'attested session id')
        require(identity in by_id and identity not in registered, 'missing or duplicate attested session')
        parent = entry.get('parent_session_id')
        require(parent is None or parent in registered, 'native parents must precede children')
        roots += parent is None
        registered.add(identity)
        item = by_id[identity]
        require(entry.get('end_at') == item['end_at'] and entry.get('projection_sha256') == item['projection_sha256'],
                'native closing boundary differs from audit')
        require(entry.get('status') in ('success', 'failed', 'aborted'), 'native session outcome required')
    require(registered == set(by_id) and roots == 1, 'native inventory is incomplete or has multiple roots')
    summed = {key: sum(item['model_tokens'][key] for item in audits) for key in FIELDS}
    conditions = {key: manifest[key] for key in CONDITIONS + ('effort',)}
    conditions['native_cli_versions'] = sorted({text(item.get('cli_version'), 'native cli_version') for item in audits})
    return {'schema': 1, 'kind': 'native-codex-closed-window', 'run_id': manifest['run_id'],
            'variant': manifest['variant'], 'conditions': conditions,
            'status': status, 'quality': quality, 'attestation': attestation,
            'accounting_basis': 'attested-native-session-cumulative-windows', 'usage_complete': True,
            'model_tokens': summed, 'agents': len(audits), 'audits': audits}


def compare_windows(baseline, candidate):
    require(all(item.get('kind') == 'native-codex-closed-window' for item in (baseline, candidate)),
            'closed native windows required')
    require(baseline['variant'] == 'baseline' and candidate['variant'] == 'reposcout', 'invalid comparison variants')
    require(baseline['conditions'] == candidate['conditions'], 'incomparable native scenarios')
    eligible = all(item['conditions']['evidence_kind'] == 'observed' and item['usage_complete']
                   and item['status'] == 'success' and item['quality']['passed']
                   and not item['quality']['regressions'] and not item['quality']['missing_evidence']
                   for item in (baseline, candidate))
    if eligible:
        require(baseline['run_id'] != candidate['run_id'], 'paired runs must have different identities')
        require(not ({item['session_id'] for item in baseline['audits']} &
                     {item['session_id'] for item in candidate['audits']}), 'paired runs share native sessions')
    return {'schema': 1, 'kind': 'native-codex-window-comparison', 'eligible': eligible,
            'baseline': baseline, 'reposcout': candidate,
            'token_delta': candidate['model_tokens']['total_tokens'] - baseline['model_tokens']['total_tokens']
            if eligible else None}


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest='command', required=True)
    inspect = commands.add_parser('audit')
    inspect.add_argument('log')
    inspect.add_argument('--session-id', required=True)
    inspect.add_argument('--start-after', type=int, default=0)
    inspect.add_argument('--end-at', type=int)
    close = commands.add_parser('close')
    close.add_argument('manifest')
    close.add_argument('attestation')
    close.add_argument('quality')
    close.add_argument('audits', nargs='+')
    compare = commands.add_parser('compare')
    compare.add_argument('baseline')
    compare.add_argument('reposcout')
    args = parser.parse_args()
    try:
        if args.command == 'audit':
            result = audit(sanitized_records(args.log), args.session_id, args.start_after, args.end_at)
        elif args.command == 'close':
            result = close_window(read_json(args.manifest), [read_json(path) for path in args.audits],
                                  read_json(args.attestation), read_json(args.quality))
        else:
            result = compare_windows(read_json(args.baseline), read_json(args.reposcout))
        print(json.dumps(result, sort_keys=True, indent=2))
    except (InvalidLedger, OSError, ValueError, TypeError, KeyError) as error:
        parser.exit(2, f'{error}\n')


if __name__ == '__main__':
    main()
