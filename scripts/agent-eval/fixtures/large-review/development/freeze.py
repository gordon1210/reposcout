"""Hash and deterministically archive the completed LOCAL private fixture bundle."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import tarfile
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from publication import private_destination

parser = argparse.ArgumentParser()
parser.add_argument("--archive", type=Path, required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parent
archive = private_destination(args.archive)
if archive == root or root in archive.parents:
    raise ValueError("Archive must be outside the source bundle")
files = []
for path in sorted(root.rglob("*")):
    if path.is_symlink():
        raise ValueError(f"Symlinks are not permitted: {path}")
    if path.is_file() and path.name not in {"freeze-manifest.json", "BUILD_ROOT"}:
        data = path.read_bytes()
        files.append({"path": str(path.relative_to(root)), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
identity = hashlib.sha256(json.dumps(files, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
manifest = {"schema": 1, "content_sha256": identity, "files": files,
            "public_source_directory": "application", "private_oracles_outside_public_source": True}
manifest_bytes = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
descriptor = os.open(archive, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
with os.fdopen(descriptor, "wb") as raw:
    with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as output:
            members = [(row["path"], (root / row["path"]).read_bytes()) for row in files]
            members.append(("freeze-manifest.json", manifest_bytes))
            for name, data in sorted(members):
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mtime = 0
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                info.mode = 0o644
                output.addfile(info, io.BytesIO(data))
print(json.dumps({"content_sha256": identity, "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                  "archive": str(archive), "files": len(files)}))
