"""Prepare a disposable filesystem and fail-closed Claude Code tool permission profile."""

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import stat
import subprocess
import tempfile

from codex_isolation import (
    REPOSCOUT_CHILD_TIMEOUT_SECONDS, REQUIRED_RUNTIME_TOOLS, RUNTIME_PROBES, RUNTIME_TOOLS, SKILL_VARIANTS, VARIANTS,
    IsolationError, RuntimeUnavailableError, _controller_ca_source, _inside, _private_directory,
    _regular_file, _stage_runtime_file, _verify_copy, directory_sha256, fingerprint_manifest,
    runtime_tool_manifest, validate_public_tree,
)

ISOLATION_VERSION = 1
CLAUDE_TOOLS = ("Bash", "Glob", "Grep", "Read", "Skill")
MODEL_TOOLS = (*CLAUDE_TOOLS, "StructuredOutput")
BUILTIN_SKILLS = ("doctor", "plugin-authoring")
BUILTIN_PLUGINS = ("cc-plugin-agents-md", "cc-plugin-plugin-authoring")
SANDBOX_TOOLS = ("bwrap", "socat")
# Claude's Bash sandbox mounts write-protected placeholders for these names in its working directory.
SANDBOX_PLACEHOLDERS = (".bash_profile", ".bashrc", ".gitconfig", ".gitmodules", ".idea", ".mcp.json", ".profile",
                        ".ripgreprc", ".vscode", ".zprofile", ".zshrc")
CLAUDE_DESTINATION = "/opt/claude/bin/claude"
TOKEN_DESCRIPTOR = 3
MAX_CLAUDE_BINARY_BYTES = 512 * 1024 * 1024
FORBIDDEN_FIXTURE_ENTRIES = (".claude", ".agents", ".mcp.json", ".codex", "CLAUDE.local.md")
CLAUDE_ENVIRONMENT = {
    "PATH": "/opt/tools:/usr/bin:/bin",
    "HOME": "/home/eval",
    "SHELL": "/usr/bin/bash",
    "LANG": "C.UTF-8",
    "LC_ALL": "C.UTF-8",
    "TERM": "dumb",
    "TMPDIR": "/tmp",
    "XDG_CACHE_HOME": "/scratch/cache",
    "XDG_CONFIG_HOME": "/home/eval/.config",
    "REPOSCOUT_GLOBAL_CONFIG": "/inputs/reposcout-global.toml",
    "PYTHONDONTWRITEBYTECODE": "1",
    "CLAUDE_CONFIG_DIR": "/controller/cfg",
    "CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR": str(TOKEN_DESCRIPTOR),
    "DISABLE_AUTOUPDATER": "1",
    "DISABLE_TELEMETRY": "1",
    "DISABLE_ERROR_REPORTING": "1",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
    "CLAUDE_CODE_DISABLE_AUTO_MEMORY": "1",
    "CLAUDE_CODE_DISABLE_BUNDLED_SKILLS": "1",
}
# The launcher reads one token line from stdin, exposes it only through an
# inherited pipe descriptor, and leaves the remaining stdin as Claude's prompt.
LAUNCHER_SOURCE = r'''
import json, os, sys
command = json.loads(sys.argv[1])
token = bytearray()
byte = b""
while len(token) <= 16384:
    byte = os.read(0, 1)
    if not byte or byte == b"\n":
        break
    token += byte
if byte != b"\n" or not token:
    sys.exit(97)
read, write = os.pipe()
os.write(write, bytes(token))
os.close(write)
if read != 3:
    os.dup2(read, 3)
    os.close(read)
os.set_inheritable(3, True)
os.execv(command[0], command)
'''


