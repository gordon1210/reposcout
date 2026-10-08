"""Bounded current Codex exec observations, separate from strict historical ledgers.

An exec turn usage object is an observation, not a final provider-call ledger.
Controller closure can establish the corresponding emitted-usage basis; it cannot
establish subscription charges, complete provider calls or actual model context.
"""

import argparse
import hashlib
import json
import re
import shlex
from datetime import datetime
from pathlib import Path

from accounting import InvalidLedger, fingerprint, object_pairs, read_json, require

ADAPTER = 'codex-exec-jsonl-v1'
SCHEMA = 1
MAX_TRACE_BYTES = 64 * 1024 * 1024
MAX_LINE_BYTES = 2 * 1024 * 1024
MAX_EVENTS = 100_000
MAX_INVOCATIONS = 100
MAX_INTEGER = 2 ** 63 - 1
CORE_FIELDS = ('input_tokens', 'cached_input_tokens', 'output_tokens')
OPTIONAL_FIELDS = ('cache_write_input_tokens', 'reasoning_output_tokens', 'total_tokens')
USAGE_FIELDS = CORE_FIELDS + OPTIONAL_FIELDS
SCOPES = ('unknown', 'turn', 'invocation', 'thread-cumulative')
KNOWN_ITEMS = {'reasoning', 'agent_message', 'command_execution', 'file_change',
               'mcp_tool_call', 'web_search', 'todo_list', 'error', 'context_compaction'}
EFFECT_ITEMS = {'command_execution', 'file_change', 'mcp_tool_call', 'web_search'}
READ_COMMANDS = {'cat', 'sed', 'head', 'tail', 'bat', 'less', 'more'}
SOURCE_COMMANDS = READ_COMMANDS | {'rg', 'grep'}
SHELLS = {'bash', 'sh', 'dash', 'zsh'}
REPOSCOUT = {'reposcout', 'reposcoutdev'}


def _integer(value):
    return type(value) is int and 0 <= value <= MAX_INTEGER


def _identity(value):
    return isinstance(value, str) and 0 < len(value) <= 4096


def _safe_name(value):
    return value if isinstance(value, str) and re.fullmatch(r'[A-Za-z0-9_.:-]{1,128}', value) else 'invalid'


def _usage(value):
    result = dict.fromkeys(USAGE_FIELDS)
    unknown = []
    errors = []
    if not isinstance(value, dict):
        return result, list(USAGE_FIELDS), ['missing-or-unsupported-usage-object']
    extra = set(value) - set(USAGE_FIELDS)
    if extra:
        errors.append('unknown-usage-format')
        unknown.extend('unsupported:' + _safe_name(field) for field in sorted(extra))
    for field in USAGE_FIELDS:
        if field not in value or value[field] is None:
            unknown.append(field)
        elif _integer(value[field]):
            result[field] = value[field]
        else:
            errors.append('invalid-usage-number:' + field)
            unknown.append(field)
    if result['input_tokens'] is not None and result['cached_input_tokens'] is not None:
        if result['cached_input_tokens'] > result['input_tokens']:
            errors.append('cached-input-exceeds-input')
    if result['output_tokens'] is not None and result['reasoning_output_tokens'] is not None:
        if result['reasoning_output_tokens'] > result['output_tokens']:
            errors.append('reasoning-exceeds-output')
    if all(result[field] is not None for field in ('input_tokens', 'output_tokens', 'total_tokens')):
        if result['total_tokens'] != result['input_tokens'] + result['output_tokens']:
            errors.append('reported-total-disagrees')
    return result, unknown, errors


def _sum_usage(values):
    return {field: sum(value[field] for value in values) if values and
            all(value[field] is not None for value in values) else None for field in USAGE_FIELDS}


def _known_sum(values):
    return {field: sum(value[field] for value in values if value[field] is not None)
            if any(value[field] is not None for value in values) else None for field in USAGE_FIELDS}


def _timestamp(value):
    if not isinstance(value, str):
        return None
    try:
        at = datetime.fromisoformat(value.replace('Z', '+00:00'))
        return at if at.tzinfo is not None else None
    except ValueError:
        return None


