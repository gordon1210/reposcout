"""Measure bounded local query latency separately from model-token and task-quality comparisons."""

import argparse
import hashlib
import json
import shutil
import subprocess
import time
from pathlib import Path

import accounting
import fixtures
import pilot

EDITS = {
    'F1': ('src/inventory.rs', b'value.max(0).min(limit)', b'value.max(0).max(0).min(limit)'),
    'F2': ('src/inventory.rs', b'value.max(0).min(limit)', b'value.max(0).max(0).min(limit)'),
    'F3': ('src/payments.rs', b'attempt.min(8)', b'attempt.min(7)'),
    'F4': ('src/checkout.rs', b'invoice_total(quantity, 250)', b'invoice_total(quantity, 251)'),
    'F5': ('src/billing.rs', b'subtotal + tax_for(subtotal)', b'subtotal.saturating_add(tax_for(subtotal))'),
    'F6': ('src/checkout.rs', b'invoice_total(quantity, 250)', b'invoice_total(quantity, 251)'),
}


def measured_command(command, workspace):
    started = time.monotonic_ns()
    peak_rss_kib = 0
    stopped = None
    with subprocess.Popen(command, cwd=workspace, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as child:
        while True:
            try:
                stdout, stderr = child.communicate(timeout=0.2)
                break
            except subprocess.TimeoutExpired:
                rss = subprocess.run(['ps', '-o', 'rss=', '-p', str(child.pid)], check=False,
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True).stdout.strip()
                if rss.isdigit():
                    peak_rss_kib = max(peak_rss_kib, int(rss))
                if time.monotonic_ns() - started > 180_000_000_000 or peak_rss_kib > 1_048_576:
                    stopped = 'time-or-memory-limit'
                    child.kill()
                    stdout, stderr = child.communicate()
                    break
        result = {'pid': child.pid, 'exit_code': child.returncode, 'elapsed_ns': time.monotonic_ns() - started,
                  'sampled_peak_rss_kib': peak_rss_kib if peak_rss_kib else None,
                  'stdout_bytes': len(stdout), 'stderr_bytes': len(stderr), 'stopped': stopped}
    return result, stdout, stderr


def matrix(destination, binary, expected_sha256):
    destination = Path(destination).resolve()
    binary = Path(binary).resolve(strict=True)
    accounting.require(hashlib.sha256(binary.read_bytes()).hexdigest() == expected_sha256, 'latency binary changed')
    destination.mkdir(parents=True, exist_ok=False)
    results = []
    for case in [item for item in fixtures.cases() if item['mode'] == 'isolated']:
        feature = case['feature']
        case_root = destination / feature
        workspace = case_root / 'repository'
        shutil.copytree(fixtures.ROOT / 'repository', workspace)
        inputs = case_root / 'inputs'
        inputs.mkdir()
        shutil.copyfile(fixtures.ROOT / 'diagnostics.jsonl', inputs / 'diagnostics.jsonl')
        before_tree = None
        if feature == 'F2':
            target = workspace / 'src/inventory.rs'
            after = target.read_bytes()
            target.write_bytes((fixtures.ROOT / 'before/src/inventory.rs').read_bytes())
            pilot.git(workspace, 'init', '--quiet', '--template=')
            pilot.git(workspace, 'add', '--', 'src')
            before_tree = pilot.git(workspace, 'write-tree')
            target.write_bytes(after)
        command = pilot.guidance(case, 'reposcout', binary, before_tree)[0]
        for phase in ('cold-requested-fresh-root', 'warm-requested-repeat', 'after-edit'):
            edit = None
            if phase == 'after-edit':
                relative, old, new = EDITS[feature]
                target = workspace / relative
                before = target.read_bytes()
                accounting.require(before.count(old) == 1, 'ambiguous latency fixture edit')
                after = before.replace(old, new, 1)
                target.write_bytes(after)
                edit = {'path': relative, 'before_sha256': fixtures.digest(before), 'after_sha256': fixtures.digest(after)}
            accounting.require(fixtures.digest(binary.read_bytes()) == expected_sha256, 'latency binary changed')
            measurement, stdout, stderr = measured_command(command, workspace)
            stdout_path = case_root / (phase + '.stdout')
            stderr_path = case_root / (phase + '.stderr')
            with stdout_path.open('xb') as stream:
                stream.write(stdout)
            with stderr_path.open('xb') as stream:
                stream.write(stderr)
            entry = {'feature': feature, 'phase': phase, 'command': command, 'workspace': str(workspace),
                     'binary_sha256': expected_sha256, 'edit': edit, 'cache_hits': None,
                     'cache_state_coverage': 'fresh-root-or-repeat-request; cache-hit-status-not-exposed',
                     'stdout_sha256': fixtures.digest(stdout), 'stderr_sha256': fixtures.digest(stderr),
                     'stdout_path': str(stdout_path), 'stderr_path': str(stderr_path), **measurement}
            pilot.write_new(case_root / (phase + '.json'), entry)
            results.append(entry)
            if measurement['stopped'] or measurement['exit_code'] != 0:
                pilot.write_new(destination / 'incomplete.json', {'schema': 1, 'results': results, 'complete': False})
                raise accounting.InvalidLedger('latency query failed; retained measured results')
    result = {'schema': 1, 'kind': 'query-latency-matrix', 'complete': True,
              'model_tokens_measured': False, 'results': results}
    pilot.write_new(destination / 'matrix.json', result)
    return {'matrix': str(destination / 'matrix.json'), 'measurements': len(results), 'model_tokens_measured': False}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('destination')
    parser.add_argument('--binary', required=True)
    parser.add_argument('--expected-sha256', required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(matrix(args.destination, args.binary, args.expected_sha256), sort_keys=True))
    except (accounting.InvalidLedger, OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(2, f'{error}\n')


if __name__ == '__main__':
    main()