def claude_settings():
    """Managed by the controller and passed explicitly; no user or local settings are loaded."""
    return {
        "permissions": {
            "allow": list(CLAUDE_TOOLS),
            "deny": ["Read(//controller/**)", "Read(//proc/**)", "Read(//home/**)"],
            "defaultMode": "dontAsk",
            "disableBypassPermissionsMode": "disable",
            "blockReadsOutsideWorkingDirectories": True,
            "additionalDirectories": ["/scratch"],
        },
        "sandbox": {
            "enabled": True,
            "failIfUnavailable": True,
            "allowUnsandboxedCommands": False,
            "autoAllowBashIfSandboxed": True,
            "network": {"allowedDomains": [], "strictAllowlist": True},
            "filesystem": {"denyRead": ["/controller", "/tmp/cc-socks"], "allowWrite": ["/scratch"],
                           "denyWrite": ["/workspace/.claude"]},
        },
        "disableAllHooks": True,
    }


def claude_arguments(spec):
    """Fixed headless flags; the prompt arrives on stdin after the launcher consumes the token line."""
    return [
        CLAUDE_DESTINATION, "-p", "--output-format", "stream-json", "--verbose",
        "--model", spec.model, "--effort", spec.effort,
        "--tools", ",".join(CLAUDE_TOOLS), "--allowedTools", ",".join(CLAUDE_TOOLS),
        "--no-session-persistence", "--strict-mcp-config", "--setting-sources", "project",
        "--settings", "/inputs/settings.json", "--add-dir", "/scratch",
        "--json-schema", json.dumps(spec.answer_schema, sort_keys=True, separators=(",", ":")),
    ]


def expected_skills(spec):
    return sorted((*BUILTIN_SKILLS, *(("reposcout",) if spec.skill_dir is not None else ())))


def configuration_manifest(spec):
    """Reproducible, nonsecret settings; source and executable hashes are campaign facts."""
    arguments = claude_arguments(spec)
    return {
        "schema": 1,
        "runtime": "claude",
        "isolation_version": ISOLATION_VERSION,
        "variant": spec.variant,
        "cli_version": spec.cli_version,
        "model": spec.model,
        "effort": spec.effort,
        "claude_arguments_sha256": fingerprint_manifest(arguments),
        "answer_schema_sha256": fingerprint_manifest(spec.answer_schema),
        "settings_sha256": fingerprint_manifest(claude_settings()),
        "tools": list(CLAUDE_TOOLS),
        "model_tools": list(MODEL_TOOLS),
        "expected_skills": expected_skills(spec),
        "claude_environment": CLAUDE_ENVIRONMENT,
        "setting_sources": ["project"],
        "outer_policy": "private-mount-pid-user-ipc-uts-namespace;controller-network-only",
        "tool_policy": "nested-claude-bash-sandbox;private-tool-network-and-pid-namespace;"
                       "empty-proxy-allowlist;file-tools-confined-to-workspace-and-scratch",
        "authentication": "controller-read-oauth-access-token;inherited-pipe-descriptor;never-staged",
        "public_workspace": "/workspace",
        "workspace_writable": True,
        "private_paths": ["/controller", "/proc"],
        "runtime_tools": list(RUNTIME_TOOLS),
        "sandbox_tools": list(SANDBOX_TOOLS),
        "expected_inputs": {
            name: getattr(spec, f"expected_{name}_sha256")
            for name in ("workspace", "claude", "controller_ca", "reposcout", "skill", "runtime_tools", "sandbox_tools")
        },
        "required_runtimes": list(spec.required_runtimes),
        "controller_ca_destination": "/etc/ssl/cert.pem",
        "limits": {
            "timeout_seconds": spec.timeout_seconds,
            "memory_limit_bytes": spec.memory_limit_bytes,
            "host_memory_reserve_bytes": spec.host_memory_reserve_bytes,
            "max_output_bytes": spec.max_output_bytes,
            "reposcout_child_timeout_seconds": REPOSCOUT_CHILD_TIMEOUT_SECONDS,
        },
    }


def claude_binary(path):
    binary = _regular_file(Path(path).resolve(strict=True))
    if not os.access(binary, os.X_OK) or not 0 < binary.stat().st_size <= MAX_CLAUDE_BINARY_BYTES:
        raise IsolationError("Claude binary is not a bounded executable")
    return binary


