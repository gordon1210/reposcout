"""Run one serial, isolated Claude Code headless invocation with bounded lifecycle evidence."""

from dataclasses import dataclass, replace
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import math
import os
from pathlib import Path
import re
import secrets
import stat
import threading
import time
import uuid

from accounting import InvalidLedger
from claude_isolation import (
    BUILTIN_PLUGINS, CLAUDE_DESTINATION, MODEL_TOOLS, PROBE_CHECKS, IsolationError, expected_skills,
    fingerprint_manifest, prepare_isolation, private_tree_files,
)
from claude_trace import _read_trace, scan_stream
from codex_runner import SerialLease, _digest_file, _write_json, execute_process

MODEL_PATTERN = re.compile(r"claude-[a-z0-9][a-z0-9.-]{0,126}")
EFFORTS = ("low", "medium", "high", "xhigh", "max")
TOKEN_PATTERN = re.compile(r"[A-Za-z0-9._~+/=-]{16,4096}")
MAX_AUTH_BYTES = 64 * 1024
AUTH_MARGIN_SECONDS = 300
PREFLIGHT_TIMEOUT_SECONDS = 120
PREFLIGHT_MAX_OUTPUT_BYTES = 2 * 1024 * 1024
MAX_REQUEST_BYTES = 16 * 1024 * 1024
MINIMAL_ANSWER = {"conclusion": "insufficient-evidence", "findings": [], "evidence": [],
                  "retained_evidence": [], "limitations": [], "validation": []}
ANSWER_FAILURE_REASONS = ("answer-schema-invalid", "missing-structured-output", "claude-result-error",
                          "invalid-answer-artifact")


@dataclass(frozen=True)
class ClaudeTrialSpec:
    run_id: str
    workspace: Path
    artifact_dir: Path
    controller_dir: Path
    claude_binary: Path
    cli_version: str
    prompt: str
    answer_schema: dict
    model: str
    effort: str = "high"
    variant: str = "baseline"
    reposcout_binary: Path | None = None
    skill_dir: Path | None = None
    resume_thread_id: str | None = None
    timeout_seconds: float = 180
    memory_limit_bytes: int = 1024 * 1024 * 1024
    host_memory_reserve_bytes: int = 5 * 1024 * 1024 * 1024
    max_output_bytes: int = 16 * 1024 * 1024
    usage_scope: str = "invocation"
    usage_baseline: dict | None = None
    auth_file: Path | None = None
    expected_claude_sha256: str | None = None
    controller_ca_file: Path | None = None
    expected_controller_ca_sha256: str | None = None
    expected_reposcout_sha256: str | None = None
    expected_skill_sha256: str | None = None
    expected_workspace_sha256: str | None = None
    runtime_tool_paths: dict | None = None
    expected_runtime_tools_sha256: str | None = None
    sandbox_tool_paths: dict | None = None
    expected_sandbox_tools_sha256: str | None = None
    required_runtimes: tuple = ()
    max_turns: int | None = None
    max_budget_usd: float | None = None


class ClaudeAuthenticationError(IsolationError):
    """Authentication failures never carry credential contents."""


