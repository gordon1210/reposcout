"""Prepare a disposable filesystem and fail-closed Codex tool permission profile."""

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import tempfile


ISOLATION_VERSION = 1
VARIANTS = ("baseline", "reposcout", "reposcout-cli")
MAX_PUBLIC_BYTES = 64 * 1024 * 1024
MAX_PUBLIC_FILES = 4096
MAX_RUNTIME_FILE_BYTES = 512 * 1024 * 1024
MAX_CA_BUNDLE_BYTES = 2 * 1024 * 1024
REPOSCOUT_CHILD_TIMEOUT_SECONDS = 180
CODEX_RUNTIME_ASSETS = (
    "bin/codex-code-mode-host", "codex-package.json", "codex-path/rg",
    "codex-resources/bwrap", "codex-resources/zsh/bin/zsh",
)
RUNTIME_TOOLS = (
    "bash", "sh", "cat", "nl", "ls", "head", "tail", "sed", "awk", "grep", "rg", "git",
    "python3", "node", "jq", "find", "sort", "cut", "wc", "pwd", "basename",
    "dirname", "readlink", "stat", "file", "env", "timeout", "sleep", "printf",
)
REQUIRED_RUNTIME_TOOLS = ("bash", "sh", "git", "rg", "python3")
RUNTIME_PROBES = {
    "python3": ["python3", "-I", "-B", "-c",
                "import json, pathlib, unittest; assert json.loads('[1]') == [1]"],
    "node": ["node", "--input-type=module", "-e",
             "import assert from 'node:assert/strict'; import fs from 'node:fs'; "
             "assert.equal(typeof fs.readFileSync, 'function')"],
}
TOOL_ENVIRONMENT = {
    "PATH": "/opt/tools:/usr/bin:/bin",
    "HOME": "/home/eval",
    "LANG": "C.UTF-8",
    "LC_ALL": "C.UTF-8",
    "TMPDIR": "/scratch",
    "XDG_CACHE_HOME": "/scratch/cache",
    "XDG_CONFIG_HOME": "/home/eval/.config",
    "REPOSCOUT_GLOBAL_CONFIG": "/inputs/reposcout-global.toml",
    "PYTHONDONTWRITEBYTECODE": "1",
}


class IsolationError(ValueError):
    """A requested environment could expose non-public input or weaken isolation."""


class RuntimeUnavailableError(IsolationError):
    def __init__(self, tool):
        super().__init__(f"Required runtime tool is unavailable: {tool}")
        self.tool = tool


def fingerprint_manifest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def _file_sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def directory_sha256(path):
    """Hash the validated copied tree with the campaign's relative-path/file-hash format."""
    path = Path(path)
    files = {}
    for child in sorted(path.rglob("*")):
        if child.is_symlink():
            raise IsolationError("Copied input contains a symbolic link")
        if child.is_file():
            files[child.relative_to(path).as_posix()] = _file_sha256(child)
    return fingerprint_manifest(files)


def _verify_copy(name, actual, expected):
    if expected is not None and actual != expected:
        raise IsolationError(f"Copied {name} differs from its prepared fingerprint")
    return {"sha256": actual, "expected_sha256": expected, "matched": expected == actual if expected else None}


def permission_overrides():
    """CLI overrides apply identically to the probe and the actual model execution."""
    return [
        'default_permissions="eval"',
        'approval_policy="never"',
        'permissions.eval={filesystem={":root"="deny",":minimal"="read",'
        '"/workspace"="read","/inputs"="read","/opt/tools"="read","/opt/codex"="read",'
        '"/scratch"="write","/home/eval"="read","/controller"="deny",'
        '"/artifacts"="deny","/proc"="deny"},network={enabled=false}}',
        'web_search="disabled"',
        'features.multi_agent=false',
        'features.multi_agent_v2=false',
        'features.memories=false',
        'features.plugins=false',
        'features.remote_plugin=false',
        'features.apps=false',
        'apps._default.enabled=false',
        'features.browser_use=false',
        'features.browser_use_external=false',
        'features.browser_use_full_cdp_access=false',
        'features.computer_use=false',
        'features.image_generation=false',
        'features.in_app_local_automation=false',
        'features.skill_search=false',
        'features.hooks=false',
        'features.shell_snapshot=false',
        'features.skill_mcp_dependency_install=false',
        'cli_auth_credentials_store="file"',
        'shell_environment_policy.inherit="none"',
        'shell_environment_policy.experimental_use_profile=false',
        'shell_environment_policy.set={' + ",".join(
            f'{json.dumps(key)}={json.dumps(value)}' for key, value in TOOL_ENVIRONMENT.items()
        ) + '}',
    ]