def claude_version(path):
    """Read the version with a throwaway home and configuration, never the user's."""
    binary = claude_binary(path)
    with tempfile.TemporaryDirectory(prefix="reposcout-claude-version-") as home:
        environment = {"PATH": "/usr/bin:/bin", "HOME": home, "CLAUDE_CONFIG_DIR": home,
                       "DISABLE_AUTOUPDATER": "1", "DISABLE_TELEMETRY": "1",
                       "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"}
        result = subprocess.run([str(binary), "--version"], env=environment, stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=30, check=True, cwd=home)
    if not 0 < len(result.stdout) <= 4096:
        raise IsolationError("Invalid Claude version output")
    return result.stdout.strip()


def sandbox_tool_manifest(paths=None):
    """Pin the executables Claude's Bash sandbox launches inside the outer namespace."""
    if paths is not None and (not isinstance(paths, dict) or set(paths) - set(SANDBOX_TOOLS)):
        raise IsolationError("Unknown sandbox tool selection")
    result = {}
    for name in SANDBOX_TOOLS:
        if paths is None:
            system = Path("/usr/bin") / name
            selected = str(system) if system.is_file() else shutil.which(name)
        else:
            selected = paths.get(name)
        if selected is None:
            raise RuntimeUnavailableError(name)
        source = Path(selected)
        if not source.is_absolute():
            raise IsolationError("Sandbox tool paths must be absolute")
        source = _regular_file(source.resolve(strict=True))
        if not os.access(source, os.X_OK):
            raise IsolationError(f"Sandbox tool is not executable: {name}")
        result[name] = {"path": str(source), "sha256": _file_sha256(source), "destination": f"/usr/bin/{name}"}
    return result


def _file_sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _validate_spec(spec):
    if spec.variant not in VARIANTS:
        raise IsolationError("Unknown evaluation variant")
    if spec.variant == "baseline" and (spec.reposcout_binary is not None or spec.skill_dir is not None):
        raise IsolationError("Baseline cannot receive RepoScout or its skill")
    if spec.variant != "baseline" and spec.reposcout_binary is None:
        raise IsolationError("RepoScout arm requires a pinned binary")
    if (spec.variant in SKILL_VARIANTS) != (spec.skill_dir is not None):
        raise IsolationError("Only the skill arm may receive the RepoScout skill")
    workspace = Path(spec.workspace).resolve(strict=True)
    validate_public_tree(workspace)
    for relative in FORBIDDEN_FIXTURE_ENTRIES:
        if (workspace / relative).exists() or (workspace / relative).is_symlink():
            raise IsolationError(f"Prepared fixture contains unsupported runtime configuration: {relative}")
    controller = _private_directory(spec.controller_dir)
    artifacts = Path(spec.artifact_dir).resolve()
    if any(_inside(private, workspace) or _inside(workspace, private) for private in (controller, artifacts)):
        raise IsolationError("Public workspace and private controller/artifacts must be disjoint")
    if _inside(artifacts, controller) or _inside(controller, artifacts):
        raise IsolationError("Controller and artifact directories must be disjoint")
    binary = claude_binary(spec.claude_binary)
    if any(_inside(binary, private) for private in (controller, artifacts, workspace)):
        raise IsolationError("Claude runtime cannot overlap controller, artifacts or fixture")
    return workspace, controller, artifacts, binary


PROBE_CHECKS = frozenset((
    "public_read", "denied:/controller/isolation-canary", "denied:/controller/cfg",
    "denied:/proc/1/root/controller/isolation-canary", "protected:/proc/1/environ",
    "claude_process_invisible", "no_inherited_descriptors", "credential_environment_absent",
    "messaging_socket_denied", "host_home_absent", "host_workspace_absent", "global_config_absent",
    "skill_inventory", "skill_configuration_protected", "tool_inventory", "scratch_writable",
    "network_denied", "proxy_denied", "controller_port_unreachable", "required_runtimes", "workspace_git_clean",
))


