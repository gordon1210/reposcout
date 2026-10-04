#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "bounded Git fixtures fail immediately on invalid setup"
)]

#[path = "support/command.rs"]
mod test_command;

#[path = "review_context/regressions.rs"]
mod regressions;

#[path = "review_context/post_review.rs"]
mod post_review;

#[path = "review_context/projection_regressions.rs"]
mod projection_regressions;

use git2::{IndexAddOption, Oid, Repository, Signature};
use reposcout::metrics::tokens::TokenCounter;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

struct Fixture {
    directory: TempDir,
    base: Oid,
    head: Oid,
}

impl Fixture {
    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn report(&self, flags: &[&str]) -> Value {
        let output = self
            .command(flags)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice(&output).unwrap()
    }

    fn command(&self, flags: &[&str]) -> assert_cmd::Command {
        self.command_format(flags, "json")
    }

    fn command_format(&self, flags: &[&str], format: &str) -> assert_cmd::Command {
        let mut command = test_command::reposcout_command();
        command
            .arg("review-context")
            .arg(self.path())
            .args([
                "--base",
                &self.base.to_string(),
                "--head",
                &self.head.to_string(),
                "--no-cache",
                "--no-project-config",
                "--budget",
                "65536",
                "--max-output-bytes",
                "1048576",
                "-f",
                format,
            ])
            .args(flags);
        command
    }
}

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

fn commit(root: &Path) -> Oid {
    let repository = Repository::open(root).unwrap();
    let mut index = repository.index().unwrap();
    index.add_all(["*"], IndexAddOption::DEFAULT, None).unwrap();
    index.update_all(["*"], None).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repository.find_tree(tree_id).unwrap();
    let signature = Signature::now("RepoScout tests", "tests@example.com").unwrap();
    let parent = repository
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok());
    let parents: Vec<_> = parent.iter().collect();
    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "fixture",
            &tree,
            &parents,
        )
        .unwrap()
}

fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    Repository::init(root).unwrap();
    write(root, "package.json", "{\"name\":\"fixture\"}\n");
    write(
        root,
        "tsconfig.json",
        "{\"compilerOptions\":{\"baseUrl\":\".\",\"paths\":{\"@api\":[\"src/api.ts\"]}}}\n",
    );
    write(
        root,
        "src/api.ts",
        "export function answer(): number { return 1; }\n",
    );
    write(
        root,
        "src/service.ts",
        "import { answer } from '@api';\nexport function run(): number { return answer(); }\n",
    );
    write(
        root,
        "src/app.ts",
        "import { run } from './service';\nexport function app(): number { return run(); }\n",
    );
    write(
        root,
        "tests/api.test.ts",
        "import { answer } from '../src/api';\nexport function check(): number { return answer(); }\n",
    );
    let base = commit(root);
    write(
        root,
        "src/api.ts",
        "export function answer(): number { return 2; }\n",
    );
    let head = commit(root);
    Fixture {
        directory,
        base,
        head,
    }
}

fn candidate<'a>(report: &'a Value, side: &str, path: &str) -> &'a Value {
    report["context"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["side"] == side && file["path"] == path)
        .unwrap_or_else(|| panic!("missing {side}:{path}: {report}"))
}

