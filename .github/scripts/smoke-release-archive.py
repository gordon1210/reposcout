#!/usr/bin/env python3
"""Run the executable from the packed release archive on a tiny isolated tree."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile


def smoke(archive: Path, global_config: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="reposcout-release-smoke-") as temporary:
        root = Path(temporary)
        binary = root / "reposcout"
        with tarfile.open(archive, mode="r:xz") as package:
            candidates = [
                member
                for member in package.getmembers()
                if Path(member.name).name == "reposcout" and member.isfile()
            ]
            if len(candidates) != 1:
                raise ValueError("release archive must contain exactly one regular reposcout binary")
            if candidates[0].mode & 0o111 == 0:
                raise ValueError("packed reposcout binary is not executable")
            source = package.extractfile(candidates[0])
            if source is None:
                raise ValueError("cannot read the packed reposcout binary")
            with binary.open("wb") as destination, source:
                shutil.copyfileobj(source, destination)
        binary.chmod(candidates[0].mode & 0o777)

        subprocess.run([binary, "--version"], check=True, capture_output=True, text=True, timeout=30)
        sample = root / "sample"
        sample.mkdir()
        (sample / "tiny.rs").write_text("fn main() {}\n")
        environment = os.environ.copy()
        environment["REPOSCOUT_GLOBAL_CONFIG"] = str(global_config)
        output = subprocess.run(
            [binary, "-f", "json", "--summary", sample],
            check=True,
            capture_output=True,
            text=True,
            env=environment,
            timeout=30,
        )
        report = json.loads(output.stdout)
        if report["diagnostics"]["analyzed_files"] != 1:
            raise ValueError("packed binary did not analyze the one-file smoke tree")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--global-config", required=True, type=Path)
    args = parser.parse_args()
    smoke(args.archive, args.global_config.resolve())


if __name__ == "__main__":
    main()
