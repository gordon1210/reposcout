#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests intentionally fail immediately for invalid fixtures or assertions"
)]

use reposcout::config::Config;
use reposcout::model::{SourceQueryStatus, SourceRevision};
use reposcout::query::{SourceQueryOptions, SourceQueryTarget, SourceSelector, read_source};
use reposcout::report::Format;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

struct Fixture {
    directory: tempfile::TempDir,
    repo: git2::Repository,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(directory.path()).unwrap();
        fs::write(directory.path().join("lib.rs"), "fn value() -> u32 { 1 }\n").unwrap();
        let fixture = Self { directory, repo };
        fixture.stage();
        let tree_id = fixture.repo.index().unwrap().write_tree().unwrap();
        let tree = fixture.repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        fixture
            .repo
            .commit(Some("HEAD"), &signature, &signature, "base", &tree, &[])
            .unwrap();
        drop(tree);
        fixture
    }

    fn stage(&self) {
        let mut index = self.repo.index().unwrap();
        index.add_path(Path::new("lib.rs")).unwrap();
        index.write().unwrap();
    }

    fn edit(&self, value: u32) {
        fs::write(
            self.directory.path().join("lib.rs"),
            format!("fn value() -> u32 {{ {value} }}\n"),
        )
        .unwrap();
    }
}

fn options(revisions: Vec<SourceRevision>) -> SourceQueryOptions {
    SourceQueryOptions {
        targets: revisions
            .into_iter()
            .map(|snapshot| SourceQueryTarget {
                path: "lib.rs".into(),
                selector: SourceSelector::Symbol("value".to_string()),
                expected_hash: None,
                snapshot,
            })
            .collect(),
        token_budget: 4_096,
        byte_budget: 16_384,
        format: Format::Json,
        pretty_json: false,
    }
}

fn config() -> Config {
    Config {
        use_cache: false,
        ..Config::default()
    }
}

#[test]
fn batch_keeps_three_snapshot_bodies_and_file_identities_separate() {
    let fixture = Fixture::new();
    fixture.edit(2);
    fixture.stage();
    fixture.edit(3);
    let output = read_source(
        fixture.directory.path(),
        &config(),
        &[],
        &options(vec![
            SourceRevision::Tree("HEAD".into()),
            SourceRevision::Index,
            SourceRevision::Worktree,
        ]),
    )
    .unwrap();
    assert_eq!(output.report.sources.len(), 3);
    for (index, value) in [1, 2, 3].iter().enumerate() {
        let result = &output.report.results[index];
        assert_eq!(result.status, SourceQueryStatus::Complete);
        let source = output
            .report
            .sources
            .iter()
            .find(|source| Some(source.id) == result.source)
            .unwrap();
        assert_eq!(source.content, format!("fn value() -> u32 {{ {value} }}"));
    }
    let revisions = output
        .report
        .files
        .iter()
        .map(|file| &file.snapshot)
        .collect::<Vec<_>>();
    assert!(
        revisions
            .iter()
            .any(|revision| matches!(revision, SourceRevision::Tree(id) if id.len() == 40))
    );
    assert!(revisions.contains(&&SourceRevision::Index));
    assert!(revisions.contains(&&SourceRevision::Worktree));
}

#[test]
fn index_and_worktree_hashes_reject_later_content() {
    let fixture = Fixture::new();
    let mut query = options(vec![SourceRevision::Index, SourceRevision::Worktree]);
    let first = read_source(fixture.directory.path(), &config(), &[], &query).unwrap();
    for (target, result) in query.targets.iter_mut().zip(&first.report.results) {
        target.expected_hash = first
            .report
            .files
            .iter()
            .find(|file| Some(file.id) == result.file)
            .unwrap()
            .sha256
            .clone();
    }
    fixture.edit(4);
    fixture.stage();
    fixture.edit(5);
    let output = read_source(fixture.directory.path(), &config(), &[], &query).unwrap();
    assert!(
        output
            .report
            .results
            .iter()
            .all(|result| result.status == SourceQueryStatus::Stale)
    );
    assert!(output.report.sources.is_empty());
}

