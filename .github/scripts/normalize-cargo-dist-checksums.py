#!/usr/bin/env python3
"""Normalize cargo-dist SHA-256 files to exactly one final LF.

cargo-dist 0.32.0 emits two final LFs. Accepting either one or two keeps this compatible with the
upstream fix while rejecting unrelated format drift.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import stat
import tempfile


SHA256_RECORD = re.compile(rb"[0-9a-f]{64} \*[^\r\n]+")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("checksums", nargs="+", type=Path)
    return parser.parse_args()


def normalize_checksum(contents: bytes, path: Path) -> bytes:
    if b"\r" in contents:
        raise SystemExit(f"{path} contains unsupported CR characters")
    if not contents.endswith(b"\n"):
        raise SystemExit(f"{path} does not end with a newline")

    normalized = contents[:-1] if contents.endswith(b"\n\n") else contents
    if b"\n\n" in normalized:
        raise SystemExit(f"{path} contains an unexpected blank line")

    records = normalized.removesuffix(b"\n").split(b"\n")
    if not records or any(SHA256_RECORD.fullmatch(record) is None for record in records):
        raise SystemExit(f"{path} contains an invalid SHA-256 checksum record")

    return normalized


def write_bytes_atomic(path: Path, contents: bytes, mode: int) -> None:
    with tempfile.NamedTemporaryFile(
        mode="wb", dir=path.parent, prefix=f".{path.name}.", delete=False
    ) as temporary:
        temporary.write(contents)
        temporary.flush()
        os.fsync(temporary.fileno())
        temporary_path = Path(temporary.name)

    try:
        os.chmod(temporary_path, stat.S_IMODE(mode))
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def main() -> None:
    args = parse_args()
    normalized_count = 0

    for path in args.checksums:
        try:
            metadata = path.lstat()
        except FileNotFoundError as error:
            raise SystemExit(f"checksum file does not exist: {path}") from error
        if not stat.S_ISREG(metadata.st_mode):
            raise SystemExit(f"checksum path is not a regular file: {path}")

        contents = path.read_bytes()
        normalized = normalize_checksum(contents, path)
        if normalized != contents:
            write_bytes_atomic(path, normalized, metadata.st_mode)
            normalized_count += 1

    print(
        f"validated formatting for {len(args.checksums)} cargo-dist checksum file(s); "
        f"normalized {normalized_count}"
    )


if __name__ == "__main__":
    main()
