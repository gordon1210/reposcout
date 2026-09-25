#!/usr/bin/env python3
"""Verify that both release targets and all universal publish files arrived."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import re


TARGETS = ("aarch64-apple-darwin", "x86_64-unknown-linux-gnu")
ARCHIVES = tuple(f"reposcout-{target}.tar.xz" for target in TARGETS) + ("source.tar.gz",)
OTHER_FILES = ("reposcout-installer.sh", "reposcout.cdx.xml")
CHECKSUM_RECORD = re.compile(r"([0-9a-f]{64}) \*([^\r\n]+)")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_checksums(path: Path) -> dict[str, str]:
    records: dict[str, str] = {}
    for line in path.read_text().splitlines():
        match = CHECKSUM_RECORD.fullmatch(line)
        if match is None or match[2] in records:
            raise ValueError(f"invalid or duplicate checksum record in {path}")
        records[match[2]] = match[1]
    return records


def verify(directory: Path, include_host_manifest: bool = False) -> None:
    expected = set(ARCHIVES) | {f"{archive}.sha256" for archive in ARCHIVES}
    expected.update(OTHER_FILES)
    expected.add("sha256.sum")
    if include_host_manifest:
        expected.add("dist-manifest.json")
    for name in sorted(expected):
        file = directory / name
        if not file.is_file() or file.is_symlink() or file.stat().st_size == 0:
            raise ValueError(f"missing or empty release artifact: {name}")

    unified = read_checksums(directory / "sha256.sum")
    if set(unified) != set(ARCHIVES):
        raise ValueError("unified checksums do not name exactly the expected release archives")
    for name in ARCHIVES:
        sidecar = read_checksums(directory / f"{name}.sha256")
        digest = sha256(directory / name)
        if sidecar != {name: digest} or unified[name] != digest:
            raise ValueError(f"release checksum mismatch: {name}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--include-host-manifest", action="store_true")
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    verify(args.directory, args.include_host_manifest)


if __name__ == "__main__":
    main()