fn has_role(file: &Value, role: &str) -> bool {
    file["roles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == role)
}

#[test]
fn immutable_review_finds_direct_transitive_and_concrete_consumers() {
    let fixture = fixture();
    let report = fixture.report(&["--context"]);
    assert_eq!(
        report["comparison"]["base_commit"],
        fixture.base.to_string()
    );
    assert_eq!(
        report["comparison"]["head_commit"],
        fixture.head.to_string()
    );
    assert_eq!(report["totals"]["changes"], 1);
    for side in ["base", "head"] {
        assert!(has_role(
            candidate(&report, side, "src/service.ts"),
            "direct-dependent"
        ));
        assert!(has_role(
            candidate(&report, side, "src/app.ts"),
            "transitive-dependent"
        ));
        assert_eq!(
            candidate(&report, side, "tests/api.test.ts")["test_evidence"][0],
            "filename-convention"
        );
        assert!(
            report["relations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|edge| edge["side"] == side
                    && edge["kind"] == "symbol-reference"
                    && edge["edge"]["source"] == "src/service.ts"
                    && edge["edge"]["target"] == "src/api.ts")
        );
    }
    let definitions = &report["changes"][0]["head"]["definitions"];
    assert_eq!(definitions[0]["symbol"]["name"], "answer");
    assert!(
        report["context"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file.get("source").is_none())
    );
}

#[test]
fn dirty_sources_index_and_resolver_configs_do_not_change_revision_results() {
    let fixture = fixture();
    let before = fixture.report(&["--source", "--diff"]);
    write(
        fixture.path(),
        "src/api.ts",
        "export function DIRTY_MARKER() {}\n",
    );
    write(
        fixture.path(),
        "tsconfig.json",
        "{\"compilerOptions\":{\"paths\":{\"@api\":[\"absent.ts\"]}}}\n",
    );
    let repository = Repository::open(fixture.path()).unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("src/api.ts")).unwrap();
    index.write().unwrap();
    assert_eq!(fixture.report(&["--source", "--diff"]), before);
    assert!(!before.to_string().contains("DIRTY_MARKER"));
}

#[test]
fn base_and_head_resolver_maps_can_disagree_without_crossing_revisions() {
    let mut fixture = fixture();
    write(
        fixture.path(),
        "src/alternate.ts",
        "export function answer(): number { return 3; }\n",
    );
    write(
        fixture.path(),
        "tsconfig.json",
        "{\"compilerOptions\":{\"baseUrl\":\".\",\"paths\":{\"@api\":[\"src/alternate.ts\"]}}}\n",
    );
    fixture.head = commit(fixture.path());
    let report = fixture.report(&[]);
    let edges = report["relations"].as_array().unwrap();
    assert!(edges.iter().any(|edge| edge["side"] == "base"
        && edge["edge"]["source"] == "src/service.ts"
        && edge["edge"]["target"] == "src/api.ts"));
    assert!(!edges.iter().any(|edge| edge["side"] == "head"
        && edge["edge"]["source"] == "src/service.ts"
        && edge["edge"]["target"] == "src/api.ts"));
    assert!(edges.iter().any(|edge| edge["side"] == "head"
        && edge["edge"]["source"] == "src/service.ts"
        && edge["edge"]["target"] == "src/alternate.ts"));
}

#[test]
fn deleted_file_retains_its_old_callers_and_unknown_head_edges() {
    let mut fixture = fixture();
    fs::remove_file(fixture.path().join("src/api.ts")).unwrap();
    fixture.head = commit(fixture.path());
    let report = fixture.report(&[]);
    assert_eq!(report["changes"][0]["status"], "deleted");
    assert!(report["changes"][0]["head"].is_null());
    assert_eq!(report["changes"][0]["hunk_status"], "available");
    assert_eq!(report["changes"][0]["base"]["mapping_status"], "available");
    assert_eq!(report["totals"]["changes_without_hunks"], 0);
    assert!(has_role(
        candidate(&report, "base", "src/service.ts"),
        "direct-dependent"
    ));
    assert!(
        report["coverage"][1]["unresolved_imports"]
            .as_u64()
            .unwrap()
            > 0
    );
    std::mem::swap(&mut fixture.base, &mut fixture.head);
    let added = fixture.report(&[]);
    assert_eq!(added["changes"][0]["status"], "added");
    assert_eq!(added["changes"][0]["hunk_status"], "available");
    assert_eq!(added["changes"][0]["head"]["mapping_status"], "available");
    assert_eq!(added["totals"]["changes_without_hunks"], 0);
}

