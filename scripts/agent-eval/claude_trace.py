"""Bounded Claude Code ``stream-json`` observations in the Codex evidence projection.

The final ``result`` event carries the invocation usage total. Assistant events
repeat a message's start-of-stream usage once per content block, so they are
deduplicated by message identity and used only as a cross-check. Values the
stream does not report, such as reasoning tokens, stay unknown.
"""

import argparse
import hashlib
import json
import re

from accounting import InvalidLedger, fingerprint, read_json, require
from codex_trace import (
    MAX_INTEGER, MAX_INVOCATIONS, USAGE_FIELDS, CORE_FIELDS, _command_projection, _command_statistics,
    _identity, _integer, _known_sum, _read_trace, _safe_name, _sum_usage, _timestamp,
)

ADAPTER = 'claude-stream-json-v1'
SCHEMA = 1
SHELL_TOOLS = {'Bash'}
FILE_TOOLS = {'Read', 'Grep', 'Glob', 'Skill'}
OUTPUT_TOOL = 'StructuredOutput'
SOURCE_TOOLS = {'Read', 'Grep'}
KNOWN_EVENTS = {'system', 'assistant', 'user', 'result', 'rate_limit_event'}
KNOWN_SYSTEM = {'init', 'api_retry', 'compact_boundary', 'status', 'thinking_tokens', 'permission_denied'}
CLAUDE_USAGE = ('input_tokens', 'cache_creation_input_tokens', 'cache_read_input_tokens', 'output_tokens')
MODEL_USAGE = {'input_tokens': 'inputTokens', 'cache_creation_input_tokens': 'cacheCreationInputTokens',
               'cache_read_input_tokens': 'cacheReadInputTokens', 'output_tokens': 'outputTokens'}
ANSWER_FAILURES = ('answer-schema-invalid', 'missing-structured-output', 'claude-result-error',
                   'invalid-answer-artifact', 'runtime-isolation-violation', 'credential-exposure')
EXIT_CODE = re.compile(r'Exit code (-?\d{1,20})(?:\n|$)')
CAP_SUBTYPES = {'error_max_turns': 'max-turns', 'error_max_budget_usd': 'max-budget'}
SKILL_FILE = re.compile(r'(?:^|/)skills/reposcout/SKILL\.md$')
REPOSCOUT_TOKEN = re.compile(r'(?<![\w-])reposcout(?![\w-])')
_WRAPPERS = r'(?:(?:[A-Za-z_]\w*=\S*|env|time|exec|command|nice|sudo)\s+|timeout\s+\S+\s+)*'
REPOSCOUT_INVOCATION = re.compile(r'(?:^|[;&|(`{\n]|\$\()\s*' + _WRAPPERS + r'(?:\S*/)?reposcout(?![\w-])')
_VCS_OPTIONS = r'(?:-[Cc]\s+\S+\s+|--[\w-]+(?:=\S+)?\s+|-\w\s+)*'
DIFF_COMMAND = re.compile(r'(?<![\w-])git\s+' + _VCS_OPTIONS + r'(?:diff|show)(?![\w-])')
LOG_PATCH_COMMAND = re.compile(r'(?<![\w-])git\s+' + _VCS_OPTIONS
                               + r'log(?![\w-])[^;&|\n]*?\s(?:-p|--patch|-[A-Za-z]*p[A-Za-z]*)(?=\s|$)')
REVIEW_CONTEXT = re.compile(r'(?<![\w-])review-context(?![\w-])')
BRIEF_FLAG = re.compile(r'(?<![\w-])--brief(?![\w-])')


def _claude_usage(value):
    """Raw Claude counters, each either a bounded integer or unknown."""
    if not isinstance(value, dict):
        return dict.fromkeys(CLAUDE_USAGE), ['missing-or-unsupported-usage-object']
    raw = {}
    errors = []
    for field in CLAUDE_USAGE:
        item = value.get(field)
        if item is None:
            raw[field] = None
        elif _integer(item):
            raw[field] = item
        else:
            raw[field] = None
            errors.append('invalid-usage-number:' + field)
    return raw, errors


