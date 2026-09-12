"""Extract bounded, sanitized tool and lifecycle evidence from native logs without exporting source or response bodies."""

import argparse
import json
from datetime import datetime
from pathlib import Path

from accounting import InvalidLedger, fingerprint, integer, object_pairs, require
from native_codex import MAX_LINE_BYTES, MAX_LOG_BYTES, MAX_RECORDS


def timestamp(value):
    return datetime.fromisoformat(value.replace('Z', '+00:00'))


def collect(path, session_id):
    total_bytes = 0
    commands = {}
    calls = {}
    outputs = {}
    lifecycle = []
    metadata = None
    child_markers = 0
    error_markers = 0
    compactions = 0
    token_records = 0
    response_ids = set()
    with Path(path).open('rb') as stream:
        for ordinal, line in enumerate(iter(lambda: stream.readline(MAX_LINE_BYTES + 1), b''), 1):
            total_bytes += len(line)
            require(total_bytes <= MAX_LOG_BYTES and len(line) <= MAX_LINE_BYTES and ordinal <= MAX_RECORDS,
                    'native evidence exceeds bounded limits')
            try:
                record = json.loads(line, object_pairs_hook=object_pairs)
            except ValueError:
                raise InvalidLedger(f'invalid native JSON at record {ordinal}') from None
            payload = record.get('payload', {})
            kind = record.get('type')
            if not isinstance(payload, dict):
                continue
            event = payload.get('type')
            at = record.get('timestamp')
            if kind == 'session_meta':
                require(metadata is None and payload.get('id') == session_id, 'unexpected session metadata')
                source = payload.get('source', {})
                spawn = source.get('subagent', {}).get('thread_spawn', {}) if isinstance(source, dict) else {}
                metadata = {key: payload.get(key) for key in ('id', 'cli_version', 'model_provider', 'agent_path', 'agent_role')}
                metadata['parent_thread_id'] = spawn.get('parent_thread_id')
            elif kind == 'token_usage_record':
                require(payload.get('thread_id') == session_id, 'foreign response usage')
                identity = payload.get('response_id')
                require(isinstance(identity, str) and identity not in response_ids, 'duplicate response usage')
                response_ids.add(identity)
            elif kind == 'compacted' or event in ('context_compacted', 'ContextCompaction'):
                compactions += 1
            elif kind == 'event_msg':
                if event == 'token_count':
                    token_records += 1
                elif event in ('task_started', 'task_complete', 'turn_aborted', 'error'):
                    lifecycle.append({'type': event, 'timestamp': at, **{key: payload.get(key) for key in
                                      ('turn_id', 'duration_ms', 'time_to_first_token_ms')}})
                    error_markers += event in ('turn_aborted', 'error')
                elif isinstance(event, str) and ('spawn' in event or event.startswith('collab_')):
                    child_markers += 1
                elif event == 'item_completed':
                    item = payload.get('item', {})
                    item_kind = item.get('type')
                    if item_kind == 'ContextCompaction':
                        compactions += 1
                    elif item_kind == 'CommandExecution':
                        identity = item.get('id')
                        require(identity not in commands, 'duplicate command completion')
                        duration = item.get('duration', {})
                        seconds = integer(duration.get('secs'), 'duration seconds')
                        nanos = integer(duration.get('nanos'), 'duration nanos')
                        require(nanos < 1_000_000_000, 'invalid duration nanos')
                        commands[identity] = {'id': identity, 'status': item.get('status'), 'exit_code': item.get('exit_code'),
                            'stdout_bytes': len(item.get('stdout', '').encode()),
                            'stderr_bytes': len(item.get('stderr', '').encode()),
                            'reported_duration_ns': seconds * 1_000_000_000 + nanos}
            elif kind == 'response_item' and event in ('custom_tool_call', 'function_call'):
                identity = payload.get('call_id')
                require(identity not in calls, 'duplicate tool call')
                name = payload.get('name')
                raw = payload.get('input', payload.get('arguments', ''))
                child_markers += name in ('spawn_agent', 'followup_task') or any(
                    marker in raw for marker in ('spawn_agent(', 'spawn_agent"', 'followup_task(', 'grok -p', 'claude -p'))
                calls[identity] = {'call_id': identity, 'name': name, 'timestamp': at, 'input_utf8_bytes': len(raw.encode())}
            elif kind == 'response_item' and event in ('custom_tool_call_output', 'function_call_output'):
                identity = payload.get('call_id')
                require(identity not in outputs, 'duplicate tool output')
                value = payload.get('output')
                if isinstance(value, list):
                    require(all(isinstance(item, dict) and isinstance(item.get('text'), str) for item in value),
                            'unsupported tool-output content')
                    size = sum(len(item['text'].encode()) for item in value)
                else:
                    require(isinstance(value, str), 'unsupported tool-output shape')
                    size = len(value.encode())
                outputs[identity] = {'timestamp': at, 'returned_text_utf8_bytes': size}
    require(metadata is not None, 'missing native metadata')
    require(set(calls) == set(outputs), 'unmatched native tool calls and outputs')
    for identity, call in calls.items():
        output = outputs[identity]
        elapsed = (timestamp(output['timestamp']) - timestamp(call['timestamp'])).total_seconds() * 1000
        require(elapsed >= 0, 'tool output precedes call')
        call.update(output)
        call['tool_roundtrip_elapsed_ms'] = round(elapsed, 3)
    result = {'schema': 1, 'session_id': session_id, 'metadata': metadata, 'lifecycle': lifecycle,
              'token_count_records': token_records, 'response_ids': sorted(response_ids),
              'child_activity_markers': child_markers, 'error_markers': error_markers, 'compaction_events': compactions,
              'commands': list(commands.values()), 'tool_calls': list(calls.values()),
              'tool_returned_text_utf8_bytes': sum(value['returned_text_utf8_bytes'] for value in outputs.values()),
              'command_stdout_bytes': sum(value['stdout_bytes'] for value in commands.values()),
              'command_stderr_bytes': sum(value['stderr_bytes'] for value in commands.values()),
              'source_overlap': None, 'source_followup_reads': None,
              'source_metrics_coverage': 'not-derived-from-uninstrumented-shell-commands',
              'child_detection_coverage': 'native-events-and-explicit-call-markers; external-lifecycle-attestation-required'}
    result['evidence_sha256'] = fingerprint(result)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('log')
    parser.add_argument('--session-id', required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(collect(args.log, args.session_id), sort_keys=True, indent=2))
    except (InvalidLedger, OSError, ValueError, TypeError, KeyError) as error:
        parser.exit(2, f'{error}\n')


if __name__ == '__main__':
    main()