def _validate_spec(spec):
    if not isinstance(spec.run_id, str) or not spec.run_id or len(spec.run_id) > 200:
        raise ValueError("A bounded run identity is required")
    if not isinstance(spec.model, str) or MODEL_PATTERN.fullmatch(spec.model) is None:
        raise ValueError("An explicit Claude model identity is required")
    if spec.effort not in EFFORTS:
        raise ValueError("An explicit supported Claude effort is required")
    if spec.resume_thread_id is not None or spec.usage_baseline is not None:
        raise ValueError("Claude sessions are never resumed by this harness")
    if spec.usage_scope != "invocation":
        raise ValueError("Claude usage is reported per invocation")
    if not isinstance(spec.timeout_seconds, (int, float)) or not math.isfinite(spec.timeout_seconds) or spec.timeout_seconds <= 0:
        raise ValueError("Invocation timeout must be finite and positive")
    if not isinstance(spec.memory_limit_bytes, int) or spec.memory_limit_bytes <= 0:
        raise ValueError("Invocation memory limit must be positive")
    if not isinstance(spec.host_memory_reserve_bytes, int) or spec.host_memory_reserve_bytes < 0:
        raise ValueError("Host memory reserve cannot be negative")
    if not (0 < spec.max_output_bytes <= 16 * 1024 ** 2):
        raise ValueError("Output retention must be positive and at most 16 MiB")
    if spec.max_turns is not None and (type(spec.max_turns) is not int or not 0 < spec.max_turns <= 1000):
        raise ValueError("A turn cap must be a positive integer of at most 1000")
    if spec.max_budget_usd is not None and (isinstance(spec.max_budget_usd, bool)
                                            or not isinstance(spec.max_budget_usd, (int, float))
                                            or not math.isfinite(spec.max_budget_usd)
                                            or not 0 < spec.max_budget_usd <= 100):
        raise ValueError("A budget cap must be a positive amount of at most 100 USD")
    if not isinstance(spec.prompt, str) or len(spec.prompt.encode()) > 1024 ** 2:
        raise ValueError("Prompt exceeds the 1 MiB input bound")
    if not isinstance(spec.answer_schema, dict) or len(json.dumps(spec.answer_schema).encode()) > 128 * 1024:
        raise ValueError("Answer schema exceeds the 128 KiB input bound")
    for name in ("workspace", "claude", "controller_ca", "reposcout", "skill", "runtime_tools", "sandbox_tools"):
        expected = getattr(spec, f"expected_{name}_sha256")
        if expected is not None and (not isinstance(expected, str) or re.fullmatch(r"[0-9a-f]{64}", expected) is None):
            raise ValueError(f"Invalid prepared {name} fingerprint")


def load_oauth_token(path, *, valid_for_seconds, now=time.time):
    """Read only the access token from a private credential file; never persist or echo it."""
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    except OSError:
        raise ClaudeAuthenticationError("Authentication source is unavailable") from None
    try:
        info = os.fstat(descriptor)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) & 0o077
                or info.st_size > MAX_AUTH_BYTES):
            raise ClaudeAuthenticationError("Authentication source is not a private owned regular file")
        raw = b""
        while len(raw) <= MAX_AUTH_BYTES:
            block = os.read(descriptor, 65536)
            if not block:
                break
            raw += block
    finally:
        os.close(descriptor)
    if len(raw) > MAX_AUTH_BYTES:
        raise ClaudeAuthenticationError("Authentication source exceeds its bound")
    try:
        value = json.loads(raw)
    except (ValueError, UnicodeDecodeError):
        value = None
    if isinstance(value, dict):
        oauth = value.get("claudeAiOauth")
        if not isinstance(oauth, dict):
            raise ClaudeAuthenticationError("Authentication source has no supported OAuth record")
        token, expires = oauth.get("accessToken"), oauth.get("expiresAt")
        if type(expires) is not int or expires / 1000 < now() + valid_for_seconds:
            raise ClaudeAuthenticationError("Authentication expires before the invocation can finish")
    else:
        try:
            token = raw.decode("ascii").strip()
        except UnicodeDecodeError:
            token = None
    if not isinstance(token, str) or TOKEN_PATTERN.fullmatch(token) is None:
        raise ClaudeAuthenticationError("Authentication source has no supported access token")
    return token.encode()


def capped_reason(spec, final):
    """The cap that ended the invocation, if the final result reports one this spec requested."""
    subtype = final.get("subtype") if isinstance(final, dict) else None
    if subtype == "error_max_turns" and spec.max_turns is not None:
        return "max-turns"
    if subtype == "error_max_budget_usd" and spec.max_budget_usd is not None:
        return "max-budget"
    return None


