"""Prepare bounded evaluation trials and record answer evidence without executing models or inferring usage completeness."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

import fixtures
from accounting import InvalidLedger, fingerprint, read_json, require


def write_new(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, sort_keys=True, indent=2)
        stream.write('\n')


def git(workspace, *args):
    environment = {key: value for key, value in os.environ.items() if not key.startswith('GIT_')}
    return subprocess.run(['git', '-C', str(workspace), '-c', 'core.hooksPath=/dev/null',
                           '-c', 'core.attributesFile=/dev/null', '-c', 'core.excludesFile=/dev/null', *args],
                          check=True, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          text=True).stdout.strip()


def guidance(case, variant, binary, before_tree):
    task = case['task']
    feature = case['feature']
    if variant == 'baseline':
        if feature == 'F2':
            return [['git', 'diff', '--no-ext-diff', '--unified=0', before_tree, '--', 'src']]
        if 'selector' in task:
            seed = task['selector']
            if feature == 'F6':
                return [['rg', '-n', '--', seed['symbol'] + r'\(', 'src']]
            return [['rg', '-n', '--', 'pub fn ' + seed['symbol'], seed['path']]]
        if feature == 'F3':
            return [['rg', '-n', '--', 'retry|duplicate_payment', 'src']]
        return [['cat', '../inputs/diagnostics.jsonl']]
    common = ['--format', 'json', '--no-project-config']
    if feature == 'F1':
        seed = task['selector']
        command = ['read', '.', '--symbol', seed['path'], seed['symbol']]
    elif feature == 'F2':
        command = ['changes', '.', '--since', before_tree, '--source']
    elif feature == 'F3':
        command = ['find', task['query'], '.']
    elif feature == 'F4':
        command = ['.', '--task-diagnostics', '../inputs/diagnostics.jsonl',
                   '--task-diagnostics-format', 'rustc-json', '--agent-summary', '--context-budget', '2048']
    elif feature == 'F5':
        seed = task['selector']
        command = ['plan', '.', '--symbol', seed['path'], seed['symbol'], '--source']
    else:
        seed = task['selector']
        command = ['consumers', '.', '--symbol', seed['path'], seed['symbol']]
    return [[str(binary), *command, *common]]


def prepare(destination, binary, templates, task_ids=None, revision=None, policy_path=None, composed=False):
    destination = Path(destination).resolve()
    binary = Path(binary).resolve(strict=True)
    require(binary.is_file(), 'pilot binary must be a regular file')
    fixed = read_json(fixtures.ROOT / 'tasks.json')
    require(fixed['cases'] == fixtures.cases(), 'fixture source differs from fixed task manifest')
    selected = [fixtures.composed_case()] if composed else [case for case in fixed['cases'] if not task_ids or case['task_id'] in task_ids]
    require(selected and (not task_ids or {case['task_id'] for case in selected} == set(task_ids)), 'unknown revision task')
    require(revision is None or (0 < len(revision) <= 32 and all(char.isascii() and (char.isalnum() or char == '-') for char in revision)), 'invalid revision label')
    template = read_json(templates)
    policy = read_json(policy_path) if policy_path else None
    require(not composed or policy is not None, "composed workflow requires explicit routing policy")
    require(isinstance(template.get('template'), str) and isinstance(template.get('variant_guidance'), dict),
            'writer prompt templates missing')
    destination.mkdir(parents=True, exist_ok=True)
    captured = destination / 'snapshot'
    if not captured.exists():
        captured.mkdir()
        shutil.copytree(fixtures.ROOT / 'repository', captured / 'repository')
        shutil.copytree(fixtures.ROOT / 'before', captured / 'before')
        shutil.copyfile(fixtures.ROOT / 'diagnostics.jsonl', captured / 'diagnostics.jsonl')
        shutil.copyfile(fixtures.ROOT / 'tasks.json', captured / 'tasks.json')
    require(read_json(captured / 'tasks.json') == fixed, 'captured task manifest differs')
    captured_hashes = {str(path.relative_to(captured / 'repository')): fixtures.digest(path.read_bytes())
                       for path in sorted((captured / 'repository').rglob('*.rs'))}
    captured_hashes['@before/src/inventory.rs'] = fixtures.digest((captured / 'before/src/inventory.rs').read_bytes())
    captured_hashes['@diagnostics.jsonl'] = fixtures.digest((captured / 'diagnostics.jsonl').read_bytes())
    require(captured_hashes == fixed['cases'][0]['fixture_files'], 'captured fixture source differs')
    trials_root = destination / 'trials'
    trials_root.mkdir(exist_ok=False)
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    trials = []
    for case in selected:
        for variant in ('baseline', 'reposcout'):
            trial_id = case['task_id'].replace(':', '-') + ('-' + revision if revision else '') + '-' + variant
            trial_root = trials_root / trial_id
            workspace = trial_root / 'repository'
            inputs = trial_root / 'inputs'
            shutil.copytree(captured / 'repository', workspace)
            inputs.mkdir()
            shutil.copyfile(captured / 'diagnostics.jsonl', inputs / 'diagnostics.jsonl')
            before_tree = None
            if case['feature'] in ('F2', 'COMPOSED'):
                target = workspace / 'src/inventory.rs'
                after = target.read_bytes()
                target.write_bytes((captured / 'before/src/inventory.rs').read_bytes())
                git(workspace, 'init', '--quiet', '--template=')
                git(workspace, 'add', '--', 'src')
                before_tree = git(workspace, 'write-tree')
                target.write_bytes(after)
                shutil.copyfile(captured / 'before/src/inventory.rs', inputs / 'inventory-before.rs')
            public_task = dict(case['task'])
            if case['feature'] in ('F2', 'COMPOSED'):
                public_task['before'] = '../inputs/inventory-before.rs'
                public_task['before_tree'] = before_tree
            if case['feature'] == 'F4':
                public_task['diagnostics'] = '../inputs/diagnostics.jsonl'
            instructions = {'instruction': policy['composed_instruction']} if composed else template['cases'][case['feature']]
            instruction = instructions.get('end_to_end_instruction', instructions['instruction']) if case['mode'] == 'end-to-end' else instructions['instruction']
            public_task['instruction'] = instruction
            routes = guidance(case, variant, binary, before_tree) if case['mode'] == 'isolated' else []
            if case['mode'] == 'isolated' and policy is None:
                public_task['entry'] = template['cases'][case['feature']]['entry']
                public_task['entry_commands'] = routes
            answer_path = trial_root / 'answer.json'
            variant_key = 'baseline' if variant == 'baseline' else 'reposcout_' + case['mode'].replace('-', '_')
            if policy and variant == 'reposcout':
                variant_key = 'reposcout_end_to_end'
            variant_guidance = template['variant_guidance'][variant_key].replace('{{TOOL_PATH}}', str(binary))
            if policy:
                selected_policy = policy['baselineguidance'] if variant == 'baseline' else policy['policytext']
                variant_guidance += '\n' + policy['rerun_policy_template'].replace('{{ROUTING_GUIDANCE}}', selected_policy)
            if composed and variant == 'reposcout':
                variant_guidance += '\n' + policy['composed_variant_guidance']
            replacements = {'WORKSPACE': str(workspace), 'TASK_JSON': json.dumps(public_task, sort_keys=True),
                            'ANSWER_PATH': str(answer_path), 'RUN_ID': trial_id, 'VARIANT_GUIDANCE': variant_guidance}
            prompt = template['template']
            for key, value in replacements.items():
                prompt = prompt.replace('{{' + key + '}}', value)
            prompt_path = trial_root / 'prompt.txt'
            with prompt_path.open('x') as stream:
                stream.write(prompt + '\n')
            common_context = {'common_template': template['template'], 'task': case['task'],
                              'mode': case['mode'], 'profile': 'astra_specialist', 'instruction': instruction}
            if policy:
                common_context['routing_policy_sha256'] = fingerprint(policy)
            record = {'schema': 1, 'run_id': trial_id, 'task_id': case['task_id'], 'variant': variant,
                      'feature': case['feature'], 'mode': case['mode'], 'evidence_kind': 'observed',
                      'execution': 'not-run', 'profile': 'astra_specialist', 'fork_turns': 'none',
                      'children_allowed': False, 'workspace': str(workspace), 'prompt_path': str(prompt_path),
                      'answer_path': str(answer_path), 'before_tree': before_tree,
                      'binary': str(binary), 'binary_sha256': binary_hash,
                      'task_sha256': case['task_sha256'], 'oracle_sha256': case['oracle_sha256'],
                      'fixture_sha256': case['fixture_sha256'], 'fixture_files': case['fixture_files'],
                      'prompt_sha256': fixtures.digest((prompt + '\n').encode()),
                      'start_context_sha256': fingerprint(common_context),
                      'experiment_kind': 'composed-workflow' if composed else 'routing-remeasure' if policy else 'original',
                      'routing_policy_sha256': fingerprint(policy) if policy else None,
                      'routing_policy_version': policy['version'] if policy else None,
                      'variant_guidance_sha256': fixtures.digest(template['variant_guidance'][variant_key].encode()),
                      'quality_scope': 'retrieval-and-behavior-answer; no-code-change-validation',
                      'agent_id': None, 'native_log': None, 'quality': None}
            write_new(trial_root / 'trial.json', record)
            trials.append(record)
    result = {'schema': 1, 'execution': 'not-run', 'trial_count': len(trials), 'paired_case_count': len(selected),
              'binary_sha256': binary_hash, 'trials': trials}
    write_new(destination / 'campaign.json', result)
    return {'campaign': str(destination / 'campaign.json'), 'trial_count': len(trials), 'execution': 'not-run'}


def verify_trial(trial, answer):
    case = fixtures.composed_case() if trial['task_id'].startswith('COMPOSED:') else next(item for item in fixtures.cases() if item['task_id'] == trial['task_id'])
    require(case['fixture_sha256'] == trial['fixture_sha256'], 'evaluator fixture changed')
    workspace = Path(trial['workspace'])
    actual = {str(path.relative_to(workspace)): fixtures.digest(path.read_bytes())
              for path in sorted(workspace.rglob('*.rs')) if '.git' not in path.relative_to(workspace).parts}
    expected = {key: value for key, value in case['fixture_files'].items() if not key.startswith('@')}
    require(actual == expected, 'trial source snapshot changed')
    require(fixtures.digest(Path(trial['binary']).read_bytes()) == trial['binary_sha256'], 'pilot binary changed')
    return fixtures.verify_answer(case, answer)


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest='command', required=True)
    setup = commands.add_parser('prepare')
    setup.add_argument('destination')
    setup.add_argument('--binary', required=True)
    setup.add_argument('--templates', required=True)
    setup.add_argument('--task', action='append')
    setup.add_argument('--revision')
    setup.add_argument('--policy')
    setup.add_argument('--composed', action='store_true')
    verify = commands.add_parser('verify')
    verify.add_argument('trial')
    verify.add_argument('answer')
    store = commands.add_parser('store-final')
    store.add_argument('trial')
    store.add_argument('answer')
    store.add_argument('--agent-id', required=True)
    store.add_argument('--native-log', required=True)
    store.add_argument('--lifecycle-evidence-sha256', required=True)
    args = parser.parse_args()
    try:
        if args.command == 'prepare':
            result = prepare(args.destination, args.binary, args.templates, args.task, args.revision, args.policy, args.composed)
        else:
            trial = read_json(args.trial)
            result = verify_trial(trial, read_json(args.answer))
            if args.command == 'store-final':
                from accounting import sha256
                sha256(args.lifecycle_evidence_sha256, 'lifecycle evidence')
                stored = {'schema': 1, 'run_id': trial['run_id'], 'agent_id': args.agent_id,
                          'native_log': args.native_log, 'quality': result,
                          'lifecycle_evidence_sha256': args.lifecycle_evidence_sha256,
                          'usage_status': 'not-imported'}
                write_new(Path(args.trial).parent / 'final.json', stored)
        print(json.dumps(result, indent=2, sort_keys=True))
    except (InvalidLedger, OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(2, f'{error}\n')


if __name__ == '__main__':
    main()