PROBE_SOURCE = r'''
import errno, json, os, pathlib, socket, subprocess, sys, urllib.request
inherited = []
for descriptor in range(3, 256):
    try:
        os.fstat(descriptor)
    except OSError:
        continue
    inherited.append(descriptor)
expected = json.loads(pathlib.Path("/inputs/probe-expectations.json").read_text())
checks = {"no_inherited_descriptors": not inherited}
checks["public_read"] = pathlib.Path("/workspace").is_dir() and pathlib.Path("/inputs/review.schema.json").is_file()
def readable(path):
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_CLOEXEC)
    except OSError:
        return b""
    try:
        return os.read(descriptor, 1024 * 1024)
    except OSError:
        return b""
    finally:
        os.close(descriptor)
for path in ("/controller/isolation-canary", "/proc/1/root/controller/isolation-canary"):
    checks["denied:" + path] = not readable(path)
environment = readable("/proc/1/environ")
checks["protected:/proc/1/environ"] = b"sk-ant-" not in environment and b"reposcout-isolation-canary-" not in environment
try:
    checks["denied:/controller/cfg"] = not os.listdir("/controller/cfg")
except OSError:
    checks["denied:/controller/cfg"] = True
try:
    visible = [path for path in pathlib.Path("/proc").iterdir() if path.name.isdecimal()]
except OSError:
    visible = []
claude_visible = False
for process in visible:
    try:
        claude_visible |= os.readlink(process / "exe") == "/opt/claude/bin/claude"
    except OSError:
        pass
checks["claude_process_invisible"] = len(visible) <= 64 and not claude_visible
checks["credential_environment_absent"] = not any(
    ("OAUTH" in key.upper() and (key, value) != ("CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR", "3"))
    or key.upper() in ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN") or "sk-ant-" in value
    or "reposcout-isolation-canary-" in value
    for key, value in os.environ.items()
)
socket_path = os.environ.get("CLAUDE_CODE_MESSAGING_SOCKET")
messaging_denied = True
unix_socket_errno = None
try:
    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
except OSError as error:
    unix_socket_errno = error.errno
else:
    connection.settimeout(1)
    try:
        if socket_path:
            connection.connect(socket_path)
            messaging_denied = False
    except OSError as error:
        unix_socket_errno = error.errno
    finally:
        connection.close()
try:
    messaging_denied = messaging_denied and not os.listdir("/tmp/cc-socks")
except OSError:
    pass
checks["messaging_socket_denied"] = messaging_denied
checks["host_home_absent"] = not pathlib.Path(expected["host_home"]).exists()
checks["host_workspace_absent"] = not pathlib.Path(expected["host_workspace"]).exists()
checks["global_config_absent"] = not any(pathlib.Path("/home/eval/.claude", name).exists()
                                         for name in ("CLAUDE.md", "settings.json", "skills", "agents", "commands", "plugins"))
skills = pathlib.Path("/workspace/.claude/skills")
checks["skill_inventory"] = sorted(p.name for p in skills.iterdir()) == expected["skills"] if skills.is_dir() else False
protected = True
for path in ("/workspace/.claude/settings.json", "/workspace/.claude/skills/probe/SKILL.md"):
    try:
        pathlib.Path(path).parent.mkdir(parents=True, exist_ok=True)
        pathlib.Path(path).write_text("{}")
        protected = False
    except OSError:
        pass
checks["skill_configuration_protected"] = protected
checks["tool_inventory"] = pathlib.Path("/opt/tools/reposcout").is_file() == expected["reposcout"]
scratch = pathlib.Path("/scratch/isolation-write-probe")
scratch.write_text("probe")
scratch.unlink()
checks["scratch_writable"] = True
network_probe = {"errno": None, "controller_errno": None, "proxy": {}, "unix_socket_errno": unix_socket_errno}
connection = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
connection.settimeout(1)
try:
    network_probe["errno"] = connection.connect_ex(("192.0.2.1", 9))
except OSError as error:
    network_probe["errno"] = error.errno
finally:
    connection.close()
checks["network_denied"] = network_probe["errno"] in (errno.EPERM, errno.EACCES, errno.ENETUNREACH, errno.EHOSTUNREACH)
connection = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
connection.settimeout(1)
try:
    network_probe["controller_errno"] = connection.connect_ex(("127.0.0.1", expected["controller_port"]))
except OSError as error:
    network_probe["controller_errno"] = error.errno
finally:
    connection.close()
checks["controller_port_unreachable"] = network_probe["controller_errno"] != 0
proxy_denied = True
for url in ("http://example.com/", "https://api.anthropic.com/"):
    try:
        with urllib.request.urlopen(url, timeout=5) as response:
            network_probe["proxy"][url] = response.status
            proxy_denied = False
    except urllib.error.HTTPError as error:
        network_probe["proxy"][url] = error.code
        proxy_denied = proxy_denied and error.code in (403, 407)
    except (OSError, ValueError) as error:
        network_probe["proxy"][url] = type(error).__name__
checks["proxy_denied"] = proxy_denied
runtime_checks = {}
for name, command in expected["runtime_probes"].items():
    try:
        check = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, timeout=5, check=False)
        runtime_checks[name] = {"passed": check.returncode == 0, "returncode": check.returncode}
    except (OSError, subprocess.TimeoutExpired) as error:
        runtime_checks[name] = {"passed": False, "error_type": type(error).__name__}
checks["required_runtimes"] = all(check["passed"] for check in runtime_checks.values())
# The preflight's own root link is the only expected change to a fixture repository.
git_status = None
if pathlib.Path("/workspace/.git").is_dir():
    try:
        status = subprocess.run(["git", "-C", "/workspace", "status", "--porcelain", "--untracked-files=all"],
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=10, check=False)
        changes = [line for line in status.stdout.splitlines() if line != "?? .escape-root"]
        checks["workspace_git_clean"] = status.returncode == 0 and not changes
        git_status = {"returncode": status.returncode, "changes": changes[:32]}
    except (OSError, subprocess.TimeoutExpired) as error:
        checks["workspace_git_clean"] = False
        git_status = {"error_type": type(error).__name__}
else:
    checks["workspace_git_clean"] = True
print(json.dumps({"schema": 1, "passed": all(checks.values()), "checks": checks,
                  "runtime_checks": runtime_checks, "network_probe": network_probe,
                  "workspace_git_status": git_status}, sort_keys=True))
sys.exit(0 if all(checks.values()) else 3)
'''