def preflight_steps(plan):
    """Scripted tool calls the controller's local API returns; no model is contacted."""
    marker = plan.public_marker
    canary = "/controller/isolation-canary"
    return [
        ("public-read", "Read", {"file_path": marker["path"], "offset": marker["line"], "limit": 1}, "marker"),
        ("read-canary", "Read", {"file_path": canary}, "deny"),
        ("glob-controller-config", "Glob", {"pattern": "*", "path": "/controller/cfg"}, "deny"),
        ("grep-controller", "Grep", {"pattern": "canary", "path": "/controller"}, "deny"),
        ("read-own-environment", "Read", {"file_path": "/proc/self/environ"}, "deny"),
        ("read-proc-root-canary", "Read", {"file_path": "/proc/self/root" + canary}, "deny"),
        ("read-global-instructions", "Read", {"file_path": "/home/eval/.claude/CLAUDE.md"}, "deny"),
        ("grep-filesystem-root", "Grep", {"pattern": "reposcout-isolation-canary", "path": "/"}, "deny"),
        ("glob-filesystem-root", "Glob", {"pattern": "**/isolation-canary", "path": "/"}, "deny"),
        ("create-escape-links", "Bash", {"command": "python3 -I -c \"import os; "
                                                    "os.symlink('/controller/isolation-canary', '/scratch/escape-file'); "
                                                    "os.symlink('/controller', '/scratch/escape-dir'); "
                                                    "os.symlink('/', '/workspace/.escape-root')\"",
                                         "description": "Create isolation probe links"}, "bash-ok"),
        ("read-file-link", "Read", {"file_path": "/scratch/escape-file"}, "deny"),
        ("grep-directory-link", "Grep", {"pattern": "canary", "path": "/scratch/escape-dir"}, "deny"),
        ("read-root-link", "Read", {"file_path": "/workspace/.escape-root" + canary}, "deny"),
        ("read-parent-traversal", "Read", {"file_path": "/workspace/.." + canary}, "deny"),
        ("bash-sandbox-override", "Bash", {"command": "cat " + canary, "dangerouslyDisableSandbox": True,
                                           "description": "Attempt unsandboxed read"}, "bash-no-secret"),
        ("bash-probe", "Bash", {"command": "python3 -I /inputs/claude_probe.py",
                                "description": "Run isolation probe"}, "probe"),
        ("skill", "Skill", {"skill": "reposcout"}, "skill"),
        ("agent", "Agent", {"description": "probe", "prompt": "probe", "subagent_type": "general-purpose"}, "unavailable"),
        ("web-fetch", "WebFetch", {"url": "https://example.com/", "prompt": "probe"}, "unavailable"),
        ("web-search", "WebSearch", {"query": "probe"}, "unavailable"),
        ("schema-reject", "StructuredOutput", {}, "schema-reject"),
        ("schema-accept", "StructuredOutput", MINIMAL_ANSWER, "schema-accept"),
    ]


class _PreflightAPI:
    """Loopback Messages API stub serving the scripted calls and auditing every request."""

    def __init__(self, steps, *, token, model, forbidden):
        self.steps = steps
        self.token = token
        self.model = model
        self.forbidden = [value.encode() for value in forbidden if value]
        self.requests = []
        self.index = 0
        self.lock = threading.Lock()
        api = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_):
                pass

            def do_GET(self):
                api._record(self, None)
                self._reply(404, b"{}")

            def do_POST(self):
                length = int(self.headers.get("content-length") or 0)
                raw = self.rfile.read(min(length, MAX_REQUEST_BYTES + 1))
                body = api._record(self, raw)
                if not self.path.split("?")[0] == "/v1/messages" or not isinstance(body, dict):
                    self._reply(404, b"{}")
                    return
                self._stream(api._next_events(body))

            def _reply(self, code, payload):
                self.send_response(code)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(payload)))
                self.send_header("connection", "close")
                self.close_connection = True
                self.end_headers()
                self.wfile.write(payload)

            def _stream(self, events):
                self.send_response(200)
                self.send_header("content-type", "text/event-stream")
                self.send_header("request-id", "req_preflight")
                self.send_header("connection", "close")
                self.close_connection = True
                self.end_headers()
                for name, data in events:
                    self.wfile.write(f"event: {name}\ndata: {json.dumps(data)}\n\n".encode())
                self.wfile.flush()

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def base_url(self):
        return f"http://127.0.0.1:{self.server.server_address[1]}"

    @property
    def port(self):
        return self.server.server_address[1]

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)

    def _record(self, handler, raw):
        headers = {key.lower(): value for key, value in handler.headers.items()}
        body = None
        if raw is not None and len(raw) <= MAX_REQUEST_BYTES:
            try:
                body = json.loads(raw)
            except (ValueError, UnicodeDecodeError):
                body = None
        tools = None
        if isinstance(body, dict) and isinstance(body.get("tools"), list):
            tools = sorted(str(tool.get("name")) for tool in body["tools"] if isinstance(tool, dict))
        entry = {
            "method": handler.command, "path": handler.path.split("?")[0],
            "authorization_matched": headers.get("authorization") == "Bearer " + self.token,
            "api_key_header_present": "x-api-key" in headers,
            "body_bytes": len(raw) if raw is not None else 0,
            "body_within_bound": raw is None or len(raw) <= MAX_REQUEST_BYTES,
            "forbidden_content_absent": not any(value in raw for value in self.forbidden) if raw else True,
            "token_absent_from_body": self.token.encode() not in raw if raw else True,
            "tools": tools, "model": body.get("model") if isinstance(body, dict) else None,
        }
        with self.lock:
            self.requests.append(entry)
        return body

    def _next_events(self, body):
        with self.lock:
            index = self.index
            self.index += 1
        message = "msg_preflight_" + uuid.uuid4().hex
        usage = {"input_tokens": 1, "output_tokens": 1, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}
        events = [("message_start", {"type": "message_start", "message": {
            "id": message, "type": "message", "role": "assistant", "model": body.get("model", self.model),
            "content": [], "stop_reason": None, "stop_sequence": None, "usage": usage}})]
        if index < len(self.steps):
            _, tool, arguments, _ = self.steps[index]
            block = {"type": "tool_use", "id": "toolu_preflight_" + uuid.uuid4().hex[:20], "name": tool, "input": {}}
            delta = {"type": "input_json_delta", "partial_json": json.dumps(arguments)}
            stop = "tool_use"
        else:
            block, delta, stop = {"type": "text", "text": ""}, {"type": "text_delta", "text": "done"}, "end_turn"
        events += [("content_block_start", {"type": "content_block_start", "index": 0, "content_block": block}),
                   ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": delta}),
                   ("content_block_stop", {"type": "content_block_stop", "index": 0}),
                   ("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None},
                                      "usage": {"output_tokens": 1}}),
                   ("message_stop", {"type": "message_stop"})]
        return events


