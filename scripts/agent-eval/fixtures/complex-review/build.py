"""Materialize one complex-review case bundle from the deterministic Fieldline generator.

The bundle holds the shared base application, one overlay per case head, the catalog and the
controller-only oracles, probes and counterfactuals. It is written to a new private directory
outside Git and passed to `review_campaign.py prepare --case-bundle`.
"""
import argparse
import json
from pathlib import Path
import shutil
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[1]))

from fieldline import build  # noqa: E402

PARTITIONS = {
    "development": {"api-migration": "api-migration", "module-split": "module-split"},
    "holdout": {"config-defaults": "config-defaults", "behavior-preserving-refactor": "refactor"},
}
# Path/digest-map SHA-256 (`review_campaign.directory_hash`) of the sealed holdout bundle.
HOLDOUT_SEAL = "df42dab9162638f301ece9889bd9524e3f4896580f2080dc39f75f222ed517f8"
CONTROLLER_FILES = ("oracle.json", "probe.py")


def write_tree(root, files):
    for path, text in files.items():
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("x", encoding="utf-8", newline="") as stream:
            stream.write(text)


def materialize(partition, output):
    """Write the bundle of `partition` into the new directory `output` and return its catalog."""
    source = HERE / partition
    catalog = json.loads((source / "catalog.json").read_text())
    cases = PARTITIONS[partition]
    if [case["case_id"] for case in catalog["cases"]] != list(cases):
        raise ValueError("catalog cases differ from the generator variants")
    base = build("base")
    output.mkdir(mode=0o700)
    write_tree(output / "application", base)
    for case in catalog["cases"]:
        head = build(cases[case["case_id"]])
        overlay = {path: text for path, text in head.items() if base.get(path) != text}
        deletions = sorted(set(base) - set(head))
        if case.get("deletions", {}).get("head", []) != deletions:
            raise ValueError(f"{case['case_id']}: catalog deletions differ from the generated head")
        write_tree(output / case["case_id"] / "head", overlay)
        for name in CONTROLLER_FILES:
            shutil.copyfile(source / case["case_id"] / name, output / case["case_id"] / name)
    for name in ("catalog.json", "counterfactuals.json"):
        shutil.copyfile(source / name, output / name)
    return catalog


def main():
    from publication import private_destination
    from review_campaign import directory_hash

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("partition", choices=sorted(PARTITIONS))
    parser.add_argument("--output", type=Path, required=True, help="New private bundle directory outside Git")
    args = parser.parse_args()
    output = private_destination(args.output)
    if output.exists():
        raise ValueError("bundle directory already exists")
    if output == HERE or HERE in output.parents:
        raise ValueError("bundle must be outside the generator sources")
    materialize(args.partition, output)
    digest = directory_hash(output)["sha256"]
    sealed = args.partition != "holdout" or digest == HOLDOUT_SEAL
    print(json.dumps({"partition": args.partition, "bundle": str(output), "bundle_sha256": digest, "sealed": sealed}))
    if not sealed:
        raise SystemExit("holdout bundle differs from the sealed digest")


if __name__ == "__main__":
    main()
