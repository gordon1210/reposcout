#!/usr/bin/env python3
"""Opt-in Linux measurements of one fixed known-source CLI workload."""

import argparse
import hashlib
import json
import os
import platform
import signal
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "tests/development_scenarios/efficiency/invoice"
CONFIG = ROOT / "tests/fixtures/test-global.toml"
FIXTURE = {
    "src/invoices.py": "invoices.py",
    "src/models.py": "models.py",
    "src/money/rounding.py": "rounding.py",
    "tests/test_invoices.py": "test_invoices.py",
}
RANGES = [
    ("src/invoices.py", 1, 2),
    ("src/models.py", 1, 9),
    ("src/invoices.py", 5, 8),
    ("src/money/rounding.py", 1, 5),
    ("tests/test_invoices.py", 1, 9),
]
RSS_LIMIT = 1024**3
OUTPUT_LIMIT = 1024**2


def digest(path):
    with path.open("rb") as stream:
        result = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024**2), b""):
            result.update(chunk)
    return result.hexdigest()


def write_json(path, value):
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                         prefix=f".{path.name}-", suffix=".tmp", delete=False) as stream:
            temporary = Path(stream.name)
            json.dump(value, stream, ensure_ascii=False, indent=2)
            stream.write("\n")
        temporary.replace(path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def reserve_output(requested):
    if requested is None:
        return Path(tempfile.mkdtemp(prefix="reposcout-measurements-"))
    path = Path(os.path.abspath(requested))
    # Reject existing targets, including dangling links, and symlink-shaped parents.
    if os.path.lexists(path) or any(parent.is_symlink() for parent in path.parents):
        raise ValueError("--output must be a new directory without symlink parents")
    path.mkdir(mode=0o700)
    return path


def child_environment(state):
    # A whitelist avoids inheriting developer config, Git overrides, or secrets.
    return {
        "PATH": os.defpath,
        "HOME": str(state),
        "XDG_CACHE_HOME": str(state / "cache"),
        "XDG_CONFIG_HOME": str(state / "config"),
        "REPOSCOUT_GLOBAL_CONFIG": str(CONFIG),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "LC_ALL": "C.UTF-8",
    }


def run_child(argv, cwd, env, prefix, deadline):
    """Reap only this Popen child with wait4; the watchdog never calls wait/poll."""
    if time.monotonic() > deadline:
        raise ValueError("run-timeout-180s before process launch")
    stdout_path = prefix.with_suffix(".stdout")
    stderr_path = prefix.with_suffix(".stderr")
    argv_path = prefix.with_suffix(".argv")
    argv_path.write_bytes(json.dumps(argv, ensure_ascii=False, separators=(",", ":")).encode())
    metadata_path = prefix.with_suffix(".process.json")
    result = {"argv": argv, "cwd": str(cwd), "exit_code": None, "stopped": None}
    write_json(metadata_path, result)
    stop = threading.Event()
    watchdog = None
    pidfd = None
    child = None
    reaped = False
    with stdout_path.open("xb") as stdout, stderr_path.open("xb") as stderr:
        started = time.monotonic_ns()
        try:
            child = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                     stdout=stdout, stderr=stderr)
            result["pid"] = child.pid
            print(f"[measure] {prefix.name}: owned PID {child.pid}", file=sys.stderr)
            pidfd = os.pidfd_open(child.pid)

            def watch():
                while not stop.wait(0.02):
                    reason = None
                    if time.monotonic_ns() - started > 30_000_000_000:
                        reason = "command-timeout-30s"
                    elif time.monotonic() > deadline:
                        reason = "run-timeout-180s"
                    try:
                        try:
                            status = Path(f"/proc/{child.pid}/status").read_text()
                        except FileNotFoundError:
                            status = ""  # A very short process may already have exited.
                        for line in status.splitlines():
                            if line.startswith("VmRSS:") and int(line.split()[1]) * 1024 > RSS_LIMIT:
                                reason = "sampled-rss-limit-1GiB"
                        if stdout_path.stat().st_size + stderr_path.stat().st_size > OUTPUT_LIMIT:
                            reason = "output-limit-1MiB"
                    except (OSError, ValueError):
                        reason = "resource-monitor-unavailable"
                    if reason:
                        result["stopped"] = reason
                        try:
                            signal.pidfd_send_signal(pidfd, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        return

            watchdog = threading.Thread(target=watch)
            watchdog.start()
            _, status, usage = os.wait4(child.pid, 0)
            reaped = True
            result.update(exit_code=os.waitstatus_to_exitcode(status),
                          wall_seconds=(time.monotonic_ns() - started) / 1e9,
                          peak_rss_bytes=int(usage.ru_maxrss) * 1024,
                          user_cpu_seconds=usage.ru_utime, system_cpu_seconds=usage.ru_stime)
            child.returncode = result["exit_code"]
            if result["peak_rss_bytes"] > RSS_LIMIT:
                result["stopped"] = "kernel-peak-rss-limit-1GiB"
            elif result["wall_seconds"] > 30:
                result["stopped"] = "command-timeout-30s"
            elif time.monotonic() > deadline:
                result["stopped"] = "run-timeout-180s"
        except BaseException as error:
            result["error"] = str(error)
            raise
        finally:
            stop.set()
            if watchdog is not None:
                watchdog.join()
            if child is not None and not reaped:
                # An unreaped child retains its PID; pidfd also pins the identity when available.
                try:
                    if pidfd is not None:
                        signal.pidfd_send_signal(pidfd, signal.SIGKILL)
                    else:
                        os.kill(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                _, status, _ = os.wait4(child.pid, 0)
                child.returncode = os.waitstatus_to_exitcode(status)
            if pidfd is not None:
                os.close(pidfd)
            write_json(metadata_path, result)
    for stream, path in [("stdout", stdout_path), ("stderr", stderr_path), ("argv", argv_path)]:
        result[stream + "_path"] = path.name
        result[stream + "_bytes"] = path.stat().st_size
        result[stream + "_sha256"] = digest(path)
    if result["stdout_bytes"] + result["stderr_bytes"] > OUTPUT_LIMIT:
        result["stopped"] = "output-limit-1MiB"
    write_json(metadata_path, result)
    return result


def require_success(result):
    if result["exit_code"] != 0 or result["stopped"]:
        raise ValueError(f"CLI failed or reached a resource guard: {result}")


def cache_inventory(state):
    cache = state / "cache"
    return [{"path": str(path.relative_to(cache)), "bytes": path.stat().st_size,
             "sha256": digest(path)} for path in sorted(cache.rglob("*")) if path.is_file()]


def validate_response(raw, fixture):
    report = json.loads(raw)
    if report["requested_targets"] != len(RANGES) or report["omitted_targets"] != 0:
        raise ValueError("fixed query did not return all requested targets")
    files = {item["id"]: item for item in report["files"]}
    chunks = {item["id"]: item for item in report["sources"]}
    results = report["results"]
    if len(results) != len(RANGES):
        raise ValueError("fixed query changed its result count")
    for actual, (path, start, end) in zip(results, RANGES):
        source = (fixture / path).read_bytes()
        if b"\r" in source:
            raise ValueError("fixed authored source must use LF-only physical lines")
        lines = source.splitlines(keepends=True)
        expected = b"".join(lines[start - 1:end]).decode("utf-8")
        span = {"start_byte": sum(map(len, lines[:start - 1])),
                "end_byte": sum(map(len, lines[:end])), "start_line": start, "end_line": end}
        file = files[actual["file"]]
        chunk = chunks[actual["source"]]
        if (actual["status"] != "complete" or actual["selection"] != "range"
                or actual["requested_range"] != {"start": start, "end": end}
                or file["path"] != path or chunk["file"] != actual["file"]
                or file["snapshot"] != {"kind": "worktree"} or chunk["span"] != span
                or file["sha256"] != digest(fixture / path)
                or chunk["content"] != expected):
            raise ValueError(f"fixed query changed the expected complete source at {path}:{start}-{end}")
    return report


def tokenize(binary, rows, work, output, encoding, deadline):
    token_root = work / "token-inputs"
    token_root.mkdir()
    for index, row in enumerate(rows):
        for stream in ["stdout", "stderr", "argv"]:
            raw = (output / row[stream + "_path"]).read_bytes()
            raw.decode("utf-8")  # Never silently count replacement characters.
            (token_root / f"{index}-{stream}.md").write_bytes(raw)
    state = work / "token-state"
    state.mkdir()
    result = run_child([str(binary), "tokens", ".", "--encoding", encoding,
                        "--no-cache", "--no-project-config", "-f", "json", "--quiet"],
                       token_root, child_environment(state), output / "tokenization", deadline)
    require_success(result)
    report = json.loads((output / result["stdout_path"]).read_bytes())
    counts = {Path(item["path"]).name: item["tokens"] for item in report["files"]}
    if len(counts) != len(rows) * 3:
        raise ValueError("token pass omitted complete stream or argv inputs")
    for index, row in enumerate(rows):
        for stream in ["stdout", "stderr", "argv"]:
            row[stream + "_tokens"] = counts[f"{index}-{stream}.md"]
        row["response_tokens"] = row["stdout_tokens"] + row["stderr_tokens"]
        row["interaction_tokens"] = row["response_tokens"] + row["argv_tokens"]
    return result


def summaries(rows):
    result = {}
    for state in ["reposcout-cache-cold", "reposcout-cache-warm"]:
        samples = [row for row in rows if row["cache_state"] == state]
        result[state] = {"samples": len(samples)}
        for metric in ["wall_seconds", "peak_rss_bytes", "response_tokens", "argv_tokens"]:
            values = [row[metric] for row in samples]
            result[state][metric] = {"min": min(values), "median": statistics.median(values),
                                     "max": max(values)}
    return result


def measure(args):
    binary = args.binary.resolve(strict=True)
    output = reserve_output(args.output)
    print(f"[measure] retained artifacts: {output}", file=sys.stderr)
    deadline = time.monotonic() + 180
    report = {"schema": 1, "complete": False, "workload": "invoice-known-source-ranges",
              "encoding": args.encoding, "repeats": args.repeats, "measurements": [],
              "rss_method": "Linux wait4 owned CLI child ru_maxrss KiB times 1024; not process tree",
              "cache_method": "fresh private XDG cache per cold/warm pair; OS page cache untouched",
              "provenance": {"binary": str(binary), "binary_sha256": digest(binary),
                             "measurement_script_sha256": digest(Path(__file__)),
                             "host": platform.platform(), "machine": platform.machine(),
                             "python": platform.python_version(),
                             "global_config_sha256": digest(CONFIG),
                             "cargo_toml_sha256": digest(ROOT / "Cargo.toml"),
                             "cargo_lock_sha256": digest(ROOT / "Cargo.lock")}}
    destination = output / "results.json"
    try:
        with tempfile.TemporaryDirectory(prefix="reposcout-measurements-work-") as temporary:
            work = Path(temporary)
            fixture = work / "repository"
            fixture.mkdir()
            report["fixture"] = {}
            for target, source in FIXTURE.items():
                path = fixture / target
                path.parent.mkdir(parents=True, exist_ok=True)
                content = (SOURCE / source).read_bytes()
                if len(content) > 64 * 1024:
                    raise ValueError("authored fixture exceeds the fixed 64 KiB per-file bound")
                path.write_bytes(content)
                report["fixture"][target] = {"bytes": len(content), "sha256": digest(path)}
            version_state = work / "version-state"
            version_state.mkdir()
            version = run_child([str(binary), "--version"], work, child_environment(version_state),
                                output / "version", deadline)
            require_success(version)
            report["provenance"]["binary_version"] = (output / version["stdout_path"]).read_text().strip()
            # The checkout identity describes the measurement setup, not a proven binary source revision.
            revision = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                      env=child_environment(version_state), stdout=subprocess.PIPE,
                                      stderr=subprocess.PIPE, timeout=5, check=True)
            report["provenance"]["checkout_head"] = revision.stdout.decode().strip()
            status = subprocess.run(["git", "status", "--porcelain", "--untracked-files=no"], cwd=ROOT,
                                    env=child_environment(version_state), stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, timeout=5, check=True)
            report["provenance"]["checkout_tracked_status"] = status.stdout.decode()
            argv = [str(binary), "read", ".", "--profile", "agent", "--no-project-config",
                    "--encoding", args.encoding, "--budget", "4096", "--max-output-bytes", "12288",
                    "-f", "json", "--quiet"]
            for path, start, end in RANGES:
                argv.extend(["--range", path, str(start), str(end)])
            write_json(destination, report)
            reference = None
            for repeat in range(1, args.repeats + 1):
                state = work / f"state-{repeat}"
                state.mkdir()
                for temperature in ["cold", "warm"]:
                    before = cache_inventory(state)
                    if (temperature == "cold") != (not before):
                        raise ValueError("required fresh or populated RepoScout cache was unavailable")
                    row = run_child(argv, fixture, child_environment(state),
                                    output / f"pair-{repeat:02}-{temperature}", deadline)
                    row.update(repeat=repeat, cache_state=f"reposcout-cache-{temperature}",
                               cache_before=before, cache_after=cache_inventory(state), cache_hits=None)
                    report["measurements"].append(row)
                    write_json(destination, report)
                    require_success(row)
                    actual = validate_response((output / row["stdout_path"]).read_bytes(), fixture)
                    if reference is not None and actual != reference:
                        raise ValueError("complete cold/warm response semantics differ")
                    reference = actual
            report["tokenization"] = tokenize(binary, report["measurements"], work, output,
                                               args.encoding, deadline)
            if digest(binary) != report["provenance"]["binary_sha256"]:
                raise ValueError("binary changed during the measurement run")
            report["summary"] = summaries(report["measurements"])
            report["complete"] = True
    except BaseException as error:
        report["error"] = str(error)
        raise
    finally:
        write_json(destination, report)
    return {"results": str(destination), "summary": report["summary"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/reposcout")
    parser.add_argument("--output", type=Path, help="new artifact directory (default: retained /tmp directory)")
    parser.add_argument("--encoding", choices=["o200k_base", "cl100k_base"], default="o200k_base")
    parser.add_argument("--repeats", type=int, choices=range(1, 11), default=3, metavar="1..10")
    args = parser.parse_args()
    if (sys.platform != "linux" or not hasattr(os, "pidfd_open")
            or not hasattr(signal, "pidfd_send_signal")):
        parser.error("requires Linux with pidfds and Python 3.9+; no sampled-peak fallback")
    try:
        print(json.dumps(measure(args), ensure_ascii=False))
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(2, f"measurement incomplete: {error}\n")


if __name__ == "__main__":
    main()
