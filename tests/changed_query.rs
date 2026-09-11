#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration tests intentionally fail immediately when fixtures or assertions are invalid"
)]

#[path = "support/command.rs"]
mod test_command;

use git2::{IndexAddOption, Oid, Repository, Signature};
use reposcout::metrics::tokens::TokenCounter;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use test_command::reposcout_command;

const BASE_BODY: &str = "BASE_BODY_LITERAL_41";
const HEAD_BODY: &str = "HEAD_BODY_LITERAL_42";
const INDEX_BODY: &str = "INDEX_BODY_LITERAL_43";
const WORKTREE_BODY: &str = "WORKTREE_BODY_LITERAL_44";
const UNTRACKED_BODY: &str = "UNTRACKED_BODY_LITERAL_45";

struct Fixture {
    directory: TempDir,
    base: Oid,
}

impl Fixture {
    fn path(&self) -> &Path {
        self.directory.path()
    }
}

fn function_source(name: &str, body: &str) -> String {
    format!("pub fn {name}() -> &'static str {{\n    \"{body}\"\n}}\n")
}

fn commit_index(repo: &Repository, message: &str) -> Oid {
    let mut index = repo.index().unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = Signature::now("reposcout tests", "tests@example.com").unwrap();
    if let Ok(parent) = repo.head().and_then(|head| head.peel_to_commit()) {
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        )
        .unwrap()
    } else {
        repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
            .unwrap()
    }
}

fn stage_all(repo: &Repository) {
    let mut index = repo.index().unwrap();
    index.add_all(["*"], IndexAddOption::DEFAULT, None).unwrap();
    index.write().unwrap();
}

fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::init(directory.path()).unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(
        directory.path().join("src/item.rs"),
        function_source("changed", BASE_BODY),
    )
    .unwrap();
    fs::write(
        directory.path().join("src/deleted.rs"),
        function_source("removed", "DELETED_BODY_LITERAL_46"),
    )
    .unwrap();
    fs::write(
        directory.path().join("outside.rs"),
        function_source("outside", "OUTSIDE_BASE_LITERAL_47"),
    )
    .unwrap();
    stage_all(&repo);
    let base = commit_index(&repo, "base");

    fs::rename(
        directory.path().join("src/item.rs"),
        directory.path().join("src/renamed.rs"),
    )
    .unwrap();
    fs::write(
        directory.path().join("src/renamed.rs"),
        function_source("changed", HEAD_BODY),
    )
    .unwrap();
    fs::remove_file(directory.path().join("src/deleted.rs")).unwrap();
    fs::write(
        directory.path().join("outside.rs"),
        function_source("outside", "OUTSIDE_HEAD_LITERAL_48"),
    )
    .unwrap();
    {
        let mut index = repo.index().unwrap();
        index.remove_path(Path::new("src/item.rs")).unwrap();
        index.remove_path(Path::new("src/deleted.rs")).unwrap();
        index.add_path(Path::new("src/renamed.rs")).unwrap();
        index.add_path(Path::new("outside.rs")).unwrap();
        index.write().unwrap();
    }
    commit_index(&repo, "head");

    fs::write(
        directory.path().join("src/renamed.rs"),
        function_source("changed", INDEX_BODY),
    )
    .unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("src/renamed.rs")).unwrap();
        index.write().unwrap();
    }
    fs::write(
        directory.path().join("src/renamed.rs"),
        function_source("changed", WORKTREE_BODY),
    )
    .unwrap();
    fs::write(
        directory.path().join("src/untracked.rs"),
        function_source("untracked", UNTRACKED_BODY),
    )
    .unwrap();

    Fixture { directory, base }
}

fn run_json(command_name: &str, root: &Path, arguments: &[&str]) -> Value {
    let output = reposcout_command()
        .arg(command_name)
        .arg(root)
        .args(arguments)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
}

fn contents(report: &Value) -> Vec<&str> {
    report
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|source| source["content"].as_str())
        .collect()
}