def _shell_segments(command, depth=0):
    """Recognize lexical executable positions; never execute or expand shell input."""
    if depth > 3 or not isinstance(command, str):
        return [], False, False
    try:
        lexer = shlex.shlex(command, posix=True, punctuation_chars=';&|()<>')
        lexer.whitespace_split = True
        words = list(lexer)
    except ValueError:
        return [], False, False
    if not words:
        return [], True, False
    first = Path(words[0]).name
    if first in SHELLS:
        for index, word in enumerate(words[1:], 1):
            if word.startswith('-') and 'c' in word[1:]:
                return _shell_segments(words[index + 1], depth + 1) if index + 1 < len(words) else ([], False, False)
        return [], False, False
    literal = not any(re.fullmatch(r'[;&|()<>]+', word) for word in words)
    segments = []
    segment = []
    supported = True
    for word in words + [';']:
        if word in (';', '&&', '||', '|', '&'):
            if segment:
                while segment and re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*=.*', segment[0]):
                    segment.pop(0)
                if segment and Path(segment[0]).name == 'env':
                    segment.pop(0)
                    while segment and re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*=.*', segment[0]):
                        segment.pop(0)
                if segment:
                    segments.append(segment)
            segment = []
        elif word in ('(', ')') or '$(' in word or '`' in word or '${' in word:
            supported = False
            segment.append(word)
        else:
            segment.append(word)
    return segments, supported, literal


def _command_projection(item, started, ended_at):
    command = item.get('command')
    segments, parsed, literal = _shell_segments(command)
    # Unresolved shell expansion could move executable positions; retain an unknown.
    recognized = segments if parsed else []
    reposcout = [segment for segment in recognized if Path(segment[0]).name in REPOSCOUT]
    reads = [segment for segment in recognized if Path(segment[0]).name in SOURCE_COMMANDS]
    skills = [segment for segment in recognized if Path(segment[0]).name in READ_COMMANDS and
              any(argument.endswith('SKILL.md') for argument in segment[1:])]
    refs = [segment for segment in recognized if Path(segment[0]).name in READ_COMMANDS and
            any('/skills/reposcout/references/' in argument for argument in segment[1:])]
    # A conditional shell segment is only a marker. A launched, direct literal
    # command is the conservative observable lower bound for actual invocation.
    direct = parsed and literal and len(recognized) == 1
    output = item.get('aggregated_output')
    try:
        output_bytes = len(output.encode()) if isinstance(output, str) else None
        command_hash = hashlib.sha256(command.encode()).hexdigest() if isinstance(command, str) else None
    except UnicodeEncodeError:
        output_bytes = None
        command_hash = None
    exit_code = item.get('exit_code')
    exit_code = exit_code if type(exit_code) is int and -MAX_INTEGER <= exit_code <= MAX_INTEGER else None
    executable_observed = direct and exit_code is not None and exit_code not in (126, 127)
    read_succeeded = direct and exit_code == 0
    elapsed = item.get('duration_ms')
    elapsed = elapsed if _integer(elapsed) else None
    timing_basis = 'item-reported-duration-ms' if elapsed is not None else None
    began = _timestamp(started.get('timestamp')) if started else None
    ended = _timestamp(ended_at)
    if elapsed is None and began is not None and ended is not None and ended >= began:
        elapsed = round((ended - began).total_seconds() * 1000, 3)
        timing_basis = 'event-timestamp-interval'
    return {
        'item_id': item.get('id'), 'status': item.get('status'), 'exit_code': exit_code,
        'command_sha256': command_hash,
        'command_parse_supported': parsed, 'recognized_reposcout_invocations': len(reposcout) if executable_observed else 0,
        'reposcout_command_markers': len(reposcout),
        'recognized_reposcout_unavailable_attempts': len(reposcout) if direct and exit_code in (126, 127) else 0,
        'recognized_reposcout_help_invocations': sum('--help' in segment or '-h' in segment for segment in reposcout) if executable_observed else 0,
        'recognized_skill_read_invocations': len(skills) if read_succeeded else 0,
        'skill_read_command_markers': len(skills),
        'recognized_reposcout_reference_read_invocations': len(refs) if read_succeeded else 0,
        'contains_source_read_command_marker': bool(reads),
        'reported_output_utf8_bytes': output_bytes,
        'elapsed_ms': elapsed, 'timing_basis': timing_basis,
        'failed_command': exit_code is not None and exit_code != 0 or item.get('status') == 'failed',
    }


