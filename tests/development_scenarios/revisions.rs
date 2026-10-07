//! Committed review journeys; expectations come from `docs/review-context.md` and
//! `docs/source-queries.md`, including deliberate asymmetry for absent declarations.

use super::support::Fixture;
use reposcout::metrics::tokens::TokenCounter;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};

fn review_command(fixture: &Fixture, base: &str, head: &str) -> assert_cmd::Command {
    fixture.command(&[
        "review-context",
        ".",
        "--base",
        base,
        "--head",
        head,
        "--no-cache",
        "--no-project-config",
    ])
}

fn review(fixture: &Fixture, base: &str, head: &str, extra: &[&str]) -> Value {
    let output = review_command(fixture, base, head)
        .args([
            "--budget",
            "65536",
            "--max-output-bytes",
            "1048576",
            "-f",
            "json",
            "--quiet",
        ])
        .args(extra)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

fn change<'a>(report: &'a Value, path: &str) -> &'a Value {
    report["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["base"]["path"] == path || change["head"]["path"] == path)
        .unwrap_or_else(|| panic!("missing change {path}: {report}"))
}

fn candidate<'a>(report: &'a Value, side: &str, path: &str) -> &'a Value {
    report["context"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["side"] == side && candidate["path"] == path)
        .unwrap_or_else(|| panic!("missing candidate {side}:{path}: {report}"))
}