def configuration_manifest(spec):
    """Reproducible, nonsecret settings; source and executable hashes are campaign facts."""
    return {
        "schema": 1,
        "isolation_version": ISOLATION_VERSION,
        "variant": spec.variant,
        "cli_version": spec.cli_version,
        "model": spec.model,
        "effort": spec.effort,
        "permission_overrides": permission_overrides(),
        "tool_environment": TOOL_ENVIRONMENT,
        "outer_policy": "private-mount-pid-user-ipc-uts-namespace;controller-network-only",
        "authentication": "caller-owned-private-controller-copy;tool-denied",
        "public_workspace": "/workspace",
        "private_paths": ["/controller", "/artifacts", "/proc"],
        "runtime_tools": list(RUNTIME_TOOLS),
        "codex_runtime_assets": ["bin/codex", *CODEX_RUNTIME_ASSETS],
        "expected_inputs": {
            name: getattr(spec, f"expected_{name}_sha256")
            for name in ("workspace", "codex", "codex_runtime", "controller_ca", "reposcout", "skill", "runtime_tools")
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


def _regular_file(path):
    path = Path(path)
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode):
        raise IsolationError(f"Expected a regular file: {path.name}")
    return path


def _private_directory(path):
    path = Path(path)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise IsolationError(f"Private directory must be owned by the current user with mode 0700: {path.name}")
    return path.resolve(strict=True)


def validate_public_tree(root, *, max_bytes=MAX_PUBLIC_BYTES, max_files=MAX_PUBLIC_FILES):
    """Do not follow links or let arbitrary object-store alternates escape the fixture."""
    root = Path(root)
    if not stat.S_ISDIR(root.lstat().st_mode):
        raise IsolationError("Public fixture must be a real directory")
    total = count = 0
    for directory, directories, files in os.walk(root, followlinks=False):
        for name in directories + files:
            path = Path(directory) / name
            info = path.lstat()
            if stat.S_ISDIR(info.st_mode):
                continue
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                raise IsolationError("Public fixture contains a link or non-regular file")
            total += info.st_size
            count += 1
            if total > max_bytes or count > max_files:
                raise IsolationError("Public fixture exceeds bounded copy limits")
    for relative in (".git/objects/info/alternates", ".git/commondir", ".codex"):
        if (root / relative).exists():
            raise IsolationError(f"Public fixture contains unsupported external configuration: {relative}")
    if (root / ".git").exists() and not (root / ".git").is_dir():
        raise IsolationError("Public fixture cannot use a linked Git worktree")
    return {"files": count, "bytes": total}


def _inside(path, root):
    return path == root or root in path.parents


def _codex_runtime_sources(binary):
    binary = _regular_file(Path(binary).resolve(strict=True))
    sources = {"bin/codex": binary}
    for relative in CODEX_RUNTIME_ASSETS:
        source = binary.parent.parent / relative
        if source.exists() or source.is_symlink():
            if not _inside(source.resolve(strict=True), binary.parent.parent):
                raise IsolationError("Adjacent runtime asset escapes the selected installation")
            sources[relative] = _regular_file(source)
    for source in sources.values():
        if source.stat().st_size > MAX_RUNTIME_FILE_BYTES:
            raise IsolationError("Runtime asset exceeds the bounded staging size")
    return sources


def codex_runtime_manifest(binary):
    """Pin precisely the adjacent assets that isolation will copy, including their inventory."""
    return {relative: _file_sha256(source) for relative, source in _codex_runtime_sources(binary).items()}


def runtime_tool_manifest(paths=None, *, required=REQUIRED_RUNTIME_TOOLS):
    """Resolve only known executables; persist selected paths privately and pin their bytes."""
    if paths is not None and (not isinstance(paths, dict) or set(paths) - set(RUNTIME_TOOLS)):
        raise IsolationError("Unknown runtime tool selection")
    if set(required) - set(RUNTIME_TOOLS):
        raise IsolationError("Unknown required runtime tool")
    result = {}
    for name in RUNTIME_TOOLS:
        if paths is None:
            system = Path("/usr/bin") / name
            selected = str(system) if system.is_file() else shutil.which(name)
        else:
            selected = paths.get(name)
        if selected is None:
            if name in required:
                raise RuntimeUnavailableError(name)
            continue
        source = Path(selected)
        if not source.is_absolute():
            raise IsolationError("Runtime tool paths must be absolute")
        source = _regular_file(source.resolve(strict=True))
        if not os.access(source, os.X_OK) or not 0 < source.stat().st_size <= MAX_RUNTIME_FILE_BYTES:
            raise IsolationError(f"Runtime tool is not a bounded executable: {name}")
        result[name] = {"path": str(source), "sha256": _file_sha256(source),
                        "destination": f"/usr/bin/{name}"}
    return result


def runtime_tool_identities(manifest):
    """Host paths are private; the copied tool names and bytes define the public identity."""
    return {name: value["sha256"] for name, value in manifest.items()}


def _controller_ca_source(path=None):
    candidates = [Path(path)] if path is not None else [
        Path("/etc/ssl/cert.pem"), Path("/etc/ssl/certs/ca-certificates.crt"),
    ]
    source = next((candidate.resolve(strict=True) for candidate in candidates if candidate.is_file()), None)
    if source is None:
        raise IsolationError("No public system CA bundle is available for the trusted controller")
    _regular_file(source)
    size = source.stat().st_size
    if not 0 < size <= MAX_CA_BUNDLE_BYTES:
        raise IsolationError("Controller CA bundle exceeds the bounded staging size")
    return source


def controller_ca_manifest(path=None):
    """Return a bounded public CA bundle pin; callers keep its source path private."""
    source = _controller_ca_source(path)
    return {"path": str(source), "sha256": _file_sha256(source), "bytes": source.stat().st_size,
            "destination": "/etc/ssl/cert.pem"}


def _validate_spec(spec):
    if spec.variant not in VARIANTS:
        raise IsolationError("Unknown evaluation variant")
    if spec.variant == "baseline" and (spec.reposcout_binary is not None or spec.skill_dir is not None):
        raise IsolationError("Baseline cannot receive RepoScout or its skill")
    if spec.variant != "baseline" and spec.reposcout_binary is None:
        raise IsolationError("RepoScout arm requires a pinned binary")
    if (spec.variant == "reposcout") != (spec.skill_dir is not None):
        raise IsolationError("Only the skill arm may receive the RepoScout skill")
    workspace = Path(spec.workspace).resolve(strict=True)
    validate_public_tree(workspace)
    if (workspace / ".agents").exists():
        raise IsolationError("Prepared fixture must not contain an inherited skill catalog")
    controller = _private_directory(spec.controller_dir)
    auth = _regular_file(controller / "auth.json")
    auth_info = auth.stat()
    if auth_info.st_uid != os.getuid() or auth_info.st_nlink != 1 or stat.S_IMODE(auth_info.st_mode) & 0o077:
        raise IsolationError("Controller authentication must be a private, user-owned file")
    artifacts = Path(spec.artifact_dir).resolve()
    if any(_inside(private, workspace) or _inside(workspace, private) for private in (controller, artifacts)):
        raise IsolationError("Public workspace and private controller/artifacts must be disjoint")
    if _inside(artifacts, controller) or _inside(controller, artifacts):
        raise IsolationError("Controller and artifact directories must be disjoint")
    binary = _regular_file(Path(spec.codex_binary).resolve(strict=True))
    if any(_inside(binary, private) for private in (controller, artifacts, workspace)):
        raise IsolationError("Codex runtime cannot overlap controller, artifacts or fixture")
    if not os.access(binary, os.X_OK):
        raise IsolationError("Codex native binary is not executable")
    return workspace, controller, artifacts, binary


def _stage_runtime_file(source, destination, private_roots):
    source = _regular_file(source)
    resolved = source.resolve(strict=True)
    if any(_inside(resolved, private) for private in private_roots):
        raise IsolationError("Runtime asset overlaps private data or the public fixture")
    if source.stat().st_size > MAX_RUNTIME_FILE_BYTES:
        raise IsolationError("Runtime asset exceeds the bounded staging size")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination, follow_symlinks=False)
    _regular_file(destination)
    destination.chmod(stat.S_IMODE(source.stat().st_mode) & 0o755)
    return _file_sha256(destination)


