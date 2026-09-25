#!/usr/bin/env python3
"""Reject release binaries with unexpected dynamic dependencies."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess


LINUX_LZMA = re.compile(r"^\s*NEEDED\s+liblzma(?:\.|\s|$)", re.MULTILINE)


def verify(binary: Path, runner_os: str) -> None:
    if not binary.is_file() or not binary.stat().st_mode & 0o111:
        raise SystemExit(f"release binary is missing or not executable: {binary}")

    command = ["otool", "-L", str(binary)] if runner_os == "macOS" else ["objdump", "-p", str(binary)]
    try:
        result = subprocess.run(command, capture_output=True, text=True, check=True)
    except (OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"dependency analysis failed: {error}") from error

    if runner_os == "macOS":
        lines = result.stdout.splitlines()
        if len(lines) < 2 or not lines[0].endswith(":"):
            raise SystemExit("otool returned an invalid library listing")
        for line in lines[1:]:
            fields = line.split()
            if not fields:
                continue
            dependency = fields[0]
            if not dependency.startswith(("/usr/lib/", "/System/Library/")):
                raise SystemExit(f"unexpected non-system dependency: {dependency}")
    elif LINUX_LZMA.search(result.stdout):
        raise SystemExit("unexpected dynamic liblzma dependency")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--os", choices=["macOS", "Linux"], required=True)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    verify(args.binary, args.os)


if __name__ == "__main__":
    main()