def _read_trace(path):
    records = []
    digest = hashlib.sha256()
    total = 0
    failures = []
    with Path(path).open('rb') as stream:
        for ordinal, line in enumerate(iter(lambda: stream.readline(MAX_LINE_BYTES + 1), b''), 1):
            total += len(line)
            digest.update(line)
            if total > MAX_TRACE_BYTES or len(line) > MAX_LINE_BYTES or ordinal > MAX_EVENTS:
                failures.append({'record': ordinal, 'code': 'trace-limit-exceeded'})
                break
            if not line.strip():
                continue
            try:
                record = json.loads(line, object_pairs_hook=object_pairs,
                                    parse_constant=lambda value: (_ for _ in ()).throw(ValueError('invalid constant')))
                if not isinstance(record, dict):
                    raise ValueError('nonobject record')
            except (ValueError, UnicodeDecodeError, RecursionError):
                failures.append({'record': ordinal, 'code': 'invalid-json-record'})
                break
            records.append((ordinal, record))
    return records, digest.hexdigest(), total, failures


def _controller_check(controller, digest, size, thread_id, terminal_status):
    errors = []
    if not isinstance(controller, dict):
        return False, ['missing-controller-closure']
    for field in ('run_id', 'invocation_id'):
        if not _identity(controller.get(field)):
            errors.append('missing-controller-' + field)
    if controller.get('stdout_sha256', controller.get('trace_sha256')) != digest:
        errors.append('controller-trace-hash-mismatch')
    if controller.get('stdout_bytes') != size or type(controller.get('stdout_bytes')) is not int:
        errors.append('controller-trace-size-mismatch')
    if controller.get('thread_id') != thread_id or thread_id is None:
        errors.append('controller-thread-mismatch')
    resumed = controller.get('resumed_thread_id')
    if resumed is not None and resumed != thread_id:
        errors.append('controller-resume-thread-mismatch')
    if controller.get('status') not in ('completed', 'failed', 'aborted', 'not-started'):
        errors.append('invalid-controller-status')
    if controller.get('status') == 'completed' and (controller.get('returncode') != 0 or
            type(controller.get('returncode')) is not int or terminal_status != 'completed'):
        errors.append('controller-terminal-status-mismatch')
    if controller.get('status') == 'failed' and (controller.get('returncode') == 0 or terminal_status == 'completed'):
        errors.append('controller-terminal-status-mismatch')
    closed = (controller.get('stream_complete') is True and controller.get('process_tree_drained') is True and
              controller.get('stdout_truncated') is False and controller.get('termination_reason') is None)
    if not closed:
        errors.append('controller-stream-not-closed')
    return not errors, errors


def _normalize_turns(turns, scope, baseline, resumed):
    values = [turn['reported_usage'] for turn in turns if turn['reported_usage'] is not None]
    errors = []
    if not values:
        return dict.fromkeys(USAGE_FIELDS), [], ['no-reported-usage']
    if scope == 'unknown':
        if len(turns) == 1 and not resumed:
            return dict(values[0]), values, errors
        return dict.fromkeys(USAGE_FIELDS), [], ['usage-scope-unknown-for-multiple-or-resumed-turns']
    if scope == 'turn':
        all_values = [turn['reported_usage'] or dict.fromkeys(USAGE_FIELDS) for turn in turns]
        return _sum_usage(all_values), all_values, errors
    if baseline is not None:
        previous, _, baseline_errors = _usage(baseline)
        errors.extend('baseline:' + error for error in baseline_errors)
    elif scope == 'thread-cumulative' and resumed:
        return dict.fromkeys(USAGE_FIELDS), [], ['missing-cumulative-resume-baseline']
    else:
        previous = dict.fromkeys(USAGE_FIELDS, 0)
    deltas = []
    for value in values:
        delta = {}
        for field in USAGE_FIELDS:
            current, prior = value[field], previous[field]
            delta[field] = current - prior if current is not None and prior is not None else None
            if delta[field] is not None and delta[field] < 0:
                errors.append('cumulative-usage-reset:' + field)
                delta[field] = None
        _, _, delta_errors = _usage(delta)
        errors.extend('cumulative-delta:' + error for error in delta_errors)
        deltas.append(delta)
        previous = value
    return _sum_usage(deltas), deltas, errors