@dataclass
class ClaudeIsolationPlan:
    root: Path
    outer_prefix: list
    outer_suffix: list
    profile_sha256: str
    manifest: dict
    probe_expectations: dict
    controller_canary: Path
    canary_secret: str
    input_receipt: dict
    public_marker: dict

    def model_command(self, spec, *, base_url=None):
        """Root process is the token launcher; it execs the outer bubblewrap namespace."""
        command = list(self.outer_prefix)
        if base_url is not None:
            command += ["--setenv", "ANTHROPIC_BASE_URL", base_url]
        command += self.outer_suffix + claude_arguments(spec)
        return ["/usr/bin/python3", "-I", "-S", "-c", LAUNCHER_SOURCE, json.dumps(command)]

    def write_probe_expectations(self, controller_port):
        path = self.root / "inputs/probe-expectations.json"
        path.write_text(json.dumps({**self.probe_expectations, "controller_port": controller_port}, sort_keys=True))

    def scan_roots(self):
        return [self.root / name for name in ("workspace", "home", "scratch", "controller", "inputs")]

    def cleanup(self):
        if self.root.exists():
            if not shutil.rmtree.avoids_symlink_attacks:
                raise IsolationError("Safe temporary-tree cleanup is unavailable")
            shutil.rmtree(self.root)


def _public_marker(public):
    """A short public line the preflight reads back through the Read tool."""
    for path in sorted(public.rglob("*")):
        relative = path.relative_to(public)
        if relative.parts[0] in (".git", ".claude") or not path.is_file() or path.stat().st_size > 1024 * 1024:
            continue
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            continue
        for number, line in enumerate(lines[:200], 1):
            if 8 <= len(line.strip()) and "\t" not in line:
                return {"path": "/workspace/" + relative.as_posix(), "line": number, "text": line.strip()[:60]}
    raise IsolationError("Public fixture has no readable text marker for the file-tool preflight")


