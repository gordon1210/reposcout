from dataclasses import replace
import hashlib
import errno
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import uuid
import signal
import select
from types import SimpleNamespace

from codex_runner import (
    OwnedProcesses, SerialLease, _drain_owned_processes, _pidfd_terminal,
    _process_children, _record_alive, execute_process, run_trial,
)
from codex_isolation import IsolationError, PROBE_CHECKS, prepare_isolation
from test_codex_isolation import prepared_spec


class FakeExecutor:
    def __init__(self, *, probe_passes=True, model_status="completed", thread_id=None,
                 raise_model=False, truncated=False, model_drained=True, runtime_passes=True):
        self.probe_passes = probe_passes
        self.model_status = model_status
        self.thread_id = thread_id or str(uuid.uuid4())
        self.raise_model = raise_model
        self.truncated = truncated
        self.model_drained = model_drained
        self.runtime_passes = runtime_passes
        self.calls = []

    def __call__(self, command, **kwargs):
        self.calls.append((command, kwargs))
        probe = "/inputs/probe_launcher.py" in command
        stdout, stderr = Path(kwargs["stdout_path"]), Path(kwargs["stderr_path"])
        stderr.write_bytes(b"")
        if probe:
            stdout.write_text(json.dumps({
                "schema": 1, "passed": self.probe_passes and self.runtime_passes,
                "checks": {key: self.probe_passes and (self.runtime_passes or key != "required_runtimes")
                           for key in sorted(PROBE_CHECKS)},
                "runtime_checks": {"node": {"passed": self.runtime_passes,
                                             "returncode": 0 if self.runtime_passes else 127}},
            }) + "\n")
        else:
            if self.raise_model:
                raise OSError("synthetic execution failure")
            events = [
                {"type": "thread.started", "thread_id": self.thread_id},
                {"type": "turn.completed", "usage": {"input_tokens": 100, "output_tokens": 12,
                                                    "cached_input_tokens": 40}},
            ]
            stdout.write_text("".join(json.dumps(event) + "\n" for event in events))
            (stdout.parent / "answer.json").write_text('{"findings":[]}')
        status = "completed" if probe else self.model_status
        return {
            "status": status, "returncode": 0 if status == "completed" else -15,
            "duration_ms": 25, "peak_rss_bytes": 4096,
            "termination_reason": "output-limit" if self.truncated and not probe else None,
            "stream_complete": (not self.truncated and status == "completed") or probe,
            "process_tree_drained": True if probe else self.model_drained,
            "stdout_truncated": self.truncated and not probe, "stderr_truncated": False,
            "stdout_total_bytes": stdout.stat().st_size, "stderr_total_bytes": 0,
            "ownership_complete": True, "owned_processes": [],
        }


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.spec = prepared_spec(self.root)
        self.lease_path = self.root / "serial.lock"
        self.plans = []

        def prepare(spec):
            plan = prepare_isolation(spec)
            self.plans.append(plan)
            return plan

        self.lease_patch = mock.patch("codex_runner.SerialLease", side_effect=lambda: SerialLease(self.lease_path))
        self.plan_patch = mock.patch("codex_runner.prepare_isolation", side_effect=prepare)
        self.lease_patch.start()
        self.plan_patch.start()

    def tearDown(self):
        self.lease_patch.stop()
        self.plan_patch.stop()
        # Every executor in these tests is fake: retained resources have no live process.
        for plan in self.plans:
            plan.cleanup()
        self.temporary.cleanup()

    def test_preflight_failure_never_starts_a_model(self):
        fake = FakeExecutor(probe_passes=False)
        result = run_trial(self.spec, executor=fake)
        self.assertEqual(len(fake.calls), 1)
        self.assertEqual(result["status"], "not-started")
        self.assertEqual(result["termination_reason"], "isolation-preflight-failed")
        self.assertFalse(result["usage_complete"])
        self.assertIsNone(result["stdout_path"])
        self.assertFalse((self.spec.controller_dir / "isolation-canary").exists())

    def test_preflight_only_does_not_submit_prompt_or_consume_session(self):
        fake = FakeExecutor()
        result = run_trial(self.spec, preflight_only=True, executor=fake)
        self.assertEqual(len(fake.calls), 1)
        self.assertEqual(fake.calls[0][1]["input_bytes"], b"")
        self.assertEqual(result["status"], "preflight-passed")
        self.assertIsNone(result["thread_id"])

    def test_required_runtime_failure_is_setup_failure_before_any_model(self):
        fake = FakeExecutor()
        result = run_trial(replace(self.spec, required_runtimes=("node",)), executor=fake)
        self.assertEqual(fake.calls, [])
        self.assertEqual(result["status"], "not-started")
        self.assertTrue(result["campaign_fatal"])
        self.assertEqual(result["failure_phase"], "isolation-prepare")
        self.assertIsNone(result["stdout_path"])

    def test_runtime_probe_failure_is_retained_and_never_submits_prompt(self):
        fake = FakeExecutor(runtime_passes=False)
        spec = replace(self.spec, required_runtimes=("node",),
                       runtime_tool_paths={**self.spec.runtime_tool_paths, "node": str(self.spec.codex_binary)})
        result = run_trial(spec, executor=fake)
        self.assertEqual(len(fake.calls), 1)
        self.assertEqual(fake.calls[0][1]["input_bytes"], b"")
        self.assertEqual(result["status"], "not-started")
        self.assertTrue(result["campaign_fatal"])
        self.assertEqual(result["failure_phase"], "isolation-preflight")
        self.assertEqual(result["preflight"]["runtime_checks"]["node"], {"passed": False, "returncode": 127})
        self.assertIsNone(result["stdout_path"])

    def test_setup_and_preflight_consume_the_invocation_time_budget(self):
        fake = FakeExecutor()
        with mock.patch("codex_runner.time.monotonic", side_effect=(10.0, 50.0)):
            result = run_trial(replace(self.spec, timeout_seconds=30), executor=fake)
        self.assertEqual(len(fake.calls), 1)
        self.assertEqual(result["status"], "aborted")
        self.assertEqual(result["termination_reason"], "timeout-before-model")
        self.assertIsNone(result["stdout_path"])

    def test_completed_lifecycle_binds_exact_trace_and_prompt(self):
        fake = FakeExecutor()
        result = run_trial(self.spec, executor=fake)
        trace = Path(result["stdout_path"]).read_bytes()
        self.assertEqual(result["status"], "completed")
        self.assertEqual(result["stdout_sha256"], hashlib.sha256(trace).hexdigest())
        self.assertEqual(result["stdout_bytes"], len(trace))
        self.assertEqual(result["thread_id"], fake.thread_id)
        self.assertEqual(fake.calls[1][1]["input_bytes"], self.spec.prompt.encode())
        self.assertEqual(result["wall_seconds"], 0.025)
        self.assertFalse(result["usage_complete"])
        self.assertEqual(result["usage_scope"], "unknown")
        self.assertEqual(json.loads((self.spec.artifact_dir / "lifecycle.json").read_text()), result)
        self.assertTrue((self.spec.controller_dir / "auth.json").exists())
        self.assertFalse((self.spec.controller_dir / "isolation-canary").exists())

    def test_truncated_aborted_trace_remains_incomplete_and_available(self):
        fake = FakeExecutor(model_status="aborted", truncated=True)
        result = run_trial(self.spec, executor=fake)
        self.assertEqual(result["status"], "aborted")
        self.assertTrue(result["stdout_truncated"])
        self.assertFalse(result["stream_complete"])
        self.assertEqual(result["termination_reason"], "output-limit")
        self.assertTrue(Path(result["stdout_path"]).is_file())
        self.assertFalse(result["usage_complete"])

    def test_resume_uses_only_exact_session_and_preserves_controller(self):
        identity = str(uuid.uuid4())
        fake = FakeExecutor(thread_id=identity)
        result = run_trial(replace(self.spec, resume_thread_id=identity), executor=fake)
        command = fake.calls[1][0]
        self.assertIn("resume", command)
        self.assertEqual(command[command.index("resume") + 1:], [identity, "-"])
        self.assertNotIn("--last", command)
        self.assertNotIn("--ephemeral", command)
        self.assertEqual(result["resumed_thread_id"], identity)
        self.assertTrue((self.spec.controller_dir / "auth.json").is_file())

    def test_foreign_session_fails_without_discarding_raw_trace(self):
        fake = FakeExecutor()
        result = run_trial(replace(self.spec, resume_thread_id=str(uuid.uuid4())), executor=fake)
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_type"], "IsolationError")
        self.assertTrue(Path(result["stdout_path"]).is_file())
        self.assertFalse(result["usage_complete"])

    def test_unconfirmed_executor_exception_retains_runtime_and_serial_fence(self):
        fake = FakeExecutor(raise_model=True)
        result = run_trial(self.spec, executor=fake)
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_type"], "OSError")
        self.assertTrue((self.spec.workspace / "main.py").is_file())
        self.assertTrue((self.spec.controller_dir / "auth.json").is_file())
        self.assertFalse(result["process_tree_drained"])
        self.assertFalse(result["ownership_complete"])
        self.assertTrue((self.spec.controller_dir / "isolation-canary").exists())
        self.assertTrue(Path(result["retained_isolation_root"]).is_dir())
        self.assertTrue(result["serial_fence_written"])
        another = FakeExecutor()
        rejected = run_trial(self.spec, executor=another)
        self.assertEqual(rejected["status"], "not-started")
        self.assertEqual(another.calls, [])

    def test_failed_executor_with_verified_drain_cleans_runtime(self):
        result = run_trial(self.spec, executor=FakeExecutor(model_status="failed"))
        self.assertTrue(result["process_tree_drained"])
        self.assertNotIn("retained_isolation_root", result)
        self.assertFalse((self.spec.controller_dir / "isolation-canary").exists())
        self.assertEqual(self.lease_path.read_bytes(), b"")

    def test_concurrent_trial_refused_before_execution(self):
        fake = FakeExecutor()
        with SerialLease(self.lease_path):
            result = run_trial(self.spec, executor=fake)
        self.assertEqual(fake.calls, [])
        self.assertEqual(result["status"], "not-started")

    def test_serial_fence_blocks_live_identity_then_clears_after_verified_exit(self):
        result = {"process_tree_drained": False, "ownership_complete": True,
                  "owned_processes": [{"pid": 123, "start": 500}]}
        with SerialLease(self.lease_path) as lease:
            lease.guard(result)
        with mock.patch("codex_runner._record_alive", return_value=True):
            with self.assertRaisesRegex(IsolationError, "unconfirmed"):
                with SerialLease(self.lease_path):
                    self.fail("A live prior process must prevent another invocation")
        with mock.patch("codex_runner._record_alive", return_value=False):
            with SerialLease(self.lease_path):
                pass
        self.assertEqual(self.lease_path.read_bytes(), b"")

    def test_serial_fence_cannot_infer_exit_from_a_different_pid_namespace(self):
        result = {"process_tree_drained": False, "ownership_complete": True,
                  "owned_processes": [{"pid": 123, "start": 500}]}
        with mock.patch("codex_runner._pid_namespace", return_value="pid:[111]"):
            with SerialLease(self.lease_path) as lease:
                lease.guard(result)
        before = self.lease_path.read_bytes()
        with mock.patch("codex_runner._pid_namespace", return_value="pid:[222]"), \
                mock.patch("codex_runner._record_alive", return_value=False) as alive:
            with self.assertRaisesRegex(IsolationError, "unconfirmed"):
                with SerialLease(self.lease_path):
                    self.fail("A private proc view must not clear a host-scoped fence")
        alive.assert_not_called()
        self.assertEqual(self.lease_path.read_bytes(), before)

    def test_legacy_fence_without_namespace_identity_remains_unknown(self):
        self.lease_path.write_text(json.dumps({"ownership_complete": True,
                                              "owned_processes": [{"pid": 123, "start": 500}]}))
        with mock.patch("codex_runner._record_alive", return_value=False):
            with self.assertRaisesRegex(IsolationError, "unconfirmed"):
                with SerialLease(self.lease_path):
                    self.fail("A fence without observation scope cannot prove exit")

    def test_parent_can_pin_an_explicit_future_configuration(self):
        fake = FakeExecutor()
        configured = replace(self.spec, model="explicit-future-model", effort="high", timeout_seconds=40)
        result = run_trial(configured, preflight_only=True, executor=fake)
        self.assertEqual(result["status"], "preflight-passed")
        self.assertEqual(fake.calls[0][1]["timeout_seconds"], 30)

    def test_invalid_session_identity_does_not_launch_executor(self):
        fake = FakeExecutor()
        with self.assertRaises(ValueError):
            run_trial(replace(self.spec, resume_thread_id="../other-session"), executor=fake)
        self.assertEqual(fake.calls, [])

    def test_natural_namespace_teardown_receives_bounded_grace(self):
        class ExitedProcess:
            def poll(self):
                return 0

        class Teardown:
            def __init__(self, remaining):
                self.remaining = remaining

            def observe(self):
                self.remaining -= 1

            def living(self):
                return [42] if self.remaining > 0 else []

        now = [0.0]
        sleeps = []

        def sleep(duration):
            sleeps.append(duration)
            now[0] += duration

        self.assertTrue(_drain_owned_processes(
            ExitedProcess(), Teardown(3), clock=lambda: now[0], sleep=sleep,
        ))
        self.assertGreater(now[0], 0)
        self.assertFalse(_drain_owned_processes(
            ExitedProcess(), Teardown(1000), clock=lambda: now[0], sleep=sleep,
        ))
        self.assertLessEqual(max(sleeps), 0.01)
        self.assertLessEqual(now[0], 0.28)