def parse_exec_trace(path, *, controller=None, expected_thread_id=None):
    """Return sanitized observations, including malformed/failed/partial captures.

    Scope is explicit controller evidence: ``turn`` adds terminal usage;
    ``invocation`` differences cumulative usage within this process;
    ``thread-cumulative`` additionally requires a resume baseline. Unknown scope
    permits only an initial single emitted turn, a deliberately narrower basis.
    """
    records, digest, size, validation = _read_trace(path)
    turns = []
    items = {}
    commands = []
    seen_events = set()
    seen_turns = set()
    thread_id = None
    active = None
    terminal_status = None
    errors = 0
    retry_markers = 0
    unknown_events = set()
    unknown_items = set()
    compactions = 0

    def problem(ordinal, code):
        validation.append({'record': ordinal, 'code': code})

    for ordinal, record in records:
        kind = record.get('type')
        identity = record.get('event_id')
        if identity is not None:
            if not _identity(identity) or identity in seen_events:
                problem(ordinal, 'duplicate-or-invalid-event-id')
                continue
            seen_events.add(identity)
        if kind != 'thread.started' and record.get('thread_id') is not None and record.get('thread_id') != thread_id:
            problem(ordinal, 'foreign-thread-event')
            continue
        if kind == 'thread.started':
            if thread_id is not None or active is not None or turns:
                problem(ordinal, 'duplicate-or-misordered-thread-start')
                continue
            if not _identity(record.get('thread_id')):
                problem(ordinal, 'invalid-thread-id')
            else:
                thread_id = record['thread_id']
                if expected_thread_id is not None and thread_id != expected_thread_id:
                    problem(ordinal, 'unexpected-thread-id')
        elif kind == 'turn.started':
            if thread_id is None or active is not None:
                problem(ordinal, 'turn-start-without-thread-or-during-turn')
                continue
            turn_id = record.get('turn_id')
            if turn_id is not None and (not _identity(turn_id) or turn_id in seen_turns):
                problem(ordinal, 'duplicate-or-invalid-turn-id')
                continue
            if turn_id is not None:
                seen_turns.add(turn_id)
            active = {'ordinal': len(turns) + 1, 'turn_id': turn_id, 'status': 'open',
                      'reported_usage': None, 'usage_unknown_fields': list(USAGE_FIELDS),
                      'usage_errors': [], 'start_record': ordinal, 'end_record': None}
            turns.append(active)
            terminal_status = None
        elif kind in ('turn.completed', 'turn.failed', 'turn.aborted'):
            if active is None:
                problem(ordinal, 'terminal-event-without-open-turn')
                continue
            if record.get('turn_id') is not None and record.get('turn_id') != active['turn_id']:
                problem(ordinal, 'terminal-turn-id-mismatch')
            if any(item['open'] for item in items.values()):
                problem(ordinal, 'terminal-turn-with-open-items')
            active['status'] = kind.split('.')[1]
            active['end_record'] = ordinal
            if 'usage' in record or kind == 'turn.completed':
                usage, unknown, failures = _usage(record.get('usage'))
                active.update(reported_usage=usage, usage_unknown_fields=unknown, usage_errors=failures)
                for failure in failures:
                    problem(ordinal, failure)
            terminal_status = active['status']
            active = None
        elif kind in ('item.started', 'item.updated', 'item.completed'):
            if active is None:
                problem(ordinal, 'item-event-outside-turn')
                continue
            item = record.get('item')
            if not isinstance(item, dict) or not _identity(item.get('id')) or not _identity(item.get('type')):
                problem(ordinal, 'invalid-item-object')
                continue
            item_id, item_kind = item['id'], item['type']
            if item_kind not in KNOWN_ITEMS:
                unknown_items.add(_safe_name(item_kind))
            previous = items.get(item_id)
            if previous is not None and previous['type'] != item_kind:
                problem(ordinal, 'item-type-changed')
                continue
            if kind == 'item.started':
                if previous is not None:
                    problem(ordinal, 'duplicate-item-start')
                    continue
                items[item_id] = {'type': item_kind, 'open': True, 'started': record,
                                  'turn_ordinal': active['ordinal']}
            elif kind == 'item.updated':
                if previous is None or not previous['open']:
                    problem(ordinal, 'item-update-without-open-item')
            else:
                if previous is not None and not previous['open']:
                    problem(ordinal, 'duplicate-item-completion')
                    continue
                if previous is not None and previous['turn_ordinal'] != active['ordinal']:
                    problem(ordinal, 'item-crossed-turn-boundary')
                if previous is None and item_kind in EFFECT_ITEMS:
                    problem(ordinal, 'effect-completion-without-start')
                items[item_id] = {'type': item_kind, 'open': False, 'started': previous['started'] if previous else None,
                                  'turn_ordinal': active['ordinal']}
                if item_kind == 'command_execution':
                    projection = _command_projection(item, previous['started'] if previous else None, record.get('timestamp'))
                    commands.append(projection)
                    if not isinstance(item.get('command'), str) or not isinstance(item.get('aggregated_output'), str):
                        problem(ordinal, 'unsupported-command-fields')
                    if projection['command_sha256'] is None or projection['reported_output_utf8_bytes'] is None:
                        problem(ordinal, 'invalid-command-text')
                    if projection['exit_code'] is None or item.get('status') not in ('completed', 'failed'):
                        problem(ordinal, 'invalid-command-terminal-fields')
                    if previous and previous['started']['item'].get('command') != item.get('command'):
                        problem(ordinal, 'command-changed-between-start-and-completion')
                elif item_kind == 'context_compaction':
                    compactions += 1
                elif item_kind == 'error':
                    errors += 1
        elif kind == 'error':
            errors += 1
            error = record.get('message', record.get('error'))
            raw = json.dumps(error) if isinstance(error, dict) else str(error)
            retry_markers += bool(re.search(r'retry|reconnect|reconnecting', raw, re.IGNORECASE))
            if active is None:
                problem(ordinal, 'error-outside-turn')
        else:
            unknown_events.add(_safe_name(kind))
    if thread_id is None:
        problem(None, 'missing-thread-start')
    if not turns:
        problem(None, 'missing-turn-start')
    if active is not None:
        problem(None, 'missing-turn-terminal')
    if any(item['open'] for item in items.values()):
        problem(None, 'unclosed-items')

    controller = controller if isinstance(controller, dict) else None
    scope = controller.get('usage_scope', 'unknown') if controller else 'unknown'
    if scope not in SCOPES:
        problem(None, 'unsupported-controller-usage-scope')
        scope = 'unknown'
    resumed = bool(controller and controller.get('resumed_thread_id') is not None)
    normalized, deltas, usage_problems = _normalize_turns(
        turns, scope, controller.get('usage_baseline') if controller else None, resumed)
    for code in usage_problems:
        problem(None, code)
    closure_ok, closure_errors = _controller_check(controller, digest, size, thread_id, terminal_status)
    reported = [turn['reported_usage'] for turn in turns if turn['reported_usage'] is not None]
    all_success = bool(turns) and all(turn['status'] == 'completed' for turn in turns)
    core_known = all(normalized[field] is not None for field in CORE_FIELDS)
    comparable = (closure_ok and all_success and core_known and not validation and
                  not errors and not unknown_events and not unknown_items and
                  controller['status'] == 'completed')
    basis = 'exec-emitted-initial-turn-usage' if scope == 'unknown' else 'exec-' + scope + '-usage'
    observed_total = (normalized['input_tokens'] + normalized['output_tokens']
                      if normalized['input_tokens'] is not None and normalized['output_tokens'] is not None else None)
    unknown = [field for field in USAGE_FIELDS if normalized[field] is None]
    unknown.extend(field for turn in turns for field in turn['usage_unknown_fields'] if field.startswith('unsupported:'))
    unknown.extend(('provider_call_ids', 'provider_call_statuses',
                    'full_provider_usage', 'subscription_charge', 'model_context_fullness', 'source_attribution',
                    'skill_activation', 'automatic_skill_injection', 'cache_write_partition_relationship'))
    evidence = {
        'schema': SCHEMA, 'adapter': ADAPTER, 'trace_sha256': digest, 'trace_bytes': size,
        'event_count': len(records), 'thread_id': thread_id,
        'invocation_id': controller.get('invocation_id') if controller else None,
        'run_id': controller.get('run_id') if controller else None,
        'cli_version': controller.get('cli_version') if controller else None,
        'resumed_thread_id': controller.get('resumed_thread_id') if controller else None,
        'status': controller.get('status') if controller else terminal_status or 'unknown',
        'usage_scope': scope, 'usage_basis': basis,
        'usage_baseline': _usage(controller['usage_baseline'])[0] if controller and controller.get('usage_baseline') is not None else None,
        'observed_usage': normalized, 'observed_input_plus_output_tokens': observed_total,
        'usage_semantics': {'cache_write_input_tokens': 'observed-count; partition-relationship-not-established-by-exec-calibration',
                            'total': 'input-plus-output; cached-input-and-reasoning-not-extra-addends'},
        'emitted_usage_known_sum': _known_sum(reported),
        'emitted_usage_known_sum_basis': 'raw-terminal-observation-sum; may-repeat-cumulative-usage',
        'normalized_usage_records': deltas, 'turns': turns,
        'comparable_usage': comparable,
        'comparable_fields': [field for field in USAGE_FIELDS if normalized[field] is not None] if comparable else [],
        'usage_complete': False, 'provider_call_ids_available': False,
        'unknown_fields': sorted(set(unknown)), 'validation_errors': validation,
        'closure_errors': closure_errors, 'controller_closed': closure_ok,
        'unknown_event_types': sorted(unknown_events), 'unknown_item_types': sorted(unknown_items),
        'error_markers': errors, 'retry_message_markers': retry_markers, 'compaction_events': compactions,
        'commands': commands,
        'incomplete_commands': [_command_projection(item['started']['item'], item['started'], None)
                                for item in items.values() if item['open'] and item['type'] == 'command_execution'],
        'command_statistics': _command_statistics(commands),
        'controller_elapsed_ms': controller.get('duration_ms') if controller and _integer(controller.get('duration_ms')) else None,
        'money': {'subscription_charge': None, 'provider_charge': None, 'basis': 'not-reported-by-exec-stream'},
        'skill_evidence': {'actual_activation': None, 'automatic_context_injection': None,
                           'basis': 'observed-file-read-command-lower-bounds; automatic-injection-unobserved'},
        'source_metrics': {'attributed_source_bytes': None, 'model_visible_source_bytes': None,
                           'overlap_bytes': None, 'model_context_fullness': None,
                           'basis': 'shell-markers-and-returned-output; no-source-or-model-context-attribution'},
    }
    evidence['evidence_sha256'] = fingerprint(evidence)
    return evidence