PROBE_CHECKS = frozenset((
    "public_read", "denied:/controller/auth.json", "denied:/controller/isolation-canary",
    "denied:/artifacts/isolation-canary", "denied:/proc/1/root/controller/auth.json",
    "private_pid_namespace", "private_process_roots", "host_home_absent",
    "host_workspace_absent", "skill_inventory", "tool_inventory", "secret_environment_absent",
    "workspace_readonly", "scratch_writable", "network_denied", "required_runtimes",
))


PROBE_LAUNCHER_SOURCE = r'''
import json, os, sys
prefix = json.loads(sys.argv[1])
expected = json.loads(sys.argv[2])
expected["outer_pid_namespace_inode"] = os.stat("/proc/self/ns/pid").st_ino
command = prefix + ["sandbox", "--permission-profile", "eval", "-C", "/workspace", "--",
                    "/usr/bin/python3", "/inputs/isolation_probe.py",
                    json.dumps(expected, sort_keys=True)]
os.execv(command[0], command)
'''


PROBE_SOURCE = r'''
import errno, json, os, pathlib, socket, subprocess, sys
expected = json.loads(sys.argv[1])
checks = {}
checks["public_read"] = pathlib.Path("/workspace").is_dir() and pathlib.Path("/inputs/review.schema.json").is_file()
for path in ("/controller/auth.json", "/controller/isolation-canary", "/artifacts/isolation-canary",
             "/proc/1/root/controller/auth.json"):
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_CLOEXEC)
    except (PermissionError, FileNotFoundError, NotADirectoryError):
        checks["denied:" + path] = True
    else:
        os.close(descriptor)
        checks["denied:" + path] = False
checks["private_pid_namespace"] = (
    os.stat("/proc/self/ns/pid").st_ino != expected["outer_pid_namespace_inode"]
)
processes = [path for path in pathlib.Path("/proc").iterdir() if path.name.isdecimal()]
checks["private_process_roots"] = len(processes) <= 256
for process in processes[:256]:
    for private in ("controller/isolation-canary", "artifacts/isolation-canary"):
        try:
            descriptor = os.open(str(process / "root" / private), os.O_RDONLY | os.O_CLOEXEC)
        except (PermissionError, FileNotFoundError, NotADirectoryError):
            continue
        else:
            os.close(descriptor)
            checks["private_process_roots"] = False
checks["host_home_absent"] = not pathlib.Path(expected["host_home"]).exists()
checks["host_workspace_absent"] = not pathlib.Path(expected["host_workspace"]).exists()
checks["skill_inventory"] = sorted(p.name for p in pathlib.Path("/workspace/.agents/skills").iterdir()) == expected["skills"]
checks["tool_inventory"] = pathlib.Path("/opt/tools/reposcout").is_file() == expected["reposcout"]
checks["secret_environment_absent"] = not any(
    word in key.upper() for key in os.environ for word in ("TOKEN", "SECRET", "API_KEY", "PASSWORD")
)
runtime_checks = {}
for name, command in expected["runtime_probes"].items():
    try:
        check = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, timeout=5, check=False)
        runtime_checks[name] = {"passed": check.returncode == 0, "returncode": check.returncode}
    except (OSError, subprocess.TimeoutExpired) as error:
        runtime_checks[name] = {"passed": False, "error_type": type(error).__name__}
checks["required_runtimes"] = all(check["passed"] for check in runtime_checks.values())
try:
    pathlib.Path("/workspace/.isolation-write-probe").write_text("probe")
except (PermissionError, OSError):
    checks["workspace_readonly"] = True
else:
    checks["workspace_readonly"] = False
scratch = pathlib.Path("/scratch/isolation-write-probe")
scratch.write_text("probe")
scratch.unlink()
checks["scratch_writable"] = True
try:
    connection = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    connection.settimeout(0.2)
    try:
        result = connection.connect_ex(("192.0.2.1", 9))
    finally:
        connection.close()
    checks["network_denied"] = result in (errno.EPERM, errno.EACCES, errno.ENETUNREACH, errno.EHOSTUNREACH)
except OSError as error:
    checks["network_denied"] = error.errno in (errno.EPERM, errno.EACCES, errno.ENETUNREACH)
print(json.dumps({"schema": 1, "passed": all(checks.values()), "checks": checks,
                  "runtime_checks": runtime_checks}, sort_keys=True))
sys.exit(0 if all(checks.values()) else 3)
'''


