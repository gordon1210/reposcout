"""Export sanitized measured results with their accounting basis, quality evidence and comparison limits."""

import argparse
import copy
import json
from pathlib import Path

from accounting import InvalidLedger, fingerprint, read_json, require
from native_codex import compare_windows
from pilot import write_new


def safe_projection(value):
    if isinstance(value, dict):
        for key, item in value.items():
            require(key not in {'native_log', 'prompt', 'message', 'stdout', 'stderr', 'content', 'input',
                                'workspace', 'prompt_path', 'answer_path'}, 'raw evidence field in export')
            safe_projection(item)
    elif isinstance(value, list):
        for item in value:
            safe_projection(item)
    elif isinstance(value, str):
        require('/Users/' not in value and '/private/tmp/' not in value and '/tmp/' not in value,
                'local absolute path in export')


def project_trial(root, exposure=None, prompt_audit=None):
    root = Path(root)
    trial = read_json(root / 'trial.json')
    final = read_json(root / 'final.json')
    closed = copy.deepcopy(read_json(root / 'closed-window.json'))
    native = read_json(root / 'native-evidence.json')
    lifecycle = read_json(root / 'lifecycle-evidence.json')
    require(closed['run_id'] == trial['run_id'] == final['run_id'], 'run identity mismatch')
    require(native['session_id'] == final['native_session_id'], 'session identity mismatch')
    closed['attestation']['evidence_reference'] = 'lifecycle-evidence.json'
    selected = {key: trial.get(key) for key in (
        'schema', 'run_id', 'task_id', 'variant', 'feature', 'mode', 'evidence_kind', 'profile', 'fork_turns',
        'children_allowed', 'binary_sha256', 'fixture_sha256', 'fixture_files', 'task_sha256', 'oracle_sha256',
        'prompt_sha256', 'start_context_sha256', 'experiment_kind', 'routing_policy_sha256',
        'routing_policy_version', 'quality_scope')}
    selected['experiment_kind'] = selected['experiment_kind'] or 'original'
    selected.update({'accounting': closed, 'native_evidence': native, 'lifecycle_evidence': lifecycle,
                     'source_exposure': exposure, 'prompt_delivery_audit': prompt_audit,
                     'answer_sha256': fingerprint(read_json(root / 'answer.json')),
                     'artifact_canonical_json_sha256': {name: fingerprint(read_json(root / name)) for name in
                         ('trial.json', 'final.json', 'usage-audit.json', 'native-evidence.json',
                          'quality.json', 'lifecycle-evidence.json', 'attestation.json', 'closed-window.json')}})
    safe_projection(selected)
    return selected


def project_latency(matrix):
    result = {key: value for key, value in matrix.items() if key != 'results'}
    result['results'] = []
    for raw in matrix['results']:
        item = {key: value for key, value in raw.items() if key not in
                ('command', 'workspace', 'stdout_path', 'stderr_path', 'pid')}
        item['command'] = ['reposcout', *raw['command'][1:]]
        result['results'].append(item)
    safe_projection(result)
    return result


def export(campaign, destination, release, exposure_path=None, prompt_path=None):
    campaign, destination = Path(campaign), Path(destination)
    require(not destination.exists(), 'export destination already exists')
    exposure = read_json(exposure_path) if exposure_path else {'runs': []}
    prompts = read_json(prompt_path) if prompt_path else {'runs': []}
    by_exposure = {item['run_id']: item for item in exposure['runs']}
    by_prompt = {item['run_id']: item for item in prompts['runs']}
    finals = sorted(campaign.rglob('final.json'))
    require(0 < len(finals) <= 100, 'invalid bounded completed-run inventory')
    runs = [project_trial(path.parent, by_exposure.get(read_json(path)['run_id']),
                          by_prompt.get(read_json(path)['run_id'])) for path in finals]
    require(len({run['run_id'] for run in runs}) == len(runs), 'duplicate exported run')
    pairs = {}
    for run in runs:
        key = (run['experiment_kind'], run['task_id'])
        require(run['variant'] not in pairs.setdefault(key, {}), 'duplicate comparison arm')
        pairs[key][run['variant']] = run
    comparisons = []
    for (kind, task), pair in sorted(pairs.items()):
        require(set(pair) == {'baseline', 'reposcout'}, 'incomplete measured pair')
        comparison = compare_windows(pair['baseline']['accounting'], pair['reposcout']['accounting'])
        comparison = {key: value for key, value in comparison.items() if key not in ('baseline', 'reposcout')}
        comparisons.append({'experiment_kind': kind, 'task_id': task, 'comparison': comparison,
                            'baseline_run': pair['baseline']['run_id'], 'reposcout_run': pair['reposcout']['run_id'],
                            'replicates_per_arm': 1})
    release_data = read_json(release)
    release_projection = {key: value for key, value in release_data.items() if key != 'binary'}
    require(all(run['binary_sha256'] == release_projection['sha256'] for run in runs), 'binary pin drift')
    policies = {}
    for path in sorted(campaign.glob('*/routing-policy.json')):
        value = read_json(path)
        policies[str(path.relative_to(campaign))] = {'canonical_json_sha256': fingerprint(value), 'policy': value}
    superseded = []
    for path in sorted((campaign / 'superseded').rglob('trial.json')):
        value = read_json(path)
        require(not (path.parent / 'final.json').exists(), 'superseded case unexpectedly executed')
        superseded.append({key: value.get(key) for key in
                           ('run_id', 'task_id', 'prompt_sha256', 'fixture_sha256', 'binary_sha256')})
    output = {'schema': 1, 'kind': 'native-agent-evaluation', 'run_count': len(runs),
              'paired_case_count': len(comparisons), 'release': release_projection,
              'comparisons': comparisons, 'runs': runs,
              'captured_policy_snapshots': policies, 'superseded_unrun_cases': superseded,
              'latency': project_latency(read_json(campaign / 'latency-original/matrix.json')),
              'statistical_inference': False, 'synthetic_fixture': True,
              'measurement_scope': 'retrieval-and-behavior-answers; no repair/regression campaign',
              'source_exposure_complete_inventory': set(by_exposure) == {run['run_id'] for run in runs},
              'prompt_audit_complete_inventory': set(by_prompt) == {run['run_id'] for run in runs}}
    safe_projection(output)
    destination.mkdir(parents=True)
    write_new(destination / 'results.json', output)
    write_new(destination / 'integrity.json', {'schema': 1, 'results_canonical_json_sha256': fingerprint(output)})
    return {'run_count': len(runs), 'paired_case_count': len(comparisons),
            'results': str(destination / 'results.json')}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('campaign')
    parser.add_argument('destination')
    parser.add_argument('--release', required=True)
    parser.add_argument('--source-exposure')
    parser.add_argument('--prompt-audit')
    args = parser.parse_args()
    try:
        print(json.dumps(export(args.campaign, args.destination, args.release,
                                args.source_exposure, args.prompt_audit), sort_keys=True))
    except (InvalidLedger, OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(2, f'{error}\n')


if __name__ == '__main__':
    main()
