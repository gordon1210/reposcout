"""Run one serial, isolated Codex invocation and preserve bounded lifecycle evidence."""

from dataclasses import dataclass
from datetime import datetime, timezone
import errno
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import selectors
import select
import re
import signal
import stat
import subprocess
import tempfile
import time
import uuid

from codex_isolation import (
    PROBE_CHECKS, REPOSCOUT_CHILD_TIMEOUT_SECONDS, IsolationError, fingerprint_manifest, prepare_isolation,
)


@dataclass(frozen=True)
class TrialSpec:
    run_id: str
    workspace: Path
    artifact_dir: Path
    controller_dir: Path
    codex_binary: Path
    cli_version: str
    prompt: str
    answer_schema: dict
    model: str = "gpt-6.1-sol"
    effort: str = "max"
    variant: str = "baseline"
    reposcout_binary: Path | None = None
    skill_dir: Path | None = None
    resume_thread_id: str | None = None
    timeout_seconds: float = 180
    memory_limit_bytes: int = 1024 * 1024 * 1024
    host_memory_reserve_bytes: int = 12 * 1024 * 1024 * 1024
    max_output_bytes: int = 16 * 1024 * 1024
    usage_scope: str = "unknown"
    usage_baseline: dict | None = None
    expected_codex_sha256: str | None = None
    expected_codex_runtime_sha256: str | None = None
    controller_ca_file: Path | None = None
    expected_controller_ca_sha256: str | None = None
    expected_reposcout_sha256: str | None = None
    expected_skill_sha256: str | None = None
    expected_workspace_sha256: str | None = None
    runtime_tool_paths: dict | None = None
    expected_runtime_tools_sha256: str | None = None
    required_runtimes: tuple = ()


def _write_json(path, value):
    with Path(path).open("x") as stream:
        json.dump(value, stream, sort_keys=True, indent=2)
        stream.write("\n")


def _digest_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _memory_available():
    for line in Path("/proc/meminfo").read_text().splitlines():
        if line.startswith("MemAvailable:"):
            return int(line.split()[1]) * 1024
    raise OSError("Host available-memory measurement is unavailable")


def _process_identity(pid, *, on_error=None):
    try:
        raw = Path(f"/proc/{pid}/stat").read_text()
        fields = raw[raw.rfind(")") + 2:].split()
        return {"pid": pid, "ppid": int(fields[1]), "state": fields[0], "start": int(fields[19]),
                "rss": max(0, int(fields[21])) * os.sysconf("SC_PAGE_SIZE")}
    except (OSError, ValueError, IndexError) as error:
        if on_error is not None:
            on_error(error)
        return None


def _path_may_exist(path):
    try:
        path.stat()
    except FileNotFoundError:
        return False
    except OSError:
        return True
    return True


def _process_children(pid, *, on_error=None):
    # glob suppresses directory-read errors, which would hide live descendants.
    children = []
    operation, tid = "list-tasks", None
    try:
        tasks = iter(Path(f"/proc/{pid}/task").iterdir())
        while True:
            operation, tid = "list-tasks", None
            task = next(tasks, None)
            if task is None:
                break
            operation, tid = "read-children", int(task.name)
            try:
                children.extend(int(value) for value in (task / "children").read_text().split())
            except FileNotFoundError:
                if _path_may_exist(task):
                    raise
                # A thread may exit while its process and other threads remain live.
    except (OSError, ValueError) as error:
        if on_error is not None:
            on_error(operation, error, tid)
        raise
    return children


def _pidfd_terminal(descriptor, *, wait_ms=0):
    """A flags=0 pidfd becomes ready only after the last thread has exited."""
    poller = select.poll()
    poller.register(descriptor, select.POLLIN)
    events = poller.poll(wait_ms)
    masks = [mask for fd, mask in events if fd == descriptor]
    if any(mask & (select.POLLERR | select.POLLNVAL) for mask in masks):
        raise OSError(errno.EBADF, "Cannot observe retained process descriptor")
    return any(mask & (select.POLLIN | select.POLLHUP) for mask in masks)


def _pid_namespace():
    value = str(Path("/proc/self/ns/pid").readlink())
    if re.fullmatch(r"pid:\[\d+\]", value) is None:
        raise IsolationError("Supervisor PID namespace identity is unavailable")
    return value