#[test]
fn working_changes_are_body_free_until_source_is_requested() {
    let fixture = fixture();
    let body_free = run_json(
        "changes",
        fixture.path(),
        &["--working", "--format", "json"],
    );

    assert_eq!(body_free["kind"], "change_query");
    assert_eq!(body_free["mode"], "changes");
    assert_eq!(body_free["change"]["scope"], "working");
    assert!(contents(&body_free).is_empty());
    assert!(!body_free.to_string().contains(WORKTREE_BODY));
    assert!(
        body_free["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["definition"]["name"] == "changed")
    );

    let with_source = run_json(
        "changes",
        fixture.path(),
        &["--working", "--source", "--format", "json"],
    );
    assert_eq!(with_source["mode"], "changes-source");
    let delivered = contents(&with_source);
    assert!(
        delivered
            .iter()
            .any(|source| source.contains(WORKTREE_BODY))
    );
    assert!(
        delivered
            .iter()
            .any(|source| source.contains(UNTRACKED_BODY))
    );
}

#[test]
fn staged_and_read_snapshots_use_the_captured_index_and_tree() {
    let fixture = fixture();
    let staged = run_json(
        "changes",
        fixture.path(),
        &["--staged", "--source", "--format", "json"],
    );
    let staged_sources = contents(&staged);
    assert!(
        staged_sources
            .iter()
            .any(|source| source.contains(INDEX_BODY))
    );
    assert!(
        staged_sources
            .iter()
            .all(|source| !source.contains(WORKTREE_BODY))
    );
    assert!(
        staged_sources
            .iter()
            .all(|source| !source.contains(UNTRACKED_BODY))
    );

    let index = run_json(
        "read",
        fixture.path(),
        &[
            "--snapshot",
            "index",
            "--symbol",
            "src/renamed.rs",
            "changed",
            "--format",
            "json",
        ],
    );
    assert!(contents(&index)[0].contains(INDEX_BODY));
    assert_eq!(index["files"][0]["snapshot"]["kind"], "index");

    let revision = fixture.base.to_string();
    let tree = run_json(
        "read",
        fixture.path(),
        &[
            "--snapshot",
            &revision,
            "--symbol",
            "src/item.rs",
            "changed",
            "--format",
            "json",
        ],
    );
    assert!(contents(&tree)[0].contains(BASE_BODY));
    assert_eq!(tree["files"][0]["snapshot"]["kind"], "tree");
}

#[test]
fn since_reports_renames_and_deletions_and_honors_directory_scope() {
    let fixture = fixture();
    let revision = fixture.base.to_string();
    let report = run_json(
        "changes",
        &fixture.path().join("src"),
        &["--since", &revision, "--format", "json"],
    );

    let files = report["files"].as_array().unwrap();
    assert!(files.iter().all(|file| {
        file["path"]
            .as_str()
            .is_some_and(|path| !path.contains("outside"))
    }));
    let results = report["results"].as_array().unwrap();
    assert!(
        results
            .iter()
            .any(|result| result["change"]["file_status"] == "renamed")
    );
    assert!(
        results
            .iter()
            .any(|result| result["change"]["file_status"] == "deleted")
    );
    assert!(results.iter().any(|result| {
        result["change"]["counterpart"]
            .as_str()
            .is_some_and(|path| path.contains("renamed.rs") || path.contains("item.rs"))
    }));
}

#[test]
fn shared_output_budgets_bound_the_encoded_response() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::init(directory.path()).unwrap();
    fs::write(
        directory.path().join("small.rs"),
        function_source("small", "OLD"),
    )
    .unwrap();
    stage_all(&repo);
    commit_index(&repo, "base");
    fs::write(
        directory.path().join("small.rs"),
        function_source("small", "NEW"),
    )
    .unwrap();

    let output = reposcout_command()
        .arg("changes")
        .arg(directory.path())
        .args([
            "--working",
            "--source",
            "--budget",
            "256",
            "--max-output-bytes",
            "1024",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(output.len() <= 1_024);
    let rendered = std::str::from_utf8(&output).unwrap();
    assert!(TokenCounter::new("o200k_base").unwrap().count(rendered) <= 256);
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert!(
        contents(&report).is_empty()
            || report["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|result| result["status"] == "budget-omitted")
    );
}

#[test]
fn changes_and_parent_mode_reject_invalid_contracts_and_protected_output() {
    let fixture = fixture();

    reposcout_command()
        .arg("changes")
        .arg(fixture.path())
        .assert()
        .failure()
        .code(2);
    reposcout_command()
        .arg("changes")
        .arg(fixture.path())
        .args(["--working", "--staged"])
        .assert()
        .failure()
        .code(2);
    reposcout_command()
        .arg("changes")
        .arg(fixture.path())
        .args(["--working", "--format", "sarif"])
        .assert()
        .failure()
        .code(2);
    reposcout_command()
        .arg("changes")
        .arg(fixture.path())
        .args(["--working", "--output", "inside.json"])
        .current_dir(fixture.path())
        .assert()
        .failure();

    reposcout_command()
        .args(["--changed-definitions", fixture.path().to_str().unwrap()])
        .assert()
        .failure()
        .code(2);
    reposcout_command()
        .args([
            "--working",
            "--review",
            "--changed-definitions",
            "--format",
            "sarif",
            fixture.path().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn file_scoped_changes_reject_output_that_would_overwrite_a_sibling_change() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::init(directory.path()).unwrap();
    fs::write(
        directory.path().join("target.rs"),
        function_source("target", "TARGET_BASE"),
    )
    .unwrap();
    fs::write(
        directory.path().join("sibling.rs"),
        function_source("sibling", "SIBLING_BASE"),
    )
    .unwrap();
    stage_all(&repo);
    commit_index(&repo, "base");
    fs::write(
        directory.path().join("target.rs"),
        function_source("target", "TARGET_CHANGED"),
    )
    .unwrap();
    let sibling = function_source("sibling", "SIBLING_CHANGED");
    fs::write(directory.path().join("sibling.rs"), &sibling).unwrap();

    reposcout_command()
        .arg("changes")
        .arg(directory.path().join("target.rs"))
        .args(["--working", "--output", "sibling.rs"])
        .current_dir(directory.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "changes output path cannot be inside the selected repository",
        ));

    assert_eq!(
        fs::read_to_string(directory.path().join("sibling.rs")).unwrap(),
        sibling
    );
}

#[test]
fn change_summary_can_embed_body_free_definition_evidence() {
    let fixture = fixture();
    let output = reposcout_command()
        .args([
            "--working",
            "--change-summary",
            "--changed-definitions",
            "--format",
            "json",
            fixture.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["report_kind"], "change-summary");
    assert_eq!(report["definition_changes"]["kind"], "change_query");
    assert_eq!(report["definition_changes"]["mode"], "changes");
    assert!(
        report["definition_changes"]
            .get("sources")
            .is_none_or(|sources| sources.as_array().is_some_and(Vec::is_empty))
    );
    assert!(!report.to_string().contains(WORKTREE_BODY));
}
