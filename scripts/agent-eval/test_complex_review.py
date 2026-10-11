import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from accounting import InvalidLedger, fingerprint
import review_campaign as campaign
import review_cases

FIXTURES = Path(__file__).resolve().parent / "fixtures/complex-review"


def load_generator():
    """The bundle generator module, loaded without bytecode or leaving its directory on the import path."""
    saved, bytecode = list(sys.path), sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    try:
        spec = importlib.util.spec_from_file_location("complex_review_build", FIXTURES / "build.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
    finally:
        sys.path[:] = saved
        sys.dont_write_bytecode = bytecode
    return module


def catalog(names, partition):
    return [{"case_id": name, "partition": partition, "title": name, "step_count": 1,
             "fixture_sha256": fingerprint(name)} for name in names]


class Stop(Exception):
    pass


class ComplexStageTests(unittest.TestCase):
    def test_complex_stages_pair_two_cases_across_two_repeats_in_their_own_partition(self):
        for stage, names, partition in (("complex-development", campaign.COMPLEX_DEVELOPMENT_CASES, "development"),
                                        ("complex-holdout", campaign.COMPLEX_HOLDOUT_CASES, "holdout"),
                                        ("complex-holdout-b", campaign.COMPLEX_HOLDOUT_B_CASES, "holdout")):
            with self.subTest(stage=stage):
                plan = campaign.build_plan(stage, 20261010, {"runtime": "claude"}, catalog(names, partition))
                self.assertEqual(plan["assignment_count"], 8)
                self.assertEqual(plan["repetitions"], 2)
                self.assertEqual({run["case_id"] for run in plan["assignments"]}, set(names))
                other = "holdout" if partition == "development" else "development"
                with self.assertRaisesRegex(InvalidLedger, "partition"):
                    campaign.build_plan(stage, 20261010, {}, catalog(names, other))
                multi_step = catalog(names, partition)
                multi_step[0]["step_count"] = 2
                with self.assertRaisesRegex(InvalidLedger, "one invocation"):
                    campaign.build_plan(stage, 20261010, {}, multi_step)

    def test_only_complex_stages_raise_the_timeout_ceiling(self):
        self.assertEqual(campaign.stage_timeout_seconds("complex-development"), 1200)
        self.assertEqual(campaign.stage_timeout_seconds("complex-holdout"), 1200)
        self.assertEqual(campaign.stage_timeout_seconds("complex-holdout-b"), 1200)
        self.assertEqual(campaign.stage_timeout_seconds("large-holdout"), 600)
        self.assertEqual(campaign.validated_timeout_seconds(1200, campaign.stage_timeout_seconds("complex-holdout")), 1200)
        with self.assertRaisesRegex(InvalidLedger, "at most 1200"):
            campaign.validated_timeout_seconds(1201, 1200)
        with self.assertRaisesRegex(InvalidLedger, "at most 600"):
            campaign.validated_timeout_seconds(601)

    def test_claude_prepare_defaults_complex_stages_to_their_ceiling_and_binds_the_suite(self):
        with tempfile.TemporaryDirectory() as temporary:
            bundles = {}
            for suite in ("complex", "large"):
                bundles[suite] = Path(temporary) / suite
                bundles[suite].mkdir()
                (bundles[suite] / "catalog.json").write_text(json.dumps({"schema": 1, "suite": suite, "cases": []}))
            arguments = {"seed": 20261010, "runtime": "claude", "claude_binary": "/unexecuted/claude",
                         "model": "claude-haiku-5-5", "reposcout_binary": "/unexecuted/reposcout",
                         "skill_dir": "/unexecuted/skill"}
            for stage, suite, timeout, expected in (("complex-holdout", "complex", None, (1200, 1200)),
                                                    ("complex-holdout-b", "complex", None, (1200, 1200)),
                                                    ("complex-development", "complex", 900, (900, 1200)),
                                                    ("large-holdout", "large", None, (180, 600))):
                with self.subTest(stage=stage, timeout=timeout), \
                        patch.object(campaign, "capture_claude_pins", side_effect=Stop) as capture, \
                        self.assertRaises(Stop):
                    campaign.prepare(Path(temporary) / "unused", stage=stage, bundle_root=bundles[suite],
                                     timeout_seconds=timeout, **arguments)
                self.assertEqual((capture.call_args.kwargs["timeout_seconds"],
                                  capture.call_args.kwargs["timeout_ceiling"]), expected)
            for stage, suite in (("complex-holdout", "large"), ("complex-holdout-b", "large"), ("large-holdout", "complex")):
                with self.subTest(stage=stage, suite=suite), \
                        patch.object(campaign, "capture_claude_pins", side_effect=Stop) as capture, \
                        self.assertRaisesRegex(InvalidLedger, "complex fixture bundle"):
                    campaign.prepare(Path(temporary) / "unused", stage=stage, bundle_root=bundles[suite], **arguments)
                capture.assert_not_called()
            with self.assertRaisesRegex(InvalidLedger, "explicit separate fixture bundle"):
                campaign.prepare(Path(temporary) / "unused", stage="complex-holdout", **arguments)

    def test_complex_suite_raises_only_its_own_source_ceiling(self):
        with tempfile.TemporaryDirectory() as temporary:
            bundle = Path(temporary)
            for suite, files, size, accepted in (("complex", 512, 4 * 1024 * 1024, True),
                                                 ("complex", 513, 1024, False),
                                                 ("complex", 64, 4 * 1024 * 1024 + 1, False),
                                                 ("large", 257, 1024, False),
                                                 ("large", 64, 2 * 1024 * 1024 + 1, False),
                                                 ("unknown", 64, 1024, False)):
                (bundle / "catalog.json").write_text(json.dumps({
                    "suite": suite, "source_limits": {"max_files": files, "max_bytes": size}, "cases": []}))
                with self.subTest(suite=suite, files=files, size=size):
                    if accepted:
                        self.assertEqual(review_cases._source_limits(bundle), (files, size))
                    else:
                        with self.assertRaises(InvalidLedger):
                            review_cases._source_limits(bundle)


class ComplexBundleTests(unittest.TestCase):
    def test_generated_bundles_match_the_stage_presets_and_the_holdout_seal(self):
        generator = load_generator()
        expected = {"development": campaign.COMPLEX_DEVELOPMENT_CASES, "holdout": campaign.COMPLEX_HOLDOUT_CASES,
                    "holdout-b": campaign.COMPLEX_HOLDOUT_B_CASES}
        with tempfile.TemporaryDirectory() as temporary:
            for partition, names in expected.items():
                with self.subTest(partition=partition):
                    bundle = Path(temporary) / partition
                    generator.materialize(partition, bundle)
                    self.assertEqual(review_cases.bundle_suite(bundle), "complex")
                    cases = review_cases.list_cases(include_holdout=True, bundle_root=bundle)
                    self.assertEqual([case["case_id"] for case in cases], list(names))
                    catalog_partition = "holdout" if partition.startswith("holdout") else partition
                    self.assertTrue(all(case["partition"] == catalog_partition and case["step_count"] == 1 for case in cases))
                    for name in names:
                        snapshots = review_cases.snapshots(name, bundle_root=bundle)
                        self.assertFalse(any("oracle" in path or "probe" in path for path in snapshots["head"]))
                        oracle = json.loads((bundle / name / "oracle.json").read_text())
                        review_cases._resolve_obligations(
                            oracle, snapshots, [{"base_revision": "base", "head_revision": "head"}])
                    if partition in generator.SEALS:
                        self.assertEqual(campaign.directory_hash(bundle)["sha256"], generator.SEALS[partition])


if __name__ == "__main__":
    unittest.main()