def _command_statistics(commands):
    def count(field):
        return sum(command[field] for command in commands)
    output = [command['reported_output_utf8_bytes'] for command in commands]
    elapsed = [command['elapsed_ms'] for command in commands if command['elapsed_ms'] is not None]
    return {
        'completed_command_items': len(commands),
        'failed_command_items': count('failed_command'),
        'recognized_reposcout_invocations': count('recognized_reposcout_invocations'),
        'reposcout_command_markers': count('reposcout_command_markers'),
        'recognized_reposcout_unavailable_attempts': count('recognized_reposcout_unavailable_attempts'),
        'recognized_reposcout_help_invocations': count('recognized_reposcout_help_invocations'),
        'recognized_skill_read_invocations': count('recognized_skill_read_invocations'),
        'skill_read_command_markers': count('skill_read_command_markers'),
        'recognized_reposcout_reference_read_invocations': count('recognized_reposcout_reference_read_invocations'),
        'commands_with_source_read_markers': count('contains_source_read_command_marker'),
        'unparsed_command_items': sum(not command['command_parse_supported'] for command in commands),
        'reported_command_output_utf8_bytes': sum(value for value in output if value is not None) if output else 0,
        'command_output_bytes_coverage': 'complete-for-completed-command-items' if all(value is not None for value in output) else 'partial',
        'output_utf8_bytes_from_commands_with_source_read_markers': sum(command['reported_output_utf8_bytes'] or 0
            for command in commands if command['contains_source_read_command_marker']),
        'repo_scout_use_basis': 'direct-literal-executable-positions-only; conservative-invocation-lower-bound',
        'command_marker_basis': 'lexical-command-position-markers; conditional-execution-not-proven',
        'source_output_basis': 'entire-command-output-containing-read-marker; not-attributed-source-bytes',
        'timed_command_items': sum(command['elapsed_ms'] is not None for command in commands),
        'command_elapsed_ms_known_sum': sum(elapsed) if elapsed else None,
    }