#[test]
fn source_counts_use_configured_encoding_and_each_file_side_once() {
    let fixture = fixture();
    for encoding in ["o200k_base", "cl100k_base"] {
        let report = fixture.report(&[
            "--encoding",
            encoding,
            "--source",
            "--context-budget",
            "60000",
            "--context-max-files",
            "100",
        ]);
        assert_eq!(report["encoding"], encoding);
        let counter = TokenCounter::new(encoding).unwrap();
        let files = report["context"].as_array().unwrap();
        let mut identities = std::collections::BTreeSet::new();
        let mut total = 0;
        for file in files {
            assert!(identities.insert((
                file["side"].as_str().unwrap(),
                file["path"].as_str().unwrap()
            )));
            let source = file["source"].as_str().unwrap();
            assert_eq!(file["tokens"], counter.count(source));
            assert_eq!(file["bytes"], source.len());
            total += counter.count(source);
        }
        assert_eq!(report["totals"]["candidate_tokens"], total);
        assert_eq!(report["totals"]["selected_tokens"], total);
    }
}

#[test]
fn context_budget_omissions_are_separate_from_output_omissions() {
    let fixture = fixture();
    let report = fixture.report(&["--context-budget", "1"]);
    assert_eq!(report["totals"]["selected_tokens"], 0);
    assert_eq!(report["totals"]["selected_files"], 0);
    assert_eq!(report["totals"]["candidates_omitted"], 0);
    assert!(
        report["totals"]["selection_omitted_files"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        report["context"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["selection"] == "token-budget")
    );
}

#[test]
fn hidden_yaml_binary_and_unsupported_paths_remain_accounted_for() {
    let mut fixture = fixture();
    write(fixture.path(), ".github/workflows/check.yml", "name: old\n");
    write(fixture.path(), "data.custom", "old text\n");
    fs::write(fixture.path().join("image.bin"), b"old\0image").unwrap();
    fixture.base = commit(fixture.path());
    write(fixture.path(), ".github/workflows/check.yml", "name: new\n");
    write(fixture.path(), "data.custom", "new text\n");
    fs::write(fixture.path().join("image.bin"), b"new\0image").unwrap();
    fixture.head = commit(fixture.path());
    let report = fixture.report(&[]);
    assert_eq!(report["totals"]["changes"], 3);
    assert_eq!(
        candidate(&report, "head", ".github/workflows/check.yml")["status"],
        "excluded"
    );
    assert_eq!(candidate(&report, "head", "image.bin")["status"], "binary");
    assert!(candidate(&report, "head", "image.bin")["tokens"].is_null());
    let hidden = fixture.report(&["--hidden"]);
    let yaml = hidden["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["head"]["path"] == ".github/workflows/check.yml")
        .unwrap();
    assert_eq!(yaml["head"]["extraction"], "unsupported");
    assert_eq!(yaml["head"]["unmapped_ranges"], 1);
    assert!(yaml["diff_tokens"].as_u64().unwrap() > 0);
}

#[test]
fn symlinks_are_inventory_entries_and_are_never_followed() {
    let mut fixture = fixture();
    std::os::unix::fs::symlink("src/api.ts", fixture.path().join("alias.ts")).unwrap();
    fixture.head = commit(fixture.path());
    let report = fixture.report(&["--source"]);
    assert_eq!(
        candidate(&report, "head", "alias.ts")["status"],
        "not-regular-file"
    );
    assert!(
        candidate(&report, "head", "alias.ts")
            .get("source")
            .is_none()
    );
}

#[test]
fn output_limits_include_pretty_json_ndjson_and_source() {
    let fixture = fixture();
    let counter = TokenCounter::new("o200k_base").unwrap();
    for format in ["json", "ndjson", "table", "markdown"] {
        let mut command = test_command::reposcout_command();
        command.arg("review-context").arg(fixture.path()).args([
            "--base",
            &fixture.base.to_string(),
            "--head",
            &fixture.head.to_string(),
            "--no-cache",
            "--source",
            "--diff",
            "--budget",
            "2500",
            "--max-output-bytes",
            "12000",
            "-f",
            format,
        ]);
        if format == "json" {
            command.arg("--pretty");
        }
        let output = command.assert().success().get_output().stdout.clone();
        assert!(output.len() <= 12000);
        assert!(counter.count(std::str::from_utf8(&output).unwrap()) <= 2500);
        if format == "ndjson" {
            assert_eq!(std::str::from_utf8(&output).unwrap().lines().count(), 1);
        }
    }
}

#[test]
fn empty_comparison_has_no_fabricated_context() {
    let mut fixture = fixture();
    fixture.base = fixture.head;
    let report = fixture.report(&[]);
    assert_eq!(report["totals"]["changes"], 0);
    assert_eq!(report["totals"]["candidates"], 0);
    assert_eq!(report["totals"]["candidate_tokens"], 0);
    assert_eq!(report["totals"]["changes_without_hunks"], 0);
}

#[test]
fn explicit_input_limits_surface_unknown_sizes_and_inventory_gaps() {
    let fixture = fixture();
    let report = fixture.report(&["--max-files", "2", "--max-file-bytes", "1"]);
    assert!(
        report["coverage"]
            .as_array()
            .unwrap()
            .iter()
            .any(|side| side["inventory_truncated"] == true)
    );
    assert!(
        report["totals"]["unknown_candidate_sizes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        candidate(&report, "head", "src/api.ts")["status"],
        "oversized"
    );
}

#[test]
fn invalid_refs_and_output_inside_the_repository_fail_without_overwriting() {
    let fixture = fixture();
    let original = fs::read(fixture.path().join("src/api.ts")).unwrap();
    let mut command = test_command::reposcout_command();
    command
        .arg("review-context")
        .arg(fixture.path())
        .args(["--base", "no-such-commit", "--error-format", "json"])
        .assert()
        .failure();
    fixture
        .command(&[])
        .arg("--output")
        .arg(fixture.path().join("src/api.ts"))
        .assert()
        .failure();
    assert_eq!(
        fs::read(fixture.path().join("src/api.ts")).unwrap(),
        original
    );
}

#[test]
fn merge_base_is_explicit_and_preserves_requested_and_actual_commits() {
    let mut fixture = fixture();
    let repository = Repository::open(fixture.path()).unwrap();
    let ancestor = repository.find_commit(fixture.base).unwrap();
    let tree = repository
        .find_commit(fixture.head)
        .unwrap()
        .tree()
        .unwrap();
    let signature = Signature::now("RepoScout tests", "tests@example.com").unwrap();
    fixture.base = repository
        .commit(
            None,
            &signature,
            &signature,
            "divergent base",
            &tree,
            &[&ancestor],
        )
        .unwrap();
    assert_eq!(fixture.report(&[])["totals"]["changes"], 0);
    let merged = fixture.report(&["--merge-base"]);
    assert_eq!(merged["totals"]["changes"], 1);
    assert_eq!(
        merged["comparison"]["requested_base_commit"],
        fixture.base.to_string()
    );
    assert_eq!(
        merged["comparison"]["base_commit"],
        ancestor.id().to_string()
    );
    assert_eq!(merged["comparison"]["mode"], "merge-base");
}

#[test]
fn renames_and_mode_only_changes_keep_both_file_identities() {
    use std::os::unix::fs::PermissionsExt;
    let mut fixture = fixture();
    fixture.base = fixture.head;
    fs::rename(
        fixture.path().join("src/app.ts"),
        fixture.path().join("src/renamed.ts"),
    )
    .unwrap();
    fs::set_permissions(
        fixture.path().join("src/api.ts"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fixture.head = commit(fixture.path());
    let report = fixture.report(&[]);
    let changes = report["changes"].as_array().unwrap();
    let renamed = changes
        .iter()
        .find(|file| file["status"] == "renamed")
        .unwrap();
    assert_eq!(renamed["base"]["path"], "src/app.ts");
    assert_eq!(renamed["head"]["path"], "src/renamed.ts");
    assert_eq!(renamed["hunks"], 0);
    assert_eq!(renamed["hunk_status"], "available");
    let executable = changes
        .iter()
        .find(|file| file["head"]["path"] == "src/api.ts")
        .unwrap();
    assert_eq!(executable["base"]["mode"], 0o100_644);
    assert_eq!(executable["head"]["mode"], 0o100_755);
    assert_eq!(executable["hunks"], 0);
    assert_eq!(executable["hunk_status"], "available");
    assert_eq!(executable["base"]["mapping_status"], "available");
    assert_eq!(executable["head"]["mapping_status"], "available");
    assert_eq!(report["totals"]["changes_without_hunks"], 0);
    assert!(
        report["relations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|relation| relation["change_basis"] != "changed-file")
    );
}

#[test]
fn user_config_selects_the_tokenizer_without_model_detection() {
    let fixture = fixture();
    let config = tempfile::NamedTempFile::new().unwrap();
    fs::write(
        config.path(),
        "encoding = 'cl100k_base'\njobs = 2\n[context]\nbudget = 23\nmax_files = 2\n",
    )
    .unwrap();
    let output = fixture
        .command(&["--context"])
        .env("REPOSCOUT_GLOBAL_CONFIG", config.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["encoding"], "cl100k_base");
    assert_eq!(report["context_budget"], 23);
    assert_eq!(report["context_max_files"], 2);
    assert!(report["totals"]["selected_tokens"].as_u64().unwrap() <= 23);
    let output = fixture
        .command(&["--encoding", "o200k_base"])
        .env("REPOSCOUT_GLOBAL_CONFIG", config.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["encoding"], "o200k_base");
}

#[test]
fn subdirectory_change_scope_still_finds_importers_outside_that_directory() {
    let fixture = fixture();
    let output = test_command::reposcout_command()
        .arg("review-context")
        .arg(fixture.path().join("src"))
        .args([
            "--base",
            &fixture.base.to_string(),
            "--head",
            &fixture.head.to_string(),
            "--budget",
            "65536",
            "--max-output-bytes",
            "1048576",
            "--no-cache",
            "-f",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert!(has_role(
        candidate(&report, "head", "tests/api.test.ts"),
        "direct-dependent"
    ));
}

#[test]
fn body_free_defaults_do_not_choose_a_reading_list() {
    let fixture = fixture();
    let report = fixture.report(&[]);
    assert!(report["context_budget"].is_null());
    assert_eq!(report["totals"]["selected_files"], 0);
    assert!(
        report["context"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["selection"] == "not-requested" && file.get("source").is_none())
    );
    assert!(
        report["changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|change| change.get("diff").is_none())
    );
}

#[test]
fn debug_log_cannot_overwrite_reviewed_content() {
    let fixture = fixture();
    let file = fixture.path().join("src/api.ts");
    let before = fs::read(&file).unwrap();
    fixture
        .command(&[])
        .arg("--debug-log")
        .arg(&file)
        .assert()
        .failure();
    assert_eq!(fs::read(file).unwrap(), before);
}

#[test]
fn unchanged_declarations_do_not_gain_concrete_changed_symbol_evidence() {
    let mut fixture = fixture();
    write(
        fixture.path(),
        "src/api.ts",
        "export function answer(): number { return 1; }\nexport function unchanged(): number { return 3; }\n",
    );
    write(
        fixture.path(),
        "src/other.ts",
        "import { unchanged } from './api';\nexport function other() { return unchanged(); }\n",
    );
    fixture.base = commit(fixture.path());
    write(
        fixture.path(),
        "src/api.ts",
        "export function answer(): number { return 2; }\nexport function unchanged(): number { return 3; }\n",
    );
    fixture.head = commit(fixture.path());
    let report = fixture.report(&[]);
    assert!(has_role(
        candidate(&report, "head", "src/other.ts"),
        "direct-dependent"
    ));
    assert!(
        !report["relations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "symbol-reference"
                && edge["edge"]["source"] == "src/other.ts")
    );
}

#[test]
fn type_relationships_are_distinct_from_imports() {
    let mut fixture = fixture();
    write(
        fixture.path(),
        "src/types.ts",
        "export class Base { value(): number { return 1; } }\n",
    );
    write(
        fixture.path(),
        "src/derived.ts",
        "import { Base } from './types';\nexport class Derived extends Base {}\n",
    );
    fixture.base = commit(fixture.path());
    write(
        fixture.path(),
        "src/types.ts",
        "export class Base { value(): number { return 2; } }\n",
    );
    fixture.head = commit(fixture.path());
    let report = fixture.report(&[]);
    assert!(
        report["relations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "type-relationship"
                && edge["type_relation"]["relation"] == "extends"
                && edge["edge"]["source"] == "src/derived.ts")
    );
}

#[test]
fn parse_errors_remain_visible_and_unsupported_files_have_no_graph_claim() {
    let mut fixture = fixture();
    write(
        fixture.path(),
        "src/api.ts",
        "export function answer( { return ; !!!\n",
    );
    write(fixture.path(), "notes.custom", "review me\n");
    fixture.head = commit(fixture.path());
    let report = fixture.report(&[]);
    assert!(report["coverage"][1]["parse_errors"].as_u64().unwrap() > 0);
    assert_eq!(report["coverage"][1]["changed_files_without_graph"], 1);
    assert_eq!(
        candidate(&report, "head", "notes.custom")["status"],
        "captured"
    );
}

#[test]
fn cached_revision_results_equal_uncached_results() {
    let fixture = fixture();
    let expected = fixture.report(&[]);
    let cache = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        let output = test_command::reposcout_command()
            .arg("review-context")
            .arg(fixture.path())
            .args([
                "--base",
                &fixture.base.to_string(),
                "--head",
                &fixture.head.to_string(),
                "--budget",
                "65536",
                "--max-output-bytes",
                "1048576",
                "--no-project-config",
                "-f",
                "json",
            ])
            .env("XDG_CACHE_HOME", cache.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let actual: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn input_limited_changes_are_separate_from_output_omissions() {
    let mut fixture = fixture();
    fixture.base = fixture.head;
    for path in ["a.custom", "b.custom", "c.custom"] {
        write(fixture.path(), path, "new content\n");
    }
    fixture.head = commit(fixture.path());
    let report = fixture.report(&["--max-files", "1", "--diff"]);
    assert_eq!(report["totals"]["changes"], 3);
    assert_eq!(report["totals"]["changes_not_analyzed"], 2);
    assert_eq!(report["totals"]["changes_omitted"], 0);
    assert_eq!(report["totals"]["diff_unavailable_files"], 2);
    assert_eq!(report["totals"]["diff_files_omitted"], 0);
    assert_eq!(report["comparison"]["rename_detection_complete"], false);
}

#[test]
fn non_utf8_changed_paths_fail_explicitly_instead_of_serializing_lossy_identity() {
    let mut fixture = fixture();
    let repository = Repository::open(fixture.path()).unwrap();
    let parent = repository.find_commit(fixture.head).unwrap();
    let mut tree = repository
        .treebuilder(Some(&parent.tree().unwrap()))
        .unwrap();
    let blob = repository.blob(b"export function invalid() {}\n").unwrap();
    tree.insert(
        std::ffi::CString::new(b"invalid-\xff.ts".to_vec()).unwrap(),
        blob,
        0o100_644,
    )
    .unwrap();
    let tree = repository.find_tree(tree.write().unwrap()).unwrap();
    let signature = Signature::now("RepoScout tests", "tests@example.com").unwrap();
    fixture.head = repository
        .commit(
            None,
            &signature,
            &signature,
            "non UTF-8 path",
            &tree,
            &[&parent],
        )
        .unwrap();
    fixture
        .command(&[])
        .assert()
        .failure()
        .stderr(predicates::str::contains("UTF-8 Git paths"));
}

#[test]
fn capability_advertises_revision_review_without_orchestration() {
    let capabilities = reposcout::query::capabilities();
    assert!(
        capabilities
            .commands
            .iter()
            .any(|name| name == "review-context")
    );
    let capability = capabilities.review_context.unwrap();
    assert!(capability.available);
    assert_eq!(capability.context_unit, "unique-whole-file-per-revision");
    assert_eq!(capability.default_tokens, 4096);
    assert_eq!(capability.max_input_bytes, 32 * 1024 * 1024);
}

#[test]
fn rust_modules_and_inline_tests_use_the_shared_snapshot_facts() {
    let mut fixture = fixture();
    write(
        fixture.path(),
        "Cargo.toml",
        "[package]\nname = 'fixture'\nversion = '0.1.0'\nedition = '2024'\n",
    );
    write(fixture.path(), "src/lib.rs", "mod api;\nmod consumer;\n");
    let original = "pub fn answer() -> usize { 1 }\n#[cfg(test)]\nmod tests { #[test] fn checks_answer() { assert_eq!(super::answer(), 1); } }\n";
    write(fixture.path(), "src/api.rs", original);
    write(
        fixture.path(),
        "src/consumer.rs",
        "use crate::api::answer;\npub fn run() -> usize { answer() }\n",
    );
    fixture.base = commit(fixture.path());
    write(fixture.path(), "src/api.rs", &original.replace('1', "2"));
    fixture.head = commit(fixture.path());
    let report = fixture.report(&[]);
    assert!(
        candidate(&report, "head", "src/api.rs")["test_evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|evidence| evidence == "rust-inline-syntax")
    );
    assert!(
        report["relations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["kind"] == "symbol-reference"
                && edge["side"] == "head"
                && edge["edge"]["source"] == "src/consumer.rs"
                && edge["edge"]["target"] == "src/api.rs")
    );
}

#[test]
fn default_output_preserves_totals_from_the_larger_report() {
    let fixture = fixture();
    let complete = fixture.report(&[]);
    let output = test_command::reposcout_command()
        .arg("review-context")
        .arg(fixture.path())
        .args([
            "--base",
            &fixture.base.to_string(),
            "--head",
            &fixture.head.to_string(),
            "--no-cache",
            "--no-project-config",
            "-f",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert!(
        TokenCounter::new("o200k_base")
            .unwrap()
            .count(std::str::from_utf8(&output).unwrap())
            <= 4096
    );
    for counter in [
        "changes",
        "definitions",
        "relations",
        "candidate_tokens",
        "candidate_bytes",
        "diff_tokens",
    ] {
        assert_eq!(report["totals"][counter], complete["totals"][counter]);
    }
    assert_eq!(report["totals"]["changes_not_analyzed"], 0);
}

#[test]
fn linked_worktree_queries_protect_git_metadata_outside_the_worktree() {
    let fixture = fixture();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("checkout");
    let repository = Repository::open(fixture.path()).unwrap();
    repository.worktree("review-fixture", &path, None).unwrap();
    let output = repository.path().join("review-output.json");
    for option in ["--output", "--debug-log"] {
        test_command::reposcout_command()
            .arg("review-context")
            .arg(&path)
            .args([
                "--base",
                &fixture.base.to_string(),
                "--head",
                &fixture.head.to_string(),
                option,
            ])
            .arg(&output)
            .assert()
            .failure()
            .stderr(predicates::str::contains("Git metadata"));
        assert!(!output.exists());
    }
}