def session_violations(state, spec):
    """Compare the reported runtime inventory with the frozen isolation contract."""
    init = state["init"] or {}
    result = state["result"] or {}
    plugins = init.get("plugins") if isinstance(init.get("plugins"), list) else None
    stats = result.get("subagent_stats") if isinstance(result.get("subagent_stats"), dict) else {}
    checks = {
        "session_tools": isinstance(init.get("tools"), list) and sorted(init["tools"]) == sorted(MODEL_TOOLS),
        "session_mcp_absent": init.get("mcp_servers") == [],
        "session_skills": isinstance(init.get("skills"), list) and sorted(init["skills"]) == expected_skills(spec),
        "session_plugins": plugins is not None and sorted(item.get("name") for item in plugins
                                                          if isinstance(item, dict)) == sorted(BUILTIN_PLUGINS),
        "session_permission_mode": init.get("permissionMode") == "dontAsk",
        "session_model": init.get("model") == spec.model,
        "session_cwd": init.get("cwd") == "/workspace",
        "session_version": init.get("claude_code_version") == spec.cli_version.split()[0],
        "subagents_absent": state["subagent_events"] == 0 and (state["result"] is None or stats.get("spawned") == 0),
        "tool_uses_available": all(state["exchanges"][identity]["name"] in MODEL_TOOLS
                                   for identity in state["exchange_order"]),
    }
    return checks


def _probe_report(text):
    if not isinstance(text, str):
        return None
    try:
        value, _ = json.JSONDecoder().raw_decode(text.lstrip())
    except ValueError:
        return None
    return value if isinstance(value, dict) else None