class ProcessOwnershipTests(unittest.TestCase):
    def setUp(self):
        patcher = mock.patch("codex_runner._pidfd_terminal", return_value=False)
        self.terminal = patcher.start()
        self.addCleanup(patcher.stop)

    @staticmethod
    def identity(pid, start, ppid=1):
        return {"pid": pid, "ppid": ppid, "start": start, "state": "S", "rss": 4096}

    def test_pid_reuse_between_identity_read_and_pidfd_open_is_rejected(self):
        first, reused = self.identity(123, 100), self.identity(123, 200)
        with mock.patch("codex_runner._process_identity", side_effect=(first, reused)), \
                mock.patch("codex_runner.os.pidfd_open", return_value=91), \
                mock.patch("codex_runner.os.close") as close:
            owned = OwnedProcesses(123)
        self.assertFalse(owned.root_confirmed)
        self.assertEqual(owned.identities, {})
        close.assert_called_once_with(91)

    def test_mismatched_identity_is_never_signalled(self):
        owned = OwnedProcesses(123, observe=False)
        owned.identities[123] = self.identity(123, 100)
        owned.pidfds[123] = 91
        with mock.patch("codex_runner._process_identity", return_value=self.identity(123, 200)), \
                mock.patch("codex_runner.signal.pidfd_send_signal") as send, \
                mock.patch("codex_runner.os.killpg") as group:
            owned.signal(signal.SIGTERM)
        send.assert_not_called()
        group.assert_not_called()

    def test_candidate_from_reused_parent_is_not_claimed(self):
        owned = OwnedProcesses(123, observe=False)
        parent = self.identity(123, 100)
        owned.identities[123] = parent
        owned.pidfds[123] = 91
        identities = [parent, self.identity(456, 300, ppid=123), self.identity(123, 200)]
        with mock.patch("codex_runner._process_identity", side_effect=identities), \
                mock.patch("codex_runner.Path.readlink", return_value=Path("/usr/bin/bash")), \
                mock.patch("codex_runner._process_children", return_value=[456]), \
                mock.patch("codex_runner.os.pidfd_open") as opened:
            owned.observe()
        self.assertEqual(set(owned.identities), {123})
        opened.assert_not_called()

    def test_live_child_pidfd_failure_marks_supervision_incomplete(self):
        owned = OwnedProcesses(123, observe=False)
        parent, child = self.identity(123, 100), self.identity(456, 300, ppid=123)
        owned.identities[123] = parent
        owned.pidfds[123] = 91
        with mock.patch("codex_runner._process_identity", side_effect=lambda pid, **_: parent if pid == 123 else child), \
                mock.patch("codex_runner.Path.readlink", return_value=Path("/usr/bin/bash")), \
                mock.patch("codex_runner._process_children", return_value=[456]), \
                mock.patch("codex_runner.os.pidfd_open", side_effect=OSError(errno.EMFILE, "synthetic descriptor exhaustion")), \
                mock.patch("codex_runner.signal.pidfd_send_signal") as send:
            owned.observe()
            owned.signal(signal.SIGKILL)
        self.assertTrue(owned.tracking_failed)
        self.assertEqual(owned.untracked_identities, {456: child})
        self.assertIn({"pid": 456, "start": 300}, owned.records())
        send.assert_called_once_with(91, signal.SIGKILL)

    def test_live_owned_proc_read_failure_marks_supervision_incomplete(self):
        for target in ("codex_runner.Path.readlink", "codex_runner._process_children"):
            with self.subTest(target=target):
                owned = OwnedProcesses(123, observe=False)
                parent = self.identity(123, 100)
                owned.identities[123] = parent
                owned.pidfds[123] = 91
                with mock.patch("codex_runner._process_identity", return_value=parent), \
                        mock.patch("codex_runner.Path.readlink", return_value=Path("/usr/bin/bash")), \
                        mock.patch(target, side_effect=PermissionError(errno.EACCES, "unreadable owned proc")):
                    owned.observe()
                self.assertTrue(owned.tracking_failed)
                self.assertEqual(owned.untracked_identities, {123: parent})

    def test_unreadable_tracked_identity_is_not_treated_as_exit(self):
        owned = OwnedProcesses(123, observe=False)
        owned.identities[123] = self.identity(123, 100)
        owned.pidfds[123] = 91
        with mock.patch("codex_runner._process_identity", return_value=None), \
                mock.patch("codex_runner.Path.stat", return_value=SimpleNamespace()):
            owned.observe()
        self.assertTrue(owned.tracking_failed)
        self.assertEqual(owned.records(), [{"pid": 123, "start": 100}])

    def test_observation_diagnostics_bind_operation_identity_and_errno_without_error_text(self):
        owned = OwnedProcesses(123, observe=False)
        owned.identities[123] = self.identity(123, 100)
        owned.pidfds[123] = 91
        with mock.patch("codex_runner.Path.read_text", side_effect=PermissionError(errno.EACCES, "private path")), \
                mock.patch("codex_runner._record_alive", return_value=True):
            owned.observe()
        self.assertTrue(owned.tracking_failed)
        self.assertEqual(owned.observation_errors, [{
            "operation": "read-stat", "pid": 123, "start": 100, "tid": None,
            "errno": errno.EACCES, "error_type": "PermissionError",
        }])
        self.assertNotIn("private path", json.dumps(owned.observation_errors))

    def test_children_diagnostic_identifies_exact_task_and_operation(self):
        diagnostics = []
        with mock.patch("codex_runner.Path.iterdir", return_value=[Path("/proc/123/task/456")]), \
                mock.patch("codex_runner.Path.read_text", side_effect=PermissionError(errno.EACCES, "private path")):
            with self.assertRaises(PermissionError):
                _process_children(123, on_error=lambda operation, error, tid: diagnostics.append(
                    (operation, error.errno, tid),
                ))
        self.assertEqual(diagnostics, [("read-children", errno.EACCES, 456)])

    def test_observation_diagnostics_are_bounded_and_deduplicated(self):
        owned = OwnedProcesses(123, observe=False)
        for tid in range(70):
            owned._note_error("read-children", 123, PermissionError(errno.EACCES, "ignored"), tid=tid)
        owned._note_error("read-children", 123, PermissionError(errno.EACCES, "ignored"), tid=69)
        self.assertEqual(len(owned.observation_errors), 64)
        self.assertEqual(owned.observation_errors_dropped, 6)
        self.assertEqual(owned.observation_errors[0]["tid"], 6)
        self.assertEqual(owned.observation_errors[-1]["tid"], 69)

    def test_exited_identity_during_observation_does_not_create_incomplete_fence(self):
        owned = OwnedProcesses(123, observe=False)
        owned.identities[123] = self.identity(123, 100)
        owned.pidfds[123] = 91
        self.terminal.side_effect = (False, True)
        with mock.patch("codex_runner._process_identity", return_value=owned.identities[123]), \
                mock.patch("codex_runner.Path.readlink", side_effect=FileNotFoundError):
            owned.observe()
        self.assertFalse(owned.tracking_failed)
        self.assertEqual(owned.untracked_identities, {})

    def test_enoent_waits_only_for_confirmed_whole_process_exit(self):
        for terminal in (False, True):
            with self.subTest(terminal=terminal):
                owned = OwnedProcesses(123, observe=False)
                identity = self.identity(123, 100)
                owned.identities[123], owned.pidfds[123] = identity, 91
                self.terminal.reset_mock()
                self.terminal.side_effect = (False, terminal)
                owned._observation_failed(identity, error=FileNotFoundError(errno.ENOENT, "gone exe"))
                self.assertEqual(owned.tracking_failed, not terminal)
                self.assertEqual(owned.confirmed_exit_races, int(terminal))
                self.assertEqual(self.terminal.call_args_list, [mock.call(91), mock.call(91, wait_ms=10)])

    def test_live_permission_denial_and_missing_pidfd_never_receive_exit_grace(self):
        owned = OwnedProcesses(123, observe=False)
        identity = self.identity(123, 100)
        owned.identities[123], owned.pidfds[123] = identity, 91
        owned._observation_failed(identity, error=PermissionError(errno.EACCES, "denied"))
        self.assertTrue(owned.tracking_failed)
        self.terminal.assert_called_once_with(91)
        self.terminal.reset_mock()
        unknown = OwnedProcesses(456, observe=False)
        with mock.patch("codex_runner._record_alive", return_value=True):
            unknown._observation_failed(self.identity(456, 200), error=FileNotFoundError(errno.ENOENT, "gone exe"))
        self.assertTrue(unknown.tracking_failed)
        self.terminal.assert_not_called()

    def test_zombie_group_leader_with_nonterminal_pidfd_is_live_and_not_pruned(self):
        owned = OwnedProcesses(123, observe=False)
        identities = {123: self.identity(123, 100), 456: self.identity(456, 200, ppid=123)}
        identities[456]["state"] = "Z"
        owned.identities.update(identities)
        owned.pidfds.update({123: 91, 456: 92})

        def executable(path):
            if str(path) == "/proc/456/exe":
                raise FileNotFoundError(errno.ENOENT, "leader exited while workers remain")
            return Path("/usr/bin/bash")

        with mock.patch("codex_runner._process_identity", side_effect=lambda pid, **_: identities.get(pid)), \
                mock.patch("codex_runner.Path.readlink", new=executable), \
                mock.patch("codex_runner._process_children", return_value=[]), \
                mock.patch("codex_runner.os.close") as close:
            owned.observe()
            self.assertEqual(set(owned.living()), {123, 456})
            self.assertTrue(owned.tracking_failed)
            close.assert_not_called()
            self.terminal.side_effect = lambda fd, **_: fd == 92
            owned.observe()
            self.assertEqual(owned.living(), [123])
        close.assert_called_once_with(92)
        self.assertTrue(owned.tracking_failed)

    def test_stale_record_without_pidfd_does_not_treat_zombie_leader_as_drained(self):
        identity = {**self.identity(123, 100), "state": "Z"}
        with mock.patch("codex_runner._process_identity", return_value=identity):
            self.assertTrue(_record_alive({"pid": 123, "start": 100}))
            self.assertFalse(_record_alive({"pid": 123, "start": 99}))

    def test_pidfd_terminal_requires_positive_terminal_masks_and_rejects_errors(self):
        for mask in (select.POLLIN, select.POLLHUP, select.POLLIN | select.POLLHUP, 0,
                     select.POLLERR, select.POLLNVAL, select.POLLIN | select.POLLERR):
            with self.subTest(mask=mask), mock.patch("codex_runner.select.poll") as poll:
                poll.return_value.poll.return_value = [(91, mask)] if mask else []
                if mask & (select.POLLERR | select.POLLNVAL):
                    with self.assertRaises(OSError):
                        _pidfd_terminal(91, wait_ms=10)
                else:
                    self.assertEqual(_pidfd_terminal(91, wait_ms=10), bool(mask))
                poll.return_value.register.assert_called_once_with(91, select.POLLIN)
                poll.return_value.poll.assert_called_once_with(10)

    def test_pidfd_poll_error_never_proves_drainage(self):
        owned = OwnedProcesses(123, observe=False)
        owned.identities[123], owned.pidfds[123] = self.identity(123, 100), 91
        self.terminal.side_effect = OSError(errno.EBADF, "unobservable descriptor")
        self.assertEqual(owned.living(), [123])
        self.assertTrue(owned.tracking_failed)
        self.assertEqual(owned.observation_errors[0]["operation"], "poll-pidfd")

    def test_task_enumeration_error_propagates_but_exited_thread_is_ignored(self):
        with mock.patch("codex_runner.Path.iterdir", side_effect=PermissionError):
            with self.assertRaises(PermissionError):
                _process_children(123)
        with mock.patch("codex_runner.Path.iterdir", return_value=[Path("/proc/123/task/456")]), \
                mock.patch("codex_runner.Path.read_text", side_effect=FileNotFoundError), \
                mock.patch("codex_runner.Path.stat", side_effect=FileNotFoundError):
            self.assertEqual(_process_children(123), [])

    def test_reposcout_clock_tracks_birth_identity_and_ignores_reused_pid(self):
        now = [0.0]
        owned = OwnedProcesses(123, observe=False, clock=lambda: now[0])
        current = {123: self.identity(123, 100), 456: self.identity(456, 300, ppid=123)}
        owned.identities.update(current)
        owned.pidfds.update({123: 91, 456: 92})
        self.terminal.side_effect = lambda fd, **_: fd == 92 and current[456]["start"] != 300

        def executable(path):
            return Path("/opt/tools/reposcoutdev" if str(path) == "/proc/456/exe" else "/opt/codex/bin/codex")

        with mock.patch("codex_runner._process_identity", side_effect=lambda pid, **_: current.get(pid)), \
                mock.patch("codex_runner.Path.readlink", new=executable), \
                mock.patch("codex_runner._process_children", return_value=[]), \
                mock.patch("codex_runner.os.close") as close:
            self.assertEqual(owned.observe()[1], 1)
            now[0] = 179.999
            owned.observe()
            self.assertFalse(owned.reposcout_timed_out())
            now[0] = 180.001
            self.assertTrue(owned.reposcout_timed_out())
            current[456] = self.identity(456, 400, ppid=1)
            owned.observe()
            now[0] = 400.0
            self.assertFalse(owned.reposcout_timed_out())
            self.assertEqual(owned.scan_started, {})
        close.assert_called_once_with(92)

    def test_extended_controller_deadline_does_not_extend_reposcout_deadline(self):
        for scan_timed_out, expected_status in ((False, "completed"), (True, "aborted")):
            with self.subTest(scan_timed_out=scan_timed_out), tempfile.TemporaryDirectory() as directory:
                pipes = [mock.Mock() for _ in range(3)]
                for descriptor, pipe in enumerate(pipes, 100):
                    pipe.fileno.return_value = descriptor
                process = SimpleNamespace(pid=123, stdin=pipes[0], stdout=pipes[1], stderr=pipes[2])
                status = [None]
                process.poll = lambda: status[0]
                process.wait = lambda **_: status[0]
                registered = {}

                def register(pipe, _, name):
                    registered[pipe] = SimpleNamespace(fileobj=pipe, fd=pipe.fileno(), data=name)

                def select(_):
                    status[0] = 0
                    return [(key, 1) for key in list(registered.values())]

                selector = SimpleNamespace(register=register, unregister=registered.pop,
                                           select=select, get_map=lambda: registered, close=lambda: None)
                owned = mock.Mock()
                owned.root_confirmed = True
                owned.tracking_failed = False
                owned.observation_errors = []
                owned.observation_errors_dropped = 0
                owned.confirmed_exit_races = 0
                owned.observe.return_value = (512, int(scan_timed_out))
                owned.reposcout_timed_out.return_value = scan_timed_out
                owned.living.return_value = []
                owned.records.return_value = [{"pid": 123, "start": 100}]
                times = iter((0.0,))
                with mock.patch("codex_runner.time.monotonic", side_effect=lambda: next(times, 200.0)), \
                        mock.patch("codex_runner._memory_available", return_value=10 ** 12), \
                        mock.patch("codex_runner.subprocess.Popen", return_value=process), \
                        mock.patch("codex_runner.OwnedProcesses", return_value=owned), \
                        mock.patch("codex_runner.selectors.DefaultSelector", return_value=selector), \
                        mock.patch("codex_runner.os.set_blocking"), \
                        mock.patch("codex_runner.os.read", return_value=b""), \
                        mock.patch("codex_runner._drain_owned_processes", return_value=True):
                    result = execute_process(
                        ["never-executed"], input_bytes=b"", stdout_path=Path(directory) / "stdout",
                        stderr_path=Path(directory) / "stderr", timeout_seconds=600,
                        memory_limit_bytes=1024, host_memory_reserve_bytes=0, max_output_bytes=1024,
                    )
                self.assertEqual(result["status"], expected_status)
                self.assertEqual(result["termination_reason"], "reposcout-child-timeout" if scan_timed_out else None)
                if scan_timed_out:
                    owned.signal.assert_called_once_with(signal.SIGTERM)
                else:
                    owned.signal.assert_not_called()

    def test_executor_io_exception_returns_verified_cleanup_evidence(self):
        process = SimpleNamespace(pid=123, stdin=io.BytesIO(), stdout=io.BytesIO(), stderr=io.BytesIO())
        status = [None]
        process.poll = lambda: status[0]

        def wait(**_):
            status[0] = -9
            return -9

        process.wait = wait
        owned = mock.Mock()
        owned.root_confirmed = True
        owned.tracking_failed = False
        owned.observation_errors = [{"operation": "read-exe", "pid": 123, "start": 100,
                                     "tid": None, "errno": errno.ENOENT, "error_type": "FileNotFoundError"}]
        owned.observation_errors_dropped = 0
        owned.confirmed_exit_races = 0
        owned.observe.return_value = (4096, 0)
        owned.living.return_value = []
        owned.records.return_value = [{"pid": 123, "start": 100}]
        with mock.patch("codex_runner._memory_available", return_value=10 ** 12), \
                mock.patch("codex_runner.subprocess.Popen", return_value=process), \
                mock.patch("codex_runner.OwnedProcesses", return_value=owned), \
                mock.patch("codex_runner.selectors.DefaultSelector"), \
                mock.patch("codex_runner.Path.open", side_effect=OSError("synthetic capture failure")), \
                mock.patch("codex_runner._drain_owned_processes", return_value=True):
            result = execute_process(["never-executed"], input_bytes=b"", stdout_path=Path("unused.stdout"),
                                     stderr_path=Path("unused.stderr"), timeout_seconds=1,
                                     memory_limit_bytes=1024, host_memory_reserve_bytes=0, max_output_bytes=1024)
        self.assertEqual(result["status"], "failed")
        self.assertTrue(result["process_tree_drained"])
        self.assertTrue(result["ownership_complete"])
        self.assertEqual(result["owned_processes"], [{"pid": 123, "start": 100}])
        self.assertEqual(result["observation_errors"], owned.observation_errors)
        self.assertFalse(result["stream_complete"])
        owned.signal.assert_called_once_with(signal.SIGKILL)
        self.assertTrue(process.stdout.closed and process.stderr.closed and process.stdin.closed)

    def test_spawn_error_proves_no_process_was_started(self):
        with mock.patch("codex_runner._memory_available", return_value=10 ** 12), \
                mock.patch("codex_runner.subprocess.Popen", side_effect=OSError("synthetic spawn failure")):
            result = execute_process(["never-executed"], input_bytes=b"", stdout_path=Path("unused.stdout"),
                                     stderr_path=Path("unused.stderr"), timeout_seconds=1,
                                     memory_limit_bytes=1024, host_memory_reserve_bytes=0, max_output_bytes=1024)
        self.assertEqual(result["status"], "not-started")
        self.assertTrue(result["process_tree_drained"])
        self.assertEqual(result["owned_processes"], [])


if __name__ == "__main__":
    unittest.main()