class OwnedProcesses:
    """Only signal identities descended from this invocation, using retained pidfds."""

    def __init__(self, root_pid, *, observe=True, clock=time.monotonic):
        self.root_pid = root_pid
        self.identities = {}
        self.pidfds = {}
        self.tracking_failed = False
        self.untracked_identities = {}
        self.scan_started = {}
        self.observation_errors = []
        self.observation_errors_dropped = 0
        self.confirmed_exit_races = 0
        self.clock = clock
        if observe:
            self.observe()

    @property
    def root_confirmed(self):
        return self.root_pid in self.pidfds

    def records(self):
        identities = {**self.identities, **self.untracked_identities}
        return [{"pid": pid, "start": value["start"]} for pid, value in sorted(identities.items())]

    def _note_error(self, operation, pid, error, *, tid=None, start=None):
        # Private metadata only: exception messages can contain unrelated paths.
        identity = self.identities.get(pid, self.untracked_identities.get(pid, {}))
        record = {"operation": operation, "pid": pid,
                  "start": identity.get("start") if start is None else start,
                  "tid": tid, "errno": getattr(error, "errno", None),
                  "error_type": type(error).__name__}
        if record in self.observation_errors:
            return
        if len(self.observation_errors) == 64:
            self.observation_errors.pop(0)
            self.observation_errors_dropped += 1
        self.observation_errors.append(record)

    def _identity(self, pid, operation="read-stat", *, start=None):
        return _process_identity(pid, on_error=lambda error: self._note_error(operation, pid, error, start=start))

    def _same_parent(self, identity, parent_pid, parent_start):
        parent = self._identity(parent_pid, "read-parent-stat")
        if parent is None and parent_pid in self.identities:
            self._observation_failed(self.identities[parent_pid])
        return (identity.get("ppid") == parent_pid and parent is not None
                and parent["start"] == parent_start)

    def _alive(self, identity):
        descriptor = self.pidfds.get(identity["pid"])
        retained = self.identities.get(identity["pid"])
        if descriptor is not None and retained is not None and retained["start"] == identity["start"]:
            try:
                return not _pidfd_terminal(descriptor)
            except OSError as error:
                self._note_error("poll-pidfd", identity["pid"], error, start=identity["start"])
                self.tracking_failed = True
                self.untracked_identities[identity["pid"]] = identity
                return True
        return _record_alive(identity)

    def _observation_failed(self, identity, *, error=None):
        if not self._alive(identity):
            return
        descriptor = self.pidfds.get(identity["pid"])
        if descriptor is not None and getattr(error, "errno", None) in (errno.ENOENT, errno.ESRCH):
            try:
                if _pidfd_terminal(descriptor, wait_ms=10):
                    self.confirmed_exit_races += 1
                    return
            except OSError as poll_error:
                self._note_error("poll-pidfd", identity["pid"], poll_error, start=identity["start"])
        self.tracking_failed = True
        self.untracked_identities[identity["pid"]] = identity

    def observe(self):
        for pid, descriptor in list(self.pidfds.items()):
            if pid != self.root_pid and not self._alive(self.identities[pid]):
                self.scan_started.pop((pid, self.identities[pid]["start"]), None)
                os.close(descriptor)
                del self.pidfds[pid]
                del self.identities[pid]
        pending = [(pid, None, None) for pid in (self.root_pid, *self.identities)]
        visited = set()
        rss = scans = 0
        while pending:
            pid, parent_pid, parent_start = pending.pop()
            if pid in visited:
                continue
            visited.add(pid)
            identity = self._identity(pid)
            if identity is None:
                if pid in self.identities:
                    self._observation_failed(self.identities[pid])
                elif _path_may_exist(Path(f"/proc/{pid}")):
                    self.tracking_failed = True
                continue
            old = self.identities.get(pid)
            if old is not None and old["start"] != identity["start"]:
                continue
            if old is None:
                if pid != self.root_pid and not self._same_parent(identity, parent_pid, parent_start):
                    continue
                try:
                    descriptor = os.pidfd_open(pid, 0)
                except (OSError, AttributeError) as error:
                    self._note_error("pidfd-open", pid, error, start=identity["start"])
                    if _record_alive(identity):
                        self.tracking_failed = True
                        self.untracked_identities[pid] = identity
                    continue
                current = self._identity(pid, "verify-pidfd-stat", start=identity["start"])
                if (current is None or current["start"] != identity["start"]
                        or (pid != self.root_pid and not self._same_parent(current, parent_pid, parent_start))):
                    os.close(descriptor)
                    if current is None:
                        self._observation_failed(identity)
                    continue
                self.identities[pid] = identity
                self.pidfds[pid] = descriptor
                identity = current
            if not self._alive(identity):
                continue
            rss += identity["rss"]
            try:
                executable = Path(f"/proc/{pid}/exe").readlink().name
                is_scan = executable in ("reposcout", "reposcoutdev")
                scans += is_scan
                scan_identity = (pid, identity["start"])
                if is_scan:
                    self.scan_started.setdefault(scan_identity, self.clock())
                else:
                    self.scan_started.pop(scan_identity, None)
            except (OSError, ValueError) as error:
                self._note_error("read-exe", pid, error, start=identity["start"])
                self._observation_failed(identity, error=error)
                continue
            try:
                children = _process_children(pid, on_error=lambda operation, error, tid: self._note_error(
                    operation, pid, error, tid=tid, start=identity["start"],
                ))
                pending.extend((child, pid, identity["start"]) for child in children)
            except (OSError, ValueError) as error:
                self._observation_failed(identity, error=error)
        return rss, scans

    def reposcout_timed_out(self):
        now = self.clock()
        for (pid, start), began in list(self.scan_started.items()):
            if not self._alive({"pid": pid, "start": start}):
                del self.scan_started[(pid, start)]
            elif now - began >= REPOSCOUT_CHILD_TIMEOUT_SECONDS:
                return True
        return False

    def living(self):
        return [
            pid for pid, old in self.identities.items()
            if self._alive(old)
        ]

    def signal(self, sig):
        for pid, descriptor in self.pidfds.items():
            current = _process_identity(pid)
            if current is None or current["start"] != self.identities[pid]["start"]:
                continue
            try:
                signal.pidfd_send_signal(descriptor, sig)
            except ProcessLookupError:
                pass

    def close(self):
        for descriptor in self.pidfds.values():
            os.close(descriptor)