@dataclass
class IsolationPlan:
    root: Path
    outer_command: list
    codex_prefix: list
    profile_sha256: str
    manifest: dict
    probe_expectations: dict
    controller_canary: Path
    artifact_canary: Path
    canary_identities: dict
    input_receipt: dict

    def probe_command(self):
        return self.outer_command + [
            "/usr/bin/python3", "/inputs/probe_launcher.py",
            json.dumps(self.codex_prefix),
            json.dumps(self.probe_expectations, sort_keys=True),
        ]

    def model_command(self, spec):
        command = self.outer_command + self.codex_prefix + [
            "exec", "--ignore-user-config", "--ignore-rules", "--json",
            "--model", spec.model, "--output-schema", "/inputs/review.schema.json",
            "--output-last-message", "/artifacts/answer.json",
        ]
        if spec.resume_thread_id:
            command += ["resume", spec.resume_thread_id, "-"]
        else:
            command += ["--cd", "/workspace", "-"]
        return command

    def cleanup(self):
        for canary in (self.controller_canary, self.artifact_canary):
            info = canary.lstat() if canary.exists() else None
            if info is not None and (info.st_dev, info.st_ino) == self.canary_identities.get(str(canary)):
                canary.unlink()
        if self.root.exists():
            if not shutil.rmtree.avoids_symlink_attacks:
                raise IsolationError("Safe temporary-tree cleanup is unavailable")
            shutil.rmtree(self.root)


