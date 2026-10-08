"""User-facing privacy boundary tests; no real host evidence or model execution."""

import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from accounting import InvalidLedger
import publication
import review_campaign
import review_export
import review_cases


def private_report():
    runs = []
    for variant, status in (("baseline", "completed"), ("reposcout", "failed"), ("reposcout-cli", "notrun")):
        run = {"case_id": "refund-boundary", "repeat_id": 1, "variant": variant, "status": status,
               "quality": None, "accounting": None}
        if status != "notrun":
            run["quality"] = {"adjudication_status": "accepted", "quality": {"passed": status == "completed"}}
            run["accounting"] = {"usage_basis": "exec-emitted-initial-turn-usage",
                                 "comparable_usage": status == "completed",
                                 "observed_usage": {"input_tokens": 100, "cached_input_tokens": 20,
                                                    "cache_write_input_tokens": None, "output_tokens": 10}}
        runs.append(run)
    return {"kind": "codex-review-campaign-report", "synthetic_fixtures": True,
            "assignment_count": len(runs), "runs": runs}


class PublicationTests(unittest.TestCase):
    def test_public_summary_preserves_all_outcomes_and_unknown_counters(self):
        value = publication.project_publication(private_report())
        self.assertEqual([row["status"] for row in value["runs"]], ["completed", "failed", "notrun"])
        self.assertEqual([row["quality"] for row in value["runs"]], ["passed", "failed", "unavailable"])
        self.assertEqual(value["runs"][1]["tokens"]["input_tokens"], 100)
        self.assertIsNone(value["runs"][0]["tokens"]["cache_write_input_tokens"])
        self.assertEqual(value["runs"][2]["tokens"], dict.fromkeys(publication.TOKEN_FIELDS))
        with self.assertRaises(InvalidLedger):
            publication.project_publication({**private_report(), "assignment_count": 4})

    def test_allowlist_omits_private_fields_at_every_source_level(self):
        report = private_report()
        private = {"host_path": "/home/private-user/checkout", "session": "private-session",
                   "conversation": "A private instruction without any secret-looking marker",
                   "pid": 12345, "controller_ca_sha256": "d" * 64, "archive": "private-raw.zip"}
        report.update(private)
        for run in report["runs"]:
            run.update(private)
            if run["accounting"]:
                run["accounting"].update(private)
                run["accounting"]["observed_usage"].update(private)
            if run["quality"]:
                run["quality"]["quality"].update(private)
        value = publication.project_publication(report)
        encoded = json.dumps(value)
        for key in private:
            self.assertNotIn(key, encoded)
        self.assertNotIn("private", encoded)
        self.assertNotIn("d" * 64, encoded)

    def test_validator_rejects_extra_fields_and_text_in_numeric_or_enum_fields(self):
        original = publication.project_publication(private_report())
        mutations = (
            lambda value: value.update({"authority": "a user approval summary"}),
            lambda value: value["runs"][0].update({"thread_id_sha256": "a" * 64}),
            lambda value: value["runs"][0].update({"case_id": "private-free-text"}),
            lambda value: value["runs"][0].update({"status": "private instruction"}),
            lambda value: value["runs"][0].update({"repeat_id": True}),
            lambda value: value["runs"][0]["tokens"].update({"input_tokens": "/home/private-user/repo"}),
            lambda value: value["runs"][0]["tokens"].update({"input_tokens": -1}),
            lambda value: value["runs"][0]["tokens"].update({"raw": "archive"}),
            lambda value: value["runs"].append(copy.deepcopy(value["runs"][0])),
        )
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                value = copy.deepcopy(original)
                mutate(value)
                with self.assertRaises(InvalidLedger):
                    publication.validate_publication(value)

    def test_private_destinations_reject_any_git_checkout_and_symlink_alias(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name, metadata_directory in (("checkout", True), ("worktree", False)):
                repo = root / name
                repo.mkdir()
                if metadata_directory:
                    (repo / ".git").mkdir()
                    (repo / ".git/HEAD").write_text("ref: refs/heads/main\n")
                else:
                    (repo / ".git").write_text("gitdir: /synthetic/metadata\n")
                with self.assertRaises(InvalidLedger):
                    publication.private_destination(repo / "nested/new/results")
            (root / "alias").symlink_to(root / "checkout", target_is_directory=True)
            with self.assertRaises(InvalidLedger):
                publication.private_destination(root / "alias/output")
            self.assertEqual(publication.private_destination(root / "private/output"), root / "private/output")
            self.assertFalse((root / "private").exists())

    def test_raw_export_and_campaign_cannot_write_inside_a_repository(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / ".git").mkdir()
            (root / ".git/HEAD").write_text("ref: refs/heads/main\n")
            with patch.object(review_export, "report", return_value=private_report()):
                with self.assertRaises(InvalidLedger):
                    review_export.export("unused", root / "raw")
            self.assertFalse((root / "raw").exists())
            with self.assertRaises(InvalidLedger):
                review_campaign.create_campaign_root(root / "campaign")
            self.assertFalse((root / "campaign").exists())

    def test_qualification_rejects_repository_tmpdir_before_output_or_probes(self):
        script = Path(__file__).parent / "fixtures/large-review/development/qualify.py"
        wrapper = ("import runpy, sys\nfrom unittest.mock import patch\n"
                   "sys.argv = [sys.argv[1]]\n"
                   "with patch('subprocess.run', side_effect=AssertionError('unexpected probe execution')):\n"
                   "    runpy.run_path(sys.argv[0], run_name='__main__')\n")
        with tempfile.TemporaryDirectory() as temporary:
            repo = Path(temporary) / "repo"
            (repo / ".git").mkdir(parents=True)
            (repo / ".git/HEAD").write_text("ref: refs/heads/main\n")
            temp_parent = repo / "tmp"
            temp_parent.mkdir()
            environment = {**os.environ, "TMPDIR": str(temp_parent)}
            result = subprocess.run([sys.executable, "-B", "-c", wrapper, str(script)],
                                    env=environment, capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(list(temp_parent.iterdir()), [])
            self.assertNotIn("unexpected probe execution", result.stderr)

    def test_fixture_archive_is_owner_only_under_permissive_umask(self):
        script = Path(__file__).parent / "fixtures/large-review/development/freeze.py"
        wrapper = ("import os, runpy, sys\nos.umask(0)\n"
                   "sys.argv = sys.argv[1:]\n"
                   "runpy.run_path(sys.argv[0], run_name='__main__')\n")
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "fixture.tar.gz"
            subprocess.run([sys.executable, "-B", "-c", wrapper, str(script), "--archive", str(archive)],
                           check=True, capture_output=True, text=True, timeout=10)
            self.assertEqual(archive.stat().st_mode & 0o777, 0o600)
            with tarfile.open(archive, "r:gz") as bundle:
                self.assertIn("freeze-manifest.json", bundle.getnames())

    def test_private_export_and_explicit_publication_use_distinct_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "public"
            with patch.object(review_export, "report", return_value=private_report()):
                publication.publish("unused", destination)
            self.assertEqual([path.name for path in destination.iterdir()], ["publication.json"])
            publication.check_path("scripts/agent-eval/publications/example/publication.json",
                                   (destination / "publication.json").read_bytes())
            self.assertEqual(destination.stat().st_mode & 0o777, 0o700)
            self.assertEqual((destination / "publication.json").stat().st_mode & 0o777, 0o600)

    def test_gate_rejects_raw_receipts_archives_and_modified_legacy_results(self):
        for name in ("scripts/agent-eval/results/new/results.json",
                     "scripts/agent-eval/publications/example/raw-trace.json",
                     "scripts/agent-eval/publications/example/evidence.zip",
                     "scripts/agent-eval/fixtures/new/proof.tar.gz",
                     "scripts/agent-eval/fixtures/new/recurring-handoff.json",
                     *publication.LEGACY):
            with self.subTest(name=name), self.assertRaises(InvalidLedger):
                publication.check_path(name, b'{}')

    def test_gate_reads_staged_bytes_even_when_the_worktree_was_cleaned(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo = Path(temporary) / "repo"
            repo.mkdir()
            subprocess.run(["git", "init", "--quiet", "--template=", str(repo)], check=True)
            name = "scripts/agent-eval/publications/example/publication.json"
            path = repo / name
            path.parent.mkdir(parents=True)
            clean = publication.project_publication(private_report())
            staged = {**clean, "authority": "private conversation prose"}
            path.write_text(json.dumps(staged))
            subprocess.run(["git", "-C", str(repo), "add", "--", name], check=True)
            path.write_text(json.dumps(clean))
            publication.check_path(name, path.read_bytes())
            with self.assertRaises(InvalidLedger):
                publication.check_repository(repo)

    def test_pre_push_rejects_a_private_intermediate_commit_deleted_at_tip(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            repo = root / "repo"
            repo.mkdir()
            subprocess.run(["git", "init", "--quiet", "--template=", str(repo)], check=True)
            signer = review_cases.create_signer(root / "private-signer")
            parent = None
            commits = []
            for files in ({}, {"scripts/agent-eval/results/private/receipt.json": "{}\n"}, {}):
                tree = review_cases._tree(repo, files)
                signed = review_cases._signed_commit(tree, parent, signer)
                parent = review_cases._git(repo, "hash-object", "-t", "commit", "-w", "--stdin",
                                           data=signed["payload"].encode())
                commits.append(parent)
            publication.check_repository(repo, commits[-1])
            for remote in ("destination", "unrelated"):
                subprocess.run(["git", "-C", str(repo), "remote", "add", remote,
                                "https://example.invalid/" + remote], check=True)
            subprocess.run(["git", "-C", str(repo), "update-ref", "refs/remotes/destination/main", commits[0]], check=True)
            subprocess.run(["git", "-C", str(repo), "update-ref", "refs/remotes/unrelated/private", commits[1]], check=True)
            updates = ["refs/heads/new " + commits[-1] + " refs/heads/new " + "0" * 40]
            with self.assertRaises(InvalidLedger):
                publication.check_outgoing(repo, updates, "destination")
            # An unrelated remote's raw ancestor must not hide an existing ref's outgoing data either.
            existing = ["refs/heads/new " + commits[-1] + " refs/heads/new " + commits[0]]
            with self.assertRaises(InvalidLedger):
                publication.check_outgoing(repo, existing, "destination")
            with self.assertRaises(InvalidLedger):
                publication.check_outgoing(repo, updates, "missing")
            # A deletion does not publish additional commit objects.
            self.assertEqual(publication.check_outgoing(repo, ["refs/heads/new " + "0" * 40
                                                               + " refs/heads/new " + commits[-1]]), 0)

    def test_publication_json_rejects_duplicate_keys_with_hidden_private_values(self):
        value = publication.project_publication(private_report())
        encoded = json.dumps(value).replace('"kind":', '"kind": "private conversational text", "kind":', 1)
        with self.assertRaises(InvalidLedger):
            publication.check_path("scripts/agent-eval/publications/example/publication.json", encoded.encode())

    def test_private_calibration_requires_exact_condition_and_complete_proof(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "calibration.json"
            evidence = {"schema": 1, "cli_version": "codex-cli synthetic", "codex_sha256": "1" * 64,
                        "model": review_campaign.MODEL, "effort": review_campaign.EFFORT,
                        "usage_scope": "thread-cumulative", "proof": {key: True for key in (
                            "same_thread", "native_before_is_exact_prefix", "prior_responses_unchanged",
                            "new_response_equals_cumulative_delta", "exec_equals_final_native",
                            "stream_complete", "process_tree_drained")}}
            path.write_text(json.dumps(evidence))
            scope, receipt = review_campaign.calibrated_usage_scope("codex-cli synthetic", "1" * 64, path)
            self.assertEqual(scope, "thread-cumulative")
            self.assertEqual(receipt["evidence_sha256"], review_campaign.file_hash(path))
            self.assertEqual(review_campaign.calibrated_usage_scope("codex-cli synthetic", "1" * 64), ("unknown", None))
            with self.assertRaises(InvalidLedger):
                review_campaign.calibrated_usage_scope("codex-cli different", "1" * 64, path)
            evidence["proof"] = "invalid private proof"
            path.write_text(json.dumps(evidence))
            with self.assertRaises(InvalidLedger):
                review_campaign.calibrated_usage_scope("codex-cli synthetic", "1" * 64, path)
            evidence["proof"] = {"same_thread": False}
            path.write_text(json.dumps(evidence))
            with self.assertRaises(InvalidLedger):
                review_campaign.calibrated_usage_scope("codex-cli synthetic", "1" * 64, path)


if __name__ == "__main__":
    unittest.main()
