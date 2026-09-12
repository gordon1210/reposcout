#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "bounded integration fixtures fail immediately on invalid setup"
)]

#[path = "support/command.rs"]
mod test_command;

use reposcout::config::Config;
use reposcout::git::DiffScope;
use reposcout::metrics::tokens::TokenCounter;
use reposcout::model::{ConsumersDirection, FindReadSelector, SourceQueryStatus, SourceRevision};
use reposcout::query::{
    ChangeQueryOptions, ConsumersQueryOptions, SourceQueryOptions, SourceQueryTarget,
    SourceSelector, consumers, query_changes, read_source,
};
use reposcout::report::Format;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn options(file: &str, symbol: &str) -> ConsumersQueryOptions {
    ConsumersQueryOptions {
        targets: vec![SourceQueryTarget {
            path: PathBuf::from(file),
            selector: SourceSelector::Symbol(symbol.to_string()),
            expected_hash: None,
            snapshot: SourceRevision::Worktree,
        }],
        direction: ConsumersDirection::Incoming,
        depth: 1,
        limit: 20,
        path_limit: 20,
        token_budget: 16_384,
        byte_budget: 262_144,
        format: Format::Json,
        pretty_json: false,
    }
}

fn fixture(language: &str) -> TempDir {
    let root = tempfile::tempdir().unwrap();
    if language == "rust" {
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n",
        )
        .unwrap();
        fs::write(root.path().join("src/lib.rs"), "mod dep;\nmod caller;\n").unwrap();
        fs::write(
            root.path().join("src/dep.rs"),
            "pub fn work() -> usize { 1 }\n",
        )
        .unwrap();
        fs::write(root.path().join("src/caller.rs"), "use crate::dep::work;\npub fn caller() { work(); work(); outer(); }\npub fn outer() { caller(); }\npub fn unknown(receiver: &Thing) { receiver.work(); }\n").unwrap();
    } else {
        fs::write(
            root.path().join("dep.ts"),
            "export function work() { return 1; }\n",
        )
        .unwrap();
        fs::write(root.path().join("caller.ts"), "import { work } from './dep';\nexport function caller() { work(); work(); outer(); }\nexport function outer() { caller(); }\nexport function unknown(receiver: any) { receiver.work(); }\n").unwrap();
    }
    root
}

fn commit_fixture(root: &Path) {
    let repo = git2::Repository::init(root).unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let oid = index.write_tree().unwrap();
    let tree = repo.find_tree(oid).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
        .unwrap();
}

#[test]
fn changed_definition_to_consumers_to_hash_checked_source_for_rust_and_typescript() {
    for language in ["rust", "typescript"] {
        let root = fixture(language);
        commit_fixture(root.path());
        let path = if language == "rust" {
            "src/dep.rs"
        } else {
            "dep.ts"
        };
        let source = fs::read_to_string(root.path().join(path))
            .unwrap()
            .replace('1', "2");
        fs::write(root.path().join(path), source).unwrap();
        let cfg = Config {
            jobs: 2,
            ..Config::default()
        };
        let changed = query_changes(
            root.path(),
            &cfg,
            &[],
            &ChangeQueryOptions {
                scope: DiffScope::Working,
                include_source: false,
                token_budget: 16_384,
                byte_budget: 262_144,
                format: Format::Json,
                pretty_json: false,
            },
        )
        .unwrap();
        let current = changed
            .report
            .files
            .iter()
            .find(|file| file.snapshot == SourceRevision::Worktree && file.path == Path::new(path))
            .unwrap();
        let mut request = options(path, "work");
        request.targets[0].expected_hash.clone_from(&current.sha256);
        let result = consumers(root.path(), &cfg, &[], &request).unwrap();
        assert_eq!(
            result.report.hits.len(),
            1,
            "{language}: {}",
            result.rendered
        );
        let hit = &result.report.hits[0];
        assert_eq!(hit.symbol.name, "caller");
        assert!(hit.evidence.len() >= 2);
        assert_eq!(result.report.depth_omitted, 1);
        assert!(result.report.coverage.resolution.unresolved > 0);
        assert!(!result.rendered.contains("return 2"));
        let target = SourceQueryTarget {
            path: hit.read.path.clone(),
            selector: match &hit.read.selector {
                FindReadSelector::Symbol(name) => SourceSelector::Symbol(name.clone()),
            },
            expected_hash: Some(hit.read.expected_hash.clone()),
            snapshot: hit.read.snapshot.clone(),
        };
        let read = read_source(
            root.path(),
            &cfg,
            &[],
            &SourceQueryOptions {
                targets: vec![target.clone()],
                token_budget: 4096,
                byte_budget: 65_536,
                format: Format::Json,
                pretty_json: false,
            },
        )
        .unwrap();
        assert_eq!(read.report.results[0].status, SourceQueryStatus::Complete);
        assert!(
            read.report
                .sources
                .iter()
                .any(|source| source.content.contains("work();"))
        );
        fs::write(root.path().join(&target.path), "pub fn replacement() {}\n").unwrap();
        let stale = read_source(
            root.path(),
            &cfg,
            &[],
            &SourceQueryOptions {
                targets: vec![target],
                token_budget: 4096,
                byte_budget: 65_536,
                format: Format::Json,
                pretty_json: false,
            },
        )
        .unwrap();
        assert_eq!(stale.report.results[0].status, SourceQueryStatus::Stale);
    }
}