def prepare_isolation(spec):
    workspace, controller, artifacts, binary = _validate_spec(spec)
    if set(spec.required_runtimes) - set(RUNTIME_PROBES):
        raise IsolationError("No availability probe for required application runtime")
    tool_manifest = runtime_tool_manifest(spec.runtime_tool_paths,
                                          required=(*REQUIRED_RUNTIME_TOOLS, *spec.required_runtimes))
    artifacts.mkdir(mode=0o700, parents=True, exist_ok=True)
    _private_directory(artifacts)
    if any(artifacts.iterdir()):
        raise IsolationError("Invocation artifact directory must be empty")
    root = Path(tempfile.mkdtemp(prefix="reposcout-codex-isolation-"))
    controller_canary = controller / "isolation-canary"
    artifact_canary = artifacts / "isolation-canary"
    created_canaries = {}
    try:
        public = root / "workspace"
        shutil.copytree(workspace, public, symlinks=False)
        input_receipt = {"workspace": _verify_copy(
            "workspace", directory_sha256(public), spec.expected_workspace_sha256,
        )}
        (public / ".agents/skills").mkdir(parents=True)
        if spec.skill_dir is not None:
            validate_public_tree(spec.skill_dir, max_bytes=2 * 1024 * 1024, max_files=128)
            shutil.copytree(spec.skill_dir, public / ".agents/skills/reposcout")
            input_receipt["skill"] = _verify_copy(
                "skill", directory_sha256(public / ".agents/skills/reposcout"), spec.expected_skill_sha256,
            )
        if (public / ".git").is_dir():
            exclude = public / ".git/info/exclude"
            exclude.parent.mkdir(exist_ok=True)
            with exclude.open("a") as stream:
                stream.write("\n/.agents/\n")
        inputs = root / "inputs"
        inputs.mkdir()
        (inputs / "review.schema.json").write_text(json.dumps(spec.answer_schema, sort_keys=True) + "\n")
        (inputs / "reposcout-global.toml").write_text("jobs = 2\n")
        (inputs / "isolation_probe.py").write_text(PROBE_SOURCE)
        (inputs / "probe_launcher.py").write_text(PROBE_LAUNCHER_SOURCE)
        home = root / "home"
        (home / ".config").mkdir(parents=True)
        scratch = root / "scratch"
        scratch.mkdir()
        runtime = root / "runtime"
        private_roots = (controller, artifacts, workspace)
        ca_source = _controller_ca_source(spec.controller_ca_file)
        staged_ca = root / "controller-ca.pem"
        ca_hash = _stage_runtime_file(ca_source, staged_ca, private_roots)
        input_receipt["controller_ca"] = _verify_copy("controller_ca", ca_hash, spec.expected_controller_ca_sha256)
        asset_hashes = {
            relative: _stage_runtime_file(source, runtime / relative, private_roots)
            for relative, source in _codex_runtime_sources(binary).items()
        }
        input_receipt["codex"] = _verify_copy("codex", asset_hashes["bin/codex"], spec.expected_codex_sha256)
        input_receipt["codex_runtime"] = _verify_copy(
            "codex_runtime", fingerprint_manifest(asset_hashes), spec.expected_codex_runtime_sha256,
        )
        input_receipt["codex_runtime_assets"] = asset_hashes
        staged_tool = root / "tools/reposcout"
        if spec.reposcout_binary is not None:
            tool_hash = _stage_runtime_file(
                Path(spec.reposcout_binary).resolve(strict=True), staged_tool, private_roots,
            )
            input_receipt["reposcout"] = _verify_copy("reposcout", tool_hash, spec.expected_reposcout_sha256)
        for canary in (controller_canary, artifact_canary):
            with canary.open("x") as stream:
                stream.write("private evaluation canary\n")
            info = canary.lstat()
            created_canaries[str(canary)] = (info.st_dev, info.st_ino)
        bwrap = Path("/usr/bin/bwrap")
        if not bwrap.is_file():
            raise IsolationError("Bubblewrap is required; no weaker fallback is available")
        command = [str(bwrap), "--unshare-all", "--share-net", "--die-with-parent", "--new-session",
                   "--clearenv", "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
                   "--dir", "/usr", "--dir", "/usr/bin", "--dir", "/etc", "--dir", "/opt",
                   "--dir", "/opt/tools", "--dir", "/home", "--symlink", "/usr/bin", "/bin"]
        runtime_destinations = []
        tool_hashes = {}
        for name, selected in tool_manifest.items():
            staged = root / "runtime-tools" / name
            tool_hashes[name] = _stage_runtime_file(Path(selected["path"]), staged, private_roots)
            if tool_hashes[name] != selected["sha256"]:
                raise IsolationError(f"Runtime tool changed while copying: {name}")
            command += ["--ro-bind", str(staged), selected["destination"]]
            runtime_destinations.append(selected["destination"])
        input_receipt["runtime_tools"] = _verify_copy(
            "runtime_tools", fingerprint_manifest(tool_hashes), spec.expected_runtime_tools_sha256,
        )
        input_receipt["runtime_tool_assets"] = tool_hashes
        for name in ("/usr/lib", "/usr/lib64", "/lib", "/lib64", "/usr/share/git-core",
                     "/etc/ssl/certs", "/etc/ld.so.cache", "/etc/resolv.conf", "/etc/nsswitch.conf"):
            path = Path(name)
            if path.exists():
                command += ["--ro-bind", str(path.resolve()), name]
                runtime_destinations.append(name)
        command += ["--ro-bind", str(runtime), "/opt/codex",
                    "--ro-bind", str(staged_ca), "/etc/ssl/cert.pem",
                    "--ro-bind", str(public), "/workspace", "--ro-bind", str(inputs), "/inputs",
                    "--ro-bind", str(home), "/home/eval", "--bind", str(scratch), "/scratch",
                    "--bind", str(controller), "/controller", "--bind", str(artifacts), "/artifacts"]
        if spec.reposcout_binary is not None:
            command += ["--ro-bind", str(staged_tool), "/opt/tools/reposcout"]
        controller_environment = {
            **TOOL_ENVIRONMENT, "CODEX_HOME": "/controller", "PATH": "/opt/codex/bin:/usr/bin:/bin",
            "SSL_CERT_FILE": "/etc/ssl/cert.pem",
        }
        for name, value in controller_environment.items():
            command += ["--setenv", name, value]
        command += ["--chdir", "/workspace", "--"]
        prefix = ["/opt/codex/bin/codex", "--no-daemon"]
        for override in permission_overrides() + [f"model_reasoning_effort={json.dumps(spec.effort)}"]:
            prefix += ["-c", override]
        manifest = configuration_manifest(spec)
        manifest["probe_sha256"] = hashlib.sha256(PROBE_SOURCE.encode()).hexdigest()
        manifest["probe_launcher_sha256"] = hashlib.sha256(PROBE_LAUNCHER_SOURCE.encode()).hexdigest()
        manifest["runtime_destinations"] = runtime_destinations
        manifest["codex_runtime_destination"] = "/opt/codex"
        manifest["input_receipt"] = input_receipt
        expected = {"skills": ["reposcout"] if spec.skill_dir is not None else [],
                    "reposcout": spec.reposcout_binary is not None,
                    "runtime_probes": {name: RUNTIME_PROBES[name] for name in spec.required_runtimes},
                    "host_home": str(Path.home()), "host_workspace": str(workspace)}
        return IsolationPlan(root, command, prefix, fingerprint_manifest(manifest), manifest, expected,
                             controller_canary, artifact_canary, created_canaries, input_receipt)
    except BaseException:
        for canary in (controller_canary, artifact_canary):
            info = canary.lstat() if canary.exists() else None
            if info is not None and (info.st_dev, info.st_ino) == created_canaries.get(str(canary)):
                canary.unlink()
        shutil.rmtree(root)
        raise