def map_usage(raw):
    """Project Claude counters onto the shared fields; input includes both cache partitions."""
    parts = (raw['input_tokens'], raw['cache_creation_input_tokens'], raw['cache_read_input_tokens'])
    return {
        'input_tokens': sum(parts) if all(part is not None for part in parts) else None,
        'cached_input_tokens': raw['cache_read_input_tokens'],
        'cache_write_input_tokens': raw['cache_creation_input_tokens'],
        'output_tokens': raw['output_tokens'],
        'reasoning_output_tokens': None,
        'total_tokens': None,
    }


def _text(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = [block.get('text') for block in content if isinstance(block, dict) and block.get('type') == 'text']
        if all(isinstance(part, str) for part in parts):
            return '\n'.join(parts)
    return None


def _bash_exit_code(exchange):
    """Exit status as the tool result reports it; a denied or detached command has none."""
    if exchange['result'] is None or exchange['permission_decision'] not in (None, 'accept'):
        return None
    if exchange['input'].get('run_in_background') is True:
        return None
    details = exchange['tool_use_result']
    if isinstance(details, dict) and details.get('interrupted') is True:
        return None
    text = exchange['output']
    if exchange['is_error'] is not True:
        return 0 if isinstance(details, dict) else None
    match = EXIT_CODE.match(text) if isinstance(text, str) else None
    if match is None:
        return None
    code = int(match.group(1))
    return code if -MAX_INTEGER <= code <= MAX_INTEGER else None


def _file_tool_projection(exchange):
    name, arguments, text = exchange['name'], exchange['input'], exchange['output']
    succeeded = exchange['result'] is not None and exchange['is_error'] is not True
    path = arguments.get('file_path') if name == 'Read' else arguments.get('path')
    path = path if isinstance(path, str) else None
    skill = (name == 'Read' and path is not None and path.endswith('/skills/reposcout/SKILL.md')
             or name == 'Skill' and arguments.get('skill') == 'reposcout')
    reference = name == 'Read' and path is not None and '/skills/reposcout/references/' in path
    try:
        output_bytes = len(text.encode()) if isinstance(text, str) else None
    except UnicodeEncodeError:
        output_bytes = None
    began, ended = _timestamp(exchange['started_at']), _timestamp(exchange['ended_at'])
    elapsed = round((ended - began).total_seconds() * 1000, 3) if began and ended and ended >= began else None
    selection = {key: arguments.get(key) for key in ('offset', 'limit', 'pattern', 'glob', 'type', 'output_mode')
                 if key in arguments}
    return {
        'item_id': exchange['id'], 'tool': name, 'kind': 'file-tool',
        'status': 'completed' if succeeded else 'failed', 'exit_code': None,
        'command_sha256': fingerprint({'tool': name, 'input': arguments}),
        'command_parse_supported': True,
        'path': path, 'range': selection or None, 'skill': arguments.get('skill') if name == 'Skill' else None,
        'permission_decision': exchange['permission_decision'],
        'recognized_reposcout_invocations': 0, 'reposcout_command_markers': 0,
        'recognized_reposcout_unavailable_attempts': 0, 'recognized_reposcout_help_invocations': 0,
        'recognized_skill_read_invocations': int(skill and succeeded),
        'skill_read_command_markers': int(skill),
        'recognized_reposcout_reference_read_invocations': int(reference and succeeded),
        'contains_source_read_command_marker': name in SOURCE_TOOLS,
        'reported_output_utf8_bytes': output_bytes,
        'elapsed_ms': elapsed, 'timing_basis': 'event-timestamp-interval' if elapsed is not None else None,
        'failed_command': not succeeded,
    }


def _shell_projection(exchange):
    exit_code = _bash_exit_code(exchange)
    item = {'id': exchange['id'], 'command': exchange['input'].get('command'),
            'aggregated_output': exchange['output'], 'exit_code': exit_code,
            'status': 'completed' if exit_code == 0 else 'failed'}
    projection = _command_projection(item, {'timestamp': exchange['started_at']}, exchange['ended_at'])
    projection.update(tool='Bash', kind='shell', permission_decision=exchange['permission_decision'],
                      sandbox_override_requested=exchange['input'].get('dangerouslyDisableSandbox') is True,
                      background_requested=exchange['input'].get('run_in_background') is True)
    return projection


def scan_stream(records):
    """Pair tool uses with results and collect session-level facts without interpretation."""
    state = {
        'init': None, 'result': None, 'session_id': None, 'messages': {}, 'message_order': [],
        'exchanges': {}, 'exchange_order': [], 'problems': [], 'unknown_events': set(),
        'subagent_events': 0, 'retry_events': 0, 'compactions': 0, 'models': set(), 'user_texts': 0,
        'thinking_estimates': 0, 'permission_denials': 0,
    }

    def problem(ordinal, code):
        state['problems'].append({'record': ordinal, 'code': code})

    for ordinal, record in records:
        kind = record.get('type')
        session = record.get('session_id')
        if state['result'] is not None:
            problem(ordinal, 'event-after-final-result')
            continue
        if kind not in KNOWN_EVENTS:
            state['unknown_events'].add(_safe_name(kind))
            continue
        if kind == 'system' and record.get('subtype') == 'init':
            if state['init'] is not None or state['messages']:
                problem(ordinal, 'duplicate-or-misordered-session-init')
                continue
            if not _identity(session):
                problem(ordinal, 'invalid-session-id')
                continue
            state['init'], state['session_id'] = record, session
            continue
        if state['init'] is None:
            problem(ordinal, 'event-before-session-init')
            continue
        if session is not None and session != state['session_id']:
            problem(ordinal, 'foreign-session-event')
            continue
        if record.get('parent_tool_use_id') is not None:
            state['subagent_events'] += 1
            problem(ordinal, 'subagent-event')
            continue
        if kind == 'system':
            subtype = record.get('subtype')
            if subtype not in KNOWN_SYSTEM:
                state['unknown_events'].add('system:' + _safe_name(subtype))
            state['retry_events'] += subtype == 'api_retry'
            state['compactions'] += subtype == 'compact_boundary'
            # Thinking estimates are progress hints; provider output_tokens already include thinking.
            state['thinking_estimates'] += subtype == 'thinking_tokens'
            state['permission_denials'] += subtype == 'permission_denied'
        elif kind == 'assistant':
            message = record.get('message')
            if not isinstance(message, dict) or not _identity(message.get('id')):
                problem(ordinal, 'invalid-assistant-message')
                continue
            identity = message['id']
            usage, errors = _claude_usage(message.get('usage'))
            for error in errors:
                problem(ordinal, 'assistant:' + error)
            previous = state['messages'].get(identity)
            if previous is None:
                state['messages'][identity] = usage
                state['message_order'].append(identity)
            elif previous != usage:
                problem(ordinal, 'message-usage-changed')
            if isinstance(message.get('model'), str):
                state['models'].add(_safe_name(message['model']))
            for block in message.get('content') if isinstance(message.get('content'), list) else []:
                if not isinstance(block, dict) or block.get('type') != 'tool_use':
                    continue
                tool_id = block.get('id')
                if not _identity(tool_id) or tool_id in state['exchanges']:
                    problem(ordinal, 'duplicate-or-invalid-tool-use-id')
                    continue
                arguments = block.get('input') if isinstance(block.get('input'), dict) else {}
                state['exchanges'][tool_id] = {
                    'id': tool_id, 'name': block.get('name') if isinstance(block.get('name'), str) else None,
                    'input': arguments, 'message_id': identity, 'started_at': record.get('timestamp'),
                    'ended_at': None, 'result': None, 'output': None, 'is_error': None,
                    'tool_use_result': None, 'permission_decision': None,
                }
                state['exchange_order'].append(tool_id)
        elif kind == 'user':
            message = record.get('message')
            content = message.get('content') if isinstance(message, dict) else None
            blocks = [block for block in content if isinstance(block, dict) and block.get('type') == 'tool_result'] \
                if isinstance(content, list) else []
            if not blocks:
                state['user_texts'] += 1
                continue
            decisions = {}
            for meta in record.get('tool_result_meta') if isinstance(record.get('tool_result_meta'), list) else []:
                if isinstance(meta, dict) and isinstance(meta.get('permission_decision'), dict):
                    decisions[meta.get('id')] = meta['permission_decision'].get('decision')
            for block in blocks:
                exchange = state['exchanges'].get(block.get('tool_use_id'))
                if exchange is None:
                    problem(ordinal, 'tool-result-without-tool-use')
                    continue
                if exchange['result'] is not None:
                    problem(ordinal, 'duplicate-tool-result')
                    continue
                exchange.update(result=block, output=_text(block.get('content')),
                                is_error=block.get('is_error') is True, ended_at=record.get('timestamp'),
                                tool_use_result=record.get('tool_use_result') if len(blocks) == 1 else None,
                                permission_decision=decisions.get(exchange['id']))
                if exchange['output'] is None:
                    problem(ordinal, 'unsupported-tool-result-content')
        elif kind == 'result':
            state['result'] = record
    if state['init'] is None:
        problem(None, 'missing-session-init')
    if state['result'] is None:
        problem(None, 'missing-final-result')
    if any(state['exchanges'][identity]['result'] is None for identity in state['exchange_order']):
        problem(None, 'unclosed-tool-uses')
    return state


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
    if controller.get('resumed_thread_id') is not None:
        errors.append('controller-resume-unsupported')
    status = controller.get('status')
    process = controller.get('process_status', status)
    if status not in ('completed', 'failed', 'aborted', 'not-started'):
        errors.append('invalid-controller-status')
    if status == 'completed' and (process != 'completed' or controller.get('returncode') != 0 or
                                  type(controller.get('returncode')) is not int or terminal_status != 'completed'):
        errors.append('controller-terminal-status-mismatch')
    if process == 'completed' and terminal_status not in ('completed', 'failed'):
        errors.append('controller-terminal-status-mismatch')
    reason = controller.get('termination_reason')
    closed = (controller.get('stream_complete') is True and controller.get('process_tree_drained') is True and
              controller.get('stdout_truncated') is False and
              (reason is None or status == 'failed' and process == 'completed' and reason in ANSWER_FAILURES))
    if not closed:
        errors.append('controller-stream-not-closed')
    return not errors, errors


def parse_exec_trace(path, *, controller=None, expected_thread_id=None):
    """Return sanitized observations, including malformed, failed and partial captures."""
    records, digest, size, validation = _read_trace(path)
    state = scan_stream(records)
    validation.extend(state['problems'])

    def problem(code):
        validation.append({'record': None, 'code': code})

    thread_id = state['session_id']
    if expected_thread_id is not None and thread_id is not None and thread_id != expected_thread_id:
        problem('unexpected-session-id')
    controller = controller if isinstance(controller, dict) else None
    result = state['result'] or {}
    terminal_status = None
    if state['result'] is not None:
        terminal_status = 'completed' if result.get('subtype') == 'success' and result.get('is_error') is False else 'failed'
    raw, usage_errors = _claude_usage(result.get('usage')) if state['result'] is not None else (
        dict.fromkeys(CLAUDE_USAGE), ['no-reported-usage'])
    for error in usage_errors:
        problem('result:' + error)
    observed = map_usage(raw)
    messages = [state['messages'][identity] for identity in state['message_order']]
    cross_check = {}
    for field in ('input_tokens', 'cache_creation_input_tokens', 'cache_read_input_tokens'):
        values = [message[field] for message in messages]
        known = sum(values) if values and all(value is not None for value in values) else None
        cross_check[field] = {'assistant_message_sum': known, 'result': raw[field],
                              'matched': known is not None and known == raw[field]}
        if raw[field] is not None and known != raw[field]:
            problem('assistant-usage-disagrees-with-result:' + field)
    model_usage = result.get('modelUsage')
    models_used = sorted(_safe_name(name) for name in model_usage) if isinstance(model_usage, dict) else []
    if state['result'] is not None:
        if not isinstance(model_usage, dict) or not model_usage:
            problem('missing-model-usage')
        else:
            for field, key in MODEL_USAGE.items():
                values = [entry.get(key) if isinstance(entry, dict) else None for entry in model_usage.values()]
                if raw[field] is not None and (not all(_integer(value) for value in values) or sum(values) != raw[field]):
                    problem('model-usage-disagrees-with-result:' + field)
        expected_model = controller.get('model') if controller else None
        if expected_model is not None and (set(models_used) != {expected_model} or
                                           state['models'] - {expected_model}):
            problem('unexpected-model-usage')
        stats = result.get('subagent_stats')
        if not isinstance(stats, dict) or stats.get('spawned') != 0:
            problem('subagent-spawned-or-unreported')
    commands = []
    incomplete = []
    tool_counts = {}
    output_attempts = 0
    output_rejections = 0
    forbidden = 0
    for identity in state['exchange_order']:
        exchange = state['exchanges'][identity]
        name = exchange['name']
        tool_counts[_safe_name(name)] = tool_counts.get(_safe_name(name), 0) + 1
        if name == OUTPUT_TOOL:
            output_attempts += 1
            output_rejections += exchange['is_error'] is True
            continue
        if name not in SHELL_TOOLS | FILE_TOOLS:
            forbidden += 1
            problem('unavailable-tool-use:' + _safe_name(name))
            continue
        projection = _shell_projection(exchange) if name in SHELL_TOOLS else _file_tool_projection(exchange)
        (commands if exchange['result'] is not None else incomplete).append(projection)
        if name in SHELL_TOOLS and exchange['result'] is not None:
            if not isinstance(exchange['input'].get('command'), str) or projection['command_sha256'] is None:
                problem('invalid-command-text')
    statistics = _command_statistics(commands)
    statistics.update(tool_call_counts=dict(sorted(tool_counts.items())),
                      shell_command_items=sum(command['kind'] == 'shell' for command in commands),
                      file_tool_items=sum(command['kind'] == 'file-tool' for command in commands),
                      permission_denied_items=sum(command['permission_decision'] not in (None, 'accept')
                                                  for command in commands),
                      repo_scout_use_basis='bash-direct-literal-executable-positions-only; conservative-invocation-lower-bound')
    closure_ok, closure_errors = _controller_check(controller, digest, size, thread_id, terminal_status)
    core_known = all(observed[field] is not None for field in CORE_FIELDS)
    comparable = (closure_ok and terminal_status == 'completed' and core_known and not validation
                  and not state['unknown_events'] and controller['status'] == 'completed')
    observed_total = (observed['input_tokens'] + observed['output_tokens']
                      if observed['input_tokens'] is not None and observed['output_tokens'] is not None else None)
    unknown = [field for field in USAGE_FIELDS if observed[field] is None]
    unknown.extend(('provider_call_ids', 'provider_call_statuses', 'full_provider_usage', 'subscription_charge',
                    'model_context_fullness', 'source_attribution', 'automatic_skill_injection'))
    init = state['init'] or {}
    plugins = init.get('plugins') if isinstance(init.get('plugins'), list) else []
    reported_usage = observed if state['result'] is not None else None
    evidence = {
        'schema': SCHEMA, 'adapter': ADAPTER, 'trace_sha256': digest, 'trace_bytes': size,
        'event_count': len(records), 'thread_id': thread_id,
        'invocation_id': controller.get('invocation_id') if controller else None,
        'run_id': controller.get('run_id') if controller else None,
        'cli_version': controller.get('cli_version') if controller else None,
        'resumed_thread_id': controller.get('resumed_thread_id') if controller else None,
        'status': controller.get('status') if controller else terminal_status or 'unknown',
        'usage_scope': 'invocation', 'usage_basis': 'claude-result-invocation-usage',
        'usage_baseline': None,
        'observed_usage': observed, 'observed_input_plus_output_tokens': observed_total,
        'raw_result_usage': raw,
        'usage_semantics': {
            'input_tokens': 'input-plus-cache-creation-plus-cache-read',
            'cached_input_tokens': 'cache-read-input-tokens', 'cache_write_input_tokens': 'cache-creation-input-tokens',
            'total': 'input-plus-output; cached-input-and-reasoning-not-extra-addends',
        },
        'usage_cross_check': cross_check,
        'emitted_usage_known_sum': _known_sum([reported_usage]) if reported_usage else dict.fromkeys(USAGE_FIELDS),
        'emitted_usage_known_sum_basis': 'final-result-usage; assistant-message-usage-deduplicated-by-message-id',
        'normalized_usage_records': [observed] if reported_usage else [],
        'turns': [{'ordinal': 1, 'turn_id': None, 'status': terminal_status or 'open', 'reported_usage': reported_usage,
                   'usage_unknown_fields': [field for field in USAGE_FIELDS if observed[field] is None],
                   'usage_errors': usage_errors, 'start_record': None, 'end_record': None}],
        'provider_message_count': len(messages), 'models_used': models_used,
        'comparable_usage': comparable,
        'comparable_fields': [field for field in USAGE_FIELDS if observed[field] is not None] if comparable else [],
        'usage_complete': False, 'provider_call_ids_available': False,
        'unknown_fields': sorted(set(unknown)), 'validation_errors': validation,
        'closure_errors': closure_errors, 'controller_closed': closure_ok,
        'unknown_event_types': sorted(state['unknown_events']), 'unknown_item_types': [],
        'error_markers': int(terminal_status == 'failed'), 'retry_message_markers': state['retry_events'],
        'compaction_events': state['compactions'],
        'thinking_estimate_events': state['thinking_estimates'],
        'permission_denied_events': state['permission_denials'],
        'session': {
            'tools': init.get('tools') if isinstance(init.get('tools'), list) else None,
            'mcp_server_count': len(init['mcp_servers']) if isinstance(init.get('mcp_servers'), list) else None,
            'skills': init.get('skills') if isinstance(init.get('skills'), list) else None,
            'plugins': sorted(item.get('name') for item in plugins if isinstance(item, dict)
                              and isinstance(item.get('name'), str)),
            'permission_mode': init.get('permissionMode'), 'model': init.get('model'),
            'claude_code_version': init.get('claude_code_version'), 'api_key_source': init.get('apiKeySource'),
        },
        'result': {
            'subtype': result.get('subtype'), 'is_error': result.get('is_error'),
            'terminal_reason': result.get('terminal_reason'), 'num_turns': result.get('num_turns'),
            'structured_output_present': isinstance(result.get('structured_output'), dict),
            'permission_denials': len(result['permission_denials'])
            if isinstance(result.get('permission_denials'), list) else None,
            'subagents_spawned': (result.get('subagent_stats') or {}).get('spawned')
            if isinstance(result.get('subagent_stats'), dict) else None,
        },
        'structured_output_attempts': output_attempts, 'structured_output_rejections': output_rejections,
        'unavailable_tool_uses': forbidden, 'subagent_events': state['subagent_events'],
        'commands': commands, 'incomplete_commands': incomplete,
        'command_statistics': statistics,
        'controller_elapsed_ms': controller.get('duration_ms') if controller and _integer(controller.get('duration_ms')) else None,
        'money': {'subscription_charge': None, 'provider_charge': None,
                  'list_price_estimate_usd': result.get('total_cost_usd')
                  if isinstance(result.get('total_cost_usd'), (int, float)) else None,
                  'basis': 'claude-list-price-estimate; not-a-subscription-or-provider-charge'},
        'skill_evidence': {'actual_activation': None, 'automatic_context_injection': None,
                           'basis': 'observed-skill-tool-and-file-read-lower-bounds; automatic-injection-unobserved'},
        'source_metrics': {'attributed_source_bytes': None, 'model_visible_source_bytes': None,
                           'overlap_bytes': None, 'model_context_fullness': None,
                           'basis': 'tool-markers-and-returned-output; no-source-or-model-context-attribution'},
    }
    evidence['evidence_sha256'] = fingerprint(evidence)
    return evidence


def token_breakdown(state):
    """Final-result tokens split into uncached input, cache read, cache write and output.

    A counter the final result does not report stays None; assistant messages carry only
    start-of-stream usage and never stand in for a missing total.
    """
    final = state['result']
    raw = _claude_usage(final.get('usage'))[0] if final is not None else dict.fromkeys(CLAUDE_USAGE)
    known = [value is not None for value in raw.values()]
    source = 'result' if all(known) else 'result-partial' if any(known) else 'unavailable'
    return {'uncached_input_tokens': raw['input_tokens'], 'cache_read_input_tokens': raw['cache_read_input_tokens'],
            'cache_write_input_tokens': raw['cache_creation_input_tokens'], 'output_tokens': raw['output_tokens'],
            'source': source}


def adoption_observations(records):
    """RepoScout adoption read from raw tool_use events in stream order.

    Results, exit codes and permission decisions are ignored, so a call that never
    received a result, was denied or failed still counts as an attempt to use it.
    ``adopted`` holds for the Skill tool with ``skill=reposcout``, a Read of the skill's
    SKILL.md, or a Bash command naming a ``reposcout`` token anywhere; ``invoked`` is the
    stricter reading where the token sits at a command position. ``adopted_before_diff``
    also holds when the run never inspects a diff. A requested cap ends a run with an
    error result that is reported as ``capped``, not as a failure.
    """
    state = scan_stream(records)
    turns = {identity: number for number, identity in enumerate(state['message_order'], 1)}
    events = []
    adoption = None
    diff = None
    brief = False
    invoked = False
    for order, identity in enumerate(state['exchange_order']):
        exchange = state['exchanges'][identity]
        name, arguments = exchange['name'], exchange['input']
        turn = turns.get(exchange['message_id'])
        event = {'order': order, 'turn': turn, 'tool': name}
        kinds = []
        position = None
        if name == 'Skill' and arguments.get('skill') == 'reposcout':
            kinds.append('skill-tool')
            position = 0
        elif name == 'Read' and isinstance(arguments.get('file_path'), str) and SKILL_FILE.search(arguments['file_path']):
            kinds.append('skill-file-read')
            position = 0
        elif name == 'Bash' and isinstance(arguments.get('command'), str):
            command = arguments['command']
            event['command'] = command[:300]
            token = REPOSCOUT_TOKEN.search(command)
            if token:
                kinds.append('bash-token')
                position = token.start()
            if REVIEW_CONTEXT.search(command) and BRIEF_FLAG.search(command):
                brief = True
                kinds.append('review-brief')
            if REPOSCOUT_INVOCATION.search(command):
                kinds.append('bash-invocation')
                invoked = True
            matches = [match.start() for match in (DIFF_COMMAND.search(command), LOG_PATCH_COMMAND.search(command))
                       if match]
            if matches:
                kinds.append('diff-inspection')
                if diff is None:
                    diff = (order, min(matches), turn)
        if position is not None and adoption is None:
            adoption = (order, position, turn, kinds[0])
        event['kinds'] = kinds
        events.append(event)
    final = state['result'] or {}
    cap = CAP_SUBTYPES.get(final.get('subtype'))
    num_turns = final.get('num_turns') if _integer(final.get('num_turns')) else len(state['message_order'])
    return {
        'adopted': adoption is not None,
        'adoption_kind': adoption[3] if adoption else None,
        'first_adoption_turn': adoption[2] if adoption else None,
        'adopted_before_diff': adoption is not None and (diff is None or adoption[:2] < diff[:2]),
        'first_diff_turn': diff[2] if diff else None,
        'invoked': invoked,
        'brief_called': brief,
        'num_turns': num_turns,
        'capped': cap is not None,
        'cap_reason': cap,
        'result_subtype': final.get('subtype'),
        'terminal_reason': final.get('terminal_reason'),
        'tool_use_count': len(events),
        'events': events,
        'tokens': token_breakdown(state),
        'list_price_usd': final.get('total_cost_usd') if isinstance(final.get('total_cost_usd'), (int, float)) else None,
        'trace_problems': sorted({problem['code'] for problem in state['problems']}),
    }


def structured_output(path):
    """The final structured answer, or None when the stream has none."""
    records, _, _, _ = _read_trace(path)
    state = scan_stream(records)
    result = state['result'] or {}
    value = result.get('structured_output')
    return value if isinstance(value, dict) else None


def extract_execution_evidence(path, *, step_id, expected_sha256=None):
    """Return PRIVATE Bash receipts for semantic validation of execution claims.

    Only main-session Bash results with a reported exit status qualify. Callers
    must keep these records out of published accounting projections.
    """
    require(type(step_id) is int and 0 <= step_id < MAX_INVOCATIONS, 'invalid execution evidence step')
    records, digest, _, failures = _read_trace(path)
    require(not any(error['code'] == 'trace-limit-exceeded' for error in failures),
            'execution evidence exceeds bounded trace limits')
    if expected_sha256 is not None:
        require(expected_sha256 == digest, 'execution evidence trace hash mismatch')
    else:
        require(not failures, 'partial execution evidence requires the exact captured trace hash')
    state = scan_stream(records)
    receipts = []
    for identity in state['exchange_order']:
        exchange = state['exchanges'][identity]
        if exchange['name'] not in SHELL_TOOLS:
            continue
        command, output, exit_code = exchange['input'].get('command'), exchange['output'], _bash_exit_code(exchange)
        if not isinstance(command, str) or not command.strip() or not isinstance(output, str) or exit_code is None:
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
    """Bundle one fresh invocation; Claude sessions are never resumed by this harness."""
    traces = list(traces)
    require(0 < len(traces) <= MAX_INVOCATIONS, 'invalid bounded episode inventory')
    errors = []
    for trace in traces:
        require(trace.get('adapter') == ADAPTER and trace.get('schema') == SCHEMA, 'unsupported episode trace')
        require(trace.get('evidence_sha256') == fingerprint({key: value for key, value in trace.items()
                                                             if key != 'evidence_sha256'}),
                'episode trace projection hash mismatch')
    if len(traces) > 1:
        errors.append('claude-resume-unsupported')
    if not _identity(traces[0].get('invocation_id')):
        errors.append('duplicate-or-missing-invocation-id')
    if traces[0].get('resumed_thread_id') is not None:
        errors.append('episode-starts-with-resume')
    first = traces[0]
    prefix_stop = list(errors)
    if first.get('controller_closed') is not True:
        prefix_stop.append('invocation-controller-not-closed')
    prefix_stop.extend('invocation:' + error['code'] for error in first['validation_errors'])
    prefix = [] if prefix_stop else [first]
    observed = _sum_usage([trace['observed_usage'] for trace in traces]) if not errors else dict.fromkeys(USAGE_FIELDS)
    total = (observed['input_tokens'] + observed['output_tokens']
             if observed['input_tokens'] is not None and observed['output_tokens'] is not None else None)
    comparable = not errors and all(trace['comparable_usage'] for trace in traces)
    result = {
        'schema': SCHEMA, 'adapter': ADAPTER, 'kind': 'claude-stream-json-episode',
        'run_id': first.get('run_id'), 'thread_id': first.get('thread_id'), 'invocation_count': len(traces),
        'usage_basis': first['usage_basis'], 'observed_usage': observed,
        'known_usage_prefix': {
            'basis': 'normalized-nonoverlapping-invocations; not-a-complete-episode-total',
            'invocation_count': len(prefix),
            'observed_usage': _known_sum([trace['observed_usage'] for trace in prefix]),
            'field_invocation_indices': {
                field: [index for index, trace in enumerate(prefix) if trace['observed_usage'][field] is not None]
                for field in USAGE_FIELDS
            },
            'stopped_at_invocation': len(prefix) if len(prefix) < len(traces) else None,
            'stop_reasons': sorted(set(prefix_stop)),
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
        'money': {'subscription_charge': None, 'provider_charge': None,
                  'basis': 'claude-list-price-estimate; not-a-subscription-or-provider-charge'},
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
