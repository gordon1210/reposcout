import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from accounting import InvalidLedger
import review_cases


class ReviewCaseTests(unittest.TestCase):
    def test_main_inventory_and_frozen_holdout_are_separate(self):
        main = review_cases.list_cases()
        all_cases = review_cases.list_cases(include_holdout=True)
        self.assertEqual(len(main), 8)
        self.assertEqual(len(all_cases), 9)
        self.assertNotIn("renewal-holdout", {case["case_id"] for case in main})
        self.assertEqual(next(case["partition"] for case in all_cases if case["case_id"] == "renewal-holdout"), "holdout")
        self.assertTrue(set(review_cases.SMOKE_CASES).issubset({case["case_id"] for case in main}))
        self.assertEqual(len({case["fixture_sha256"] for case in all_cases}), 9)

    def test_preparation_pins_verified_signed_commits_without_private_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case = review_cases.prepare_case("refund-boundary", root / "public", private_directory=root / "private")
            workspace = Path(case["workspace"])
            self.assertFalse(Path(case["private_oracle_path"]).is_relative_to(workspace))
            self.assertFalse((workspace / "oracle.json").exists())
            self.assertFalse((workspace / "probe.py").exists())
            for side in ("base", "head"):
                tree = case["steps"][0][side + "_tree"]
                commit = case["steps"][0][side + "_commit"]
                self.assertEqual(review_cases._git(workspace, "cat-file", "-t", tree), "tree")
                self.assertEqual(review_cases._git(workspace, "rev-parse", "refs/eval/" + side), commit)
                self.assertEqual(review_cases._git(workspace, "rev-parse", commit + "^{tree}"), tree)
                self.assertIn("gpgsig -----BEGIN SSH SIGNATURE-----", review_cases._git(workspace, "cat-file", "commit", commit))
                review_cases._git(workspace, "-c", "gpg.format=ssh", "-c",
                                  "gpg.ssh.allowedSignersFile=" + case["signer"]["allowed_signers"], "verify-commit", commit)
            self.assertNotIn("RepoScout", (workspace / "AGENTS.md").read_text())
            self.assertNotIn(str(root), (workspace / ".git/config").read_text())
            self.assertEqual(case["public_workspace_sha256"], review_cases._workspace_fingerprint(workspace))
            self.assertEqual((workspace / "policy.py").read_text(), review_cases.snapshots("refund-boundary")["head"]["policy.py"])
            with self.assertRaises(InvalidLedger):
                review_cases.prepare_case("refund-boundary", workspace)

    def test_future_repair_is_pinned_but_not_present_before_activation(self):
        with tempfile.TemporaryDirectory() as temporary:
            case = review_cases.prepare_case("review-followup", Path(temporary) / "public")
            workspace = Path(case["workspace"])
            future = review_cases.preview_step(case, 1)
            self.assertEqual(future["base_tree"], case["steps"][0]["head_tree"])
            self.assertEqual(case["active_step"], 0)
            with self.assertRaises(subprocess.CalledProcessError):
                review_cases._git(workspace, "cat-file", "-e", future["head_tree"])
            with self.assertRaises(subprocess.CalledProcessError):
                review_cases._git(workspace, "cat-file", "-e", future["head_commit"])
            text = review_cases.snapshots("review-followup")["followup"]["percentage.py"].encode()
            blob = hashlib.sha1(b"blob " + str(len(text)).encode() + b"\0" + text).hexdigest()
            with self.assertRaises(subprocess.CalledProcessError):
                review_cases._git(workspace, "cat-file", "-e", blob)
            activated = review_cases.activate_step(case, 1)
            self.assertEqual(activated, future)
            self.assertEqual(review_cases._git(workspace, "cat-file", "-t", blob), "blob")
            self.assertEqual(review_cases._git(workspace, "cat-file", "-t", future["head_commit"]), "commit")
            self.assertIn("divmod", (workspace / "percentage.py").read_text())
            with self.assertRaises(InvalidLedger):
                review_cases.activate_step(case, 1)

    def test_shared_task_signer_produces_identical_paired_commit_and_oracle_pins(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            signer = review_cases.create_signer(root / "signing")
            left = review_cases.prepare_case("review-followup", root / "left", signer=signer)
            right = review_cases.prepare_case("review-followup", root / "right", signer=signer)
            self.assertEqual(left["steps"], right["steps"])
            self.assertEqual(left["oracle_sha256"], right["oracle_sha256"])
            self.assertEqual(left["public_workspace_sha256"], right["public_workspace_sha256"])
            for record in (left, right):
                self.assertFalse(Path(signer["private_key"]).is_relative_to(record["workspace"]))
                self.assertFalse((Path(record["workspace"]) / "fixture_ed25519").exists())

    def test_initial_and_followup_repositories_have_clean_index_and_worktree(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            signer = review_cases.create_signer(root / "signing")
            for case in review_cases.list_cases(include_holdout=True):
                record = review_cases.prepare_case(case["case_id"], root / case["case_id"], signer=signer)
                workspace = Path(record["workspace"])
                for step_index in range(case["step_count"]):
                    with self.subTest(case=case["case_id"], step=step_index):
                        if step_index:
                            review_cases.activate_step(record, step_index)
                        self.assertEqual(review_cases._git(workspace, "status", "--porcelain=v1", "--untracked-files=all"), "")
                        self.assertEqual(review_cases._git(workspace, "diff", "--cached", "--exit-code", "HEAD", "--"), "")
                        self.assertEqual(review_cases._git(workspace, "diff", "--exit-code", "--"), "")
                        self.assertEqual(review_cases._git(workspace, "write-tree"), record["steps"][step_index]["head_tree"])
                        self.assertEqual(set(review_cases._git(workspace, "ls-files").splitlines()), set(record["installed_files"]))

    def test_changed_fixture_and_nested_oracle_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            public = Path(temporary) / "public"
            with self.assertRaises(InvalidLedger):
                review_cases.prepare_case("refund-boundary", public, private_directory=public / "hidden")
            self.assertFalse(public.exists())
            case = review_cases.prepare_case("review-followup", public)
            (public / "policy.py").write_text("tampered\n")
            with self.assertRaises(InvalidLedger):
                review_cases.activate_step(case, 1)

    def test_pure_git_tree_hash_matches_git_for_nested_name_sorting(self):
        files = {"a.c": "one\n", "a/file.py": "two\n", "a-/leaf": "three\n", "z": ""}
        with tempfile.TemporaryDirectory() as temporary:
            workspace = Path(temporary)
            review_cases._git(workspace, "init", "--quiet", "--template=", "--object-format=sha1")
            self.assertEqual(review_cases.snapshot_tree_id(files), review_cases._tree(workspace, files))

    def test_every_literal_domain_oracle_matches_the_independent_runtime(self):
        for case in review_cases.list_cases(include_holdout=True):
            with self.subTest(case=case["case_id"]):
                result = review_cases.verify_domain(case["case_id"])
                self.assertTrue(result["passed"], json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    unittest.main()
