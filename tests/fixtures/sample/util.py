"""Sample Python module for reposcout fixtures."""
import os
import sys
from collections import defaultdict


def load_config(path):
    # FIXME: handle missing file
    data = {}
    if os.path.exists(path):
        with open(path) as fh:
            for line in fh:
                if "=" in line:
                    key, value = line.split("=", 1)
                    data[key.strip()] = value.strip()
    return data


def group_by(items, key_fn):
    buckets = defaultdict(list)
    for item in items:
        buckets[key_fn(item)].append(item)
    return dict(buckets)


def main(argv):
    if len(argv) < 2:
        print("usage: util.py <path>", file=sys.stderr)
        return 1
    cfg = load_config(argv[1])
    print(cfg)
    return 0
