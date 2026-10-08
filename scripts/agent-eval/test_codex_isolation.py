import json
import hashlib
import os
import errno
import io
import sys
from contextlib import redirect_stdout
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest import mock
import shutil
from dataclasses import replace

from codex_isolation import (
    IsolationError, PROBE_SOURCE, REQUIRED_RUNTIME_TOOLS, configuration_manifest, fingerprint_manifest, permission_overrides,
    codex_runtime_manifest, controller_ca_manifest, directory_sha256, prepare_isolation, validate_public_tree,
    runtime_tool_identities, runtime_tool_manifest,
)
from codex_runner import TrialSpec


def prepared_spec(root):
    workspace = root / "public"
    workspace.mkdir()
    (workspace / "main.py").write_text("def total(value):\n    return value\n")
    (workspace / ".git/info").mkdir(parents=True)
    controller = root / "controller"
    controller.mkdir(mode=0o700)
    auth = controller / "auth.json"
    auth.write_text("{}")
    auth.chmod(0o600)
    binary = root / "runtime/bin/codex"
    binary.parent.mkdir(parents=True)
    binary.write_text("synthetic executable; never executed by these tests\n")
    binary.chmod(0o700)
    ca_bundle = root / "system-ca.pem"
    ca_bundle.write_text("synthetic public CA bundle; no TLS calls in these tests\n")
    return TrialSpec(
        run_id="case-1-baseline", workspace=workspace, artifact_dir=root / "artifacts",
        controller_dir=controller, codex_binary=binary, cli_version="fixture-0.160.1",
        prompt="Review the two supplied trees.", answer_schema={"type": "object"}, controller_ca_file=ca_bundle,
        runtime_tool_paths={name: str((Path("/usr/bin") / name).resolve())
                            for name in (*REQUIRED_RUNTIME_TOOLS, "nl")},
    )


class IsolationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.spec = prepared_spec(self.root)

    def tearDown(self):
        self.temporary.cleanup()

    def test_network_probe_rejects_route_failure_without_namespace_isolation(self):
        expected = {"outer_pid_namespace_inode": 10, "outer_network_namespace_inode": 20,
                    "host_home": "/host-home", "host_workspace": "/host-source", "skills": [],
                    "reposcout": False, "runtime_probes": {}}
        for private_network, error, denied in ((False, errno.ENETUNREACH, False),
                                               (False, errno.EHOSTUNREACH, False),
                                               (True, errno.ENETUNREACH, True),
                                               (True, errno.EPERM, True), (True, 0, False)):
            with self.subTest(private_network=private_network, error=error):
                output = io.StringIO()
                connection = mock.Mock()
                connection.connect_ex.return_value = error
                def namespace(path):
                    return SimpleNamespace(st_ino=11 if str(path).endswith("/pid") else 21 if private_network else 20)
                def write(path, *_args, **_kwargs):
                    if str(path).startswith("/workspace/"):
                        raise PermissionError()
                    return 0
                with mock.patch.dict(os.environ, {}, clear=True), \
                        mock.patch.object(sys, "argv", ["probe", json.dumps(expected)]), \
                        mock.patch("os.stat", side_effect=namespace), \
                        mock.patch("os.open", side_effect=PermissionError), \
                        mock.patch.object(Path, "is_dir", return_value=True), \
                        mock.patch.object(Path, "is_file", lambda path: str(path).endswith("review.schema.json")), \
                        mock.patch.object(Path, "exists", return_value=False), \
                        mock.patch.object(Path, "iterdir", return_value=iter(())), \
                        mock.patch.object(Path, "write_text", write), \
                        mock.patch.object(Path, "unlink"), \
                        mock.patch("socket.socket", return_value=connection), redirect_stdout(output):
                    with self.assertRaises(SystemExit) as stopped:
                        exec(PROBE_SOURCE, {})
                result = json.loads(output.getvalue())
                self.assertEqual(result["checks"]["network_denied"], denied)
                self.assertEqual(result["checks"]["private_network_namespace"], private_network)
                self.assertEqual(result["network_probe"]["outer_namespace_inode"], 20)
                self.assertEqual(result["network_probe"]["tool_namespace_inode"], 21 if private_network else 20)
                self.assertEqual(result["network_probe"]["errno"], error)
                self.assertEqual(result["network_probe"]["stage"], "connect")
                self.assertEqual(result["passed"], denied)
                self.assertEqual(stopped.exception.code, 0 if denied else 3)

    def test_public_fixture_rejects_symlinks_and_hardlinks(self):
        secret = self.root / "private-oracle"
        secret.write_text("not public")
        leaf = self.spec.workspace / "borrowed"
        leaf.symlink_to(secret)
        with self.assertRaisesRegex(IsolationError, "link"):
            validate_public_tree(self.spec.workspace)
        leaf.unlink()
        os.link(secret, leaf)
        with self.assertRaisesRegex(IsolationError, "link"):
            validate_public_tree(self.spec.workspace)

    def test_git_alternates_cannot_reach_controller_repository(self):
        alternates = self.spec.workspace / ".git/objects/info/alternates"
        alternates.parent.mkdir(parents=True)
        alternates.write_text(str(self.root / "hidden-objects"))
        with self.assertRaisesRegex(IsolationError, "external configuration"):
            prepare_isolation(self.spec)

    def test_baseline_mounts_only_copied_fixture_and_explicit_runtime(self):
        plan = prepare_isolation(self.spec)
        try:
            command = plan.outer_command
            self.assertIn("--clearenv", command)
            self.assertIn("--unshare-all", command)
            self.assertIn("--die-with-parent", command)
            mounts = [
                command[index + 1:index + 3]
                for index, arg in enumerate(command) if arg in ("--bind", "--ro-bind")
            ]
            self.assertFalse(any(source in ("/", str(Path.home()), str(self.root)) for source, _ in mounts))
            public_source = next(source for source, target in mounts if target == "/workspace")
            runtime_source = next(source for source, target in mounts if target == "/opt/codex")
            self.assertEqual(runtime_source, str(plan.root / "runtime"))
            self.assertNotEqual(runtime_source, str(self.spec.codex_binary.parent.parent))
            self.assertNotEqual(public_source, str(self.spec.workspace))
            self.assertEqual((Path(public_source) / "main.py").read_bytes(),
                             (self.spec.workspace / "main.py").read_bytes())
            self.assertFalse(any(target == "/opt/tools/reposcout" for _, target in mounts))
            self.assertIn("/usr/bin/nl", plan.manifest["runtime_destinations"])
            self.assertEqual(list((Path(public_source) / ".agents/skills").iterdir()), [])
            self.assertIn("/.agents/", (Path(public_source) / ".git/info/exclude").read_text())
        finally:
            plan.cleanup()
        self.assertFalse(plan.root.exists())
        self.assertTrue((self.spec.controller_dir / "auth.json").exists())
        self.assertTrue((self.spec.workspace / "main.py").exists())

    def test_controller_binary_cannot_alias_authentication_into_runtime(self):
        binary = self.spec.controller_dir / "bin/codex"
        binary.parent.mkdir()
        self.spec.codex_binary.rename(binary)
        with self.assertRaisesRegex(IsolationError, "cannot overlap"):
            prepare_isolation(replace(self.spec, codex_binary=binary))
        self.assertEqual((self.spec.controller_dir / "auth.json").read_text(), "{}")

    def test_available_non_system_node_is_copied_pinned_and_probed(self):
        node = self.root / "selected-node"
        node.write_text("synthetic Node executable; never run by this staging test\n")
        node.chmod(0o700)
        original = Path.is_file
        with mock.patch.object(Path, "is_file", lambda path: False if path == Path("/usr/bin/node") else original(path)), \
                mock.patch("codex_isolation.shutil.which", side_effect=lambda name: str(node) if name == "node" else None):
            selected = runtime_tool_manifest(required=(*REQUIRED_RUNTIME_TOOLS, "node"))
        self.assertEqual(selected["node"]["path"], str(node))
        paths = {name: value["path"] for name, value in selected.items()}
        digest = fingerprint_manifest(runtime_tool_identities(selected))
        spec = replace(self.spec, runtime_tool_paths=paths, required_runtimes=("node",),
                       expected_runtime_tools_sha256=digest)
        plan = prepare_isolation(spec)
        try:
            command = plan.outer_command
            mounts = [command[index + 1:index + 3] for index, arg in enumerate(command) if arg == "--ro-bind"]
            copied = next(Path(source) for source, target in mounts if target == "/usr/bin/node")
            self.assertEqual(copied, plan.root / "runtime-tools/node")
            self.assertEqual(copied.read_bytes(), node.read_bytes())
            self.assertTrue(plan.input_receipt["runtime_tools"]["matched"])
            self.assertEqual(plan.probe_expectations["runtime_probes"]["node"][0], "node")
            self.assertIn("node:assert/strict", plan.probe_expectations["runtime_probes"]["node"][-1])
            self.assertNotIn(str(node), json.dumps(plan.manifest))
            node.write_text("changed after staging")
            self.assertEqual(hashlib.sha256(copied.read_bytes()).hexdigest(), selected["node"]["sha256"])
        finally:
            plan.cleanup()

    def test_missing_required_node_cannot_silently_drop_its_application_check(self):
        with self.assertRaisesRegex(IsolationError, "Required runtime tool is unavailable: node"):
            prepare_isolation(replace(self.spec, required_runtimes=("node",)))
        self.assertFalse(self.spec.artifact_dir.exists())

    def test_runtime_pin_drift_and_private_runtime_alias_are_rejected(self):
        node = self.root / "selected-node"
        node.write_text("original executable")
        node.chmod(0o700)
        paths = {**self.spec.runtime_tool_paths, "node": str(node)}
        pinned = fingerprint_manifest(runtime_tool_identities(runtime_tool_manifest(paths)))
        node.write_text("different executable")
        spec = replace(self.spec, runtime_tool_paths=paths, required_runtimes=("node",),
                       expected_runtime_tools_sha256=pinned)
        with self.assertRaisesRegex(IsolationError, "Copied runtime_tools differs"):
            prepare_isolation(spec)
        self.spec.artifact_dir.rmdir()
        private_node = self.spec.controller_dir / "node"
        node.rename(private_node)
        paths["node"] = str(private_node)
        with self.assertRaisesRegex(IsolationError, "overlaps private data"):
            prepare_isolation(replace(spec, expected_runtime_tools_sha256=None))

    def test_runtime_selection_refuses_relative_unknown_and_nonexecutable_paths(self):
        for paths in ({"unknown": "/usr/bin/bash"}, {"node": "relative/node"},
                      {"node": str(self.spec.controller_ca_file)}):
            with self.subTest(paths=paths), self.assertRaises(IsolationError):
                runtime_tool_manifest(paths, required=())

    def test_unlisted_adjacent_runtime_files_are_never_exposed(self):
        adjacent = self.spec.codex_binary.parent.parent / "auth.json"
        adjacent.write_text("synthetic adjacent private data")
        plan = prepare_isolation(self.spec)
        try:
            self.assertFalse((plan.root / "runtime/auth.json").exists())
            self.assertEqual(sorted(plan.input_receipt["codex_runtime_assets"]), ["bin/codex"])
        finally:
            plan.cleanup()

    def test_adjacent_asset_cannot_link_to_private_controller(self):
        asset = self.spec.codex_binary.parent.parent / "codex-resources/bwrap"
        asset.parent.mkdir()
        asset.symlink_to(self.spec.controller_dir / "auth.json")
        with self.assertRaisesRegex(IsolationError, "escapes"):
            prepare_isolation(self.spec)

    def test_copied_input_fingerprints_match_before_skill_injection(self):
        source_hash = hashlib.sha256(self.spec.codex_binary.read_bytes()).hexdigest()
        workspace_hash = directory_sha256(self.spec.workspace)
        pinned = replace(self.spec, expected_codex_sha256=source_hash,
                         expected_workspace_sha256=workspace_hash)
        plan = prepare_isolation(pinned)
        try:
            self.assertTrue(plan.input_receipt["codex"]["matched"])
            self.assertTrue(plan.input_receipt["workspace"]["matched"])
            self.spec.codex_binary.write_text("changed after preparation")
            self.assertEqual(hashlib.sha256((plan.root / "runtime/bin/codex").read_bytes()).hexdigest(), source_hash)
        finally:
            plan.cleanup()

    def test_actual_copy_drift_is_rejected(self):
        original = shutil.copyfile

        def changed_copy(source, destination, **options):
            result = original(source, destination, **options)
            if Path(destination).name == "codex":
                Path(destination).write_text("changed during staging")
            return result

        pinned = replace(self.spec, expected_codex_sha256=hashlib.sha256(self.spec.codex_binary.read_bytes()).hexdigest())
        with mock.patch("codex_isolation.shutil.copyfile", side_effect=changed_copy):
            with self.assertRaisesRegex(IsolationError, "Copied codex differs"):
                prepare_isolation(pinned)

    def test_adjacent_runtime_asset_drift_is_rejected_against_prepared_inventory(self):
        asset = self.spec.codex_binary.parent / "codex-code-mode-host"
        asset.write_text("prepared host")
        pinned = replace(self.spec, expected_codex_runtime_sha256=fingerprint_manifest(
            codex_runtime_manifest(self.spec.codex_binary),
        ))
        asset.write_text("changed host")
        with self.assertRaisesRegex(IsolationError, "Copied codex_runtime differs"):
            prepare_isolation(pinned)

    def test_probe_and_model_use_same_permission_overrides(self):
        plan = prepare_isolation(self.spec)
        try:
            prefix = plan.outer_command + plan.codex_prefix
            probe = plan.probe_command()
            model = plan.model_command(self.spec)
            self.assertEqual(probe[:len(plan.outer_command)], plan.outer_command)
            self.assertEqual(json.loads(probe[-2]), plan.codex_prefix)
            self.assertEqual(model[:len(prefix)], prefix)
            launcher = (plan.root / "inputs/probe_launcher.py").read_text()
            self.assertIn('"sandbox", "--permission-profile", "eval"', launcher)
            self.assertIn('os.stat("/proc/self/ns/pid").st_ino', launcher)
            self.assertNotIn("--sandbox", model)
            self.assertNotIn("--dangerously-bypass-approvals-and-sandbox", model)
            profile = next(value for value in permission_overrides() if value.startswith("permissions.eval="))
            for path in (":root", "/controller", "/artifacts", "/proc"):
                self.assertIn(json.dumps(path) + '="deny"', profile)
            self.assertIn("enabled=false", profile)
            for feature in ("apps", "multi_agent", "multi_agent_v2", "browser_use", "browser_use_external",
                            "browser_use_full_cdp_access", "computer_use", "image_generation",
                            "in_app_local_automation", "skill_search"):
                self.assertIn(f"features.{feature}=false", permission_overrides())
            self.assertIn("apps._default.enabled=false", permission_overrides())
        finally:
            plan.cleanup()

    def test_controller_ca_is_frozen_pinned_and_only_set_in_controller_environment(self):
        pin = controller_ca_manifest(self.spec.controller_ca_file)
        spec = replace(self.spec, expected_controller_ca_sha256=pin["sha256"])
        plan = prepare_isolation(spec)
        try:
            command = plan.outer_command
            mounts = [command[index + 1:index + 3] for index, arg in enumerate(command) if arg == "--ro-bind"]
            source = next(source for source, target in mounts if target == "/etc/ssl/cert.pem")
            self.assertEqual(source, str(plan.root / "controller-ca.pem"))
            self.assertTrue(plan.input_receipt["controller_ca"]["matched"])
            self.spec.controller_ca_file.write_text("source changed after staging")
            self.assertEqual(hashlib.sha256(Path(source).read_bytes()).hexdigest(), pin["sha256"])
            self.assertIn("SSL_CERT_FILE", command)
            self.assertNotIn("SSL_CERT_FILE", plan.manifest["tool_environment"])
        finally:
            plan.cleanup()

    def test_treatment_and_tool_only_have_different_skill_mounts(self):
        tool = self.root / "reposcout"
        tool.write_text("synthetic CLI")
        tool.chmod(0o700)
        skill = self.root / "skill"
        (skill / "references").mkdir(parents=True)
        (skill / "SKILL.md").write_text("---\nname: reposcout\ndescription: review\n---\n")
        (skill / "references/change.md").write_text("Inspect actual changed behavior.")
        treatment = replace(self.spec, variant="reposcout", reposcout_binary=tool, skill_dir=skill)
        plan = prepare_isolation(treatment)
        try:
            self.assertTrue((plan.root / "workspace/.agents/skills/reposcout/references/change.md").is_file())
            self.assertIn("/opt/tools/reposcout", plan.outer_command)
        finally:
            plan.cleanup()
        self.spec.artifact_dir.rmdir()
        only = replace(self.spec, variant="reposcout-cli", reposcout_binary=tool)
        plan = prepare_isolation(only)
        try:
            self.assertEqual(plan.probe_expectations["skills"], [])
            self.assertTrue(plan.probe_expectations["reposcout"])
        finally:
            plan.cleanup()

    def test_non_treatment_cannot_receive_a_skill(self):
        with self.assertRaisesRegex(IsolationError, "Baseline"):
            prepare_isolation(replace(self.spec, skill_dir=self.root))
        (self.spec.workspace / ".agents").mkdir()
        with self.assertRaisesRegex(IsolationError, "inherited skill"):
            prepare_isolation(self.spec)

    def test_existing_private_canary_is_preserved_on_failure(self):
        canary = self.spec.controller_dir / "isolation-canary"
        canary.write_text("pre-existing controller-owned data")
        with self.assertRaises(FileExistsError):
            prepare_isolation(self.spec)
        self.assertEqual(canary.read_text(), "pre-existing controller-owned data")

    def test_cleanup_preserves_replaced_canary(self):
        plan = prepare_isolation(self.spec)
        canary = plan.controller_canary
        canary.rename(self.spec.controller_dir / "retained-original")
        canary.write_text("replacement")
        plan.cleanup()
        self.assertEqual(canary.read_text(), "replacement")

    def test_manifest_is_stable_and_has_no_private_host_paths(self):
        first = configuration_manifest(self.spec)
        second = configuration_manifest(replace(
            self.spec, controller_dir=self.root / "different-controller",
            artifact_dir=self.root / "different-artifacts",
        ))
        self.assertEqual(fingerprint_manifest(first), fingerprint_manifest(second))
        self.assertNotIn(str(self.root), json.dumps(first))
        self.assertEqual(first["limits"]["host_memory_reserve_bytes"], 12 * 1024 ** 3)


if __name__ == "__main__":
    unittest.main()