#[test]
fn historical_definition_is_available_after_worktree_file_deletion() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.directory.path().join("lib.rs")).unwrap();
    let output = read_source(
        fixture.directory.path(),
        &config(),
        &[],
        &options(vec![SourceRevision::Tree("HEAD".into())]),
    )
    .unwrap();
    assert_eq!(output.report.sources[0].content, "fn value() -> u32 { 1 }");
    assert!(
        read_source(
            fixture.directory.path(),
            &config(),
            &[],
            &options(vec![SourceRevision::Tree("does-not-exist".into())])
        )
        .is_err()
    );
}

#[test]
fn aliases_of_same_tree_share_source_and_hash_expectations() {
    let fixture = Fixture::new();
    let oid = fixture
        .repo
        .head()
        .unwrap()
        .peel_to_tree()
        .unwrap()
        .id()
        .to_string();
    let mut query = options(vec![
        SourceRevision::Tree("HEAD".into()),
        SourceRevision::Tree(oid),
    ]);
    let output = read_source(fixture.directory.path(), &config(), &[], &query).unwrap();
    assert_eq!(output.report.files.len(), 1);
    assert_eq!(output.report.sources.len(), 1);
    assert_eq!(
        output.report.results[0].source,
        output.report.results[1].source
    );
    query.targets[0].expected_hash = Some("a".repeat(64));
    query.targets[1].expected_hash = Some("b".repeat(64));
    assert!(read_source(fixture.directory.path(), &config(), &[], &query).is_err());
    query.targets[1].expected_hash = None;
    let stale = read_source(fixture.directory.path(), &config(), &[], &query).unwrap();
    assert!(
        stale
            .report
            .results
            .iter()
            .all(|result| result.status == SourceQueryStatus::Stale)
    );
    assert!(stale.report.sources.is_empty());
}

#[test]
fn changed_projection_keeps_exact_counts_beyond_admission_limit() {
    let directory = tempfile::tempdir().unwrap();
    let _repo = git2::Repository::init(directory.path()).unwrap();
    let mut content = String::new();
    for n in 0..256 {
        write!(content, "fn value_{n}() {{}} ").unwrap();
    }
    fs::write(directory.path().join("lib.rs"), content).unwrap();
    let output = reposcout::query::query_changes(
        directory.path(),
        &config(),
        &[],
        &reposcout::query::ChangeQueryOptions {
            scope: reposcout::git::DiffScope::Working,
            include_source: false,
            token_budget: 65_536,
            byte_budget: 1_048_576,
            format: Format::Json,
            pretty_json: false,
        },
    )
    .unwrap();
    assert_eq!(
        output.report.change.as_ref().unwrap().mapped_definitions,
        256
    );
    assert_eq!(output.report.requested_targets, 256);
    assert_eq!(output.report.results.len(), 128);
    assert_eq!(output.report.omitted_targets, 128);
    assert!(output.report.sources.is_empty());
}

#[test]
fn snapshot_reads_honor_exact_output_exclusions() {
    let fixture = Fixture::new();
    let output = read_source(
        fixture.directory.path(),
        &config(),
        &[fixture.directory.path().join("lib.rs")],
        &options(vec![SourceRevision::Tree("HEAD".into())]),
    )
    .unwrap();
    assert_eq!(output.report.results[0].status, SourceQueryStatus::Excluded);
    assert!(output.report.sources.is_empty());
}

#[test]
fn file_type_changes_are_unavailable_counterparts_not_deleted_definitions() {
    let fixture = Fixture::new();
    let mut index = fixture.repo.index().unwrap();
    let mut entry = index.get_path(Path::new("lib.rs"), 0).unwrap();
    entry.mode = 0o120_000;
    entry.id = fixture.repo.blob(b"somewhere.rs").unwrap();
    index.add(&entry).unwrap();
    index.write().unwrap();
    let output = reposcout::query::query_changes(
        fixture.directory.path(),
        &config(),
        &[],
        &reposcout::query::ChangeQueryOptions {
            scope: reposcout::git::DiffScope::Staged,
            include_source: true,
            token_budget: 4_096,
            byte_budget: 16_384,
            format: Format::Json,
            pretty_json: false,
        },
    )
    .unwrap();
    assert!(output.report.sources.is_empty());
    assert_eq!(output.report.change.as_ref().unwrap().mapped_definitions, 0);
    assert!(
        output
            .report
            .results
            .iter()
            .any(|result| result.status == SourceQueryStatus::NotRegularFile)
    );
    assert!(
        output
            .report
            .results
            .iter()
            .all(|result| result.change.as_ref().unwrap().file_status == "typechange")
    );
}
