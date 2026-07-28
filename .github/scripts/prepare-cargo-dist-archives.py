#!/usr/bin/env python3
"""Recompress cargo-dist XZ archives within the updater's decoder limit."""

from __future__ import annotations

import argparse
import hashlib
import json
import lzma
import os
from pathlib import Path
import shutil
import tempfile


XZ_PRESET = 6
COPY_BUFFER_BYTES = 1024 * 1024


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--artifacts-dir", required=True, type=Path)
    parser.add_argument("--max-decoder-memory-mib", required=True, type=int)
    return parser.parse_args()


def recompress_archive(path: Path) -> None:
    mode = path.stat().st_mode
    with tempfile.NamedTemporaryFile(
        dir=path.parent, prefix=f".{path.name}.", delete=False
    ) as temporary:
        temporary_path = Path(temporary.name)

    try:
        with lzma.open(path, "rb") as source, lzma.open(
            temporary_path,
            "wb",
            format=lzma.FORMAT_XZ,
            check=lzma.CHECK_CRC64,
            preset=XZ_PRESET,
        ) as destination:
            shutil.copyfileobj(source, destination, COPY_BUFFER_BYTES)
        os.chmod(temporary_path, mode)
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def verify_decoder_limit(path: Path, memory_limit: int) -> None:
    decoder = lzma.LZMADecompressor(format=lzma.FORMAT_XZ, memlimit=memory_limit)
    with path.open("rb") as archive:
        while not decoder.eof:
            compressed = archive.read(COPY_BUFFER_BYTES) if decoder.needs_input else b""
            if decoder.needs_input and not compressed:
                raise SystemExit(f"{path} is a truncated XZ archive")
            decoder.decompress(compressed, max_length=COPY_BUFFER_BYTES)

        if decoder.unused_data or archive.read(1):
            raise SystemExit(f"{path} contains unexpected trailing XZ data")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(COPY_BUFFER_BYTES):
            digest.update(chunk)
    return digest.hexdigest()


def write_text_atomic(path: Path, contents: str) -> None:
    with tempfile.NamedTemporaryFile(
        mode="w", dir=path.parent, prefix=f".{path.name}.", delete=False
    ) as temporary:
        temporary.write(contents)
        temporary_path = Path(temporary.name)

    try:
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def main() -> None:
    args = parse_args()
    if args.max_decoder_memory_mib <= 0:
        raise SystemExit("--max-decoder-memory-mib must be positive")

    manifest = json.loads(args.manifest.read_text())
    prepared = 0
    for name, artifact in manifest.get("artifacts", {}).items():
        if artifact.get("kind") != "executable-zip" or not name.endswith(".tar.xz"):
            continue

        archive_path = args.artifacts_dir / name
        if not archive_path.is_file():
            continue

        checksum_name = artifact.get("checksum")
        if not isinstance(checksum_name, str):
            raise SystemExit(f"{name} has no checksum artifact in the dist manifest")

        recompress_archive(archive_path)
        verify_decoder_limit(
            archive_path, args.max_decoder_memory_mib * 1024 * 1024
        )
        digest = sha256(archive_path)
        artifact.setdefault("checksums", {})["sha256"] = digest
        write_text_atomic(
            args.artifacts_dir / checksum_name,
            f"{digest} *{name}\n",
        )
        prepared += 1

    if prepared == 0:
        raise SystemExit("the dist manifest contains no local .tar.xz release archive")

    write_text_atomic(args.manifest, json.dumps(manifest, indent=2) + "\n")
    print(f"prepared {prepared} release archive(s) with XZ preset {XZ_PRESET}")


if __name__ == "__main__":
    main()