def extract_execution_evidence(path, *, step_id, expected_sha256=None):
    """Return PRIVATE command receipts for semantic validation of execution claims.

    Receipts preserve the actual complete shell command, so a mention, echo or
    conditional command cannot be mistaken for an independently verified test.
    The output hash covers the full *reported* aggregated output; the bounded
    excerpt does not establish that Codex returned all underlying process bytes.
    Callers must keep these records out of published accounting projections.
    """
    require(type(step_id) is int and 0 <= step_id < MAX_INVOCATIONS, 'invalid execution evidence step')
    records, digest, _, failures = _read_trace(path)
    require(not any(error['code'] == 'trace-limit-exceeded' for error in failures),
            'execution evidence exceeds bounded trace limits')
    if expected_sha256 is not None:
        require(expected_sha256 == digest, 'execution evidence trace hash mismatch')
    else:
        require(not failures, 'partial execution evidence requires the exact captured trace hash')
    starts = {}
    completions = {}
    invalid = set()
    event_ids = set()
    thread_id = None
    turn_ordinal = 0
    active = False
    for _, record in records:
        event_id = record.get('event_id')
        if event_id is not None:
            require(_identity(event_id) and event_id not in event_ids, 'duplicate execution evidence event')
            event_ids.add(event_id)
        kind = record.get('type')
        if kind == 'thread.started':
            require(thread_id is None and _identity(record.get('thread_id')), 'invalid execution evidence thread')
            thread_id = record['thread_id']
        elif kind == 'turn.started':
            require(thread_id is not None and not active, 'invalid execution evidence turn start')
            turn_ordinal += 1
            active = True
        elif kind in ('turn.completed', 'turn.failed', 'turn.aborted'):
            require(active, 'execution evidence terminal without open turn')
            active = False
        elif kind in ('item.started', 'item.completed'):
            item = record.get('item')
            if not isinstance(item, dict) or item.get('type') != 'command_execution':
                continue
            identity = item.get('id')
            require(_identity(identity), 'invalid execution command identity')
            if not active or record.get('thread_id', thread_id) != thread_id:
                invalid.add(identity)
                continue
            if kind == 'item.started':
                if identity in starts or identity in completions:
                    invalid.add(identity)
                else:
                    starts[identity] = (turn_ordinal, item.get('command'))
            elif identity in completions:
                invalid.add(identity)
            else:
                completions[identity] = (turn_ordinal, item)
    receipts = []
    for identity, (ordinal, item) in completions.items():
        command = item.get('command')
        output = item.get('aggregated_output')
        exit_code = item.get('exit_code')
        if (identity in invalid or starts.get(identity) != (ordinal, command) or
                not isinstance(command, str) or not command.strip() or
                not isinstance(output, str) or type(exit_code) is not int or
                not -MAX_INTEGER <= exit_code <= MAX_INTEGER or item.get('status') not in ('completed', 'failed')):
            continue
        try:
            command_bytes, output_bytes = command.encode(), output.encode()
        except UnicodeEncodeError:
            continue
        require(len(command_bytes) <= 64 * 1024, 'execution command exceeds private receipt limit')
        receipts.append({'step_id': step_id, 'command': command, 'exit_code': exit_code,
                         'trace_sha256': digest, 'output_sha256': hashlib.sha256(output_bytes).hexdigest(),
                         'output_excerpt': output_bytes[:4096].decode('utf-8', errors='ignore')})
    require(len(receipts) <= 128, 'too many private execution receipts')
    return receipts