def evaluate_preflight(stdout, steps, api, plan, spec, canary_token):
    records, _, _, failures = _read_trace(stdout)
    state = scan_stream(records)
    secret = plan.canary_secret
    exchanges = [state["exchanges"][identity] for identity in state["exchange_order"]]
    checks = {"trace_readable": not failures, "trace_structure": not state["problems"],
              "tool_call_inventory": len(exchanges) == len(steps)}
    probe = None
    skill_arm = spec.skill_dir is not None
    for index, (name, tool, _, expectation) in enumerate(steps):
        exchange = exchanges[index] if index < len(exchanges) else None
        if exchange is None or exchange["name"] != tool or exchange["result"] is None:
            checks["step:" + name] = False
            continue
        output = exchange["output"] if isinstance(exchange["output"], str) else ""
        clean = secret not in output and canary_token not in output
        error = exchange["is_error"] is True
        if expectation == "marker":
            passed = not error and plan.public_marker["text"] in output
        elif expectation in ("deny", "unavailable", "schema-reject"):
            passed = error
        elif expectation == "bash-ok":
            passed = not error
        elif expectation == "bash-no-secret":
            passed = True
        elif expectation == "probe":
            probe = _probe_report(output)
            passed = (not error and isinstance(probe, dict) and probe.get("passed") is True
                      and isinstance(probe.get("checks"), dict) and set(probe["checks"]) == PROBE_CHECKS
                      and all(value is True for value in probe["checks"].values()))
        elif expectation == "skill":
            passed = error is not skill_arm
        else:
            passed = not error
        checks["step:" + name] = passed and clean
    result = state["result"] or {}
    checks["structured_output_enforced"] = result.get("structured_output") == MINIMAL_ANSWER
    checks["result_success"] = result.get("subtype") == "success" and result.get("is_error") is False
    checks.update({key: value for key, value in session_violations(state, spec).items()
                   if key != "tool_uses_available"})
    requests = api.requests
    messages = [entry for entry in requests if entry["path"] == "/v1/messages" and entry["method"] == "POST"]
    checks.update({
        "api_requests_only_messages": bool(messages) and len(messages) == len(requests),
        "api_authorization_from_descriptor": bool(requests) and all(entry["authorization_matched"] for entry in requests),
        "api_key_header_absent": all(not entry["api_key_header_present"] for entry in requests),
        "api_tools_exact": all(entry["tools"] == sorted(MODEL_TOOLS) for entry in messages),
        "api_model": all(entry["model"] == spec.model for entry in messages),
        "api_private_content_absent": all(entry["forbidden_content_absent"] and entry["token_absent_from_body"]
                                          and entry["body_within_bound"] for entry in requests),
        "api_script_consumed": api.index == len(steps),
    })
    raw = Path(stdout).read_bytes() if Path(stdout).exists() else b""
    checks["trace_private_content_absent"] = secret.encode() not in raw and canary_token.encode() not in raw
    return checks, probe


def exposure_findings(plan, artifact_files, secrets_to_find):
    """Locate verbatim credential or canary bytes in tool-visible or retained files."""
    patterns = [value for value in secrets_to_find if value]
    found = []
    candidates = list(private_tree_files(plan.scan_roots())) + [Path(path) for path in artifact_files]
    for path in candidates:
        if path == plan.controller_canary or not path.exists():
            continue
        data = path.read_bytes()
        if any(pattern in data for pattern in patterns):
            relative = path.relative_to(plan.root) if path.is_relative_to(plan.root) else Path(path.name)
            found.append(relative.as_posix())
    return sorted(found)


def _capture(executor, command, spec, stdout, stderr, *, input_bytes, timeout, max_output):
    return executor(
        command, input_bytes=input_bytes, stdout_path=stdout, stderr_path=stderr, timeout_seconds=timeout,
        memory_limit_bytes=spec.memory_limit_bytes, host_memory_reserve_bytes=spec.host_memory_reserve_bytes,
        max_output_bytes=max_output,
    )