#[test]
fn cycles_depth_direction_and_limits_are_deterministic_and_accounted() {
    let root = fixture("typescript");
    let cfg = Config {
        jobs: 2,
        ..Config::default()
    };
    let mut request = options("dep.ts", "work");
    request.depth = 8;
    let full = consumers(root.path(), &cfg, &[], &request).unwrap();
    assert_eq!(full.report.total_matches, 2);
    assert_eq!(full.report.depth_omitted, 0);
    assert_eq!(
        full.rendered,
        consumers(root.path(), &cfg, &[], &request)
            .unwrap()
            .rendered
    );
    request.limit = 1;
    let limited = consumers(root.path(), &cfg, &[], &request).unwrap();
    assert_eq!(limited.report.returned_matches, 1);
    assert_eq!(limited.report.limit_omitted, 1);
    request.targets = options("caller.ts", "caller").targets;
    request.direction = ConsumersDirection::Outgoing;
    request.limit = 20;
    request.path_limit = 1;
    let paths = consumers(root.path(), &cfg, &[], &request).unwrap();
    assert_eq!(paths.report.total_matches, 2);
    assert_eq!(paths.report.path_omitted, 1);
    request.path_limit = 20;
    request.token_budget = 1024;
    request.byte_budget = 4096;
    let bounded = consumers(root.path(), &cfg, &[], &request).unwrap();
    assert!(bounded.rendered.len() <= request.byte_budget);
    assert!(
        TokenCounter::new(&cfg.encoding)
            .unwrap()
            .count(&bounded.rendered)
            <= request.token_budget
    );
    assert_eq!(
        bounded.report.total_matches,
        bounded.report.returned_matches
            + bounded.report.path_omitted
            + bounded.report.limit_omitted
            + bounded.report.budget_omitted
    );
}

#[test]
fn stale_historical_outside_and_ambiguous_seeds_fail_closed() {
    let root = fixture("typescript");
    let cfg = Config {
        jobs: 2,
        ..Config::default()
    };
    let mut request = options("dep.ts", "work");
    request.targets[0].expected_hash = Some("0".repeat(64));
    assert!(consumers(root.path(), &cfg, &[], &request).is_err());
    request.targets[0].expected_hash = None;
    request.targets[0].snapshot = SourceRevision::Index;
    assert!(consumers(root.path(), &cfg, &[], &request).is_err());
    request.targets[0].snapshot = SourceRevision::Worktree;
    request.targets[0].path = PathBuf::from("../outside.ts");
    assert!(consumers(root.path(), &cfg, &[], &request).is_err());
    fs::write(
        root.path().join("ambiguous.ts"),
        "function same() {}\nfunction same() {}\n",
    )
    .unwrap();
    assert!(consumers(root.path(), &cfg, &[], &options("ambiguous.ts", "same")).is_err());
}

#[test]
fn line_seed_uses_innermost_definition_body_instead_of_binding_scope() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("nested.ts"),
        "export function outer() { function inner() { return 1; } inner(); }\n",
    )
    .unwrap();
    let mut request = options("nested.ts", "inner");
    request.targets[0].selector = SourceSelector::Line(1);
    let result = consumers(
        root.path(),
        &Config {
            jobs: 2,
            ..Config::default()
        },
        &[],
        &request,
    )
    .unwrap();
    assert!(result.report.seeds[0].name.ends_with("inner"));
    assert_eq!(result.report.hits.len(), 1);
    assert!(result.report.hits[0].symbol.name.ends_with("outer"));
    let hit = &result.report.hits[0];
    let read = read_source(
        root.path(),
        &Config {
            jobs: 2,
            ..Config::default()
        },
        &[],
        &SourceQueryOptions {
            targets: vec![SourceQueryTarget {
                path: hit.read.path.clone(),
                selector: match &hit.read.selector {
                    FindReadSelector::Symbol(name) => SourceSelector::Symbol(name.clone()),
                },
                expected_hash: Some(hit.read.expected_hash.clone()),
                snapshot: hit.read.snapshot.clone(),
            }],
            token_budget: 4096,
            byte_budget: 65_536,
            format: Format::Json,
            pretty_json: false,
        },
    )
    .unwrap();
    assert_eq!(read.report.results[0].status, SourceQueryStatus::Complete);
    assert_eq!(
        read.report.results[0].definition.as_ref().unwrap().name,
        hit.symbol.name
    );
}

#[test]
fn cli_json_ndjson_and_invalid_targets_preserve_structured_contracts() {
    let root = fixture("typescript");
    for format in ["json", "ndjson"] {
        let output = test_command::reposcout_command()
            .arg("consumers")
            .arg(root.path())
            .args(["--symbol", "dep.ts", "work", "--format", format])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(report["kind"], "consumers_query");
        assert_eq!(report["returned_matches"], 1);
        assert_eq!(
            report["hits"][0]["read"]["expected_hash"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
        if format == "ndjson" {
            assert_eq!(std::str::from_utf8(&output).unwrap().lines().count(), 1);
        }
    }
    let invalid = test_command::reposcout_command()
        .arg("consumers")
        .arg(root.path())
        .args(["--symbol", "../outside.ts", "work", "--format", "json"])
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(!String::from_utf8_lossy(&invalid.stdout).contains("\"hits\""));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("escapes the selected root"));
}