fn names(side: &Value) -> BTreeSet<&str> {
    let definitions = side["definitions"].as_array().unwrap();
    let names: BTreeSet<_> = definitions
        .iter()
        .map(|definition| definition["symbol"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names.len(),
        definitions.len(),
        "duplicate declaration identity: {side}"
    );
    names
}

type CallIdentity<'a> = (&'a str, &'a str, &'a str, &'a str, u64);

fn cross_file_calls<'a>(report: &'a Value, side: &str) -> BTreeSet<CallIdentity<'a>> {
    let relations: Vec<_> = report["relations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|relation| {
            relation["side"] == side
                && relation["kind"] == "symbol-reference"
                && relation["edge"]["source"] != relation["edge"]["target"]
        })
        .collect();
    let calls: BTreeSet<_> = relations
        .iter()
        .map(|relation| {
            assert_eq!(relation["change_basis"], "changed-definition");
            let symbol = &relation["symbol"];
            assert_eq!(symbol["kind"], "call");
            assert_eq!(symbol["source"]["path"], relation["edge"]["source"]);
            assert_eq!(symbol["target"]["path"], relation["edge"]["target"]);
            assert_eq!(symbol["source"]["kind"], "function");
            assert_eq!(symbol["target"]["kind"], "function");
            assert_eq!(symbol["site"]["start_line"], symbol["site"]["end_line"]);
            assert!(
                symbol["site"]["start_byte"].as_u64().unwrap()
                    < symbol["site"]["end_byte"].as_u64().unwrap()
            );
            for endpoint in ["source", "target"] {
                let path = symbol[endpoint]["path"].as_str().unwrap();
                assert_eq!(
                    symbol[endpoint]["source_hash"].as_str().unwrap(),
                    candidate(report, side, path)["sha256"].as_str().unwrap()
                );
            }
            (
                symbol["source"]["path"].as_str().unwrap(),
                symbol["source"]["name"].as_str().unwrap(),
                symbol["target"]["path"].as_str().unwrap(),
                symbol["target"]["name"].as_str().unwrap(),
                symbol["site"]["start_line"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        calls.len(),
        relations.len(),
        "duplicate call evidence: {report}"
    );
    calls
}

fn assert_complete_capture(report: &Value) {
    for key in [
        "changes_not_analyzed",
        "changes_without_hunks",
        "unknown_candidate_sizes",
    ] {
        assert_eq!(report["totals"][key], 0, "{key}: {report}");
    }
    assert_eq!(report["coverage"].as_array().unwrap().len(), 2);
    for coverage in report["coverage"].as_array().unwrap() {
        assert_eq!(coverage["inventory_truncated"], false);
        assert_eq!(coverage["parse_errors"], 0);
        assert_eq!(coverage["config_errors"], 0);
        assert!(
            coverage["unavailable_files"]
                .as_object()
                .unwrap()
                .is_empty()
        );
    }
}

fn assert_no_projection_omissions(report: &Value) {
    for key in [
        "changes_omitted",
        "definitions_omitted",
        "relations_omitted",
        "candidates_omitted",
    ] {
        assert_eq!(report["totals"][key], 0, "{key}: {report}");
    }
}

const GUARDED_API: &str = "pub fn stable_before(value: i32) -> i32 { value + 10 }\n\
pub fn validate(value: i32) -> i32 {\n    assert!(value >= 0);\n    value\n}\n\
pub fn nested(value: i32) -> i32 {\n    fn inner(value: i32) -> i32 {\n        assert!(value < 100);\n        value + 1\n    }\n    inner(value)\n}\n\
pub fn stable_after(value: i32) -> i32 { value + 20 }\n";

fn guard_fixture() -> (Fixture, String, String) {
    let fixture = Fixture::new("review-guard-refactor");
    fixture.write(
        "Cargo.toml",
        "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n",
    );
    fixture.write(
        "src/lib.rs",
        "pub mod api;\npub mod checkout;\npub mod audit;\npub mod stable;\npub mod decoy;\n",
    );
    fixture.write("src/api.rs", GUARDED_API);
    fixture.write(
        "src/checkout.rs",
        "pub fn total() -> i32 { crate::api::validate(-1) }\n",
    );
    fixture.write(
        "src/audit.rs",
        "pub fn record() -> i32 { crate::api::validate(2) }\n",
    );
    fixture.write(
        "src/stable.rs",
        "pub fn keep() -> i32 { crate::api::stable_before(1) + crate::api::nested(2) }\n",
    );
    fixture.write(
        "src/decoy.rs",
        "fn validate(value: i32) -> i32 { value }\npub fn unrelated() -> i32 { validate(9) }\n",
    );
    let base = fixture.commit("guarded API with real and unrelated consumers");
    let mut updated = GUARDED_API
        .replace("    assert!(value >= 0);\n", "")
        .replace("        assert!(value < 100);\n", "");
    updated.push_str("pub fn fresh() -> i32 { 42 }\n");
    fixture.write("src/api.rs", &updated);
    let head = fixture.commit("remove body guards and add an independent declaration");
    (fixture, base, head)
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn guard_edits_preserve_callers_in_both_directions_without_changing_neighbors() {
    let (fixture, guarded, unguarded) = guard_fixture();
    for (base, head, receiver) in [
        (&guarded, &unguarded, "head"),
        (&unguarded, &guarded, "base"),
    ] {
        let report = review(&fixture, base, head, &[]);
        assert_complete_capture(&report);
        assert_no_projection_omissions(&report);
        assert_eq!(report["totals"]["changes"], 1);
        let changed = change(&report, "src/api.rs");
        for side in ["base", "head"] {
            let expected = if side == receiver {
                BTreeSet::from(["fresh", "inner", "validate"])
            } else {
                BTreeSet::from(["inner", "validate"])
            };
            assert_eq!(names(&changed[side]), expected);
            assert_eq!(changed[side]["mapping_status"], "available");
            assert_eq!(changed[side]["unprocessed_ranges"], 0);
            assert_eq!(changed[side]["ambiguous_definitions"], 0);
            assert_eq!(
                changed[side]["counterpart_definitions"]
                    .as_u64()
                    .unwrap_or(0),
                if side == receiver { 2 } else { 0 }
            );
            // Calling the unchanged enclosing function or a same-spelled decoy is not
            // evidence of calling either changed declaration.
            assert_eq!(
                cross_file_calls(&report, side),
                BTreeSet::from([
                    ("src/audit.rs", "record", "src/api.rs", "validate", 1),
                    ("src/checkout.rs", "total", "src/api.rs", "validate", 1),
                ])
            );
            for caller in ["src/audit.rs", "src/checkout.rs"] {
                assert!(
                    candidate(&report, side, caller)["roles"]
                        .as_array()
                        .unwrap()
                        .contains(&Value::from("concrete-reference-source"))
                );
            }
        }
    }
}

fn alias_config(target: &str) -> String {
    format!(
        "{{\"compilerOptions\":{{\"baseUrl\":\".\",\"paths\":{{\"@pricing\":[\"{target}\"]}}}}}}\n"
    )
}

fn alias_fixture() -> (Fixture, String, String) {
    let fixture = Fixture::new("review-alias-migration");
    fixture.write("package.json", "{\"name\":\"shop\",\"private\":true}\n");
    fixture.write("tsconfig.json", &alias_config("packages/domain/v1.ts"));
    fixture.write(
        "packages/domain/v1.ts",
        "export function price() { return 100; }\n",
    );
    fixture.write(
        "packages/domain/v2.ts",
        "export function price() { return 200; }\n",
    );
    fixture.write(
        "src/checkout.ts",
        "import { price as quote } from '@pricing';\nexport function total() { return quote(); }\n",
    );
    fixture.write(
        "src/app.ts",
        "import { total } from './checkout';\nexport function start() { return total(); }\n",
    );
    fixture.write(
        "src/direct.ts",
        "import { price } from '../packages/domain/v1';\nexport function legacy() { return price(); }\n",
    );
    fixture.write(
        "src/shadow.ts",
        "import { price } from '@pricing';\nexport function injected(price: () => number) { return price(); }\n",
    );
    fixture.write(
        "src/unrelated.ts",
        "function price() { return 9; }\nexport function local() { return price(); }\n",
    );
    let base = fixture.commit("old pricing alias and independent consumers");
    fixture.write("tsconfig.json", &alias_config("packages/domain/v2.ts"));
    fixture.write(
        "packages/domain/v1.ts",
        "export function price() { return 101; }\n",
    );
    fixture.write(
        "packages/domain/v2.ts",
        "export function price() { return 201; }\n",
    );
    let head = fixture.commit("migrate alias while updating both pricing implementations");
    (fixture, base, head)
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn pricing_alias_migration_keeps_revision_local_evidence_despite_dirty_index_and_worktree() {
    let (fixture, base, head) = alias_fixture();
    let flags = ["--source", "--diff", "--context-max-files", "64"];
    let original = review(&fixture, &base, &head, &flags);
    assert_complete_capture(&original);
    assert_no_projection_omissions(&original);
    assert_eq!(original["totals"]["changes"], 3);
    for (side, resolved) in [
        ("base", "packages/domain/v1.ts"),
        ("head", "packages/domain/v2.ts"),
    ] {
        assert_eq!(
            cross_file_calls(&original, side),
            BTreeSet::from([
                ("src/checkout.ts", "total", resolved, "price", 2),
                (
                    "src/direct.ts",
                    "legacy",
                    "packages/domain/v1.ts",
                    "price",
                    2
                ),
            ])
        );
        for importer in ["src/checkout.ts", "src/shadow.ts"] {
            let edges: Vec<_> = original["relations"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|edge| {
                    edge["side"] == side
                        && edge["kind"] == "import"
                        && edge["edge"]["source"] == importer
                })
                .collect();
            assert_eq!(edges.len(), 1);
            assert_eq!(edges[0]["edge"]["target"], resolved);
            assert_eq!(edges[0]["edge"]["resolver"], "tsconfig-paths");
        }
    }
    fixture.write("tsconfig.json", &alias_config("does-not-exist.ts"));
    fixture.write(
        "packages/domain/v1.ts",
        "export function INDEX_POISON() { return 999; }\n",
    );
    fixture.remove("src/checkout.ts");
    fixture.stage_all();
    fixture.write(
        "packages/domain/v1.ts",
        "export function WORKTREE_POISON() { return 1000; }\n",
    );
    fixture.write(
        "packages/domain/v2.ts",
        "this is deliberately invalid live TypeScript\n",
    );
    fixture.write("untracked.ts", "export function UNTRACKED_POISON() {}\n");
    let index_before = fs::read(fixture.path().join(".git/index")).unwrap();
    assert_eq!(review(&fixture, &base, &head, &flags), original);
    assert_eq!(
        fs::read(fixture.path().join(".git/index")).unwrap(),
        index_before
    );
    for (side, source) in [
        ("base", "export function price() { return 100; }\n"),
        ("head", "export function price() { return 101; }\n"),
    ] {
        let file = candidate(&original, side, "packages/domain/v1.ts");
        assert_eq!(file["source"], source);
        assert_eq!(file["sha256"], source_hash(source));
    }
}

fn lifecycle_fixture() -> (Fixture, String, String) {
    let fixture = Fixture::new("review-file-lifecycle");
    fixture.write(
        "src/old_name.ts",
        "export function catalog() {\n    return ['books', 'games', 'music'];\n}\n",
    );
    fixture.write(
        "src/runner.ts",
        "import { catalog } from './old_name';\nexport function run() { return catalog(); }\n",
    );
    fixture.write(
        "src/obsolete.rs",
        "pub struct LegacyRecord {\n    pub id: u64,\n    pub active: bool,\n}\n",
    );
    fixture.write("tools/job.py", "def run():\n    return 'scheduled'\n");
    fixture.write("notes.custom", "old migration note\n");
    fs::set_permissions(
        fixture.path().join("tools/job.py"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let base = fixture.commit("catalog and legacy inventory");
    fs::rename(
        fixture.path().join("src/old_name.ts"),
        fixture.path().join("src/catalog.ts"),
    )
    .unwrap();
    fixture.write(
        "src/runner.ts",
        "import { catalog } from './catalog';\nexport function run() { return catalog(); }\n",
    );
    fixture.remove("src/obsolete.rs");
    fixture.write(
        "src/new_feature.ts",
        "export function welcome(user: string) { return `Welcome ${user}!`; }\n",
    );
    fixture.write("notes.custom", "completed migration note\n");
    fs::set_permissions(
        fixture.path().join("tools/job.py"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let head = fixture.commit("rename catalog, retire legacy, add welcome and enable job");
    (fixture, base, head)
}

fn changed_path(side: &Value) -> &str {
    if side.is_null() {
        ""
    } else {
        side["path"].as_str().unwrap()
    }
}

fn identities(report: &Value) -> BTreeSet<(&str, &str, &str)> {
    let changes = report["changes"].as_array().unwrap();
    let identities: BTreeSet<_> = changes
        .iter()
        .map(|change| {
            (
                change["status"].as_str().unwrap(),
                changed_path(&change["base"]),
                changed_path(&change["head"]),
            )
        })
        .collect();
    assert_eq!(
        identities.len(),
        changes.len(),
        "duplicate change identity: {report}"
    );
    identities
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn file_lifecycle_preserves_raw_inventory_without_inventing_text_or_absent_sides() {
    let (fixture, base, head) = lifecycle_fixture();
    let expected = BTreeSet::from([
        ("added", "", "src/new_feature.ts"),
        ("deleted", "src/obsolete.rs", ""),
        ("modified", "notes.custom", "notes.custom"),
        ("modified", "src/runner.ts", "src/runner.ts"),
        ("modified", "tools/job.py", "tools/job.py"),
        ("renamed", "src/old_name.ts", "src/catalog.ts"),
    ]);
    let forward = review(&fixture, &base, &head, &[]);
    let reverse = review(&fixture, &head, &base, &[]);
    assert_eq!(identities(&forward), expected);
    let reversed: BTreeSet<_> = expected
        .into_iter()
        .map(|(status, old, new)| {
            let status = match status {
                "added" => "deleted",
                "deleted" => "added",
                other => other,
            };
            (status, new, old)
        })
        .collect();
    assert_eq!(identities(&reverse), reversed);
    for report in [&forward, &reverse] {
        assert_complete_capture(report);
        assert_no_projection_omissions(report);
        assert_eq!(report["comparison"]["rename_detection_complete"], true);
        for path in ["src/old_name.ts", "tools/job.py"] {
            let unchanged_text = change(report, path);
            assert_eq!(unchanged_text["hunks"], 0);
            assert_eq!(unchanged_text["hunk_status"], "available");
            assert_eq!(
                unchanged_text["base"]["sha256"],
                unchanged_text["head"]["sha256"]
            );
            assert_eq!(
                unchanged_text["base"]["blob"],
                unchanged_text["head"]["blob"]
            );
            for side in ["base", "head"] {
                assert!(names(&unchanged_text[side]).is_empty());
                assert_eq!(unchanged_text[side]["ranges"], serde_json::json!([]));
            }
        }
        let raw = change(report, "notes.custom");
        assert_eq!(raw["hunks"], 1);
        for side in ["base", "head"] {
            assert_eq!(raw[side]["extraction"], "unsupported");
            assert!(names(&raw[side]).is_empty());
            assert_ne!(
                raw[side]["ranges"].as_array().unwrap().as_slice(),
                &[] as &[Value]
            );
        }
    }
    assert!(change(&forward, "src/new_feature.ts")["base"].is_null());
    assert!(change(&forward, "src/obsolete.rs")["head"].is_null());
    assert_eq!(change(&forward, "tools/job.py")["base"]["mode"], 0o100_644);
    assert_eq!(change(&forward, "tools/job.py")["head"]["mode"], 0o100_755);
    for (revision, existing, missing) in [
        (&base, "src/old_name.ts", "src/catalog.ts"),
        (&head, "src/catalog.ts", "src/old_name.ts"),
    ] {
        assert_eq!(
            read(&fixture, revision, existing, "catalog", None)["results"][0]["status"],
            "complete"
        );
        assert_eq!(
            read(&fixture, revision, missing, "catalog", None)["results"][0]["status"],
            "not-found"
        );
    }
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn divergent_branch_review_distinguishes_direct_comparison_from_merge_base() {
    let fixture = Fixture::new("review-divergent-branches");
    fixture.write("src/common.ts", "export function value() { return 1; }\n");
    fixture.write("src/deploy.ts", "export const deployment = 'baseline';\n");
    fixture.write(
        "src/client.ts",
        "import { value } from './common';\nexport function consume() { return value(); }\n",
    );
    let ancestor = fixture.commit("shared ancestor");
    fixture.write("src/deploy.ts", "export const deployment = 'production';\n");
    fixture.write(
        "main_only.py",
        "def infrastructure():\n    return {'region': 'eu', 'replicas': 3}\n",
    );
    let main = fixture.commit("main advances deployment independently");
    fixture.checkout(&ancestor);
    fixture.write("src/common.ts", "export function value() { return 2; }\n");
    fixture.write(
        "src/feature_only.ts",
        "export class Promotion { enabled = true; }\n",
    );
    let feature = fixture.commit("feature changes business behavior");
    let direct = review(&fixture, &main, &feature, &[]);
    let merged = review(&fixture, &main, &feature, &["--merge-base"]);
    assert_eq!(
        identities(&direct),
        BTreeSet::from([
            ("modified", "src/common.ts", "src/common.ts"),
            ("modified", "src/deploy.ts", "src/deploy.ts"),
            ("added", "", "src/feature_only.ts"),
            ("deleted", "main_only.py", ""),
        ])
    );
    assert_eq!(
        identities(&merged),
        BTreeSet::from([
            ("modified", "src/common.ts", "src/common.ts"),
            ("added", "", "src/feature_only.ts"),
        ])
    );
    assert_eq!(direct["comparison"]["mode"], "direct");
    assert_eq!(direct["comparison"]["base_commit"], main);
    assert_eq!(merged["comparison"]["mode"], "merge-base");
    assert_eq!(merged["comparison"]["requested_base_commit"], main);
    assert_eq!(merged["comparison"]["base_commit"], ancestor);
    for report in [&direct, &merged] {
        assert_complete_capture(report);
        assert_no_projection_omissions(report);
        assert_eq!(report["comparison"]["head_commit"], feature);
        for side in ["base", "head"] {
            assert_eq!(
                cross_file_calls(report, side),
                BTreeSet::from([("src/client.ts", "consume", "src/common.ts", "value", 2)])
            );
            assert_eq!(
                names(&change(report, "src/common.ts")[side]),
                BTreeSet::from(["value"])
            );
        }
    }
}

fn dense_api(value: u32) -> String {
    let mut source = String::new();
    for number in 0..32 {
        writeln!(source, "export function f{number}() {{ return {value}; }}").unwrap();
    }
    source
}

fn assert_projection_accounting(report: &Value) {
    assert_eq!(report["totals"]["definitions"], 66);
    for (field, array) in [
        ("changes", "changes"),
        ("relations", "relations"),
        ("candidates", "context"),
    ] {
        assert_eq!(
            report["totals"][field].as_u64().unwrap(),
            u64::try_from(report[array].as_array().unwrap().len()).unwrap()
                + report["totals"][format!("{field}_omitted")]
                    .as_u64()
                    .unwrap()
        );
    }
    let mut shown = 0_u64;
    let mut omitted = 0_u64;
    for change in report["changes"].as_array().unwrap() {
        for side in ["base", "head"] {
            shown += u64::try_from(change[side]["definitions"].as_array().unwrap().len()).unwrap();
            omitted += change[side]["definitions_omitted"].as_u64().unwrap_or(0);
            assert_eq!(change[side]["mapping_status"], "available");
        }
    }
    assert_eq!(report["totals"]["definitions_omitted"], omitted);
    assert_eq!(
        shown + omitted,
        66,
        "32 API declarations and one small declaration per side"
    );
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn constrained_review_keeps_small_changes_and_costs_under_filename_permutations() {
    for (dense, small) in [("a.ts", "z.ts"), ("z.ts", "a.ts")] {
        let fixture = Fixture::new("review-budget-fairness");
        fixture.write(dense, &dense_api(1));
        fixture.write(small, "export function small() { return 10; }\n");
        fixture.write(
            "dense-client.ts",
            &format!(
                "import {{ f0 }} from './{}';\nexport function denseClient() {{ return f0(); }}\n",
                dense.trim_end_matches(".ts")
            ),
        );
        fixture.write("small-client.ts", &format!("import {{ small }} from './{}';\nexport function smallClient() {{ return small(); }}\n", small.trim_end_matches(".ts")));
        let base = fixture.commit("two independent APIs and their consumers");
        fixture.write(dense, &dense_api(2));
        fixture.write(small, "export function small() { return 20; }\n");
        let head = fixture.commit("dense maintenance and a small independent behavior change");
        let output = review_command(&fixture, &base, &head)
            .args(["--context", "-f", "json", "--quiet"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let compact: Value = serde_json::from_slice(&output).unwrap();
        let full = review(&fixture, &base, &head, &["--context", "--source", "--diff"]);
        let counter = TokenCounter::new("o200k_base").unwrap();
        assert!(output.len() <= 65_536);
        assert!(counter.count(std::str::from_utf8(&output).unwrap()) <= 4096);
        assert_complete_capture(&compact);
        assert_complete_capture(&full);
        assert_no_projection_omissions(&full);
        assert_eq!(compact["totals"]["changes_omitted"], 0);
        assert_eq!(
            identities(&compact),
            BTreeSet::from([("modified", dense, dense), ("modified", small, small)])
        );
        assert_projection_accounting(&compact);
        assert_projection_accounting(&full);
        for side in ["base", "head"] {
            assert_eq!(
                names(&change(&compact, small)[side]),
                BTreeSet::from(["small"])
            );
        }
        for key in [
            "candidate_tokens",
            "candidate_bytes",
            "diff_tokens",
            "selected_files",
            "selected_tokens",
        ] {
            assert_eq!(
                compact["totals"][key], full["totals"][key],
                "projection changed {key}"
            );
        }
        assert_eq!(full["totals"]["candidates"], 8);
        assert_eq!(full["totals"]["selected_files"], 8);
        assert_eq!(full["totals"]["source_files"], 8);
        assert!(
            compact["context"]
                .as_array()
                .unwrap()
                .iter()
                .all(|file| file.get("source").is_none())
        );
        assert!(
            compact["changes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|change| change.get("diff").is_none())
        );
        let mut measured_tokens = 0_u64;
        for file in full["context"].as_array().unwrap() {
            let source = file["source"].as_str().unwrap();
            assert_eq!(file["bytes"], source.len());
            assert_eq!(file["sha256"], source_hash(source));
            measured_tokens += u64::try_from(counter.count(source)).unwrap();
        }
        assert_eq!(full["totals"]["candidate_tokens"], measured_tokens);
    }
}

fn source_hash(source: &str) -> String {
    let mut hash = String::with_capacity(64);
    for byte in Sha256::digest(source.as_bytes()) {
        write!(hash, "{byte:02x}").unwrap();
    }
    hash
}

fn read(fixture: &Fixture, revision: &str, path: &str, symbol: &str, hash: Option<&str>) -> Value {
    read_with_flags(fixture, revision, path, symbol, hash, &[])
}

fn read_with_flags(
    fixture: &Fixture,
    revision: &str,
    path: &str,
    symbol: &str,
    hash: Option<&str>,
    extra: &[&str],
) -> Value {
    let mut args = vec![
        "read",
        ".",
        "--snapshot",
        revision,
        "--symbol",
        path,
        symbol,
        "--no-cache",
        "--no-project-config",
    ];
    if let Some(hash) = hash {
        args.extend(["--expect-hash", path, hash]);
    }
    args.extend_from_slice(extra);
    fixture.json(&args)
}

fn assert_no_source(report: &Value) {
    if let Some(sources) = report.get("sources") {
        assert_eq!(sources.as_array().unwrap().as_slice(), &[] as &[Value]);
    }
}

fn assert_read_content(read: &Value, source: &str, tree: &str) {
    assert_eq!(read["requested_targets"], 1);
    assert_eq!(read["omitted_targets"], 0);
    assert_eq!(read["results"][0]["status"], "complete");
    assert_eq!(read["sources"].as_array().unwrap().len(), 1);
    assert_eq!(read["sources"][0]["content"], source.trim_end_matches('\n'));
    assert_eq!(read["files"][0]["sha256"], source_hash(source));
    assert_eq!(
        read["files"][0]["snapshot"],
        serde_json::json!({"kind": "tree", "revision": tree})
    );
}

fn assert_non_directory_parent_policy(
    fixture: &Fixture,
    base: &str,
    head: &str,
    snapshots: &[(&str, &str, Value)],
) {
    let safe = ["--profile", "safe"];
    let safe_report = review(fixture, base, head, &safe);
    let safe_reads: Vec<_> = snapshots
        .iter()
        .map(|(tree, hash, original)| {
            let result = read_with_flags(fixture, tree, "src/api.ts", "answer", Some(hash), &safe);
            assert_eq!(result, *original);
            result
        })
        .collect();
    fs::remove_file(fixture.path().join("src")).unwrap();
    fixture.write("src", "this is a current file, not a directory\n");

    // Historical source is pinned, but current ignore policy still applies. A
    // non-directory parent makes policy lookup fail; safe mode explicitly skips it.
    let policy_failure = review(fixture, base, head, &[]);
    let changed = change(&policy_failure, "src/api.ts");
    for side in ["base", "head"] {
        assert_eq!(changed[side]["status"], "ignore-error");
        assert!(changed[side]["sha256"].is_null());
    }
    assert_eq!(changed["hunk_status"], "unavailable");
    assert!(changed["hunks"].is_null());
    assert_eq!(policy_failure["totals"]["changes_without_hunks"], 1);
    assert_eq!(policy_failure["totals"]["unknown_candidate_sizes"], 2);
    assert_eq!(review(fixture, base, head, &safe), safe_report);
    for ((tree, hash, _), original) in snapshots.iter().zip(safe_reads) {
        let blocked = read(fixture, tree, "src/api.ts", "answer", Some(hash));
        assert_eq!(blocked["results"][0]["status"], "ignore-error");
        assert_no_source(&blocked);
        assert_eq!(
            read_with_flags(fixture, tree, "src/api.ts", "answer", Some(hash), &safe),
            original
        );
    }
    let index = read_with_flags(
        fixture,
        "index",
        "src/api.ts",
        "answer",
        Some(snapshots[1].1),
        &safe,
    );
    assert_eq!(index["results"][0]["status"], "complete");
    assert_eq!(index["files"][0]["sha256"], snapshots[1].1);
    for revision in [snapshots[1].0, "index"] {
        let alias = read_with_flags(fixture, revision, "src/alias.ts", "answer", None, &safe);
        assert_eq!(alias["results"][0]["status"], "not-regular-file");
        assert_no_source(&alias);
    }
    // Worktree traversal cannot resolve a child below a regular file; the source
    // query reports the failed filesystem read rather than supplying Git content.
    let live = read_with_flags(fixture, "worktree", "src/api.ts", "answer", None, &safe);
    assert_eq!(live["results"][0]["status"], "unreadable");
    assert_no_source(&live);
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn review_to_snapshot_read_survives_live_parent_changes_but_rejects_wrong_identity() {
    let fixture = Fixture::new("review-snapshot-handoff");
    let old = "export function answer() {\n    return 1;\n}\n";
    let new = "export function answer() {\n    return 2;\n}\n";
    fixture.write("src/api.ts", old);
    fixture.write(
        "src/caller.ts",
        "import { answer } from './api';\nexport function call() { return answer(); }\n",
    );
    symlink("api.ts", fixture.path().join("src/alias.ts")).unwrap();
    let base = fixture.commit("old API and a real Git symlink");
    fixture.write("src/api.ts", new);
    let head = fixture.commit("updated API");
    let report = review(&fixture, &base, &head, &[]);
    assert_no_projection_omissions(&report);
    assert_eq!(report["totals"]["changes"], 1);
    for coverage in report["coverage"].as_array().unwrap() {
        assert_eq!(coverage["unavailable_files"]["not-regular-file"], 1);
    }
    let snapshots: Vec<_> = [("base", old), ("head", new)]
        .into_iter()
        .map(|(side, source)| {
            let tree = report["comparison"][format!("{side}_tree")]
                .as_str()
                .unwrap();
            let hash = change(&report, "src/api.ts")[side]["sha256"]
                .as_str()
                .unwrap();
            let result = read(&fixture, tree, "src/api.ts", "answer", Some(hash));
            assert_read_content(&result, source, tree);
            (tree, hash, result)
        })
        .collect();
    let saved = tempfile::tempdir().unwrap();
    fs::rename(fixture.path().join("src"), saved.path().join("src")).unwrap();
    for state in ["missing", "symlink"] {
        if state == "symlink" {
            symlink(saved.path().join("src"), fixture.path().join("src")).unwrap();
        }
        assert_eq!(review(&fixture, &base, &head, &[]), report, "{state}");
        for (tree, hash, original) in &snapshots {
            assert_eq!(
                read(&fixture, tree, "src/api.ts", "answer", Some(hash)),
                *original,
                "{state}"
            );
        }
        assert_eq!(
            read(
                &fixture,
                "index",
                "src/api.ts",
                "answer",
                Some(snapshots[1].1)
            )["results"][0]["status"],
            "complete"
        );
        if state != "missing" {
            let live = read(&fixture, "worktree", "src/api.ts", "answer", None);
            assert_eq!(live["results"][0]["status"], "not-regular-file");
            assert_no_source(&live);
        }
    }
    for revision in [snapshots[1].0, "index"] {
        assert_eq!(
            read(&fixture, revision, "src/alias.ts", "answer", None)["results"][0]["status"],
            "not-regular-file"
        );
    }
    let stale = read(
        &fixture,
        snapshots[1].0,
        "src/api.ts",
        "answer",
        Some(&"0".repeat(64)),
    );
    assert_eq!(stale["results"][0]["status"], "stale");
    assert_no_source(&stale);
    assert_non_directory_parent_policy(&fixture, &base, &head, &snapshots);
}