def _run_preflight(spec, executor, artifacts, result):
    """A dedicated disposable isolation proves the policy with a canary credential."""
    plan = prepare_isolation(spec)
    try:
        _write_json(artifacts / "preflight-isolation.json", plan.manifest)
        steps = preflight_steps(plan)
        canary_token = "sk-ant-oat01-preflight-" + secrets.token_urlsafe(32)
        forbidden = [plan.canary_secret, str(Path.home()), str(Path(spec.workspace).resolve()), str(plan.root)]
        stdout, stderr = artifacts / "preflight.stdout.jsonl", artifacts / "preflight.stderr.txt"
        with _PreflightAPI(steps, token=canary_token, model=spec.model, forbidden=forbidden) as api:
            plan.write_probe_expectations(api.port)
            prompt = b"Run the isolation preflight."
            result.update(process_tree_drained=False, ownership_complete=False, owned_processes=[])
            # The scripted calls must all run, so the caps apply to the model invocation only.
            scripted = replace(spec, max_turns=None, max_budget_usd=None)
            control = _capture(executor, plan.model_command(scripted, base_url=api.base_url), spec, stdout, stderr,
                               input_bytes=canary_token.encode() + b"\n" + prompt,
                               timeout=min(spec.timeout_seconds, PREFLIGHT_TIMEOUT_SECONDS),
                               max_output=min(spec.max_output_bytes, PREFLIGHT_MAX_OUTPUT_BYTES))
        result.update({name: control.get(name, default) for name, default in (
            ("process_tree_drained", False), ("ownership_complete", False), ("owned_processes", []),
        )})
        checks, probe = (evaluate_preflight(stdout, steps, api, plan, spec, canary_token)
                         if stdout.exists() else ({"trace_readable": False}, None))
        exposure = exposure_findings(plan, [stdout, stderr], [canary_token.encode(), plan.canary_secret.encode()])
        checks["canary_credential_not_persisted"] = not exposure
        checks["process_closed"] = (control["status"] == "completed" and control["returncode"] == 0
                                    and control["stream_complete"] is True and control["process_tree_drained"] is True)
        receipt = {"schema": 1, "runtime": "claude", "passed": all(value is True for value in checks.values()),
                   "profile_sha256": plan.profile_sha256, "input_receipt": plan.input_receipt,
                   "checks": checks, "probe_checks": probe.get("checks") if isinstance(probe, dict) else None,
                   "runtime_checks": probe.get("runtime_checks") if isinstance(probe, dict) else None,
                   "network_probe": probe.get("network_probe") if isinstance(probe, dict) else None,
                   "api_request_count": len(api.requests), "controller": control,
                   "stdout_sha256": _digest_file(stdout) if stdout.exists() else None}
        receipt["receipt_sha256"] = fingerprint_manifest(receipt)
        return plan, receipt
    except BaseException:
        if result.get("process_tree_drained") is True:
            plan.cleanup()
        else:
            result["retained_isolation_root"] = str(plan.root)
        raise