def aggregate_episode(traces):
    """Bundle captured invocations in chronological order without guessing resume scope."""
    traces = list(traces)
    require(0 < len(traces) <= MAX_INVOCATIONS, 'invalid bounded episode inventory')
    seen = set()
    errors = []
    thread_id = traces[0].get('thread_id')
    run_id = traces[0].get('run_id')
    digests = set()
    prefix = []
    prefix_stop_reasons = []
    for index, trace in enumerate(traces):
        require(trace.get('adapter') == ADAPTER and trace.get('schema') == SCHEMA, 'unsupported episode trace')
        require(trace.get('evidence_sha256') == fingerprint({key: value for key, value in trace.items() if key != 'evidence_sha256'}),
                'episode trace projection hash mismatch')
        current_errors = []
        unbound_baseline_fields = []
        invocation = trace.get('invocation_id')
        if not _identity(invocation) or invocation in seen:
            current_errors.append('duplicate-or-missing-invocation-id')
        seen.add(invocation)
        if trace.get('trace_sha256') in digests:
            current_errors.append('duplicate-captured-trace')
        digests.add(trace.get('trace_sha256'))
        if trace.get('thread_id') != thread_id or thread_id is None or trace.get('run_id') != run_id:
            current_errors.append('episode-thread-or-run-mismatch')
        if index == 0 and trace.get('resumed_thread_id') is not None:
            current_errors.append('episode-starts-with-resume')
        if index > 0 and trace.get('resumed_thread_id') != thread_id:
            current_errors.append('missing-or-foreign-resume-link')
        if index > 0 and trace.get('usage_scope') == 'thread-cumulative':
            previous_turns = traces[index - 1].get('turns', [])
            previous = previous_turns[-1]['reported_usage'] if previous_turns else None
            baseline = trace.get('usage_baseline')
            if previous is None or not isinstance(baseline, dict) or any(baseline.get(field) != previous.get(field)
                    for field in USAGE_FIELDS if field in CORE_FIELDS or
                    previous.get(field) is not None and baseline.get(field) is not None):
                current_errors.append('cumulative-resume-baseline-mismatch')
            if previous is not None and isinstance(baseline, dict):
                unbound_baseline_fields = [field for field in USAGE_FIELDS
                                           if previous.get(field) is None and baseline.get(field) is not None]
        if trace.get('usage_scope') != traces[0].get('usage_scope'):
            current_errors.append('episode-usage-scope-changed')
        if trace.get('cli_version') != traces[0].get('cli_version'):
            current_errors.append('episode-cli-version-changed')
        if index > 0 and 'unknown' in (traces[0].get('usage_scope'), trace.get('usage_scope')):
            current_errors.append('episode-resume-usage-scope-unknown')
        errors.extend(current_errors)
        if not prefix_stop_reasons:
            prefix_stop_reasons.extend(current_errors)
            prefix_stop_reasons.extend('cumulative-resume-baseline-unbound-field:' + field
                                       for field in unbound_baseline_fields)
            if trace.get('controller_closed') is not True:
                prefix_stop_reasons.append('invocation-controller-not-closed')
            prefix_stop_reasons.extend('invocation:' + error['code'] for error in trace['validation_errors'])
            if not prefix_stop_reasons:
                prefix.append(trace)
    safe_total = not errors
    values = [trace['observed_usage'] for trace in traces]
    observed = _sum_usage(values) if safe_total else dict.fromkeys(USAGE_FIELDS)
    total = (observed['input_tokens'] + observed['output_tokens']
             if observed['input_tokens'] is not None and observed['output_tokens'] is not None else None)
    comparable = safe_total and all(trace['comparable_usage'] for trace in traces)
    result = {
        'schema': SCHEMA, 'adapter': ADAPTER, 'kind': 'codex-exec-episode',
        'run_id': run_id, 'thread_id': thread_id, 'invocation_count': len(traces),
        'usage_basis': traces[0]['usage_basis'], 'observed_usage': observed,
        'known_usage_prefix': {
            'basis': 'normalized-nonoverlapping-invocations; not-a-complete-episode-total',
            'invocation_count': len(prefix),
            'observed_usage': _known_sum([trace['observed_usage'] for trace in prefix]),
            'field_invocation_indices': {
                field: [index for index, trace in enumerate(prefix) if trace['observed_usage'][field] is not None]
                for field in USAGE_FIELDS
            },
            'stopped_at_invocation': len(prefix) if len(prefix) < len(traces) else None,
            'stop_reasons': sorted(set(prefix_stop_reasons)),
        },
        'observed_input_plus_output_tokens': total,
        'comparable_usage': comparable,
        'comparable_fields': [field for field in USAGE_FIELDS if observed[field] is not None and
                              all(field in trace['comparable_fields'] for trace in traces)] if comparable else [],
        'usage_complete': False, 'provider_call_ids_available': False,
        'unknown_fields': sorted(set(field for trace in traces for field in trace['unknown_fields']) |
                                 {field for field in USAGE_FIELDS if observed[field] is None}),
        'episode_errors': sorted(set(errors)), 'invocations': traces,
        'command_statistics': _command_statistics([command for trace in traces for command in trace['commands']]),
        'money': {'subscription_charge': None, 'provider_charge': None, 'basis': 'not-reported-by-exec-stream'},
    }
    result['evidence_sha256'] = fingerprint(result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('trace')
    parser.add_argument('--controller')
    parser.add_argument('--thread-id')
    args = parser.parse_args()
    try:
        controller = read_json(args.controller) if args.controller else None
        print(json.dumps(parse_exec_trace(args.trace, controller=controller, expected_thread_id=args.thread_id),
                         sort_keys=True, indent=2))
    except (InvalidLedger, OSError, ValueError, TypeError, KeyError) as error:
        parser.exit(2, f'{error}\n')


if __name__ == '__main__':
    main()