def _record_alive(record):
    """Unknown proc access is not proof of exit; a different start identity is."""
    current = _process_identity(record["pid"])
    if current is not None:
        # A zombie group leader can still have live threads. Without a retained
        # process-wide pidfd, only disappearance or a new birth proves exit.
        return current["start"] == record["start"]
    return _path_may_exist(Path(f'/proc/{record["pid"]}'))


def _drain_owned_processes(process, owned, *, timeout=0.25, clock=time.monotonic, sleep=time.sleep):
    """Allow bounded namespace teardown; a returned root does not imply every child has exited."""
    deadline = clock() + timeout
    while True:
        process.poll()
        owned.observe()
        if not owned.living():
            return True
        remaining = deadline - clock()
        if remaining <= 0:
            return False
        sleep(min(0.01, remaining))


class SerialLease:
    def __init__(self, path=None):
        self.path = Path(path) if path is not None else Path(tempfile.gettempdir()) / f"reposcout-codex-eval-{os.getuid()}.lock"
        self.result = None

    def guard(self, result):
        self.result = result
        result["supervisor_pid_namespace"] = self.pid_namespace

    def _store(self, value):
        data = json.dumps(value, sort_keys=True).encode() if value else b""
        os.lseek(self.fd, 0, os.SEEK_SET)
        os.ftruncate(self.fd, 0)
        os.write(self.fd, data)
        os.fsync(self.fd)

    def __enter__(self):
        self.fd = os.open(self.path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
        info = os.fstat(self.fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1:
            os.close(self.fd)
            raise IsolationError("Evaluation serialization lock has an unexpected identity")
        try:
            fcntl.flock(self.fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            os.close(self.fd)
            raise IsolationError("Another evaluation already owns the serial execution lease") from None
        try:
            self.pid_namespace = _pid_namespace()
            raw = os.read(self.fd, 65537)
            if raw:
                fence = json.loads(raw)
                if not isinstance(fence, dict):
                    raise IsolationError("Invalid serialization fence")
                records = fence.get("owned_processes")
                if (len(raw) > 65536 or fence.get("ownership_complete") is not True
                        or fence.get("supervisor_pid_namespace") != self.pid_namespace
                        or not isinstance(records, list) or not records
                        or any(not isinstance(item, dict) or type(item.get("pid")) is not int
                               or item["pid"] <= 0 or type(item.get("start")) is not int
                               or item["start"] < 0 for item in records)
                        or any(_record_alive(item) for item in records)):
                    raise IsolationError("A previous evaluation has unconfirmed process cleanup")
                self._store(None)
        except (ValueError, TypeError, OSError):
            os.close(self.fd)
            raise IsolationError("A previous evaluation has unconfirmed process cleanup") from None
        return self

    def __exit__(self, *_):
        retained = False
        try:
            if self.result is not None and self.result.get("process_tree_drained") is not True:
                try:
                    self._store({"schema": 1, "ownership_complete": self.result.get("ownership_complete", False),
                                 "owned_processes": self.result.get("owned_processes", []),
                                 "supervisor_pid_namespace": self.pid_namespace,
                                 "reason": "unconfirmed-process-cleanup"})
                    self.result["serial_fence_written"] = True
                except OSError:
                    # Keep the live flock if a durable fence cannot be written.
                    _RETAINED_LEASES.append(self.fd)
                    retained = True
                    self.result["serial_fence_written"] = False
                    self.result["serial_lease_retained"] = True
        finally:
            if not retained:
                os.close(self.fd)


_RETAINED_LEASES = []


def execute_process(command, *, input_bytes, stdout_path, stderr_path, timeout_seconds,
                    memory_limit_bytes, host_memory_reserve_bytes, max_output_bytes):
    """Capture an owned process tree. This function never loads authentication content."""
    started = time.monotonic()
    base = {
        "status": "not-started", "returncode": None, "duration_ms": 0, "peak_rss_bytes": 0,
        "termination_reason": None, "stream_complete": False, "process_tree_drained": True,
        "stdout_truncated": False, "stderr_truncated": False,
        "stdout_total_bytes": 0, "stderr_total_bytes": 0,
        "owned_processes": [], "ownership_complete": True,
        "observation_errors": [], "observation_errors_dropped": 0,
        "confirmed_exit_races": 0,
    }
    try:
        if _memory_available() < host_memory_reserve_bytes:
            return {**base, "termination_reason": "host-memory-reserve"}
    except OSError as error:
        return {**base, "termination_reason": "resource-monitor-error", "error_type": type(error).__name__}
    if not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal"):
        return {**base, "termination_reason": "process-identity-protection-unavailable"}
    try:
        process = subprocess.Popen(
            command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env={}, start_new_session=True, close_fds=True,
        )
    except OSError as error:
        return {**base, "termination_reason": "spawn-error", "error_type": type(error).__name__}
    base["process_tree_drained"] = False
    base["ownership_complete"] = False
    owned = OwnedProcesses(process.pid, observe=False)
    selector = None
    files = {}
    bytes_saved = 0
    stop_at = None
    killed = False
    complete_eof = set()
    try:
        owned.observe()
        if not owned.root_confirmed or owned.tracking_failed:
            raise IsolationError("Cannot retain the newly launched process identity")
        selector = selectors.DefaultSelector()
        for name, pipe, path in (("stdout", process.stdout, stdout_path),
                                 ("stderr", process.stderr, stderr_path)):
            files[name] = Path(path).open("xb")
            os.set_blocking(pipe.fileno(), False)
            selector.register(pipe, selectors.EVENT_READ, name)
        os.set_blocking(process.stdin.fileno(), False)
        if input_bytes:
            selector.register(process.stdin, selectors.EVENT_WRITE, "stdin")
        else:
            process.stdin.close()
        pending_input = memoryview(input_bytes)
        while selector.get_map() or process.poll() is None:
            elapsed = time.monotonic() - started
            rss, scans = owned.observe()
            base["peak_rss_bytes"] = max(base["peak_rss_bytes"], rss)
            reason = None
            if owned.tracking_failed:
                reason = "process-identity-protection-unavailable"
            elif owned.reposcout_timed_out():
                reason = "reposcout-child-timeout"
            elif elapsed >= timeout_seconds:
                reason = "timeout"
            elif rss > memory_limit_bytes:
                reason = "memory-limit"
            elif _memory_available() < host_memory_reserve_bytes:
                reason = "host-memory-reserve"
            elif scans > 1:
                reason = "concurrent-reposcout-processes"
            if reason is not None and stop_at is None:
                base["termination_reason"] = reason
                stop_at = time.monotonic()
                owned.signal(signal.SIGTERM)
            if stop_at is not None:
                if time.monotonic() - stop_at >= 1 and not killed:
                    owned.signal(signal.SIGKILL)
                    killed = True
                if time.monotonic() - stop_at >= 3:
                    break
            for key, _ in selector.select(0.1):
                if key.data == "stdin":
                    try:
                        count = os.write(key.fd, pending_input[:65536])
                        pending_input = pending_input[count:]
                    except BlockingIOError:
                        continue
                    except BrokenPipeError:
                        pending_input = memoryview(b"")
                    if not pending_input:
                        selector.unregister(key.fileobj)
                        key.fileobj.close()
                    continue
                try:
                    chunk = os.read(key.fd, 65536)
                except BlockingIOError:
                    continue
                if not chunk:
                    complete_eof.add(key.data)
                    selector.unregister(key.fileobj)
                    key.fileobj.close()
                    continue
                base[f"{key.data}_total_bytes"] += len(chunk)
                keep = chunk[:max(0, max_output_bytes - bytes_saved)]
                files[key.data].write(keep)
                files[key.data].flush()
                bytes_saved += len(keep)
                if len(keep) != len(chunk):
                    base[f"{key.data}_truncated"] = True
                    if stop_at is None:
                        base["termination_reason"] = "output-limit"
                        stop_at = time.monotonic()
                        owned.signal(signal.SIGTERM)
        drained = _drain_owned_processes(process, owned)
        if process.poll() is None or not drained:
            if base["termination_reason"] is None:
                base["termination_reason"] = "unfinished-child-process"
            owned.signal(signal.SIGKILL)
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass
        base["returncode"] = process.poll()
        base["process_tree_drained"] = _drain_owned_processes(process, owned)
        base["stream_complete"] = (
            complete_eof == {"stdout", "stderr"} and base["termination_reason"] is None
            and base["process_tree_drained"]
        )
        base["status"] = (
            "aborted" if base["termination_reason"] is not None else
            "completed" if base["returncode"] == 0 and base["stream_complete"] else "failed"
        )
    except KeyboardInterrupt:
        base["status"] = "aborted"
        base["termination_reason"] = "controller-interrupted"
    except Exception as error:
        base["status"] = "failed"
        base["termination_reason"] = (
            "process-identity-protection-unavailable" if owned.tracking_failed else "executor-error"
        )
        base["error_type"] = type(error).__name__
    finally:
        try:
            owned.observe()
            if process.poll() is None or owned.living():
                owned.signal(signal.SIGKILL)
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    pass
            base["returncode"] = process.poll()
            base["process_tree_drained"] = (
                owned.root_confirmed and not owned.tracking_failed and _drain_owned_processes(process, owned)
            )
        except Exception as error:
            base["process_tree_drained"] = False
            base["cleanup_error_type"] = type(error).__name__
        base["ownership_complete"] = owned.root_confirmed and not owned.tracking_failed
        base["process_tree_drained"] = base["process_tree_drained"] and base["ownership_complete"]
        base["owned_processes"] = owned.records()
        base["observation_errors"] = owned.observation_errors
        base["observation_errors_dropped"] = owned.observation_errors_dropped
        base["confirmed_exit_races"] = owned.confirmed_exit_races
        for resource in [process.stdin, process.stdout, process.stderr, *files.values(), selector, owned]:
            if resource is not None:
                try:
                    resource.close()
                except OSError as error:
                    base["cleanup_error_type"] = type(error).__name__
        if base.get("cleanup_error_type") is not None:
            base["status"] = "failed"
            base["termination_reason"] = base["termination_reason"] or "cleanup-error"
    base["stream_complete"] = (
        complete_eof == {"stdout", "stderr"} and base["termination_reason"] is None
        and base["process_tree_drained"]
    )
    if not base["process_tree_drained"]:
        base["status"] = "aborted"
        base["termination_reason"] = base["termination_reason"] or "unfinished-child-process"
    base["duration_ms"] = round((time.monotonic() - started) * 1000)
    return base


def _validate_spec(spec):
    if not isinstance(spec.run_id, str) or not spec.run_id or len(spec.run_id) > 200:
        raise ValueError("A bounded run identity is required")
    if not isinstance(spec.model, str) or re.fullmatch(r"[A-Za-z0-9_.:-]{1,128}", spec.model) is None:
        raise ValueError("An explicit model identity is required")
    if spec.effort not in ("none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"):
        raise ValueError("An explicit supported reasoning effort is required")
    if not isinstance(spec.timeout_seconds, (int, float)) or not math.isfinite(spec.timeout_seconds) or spec.timeout_seconds <= 0:
        raise ValueError("Invocation timeout must be finite and positive")
    if not isinstance(spec.memory_limit_bytes, int) or spec.memory_limit_bytes <= 0:
        raise ValueError("Invocation memory limit must be positive")
    if not isinstance(spec.host_memory_reserve_bytes, int) or spec.host_memory_reserve_bytes < 0:
        raise ValueError("Host memory reserve cannot be negative")
    if not (0 < spec.max_output_bytes <= 16 * 1024 ** 2):
        raise ValueError("Output retention must be positive and at most 16 MiB")
    if not isinstance(spec.prompt, str) or len(spec.prompt.encode()) > 1024 ** 2:
        raise ValueError("Prompt exceeds the 1 MiB input bound")
    if not isinstance(spec.answer_schema, dict) or len(json.dumps(spec.answer_schema).encode()) > 128 * 1024:
        raise ValueError("Answer schema exceeds the 128 KiB input bound")
    if spec.resume_thread_id is not None:
        uuid.UUID(spec.resume_thread_id)
    if spec.usage_scope not in ("invocation", "thread-cumulative", "unknown"):
        raise ValueError("Unknown usage scope")
    for name in ("workspace", "codex", "codex_runtime", "controller_ca", "reposcout", "skill", "runtime_tools"):
        expected = getattr(spec, f"expected_{name}_sha256")
        if expected is not None and (not isinstance(expected, str) or re.fullmatch(r"[0-9a-f]{64}", expected) is None):
            raise ValueError(f"Invalid prepared {name} fingerprint")


def _capture(executor, command, spec, stdout, stderr, *, prompt=b"", probe=False, timeout=None):
    remaining = spec.timeout_seconds if timeout is None else timeout
    return executor(
        command, input_bytes=prompt, stdout_path=stdout, stderr_path=stderr,
        timeout_seconds=min(remaining, 30) if probe else remaining,
        memory_limit_bytes=spec.memory_limit_bytes,
        host_memory_reserve_bytes=spec.host_memory_reserve_bytes,
        max_output_bytes=min(spec.max_output_bytes, 65536) if probe else spec.max_output_bytes,
    )


def _thread_identity(path, resumed):
    identities = set()
    with Path(path).open("rb") as stream:
        for line in stream:
            if len(line) > 1024 * 1024:
                continue
            try:
                event = json.loads(line)
            except (ValueError, UnicodeDecodeError):
                continue
            if isinstance(event, dict) and event.get("type") == "thread.started":
                identity = event.get("thread_id")
                if isinstance(identity, str):
                    identities.add(identity)
    if len(identities) > 1 or (resumed is not None and identities and identities != {resumed}):
        raise IsolationError("Codex session identity differs from the prepared invocation")
    return next(iter(identities), resumed)


def run_trial(spec, *, preflight_only=False, executor=execute_process):
    """Run a mandatory no-model probe, then one fresh or exactly resumed Codex invocation."""
    _validate_spec(spec)
    started = time.monotonic()
    invocation = uuid.uuid4().hex
    result = {
        "schema": 1, "run_id": spec.run_id, "invocation_id": invocation,
        "status": "not-started", "thread_id": None, "resumed_thread_id": spec.resume_thread_id,
        "returncode": None, "exit_code": None, "wall_seconds": 0, "elapsed_seconds": 0,
        "duration_ms": 0, "peak_rss_bytes": 0, "termination_reason": None,
        "stream_complete": False, "process_tree_drained": True, "usage_complete": False,
        "owned_processes": [], "ownership_complete": True,
        "usage_scope": spec.usage_scope, "usage_baseline": spec.usage_baseline,
        "cli_version": spec.cli_version, "model": spec.model, "effort": spec.effort,
        "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "stdout_path": None, "stderr_path": None, "answer_path": None,
        "stdout_bytes": 0, "stderr_bytes": 0,
        "stdout_sha256": None, "trace_sha256": None, "profile_sha256": None, "preflight": None,
    }
    plan = None
    artifacts = Path(spec.artifact_dir)
    phase = "serial-lease"
    try:
        with SerialLease() as lease:
            lease.guard(result)
            phase = "isolation-prepare"
            plan = prepare_isolation(spec)
            result["profile_sha256"] = plan.profile_sha256
            result["input_receipt"] = plan.input_receipt
            _write_json(artifacts / "isolation.json", plan.manifest)
            probe_stdout, probe_stderr = artifacts / "preflight.stdout.jsonl", artifacts / "preflight.stderr.txt"
            phase = "isolation-preflight"
            result.update(process_tree_drained=False, ownership_complete=False, owned_processes=[])
            control = _capture(executor, plan.probe_command(), spec, probe_stdout, probe_stderr, probe=True)
            result.update({name: control.get(name, default) for name, default in (
                ("process_tree_drained", False), ("ownership_complete", False), ("owned_processes", []),
            )})
            probe = None
            if probe_stdout.exists() and probe_stdout.stat().st_size <= 65536:
                try:
                    probe = json.loads(probe_stdout.read_bytes())
                except (ValueError, UnicodeDecodeError):
                    pass
            passed = (
                control["status"] == "completed" and control["returncode"] == 0
                and control["stream_complete"] and control["process_tree_drained"]
                and isinstance(probe, dict) and probe.get("passed") is True
                and isinstance(probe.get("checks"), dict) and set(probe["checks"]) == PROBE_CHECKS
                and all(value is True for value in probe["checks"].values())
            )
            receipt = {"schema": 1, "passed": passed, "profile_sha256": plan.profile_sha256,
                       "checks": probe.get("checks") if isinstance(probe, dict) else None,
                       "runtime_checks": probe.get("runtime_checks") if isinstance(probe, dict) else None,
                       "network_probe": probe.get("network_probe") if isinstance(probe, dict) else None,
                       "controller": control,
                       "stdout_sha256": _digest_file(probe_stdout) if probe_stdout.exists() else None}
            receipt["receipt_sha256"] = fingerprint_manifest(receipt)
            _write_json(artifacts / "preflight.json", receipt)
            result["preflight"] = receipt
            result["process_tree_drained"] = control["process_tree_drained"]
            if not passed:
                result["termination_reason"] = "isolation-preflight-failed"
                result["campaign_fatal"] = True
                result["failure_phase"] = phase
                return result
            if preflight_only:
                result["status"] = "preflight-passed"
                return result
            remaining = spec.timeout_seconds - (time.monotonic() - started)
            if remaining <= 0:
                result["status"] = "aborted"
                result["termination_reason"] = "timeout-before-model"
                return result
            stdout, stderr = artifacts / "stdout.jsonl", artifacts / "stderr.txt"
            phase = "model-execution"
            result["stdout_path"], result["stderr_path"] = str(stdout), str(stderr)
            result.update(process_tree_drained=False, ownership_complete=False, owned_processes=[])
            result.update(_capture(executor, plan.model_command(spec), spec, stdout, stderr,
                                   prompt=spec.prompt.encode(), timeout=remaining))
            result["exit_code"] = result["returncode"]
            result["wall_seconds"] = result["duration_ms"] / 1000
            result["elapsed_seconds"] = result["wall_seconds"]
            if stdout.exists():
                result["stdout_bytes"] = stdout.stat().st_size
                result["stdout_sha256"] = result["trace_sha256"] = _digest_file(stdout)
                result["thread_id"] = _thread_identity(stdout, spec.resume_thread_id)
            if stderr.exists():
                result["stderr_bytes"] = stderr.stat().st_size
            answer = artifacts / "answer.json"
            phase = "answer-validation"
            if answer.exists():
                info = answer.lstat()
                if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > 1024 * 1024:
                    result["status"], result["termination_reason"] = "failed", "invalid-answer-artifact"
                else:
                    result["answer_path"] = str(answer)
            if result["status"] == "completed" and (
                result["thread_id"] is None or result["answer_path"] is None
            ):
                result["status"], result["termination_reason"] = "failed", "missing-session-or-answer"
            return result
    except (IsolationError, OSError, ValueError, TypeError, KeyError) as error:
        result["status"] = "failed" if result["stdout_path"] else "not-started"
        result["termination_reason"] = "runner-error"
        result["error_type"] = type(error).__name__
        result["error"] = str(error)
        result["failure_phase"] = phase
        result["campaign_fatal"] = phase in ("serial-lease", "isolation-prepare", "isolation-preflight")
        return result
    finally:
        if plan is not None:
            if result.get("process_tree_drained") is True:
                plan.cleanup()
            else:
                result["retained_isolation_root"] = str(plan.root)
            lifecycle = artifacts / "lifecycle.json"
            if not lifecycle.exists():
                _write_json(lifecycle, result)