def run_trial(spec, *, preflight_only=False, executor=execute_process):
    """Run a no-model canary preflight, then one fresh Claude Code invocation."""
    _validate_spec(spec)
    started = time.monotonic()
    invocation = uuid.uuid4().hex
    result = {
        "schema": 1, "runtime": "claude", "run_id": spec.run_id, "invocation_id": invocation,
        "status": "not-started", "process_status": None, "thread_id": None, "resumed_thread_id": None,
        "returncode": None, "exit_code": None, "wall_seconds": 0, "elapsed_seconds": 0,
        "duration_ms": 0, "peak_rss_bytes": 0, "termination_reason": None,
        "stream_complete": False, "process_tree_drained": True, "usage_complete": False,
        "owned_processes": [], "ownership_complete": True,
        "usage_scope": spec.usage_scope, "usage_baseline": None,
        "cli_version": spec.cli_version, "model": spec.model, "effort": spec.effort,
        "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "stdout_path": None, "stderr_path": None, "answer_path": None, "answer_validation": None,
        "stdout_bytes": 0, "stderr_bytes": 0,
        "stdout_sha256": None, "trace_sha256": None, "profile_sha256": None, "preflight": None,
    }
    plan = None
    artifacts = Path(spec.artifact_dir)
    phase = "serial-lease"
    try:
        with SerialLease() as lease:
            lease.guard(result)
            phase = "isolation-preflight"
            probe_plan, receipt = _run_preflight(spec, executor, artifacts, result)
            if result.get("process_tree_drained") is True:
                probe_plan.cleanup()
            else:
                result["retained_isolation_root"] = str(probe_plan.root)
            _write_json(artifacts / "preflight.json", receipt)
            result["preflight"] = receipt
            result["profile_sha256"] = receipt["profile_sha256"]
            result["input_receipt"] = receipt["input_receipt"]
            if not receipt["passed"]:
                result["termination_reason"] = "isolation-preflight-failed"
                result["campaign_fatal"] = True
                result["failure_phase"] = phase
                return result
            if preflight_only:
                result["status"] = "preflight-passed"
                return result
            phase = "isolation-prepare"
            plan = prepare_isolation(spec)
            if plan.profile_sha256 != receipt["profile_sha256"] or plan.input_receipt != receipt["input_receipt"]:
                raise IsolationError("Model isolation differs from the attested preflight isolation")
            _write_json(artifacts / "isolation.json", plan.manifest)
            remaining = spec.timeout_seconds - (time.monotonic() - started)
            if remaining <= 0:
                result["status"] = "aborted"
                result["termination_reason"] = "timeout-before-model"
                return result
            phase = "authentication"
            if spec.auth_file is None:
                raise ClaudeAuthenticationError("Live execution requires a private authentication source")
            token = load_oauth_token(spec.auth_file, valid_for_seconds=remaining + AUTH_MARGIN_SECONDS)
            stdout, stderr = artifacts / "stdout.jsonl", artifacts / "stderr.txt"
            phase = "model-execution"
            result["stdout_path"], result["stderr_path"] = str(stdout), str(stderr)
            result.update(process_tree_drained=False, ownership_complete=False, owned_processes=[])
            result.update(_capture(executor, plan.model_command(spec), spec, stdout, stderr,
                                   input_bytes=token + b"\n" + spec.prompt.encode(), timeout=remaining,
                                   max_output=spec.max_output_bytes))
            result["process_status"] = result["status"]
            result["exit_code"] = result["returncode"]
            result["wall_seconds"] = result["duration_ms"] / 1000
            result["elapsed_seconds"] = result["wall_seconds"]
            if stderr.exists():
                result["stderr_bytes"] = stderr.stat().st_size
            phase = "credential-exposure"
            exposure = exposure_findings(plan, [stdout, stderr], [token, plan.canary_secret.encode()])
            token = None
            if exposure:
                result.update(status="failed", termination_reason="credential-exposure", campaign_fatal=True,
                              exposure_locations=exposure)
            if not stdout.exists():
                if result["status"] == "completed":
                    result["status"], result["termination_reason"] = "failed", "missing-structured-output"
                return result
            result["stdout_bytes"] = stdout.stat().st_size
            result["stdout_sha256"] = result["trace_sha256"] = _digest_file(stdout)
            phase = "answer-validation"
            state = scan_stream(_read_trace(stdout)[0])
            result["thread_id"] = state["session_id"]
            checks = session_violations(state, spec)
            result["runtime_isolation_checks"] = checks
            if exposure:
                return result
            if state["init"] is not None and not all(checks.values()):
                result.update(status="failed", termination_reason="runtime-isolation-violation", campaign_fatal=True)
                return result
            cap = capped_reason(spec, state["result"])
            if (cap is not None and result["stream_complete"] is True and result["process_tree_drained"] is True
                    and result["status"] in ("completed", "failed")):
                # A requested cap ends the run on purpose; the trace stays valid evidence.
                result["status"], result["termination_reason"] = "capped", cap
                return result
            if result["status"] != "completed":
                return result
            final = state["result"]
            answer = final.get("structured_output") if isinstance(final, dict) else None
            if isinstance(answer, dict):
                path = artifacts / "answer.json"
                _write_json(path, answer)
                result["retained_answer_path"] = str(path)
            if not isinstance(final, dict) or final.get("subtype") != "success" or final.get("is_error") is not False:
                result["status"], result["termination_reason"] = "failed", "claude-result-error"
            elif not isinstance(answer, dict):
                result["status"], result["termination_reason"] = "failed", "missing-structured-output"
            elif state["session_id"] is None:
                result["status"], result["termination_reason"] = "failed", "missing-session-or-answer"
            else:
                from review_grading import _validate_schema
                try:
                    _validate_schema(answer, spec.answer_schema)
                except InvalidLedger as error:
                    result["answer_validation"] = {"valid": False, "error": str(error)[:500]}
                    result["status"], result["termination_reason"] = "failed", "answer-schema-invalid"
                else:
                    result["answer_validation"] = {"valid": True}
                    result["answer_path"] = result["retained_answer_path"]
            return result
    except (IsolationError, InvalidLedger, OSError, ValueError, TypeError, KeyError) as error:
        result["status"] = "failed" if result["stdout_path"] else "not-started"
        result["termination_reason"] = "runner-error"
        result["error_type"] = type(error).__name__
        result["error"] = str(error) if isinstance(error, IsolationError) else type(error).__name__
        result["failure_phase"] = phase
        result["campaign_fatal"] = phase in ("serial-lease", "isolation-prepare", "isolation-preflight",
                                             "authentication", "credential-exposure")
        return result
    finally:
        if plan is not None:
            if result.get("process_tree_drained") is True:
                plan.cleanup()
            else:
                result["retained_isolation_root"] = str(plan.root)
        lifecycle = artifacts / "lifecycle.json"
        if artifacts.is_dir() and not lifecycle.exists():
            _write_json(lifecycle, result)