def prepare_isolation(spec):
    workspace, controller, artifacts, binary = _validate_spec(spec)
    if set(spec.required_runtimes) - set(RUNTIME_PROBES):
        raise IsolationError("No availability probe for required application runtime")
    tool_manifest = runtime_tool_manifest(spec.runtime_tool_paths,
                                          required=(*REQUIRED_RUNTIME_TOOLS, *spec.required_runtimes))
    sandbox_manifest = sandbox_tool_manifest(spec.sandbox_tool_paths)
    artifacts.mkdir(mode=0o700, parents=True, exist_ok=True)
    _private_directory(artifacts)
    root = Path(tempfile.mkdtemp(prefix="reposcout-claude-isolation-"))
    try:
        public = root / "workspace"
        shutil.copytree(workspace, public, symlinks=False)
        input_receipt = {"workspace": _verify_copy(
            "workspace", directory_sha256(public), spec.expected_workspace_sha256,
        )}
        marker = _public_marker(public)
        (public / ".claude/skills").mkdir(parents=True)
        if spec.skill_dir is not None:
            validate_public_tree(spec.skill_dir, max_bytes=2 * 1024 * 1024, max_files=128)
            shutil.copytree(spec.skill_dir, public / ".claude/skills/reposcout")
            input_receipt["skill"] = _verify_copy(
                "skill", directory_sha256(public / ".claude/skills/reposcout"), spec.expected_skill_sha256,
            )
        if (public / ".git").is_dir():
            exclude = public / ".git/info/exclude"
            exclude.parent.mkdir(exist_ok=True)
            with exclude.open("a") as stream:
                stream.write("\n/.claude/\n" + "".join(f"/{name}\n" for name in SANDBOX_PLACEHOLDERS))
        inputs = root / "inputs"
        inputs.mkdir()
        settings = claude_settings()
        (inputs / "review.schema.json").write_text(json.dumps(spec.answer_schema, sort_keys=True) + "\n")
        (inputs / "settings.json").write_text(json.dumps(settings, sort_keys=True) + "\n")
        (inputs / "reposcout-global.toml").write_text("jobs = 2\n")
        (inputs / "claude_probe.py").write_text(PROBE_SOURCE)
        home = root / "home"
        (home / ".config").mkdir(parents=True)
        scratch = root / "scratch"
        (scratch / "cache").mkdir(parents=True)
        private_controller = root / "controller"
        private_controller.mkdir(mode=0o700)
        (private_controller / "cfg").mkdir(mode=0o700)
        canary_secret = "reposcout-isolation-canary-" + secrets.token_hex(16)
        controller_canary = private_controller / "isolation-canary"
        controller_canary.write_text(canary_secret + "\n")
        private_roots = (controller, artifacts, workspace)
        ca_source = _controller_ca_source(spec.controller_ca_file)
        staged_ca = root / "controller-ca.pem"
        ca_hash = _stage_runtime_file(ca_source, staged_ca, private_roots)
        input_receipt["controller_ca"] = _verify_copy("controller_ca", ca_hash, spec.expected_controller_ca_sha256)
        staged_claude = root / "runtime/claude"
        input_receipt["claude"] = _verify_copy(
            "claude", _stage_runtime_file(binary, staged_claude, private_roots), spec.expected_claude_sha256,
        )
        staged_tool = root / "tools/reposcout"
        if spec.reposcout_binary is not None:
            tool_hash = _stage_runtime_file(
                Path(spec.reposcout_binary).resolve(strict=True), staged_tool, private_roots,
            )
            input_receipt["reposcout"] = _verify_copy("reposcout", tool_hash, spec.expected_reposcout_sha256)
        bwrap = Path("/usr/bin/bwrap")
        if not bwrap.is_file():
            raise IsolationError("Bubblewrap is required; no weaker fallback is available")
        command = [str(bwrap), "--unshare-all", "--share-net", "--die-with-parent", "--new-session",
                   "--clearenv", "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
                   "--dir", "/usr", "--dir", "/usr/bin", "--dir", "/etc", "--dir", "/opt",
                   "--dir", "/opt/tools", "--dir", "/opt/claude", "--dir", "/opt/claude/bin",
                   "--dir", "/home", "--symlink", "/usr/bin", "/bin"]
        runtime_destinations = []
        for kind, manifest_items in (("runtime_tools", tool_manifest), ("sandbox_tools", sandbox_manifest)):
            hashes = {}
            for name, selected in manifest_items.items():
                staged = root / kind / name
                hashes[name] = _stage_runtime_file(Path(selected["path"]), staged, private_roots)
                if hashes[name] != selected["sha256"]:
                    raise IsolationError(f"Runtime tool changed while copying: {name}")
                command += ["--ro-bind", str(staged), selected["destination"]]
                runtime_destinations.append(selected["destination"])
            input_receipt[kind] = _verify_copy(kind, fingerprint_manifest(hashes),
                                               getattr(spec, f"expected_{kind}_sha256"))
            input_receipt[kind.removesuffix("s") + "_assets"] = hashes
        for name in ("/usr/lib", "/usr/lib64", "/lib", "/lib64", "/usr/share/git-core",
                     "/etc/ssl/certs", "/etc/ld.so.cache", "/etc/resolv.conf", "/etc/nsswitch.conf"):
            path = Path(name)
            if path.exists():
                command += ["--ro-bind", str(path.resolve()), name]
                runtime_destinations.append(name)
        command += ["--ro-bind", str(staged_claude), CLAUDE_DESTINATION,
                    "--ro-bind", str(staged_ca), "/etc/ssl/cert.pem",
                    "--bind", str(public), "/workspace", "--ro-bind", str(inputs), "/inputs",
                    "--bind", str(home), "/home/eval", "--bind", str(scratch), "/scratch",
                    "--bind", str(private_controller), "/controller"]
        if spec.reposcout_binary is not None:
            command += ["--ro-bind", str(staged_tool), "/opt/tools/reposcout"]
        for name, value in CLAUDE_ENVIRONMENT.items():
            command += ["--setenv", name, value]
        suffix = ["--chdir", "/workspace", "--"]
        manifest = configuration_manifest(spec)
        manifest["settings"] = settings
        manifest["probe_sha256"] = hashlib.sha256(PROBE_SOURCE.encode()).hexdigest()
        manifest["launcher_sha256"] = hashlib.sha256(LAUNCHER_SOURCE.encode()).hexdigest()
        manifest["runtime_destinations"] = runtime_destinations
        manifest["claude_destination"] = CLAUDE_DESTINATION
        manifest["input_receipt"] = input_receipt
        expected = {"skills": ["reposcout"] if spec.skill_dir is not None else [],
                    "reposcout": spec.reposcout_binary is not None,
                    "runtime_probes": {name: RUNTIME_PROBES[name] for name in spec.required_runtimes},
                    "host_home": str(Path.home()), "host_workspace": str(workspace)}
        return ClaudeIsolationPlan(root, command, suffix, fingerprint_manifest(manifest), manifest, expected,
                                   controller_canary, canary_secret, input_receipt, marker)
    except BaseException:
        shutil.rmtree(root)
        raise


def private_tree_files(roots, *, max_bytes=256 * 1024 * 1024):
    """Yield owned regular files without following links; refuse unbounded scans."""
    total = 0
    for root in roots:
        root = Path(root)
        if not root.exists():
            continue
        for directory, directories, files in os.walk(root, followlinks=False):
            for name in files:
                path = Path(directory) / name
                info = path.lstat()
                if not stat.S_ISREG(info.st_mode):
                    continue
                total += info.st_size
                if total > max_bytes:
                    raise IsolationError("Credential exposure scan exceeds its bound")
                yield path
