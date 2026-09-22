#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "bounded integration fixtures fail immediately on invalid setup or assertions"
)]

#[path = "support/command.rs"]
mod test_command;

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_command::reposcout_command;

struct Fixture {
    root: TempDir,
    cache_home: TempDir,
    cache_dir: PathBuf,
}

impl Fixture {
    fn new(block_cache: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let cache_home = tempfile::tempdir().unwrap();
        let cache_dir = if cfg!(target_os = "macos") {
            cache_home.path().join("Library/Caches/reposcout")
        } else {
            cache_home.path().join("reposcout")
        };
        if block_cache {
            fs::create_dir_all(cache_dir.parent().unwrap()).unwrap();
            // A regular file blocks cache-directory creation even when tests run as root.
            fs::write(&cache_dir, "cache storage unavailable").unwrap();
        }

        let repo = git2::Repository::init(root.path()).unwrap();
        fs::write(root.path().join("lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("lib.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "base", &tree, &[])
            .unwrap();
        fs::write(root.path().join("lib.rs"), "pub fn value() -> u32 { 2 }\n").unwrap();
        index.add_path(Path::new("lib.rs")).unwrap();
        index.write().unwrap();
        fs::write(root.path().join("lib.rs"), "pub fn value() -> u32 { 3 }\n").unwrap();
        Self {
            root,
            cache_home,
            cache_dir,
        }
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = reposcout_command();
        // Isolate the child's platform cache without changing process-global environment.
        command
            .env("HOME", self.cache_home.path())
            .env("XDG_CACHE_HOME", self.cache_home.path())
            .args(arguments)
            .arg(self.root.path())
            .args(["--profile", "safe", "-f", "json"]);
        command
    }

    fn run(&self, arguments: &[&str]) -> Value {
        let output = self
            .command(arguments)
            .assert()
            .success()
            .get_output()
            .clone();
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
        serde_json::from_slice(&output.stdout).expect("valid JSON on stdout")
    }

    fn debug_events(&self, name: &str, arguments: &[&str]) -> Vec<Value> {
        let log = self.cache_home.path().join(name);
        self.command(arguments)
            .arg("--debug-log")
            .arg(&log)
            .assert()
            .success()
            .stderr("");
        fs::read_to_string(log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|event: &Value| event["event"] == "cache_save_error")
            .collect()
    }
}

#[test]
fn unavailable_cache_preserves_worktree_index_and_tree_reads_and_outlines() {
    let fixture = Fixture::new(true);
    for (snapshot, value) in [("worktree", 3), ("index", 2), ("HEAD", 1)] {
        let report = fixture.run(&[
            "read",
            "--symbol",
            "lib.rs",
            "value",
            "--snapshot",
            snapshot,
        ]);
        assert_eq!(report["results"][0]["status"], "complete");
        assert_eq!(
            report["sources"][0]["content"],
            format!("pub fn value() -> u32 {{ {value} }}")
        );
    }
    let outline = fixture.run(&["read", "--outline", "lib.rs"]);
    assert_eq!(outline["files"][0]["path"], "lib.rs");
    assert!(!fixture.root.path().join(".reposcout").exists());
    assert_eq!(
        fs::read_to_string(&fixture.cache_dir).unwrap(),
        "cache storage unavailable"
    );
}

#[test]
fn unavailable_cache_preserves_changed_sources_in_every_diff_scope() {
    let fixture = Fixture::new(true);
    for (scope, value) in [("--working", 3), ("--staged", 2), ("--since", 3)] {
        let mut arguments = vec!["changes", scope];
        if scope == "--since" {
            arguments.push("HEAD");
        }
        arguments.push("--source");
        let report = fixture.run(&arguments);
        assert!(!report["results"].as_array().unwrap().is_empty());
        let sources = report["sources"].as_array().unwrap();
        for expected in [1, value] {
            assert!(sources.iter().any(|source| {
                source["content"] == format!("pub fn value() -> u32 {{ {expected} }}")
            }));
        }
    }
}

#[test]
fn unavailable_cache_preserves_plans_and_embedded_changed_definitions() {
    let fixture = Fixture::new(true);
    let plan = fixture.run(&["plan", "--symbol", "lib.rs", "value", "--source"]);
    assert_eq!(plan["selected"][0]["name"], "value");
    assert_eq!(
        plan["source"]["sources"][0]["content"],
        "pub fn value() -> u32 { 3 }"
    );
    let report = fixture.run(&["--change-summary", "--working", "--changed-definitions"]);
    assert!(
        !report["definition_changes"]["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn cache_failures_are_observable_only_in_opt_in_debug_logs() {
    let fixture = Fixture::new(true);
    for (name, arguments, batch) in [
        ("scan.jsonl", vec![], "primary"),
        (
            "read.jsonl",
            vec!["read", "--symbol", "lib.rs", "value"],
            "explicit_sources",
        ),
        (
            "index.jsonl",
            vec!["read", "--symbol", "lib.rs", "value", "--snapshot", "index"],
            "revision_sources",
        ),
        (
            "changes.jsonl",
            vec!["changes", "--working"],
            "changed_sources",
        ),
    ] {
        let events = fixture.debug_events(name, &arguments);
        assert_eq!(events.len(), 1);
        let data = &events[0]["data"];
        assert_eq!(data["batch"], batch);
        assert!(Path::new(data["path"].as_str().unwrap()).starts_with(&fixture.cache_dir));
        assert!(
            data["message"]
                .as_str()
                .unwrap()
                .contains("failed to write analysis cache")
        );
    }
    assert!(
        fixture
            .debug_events(
                "disabled.jsonl",
                &["read", "--symbol", "lib.rs", "value", "--no-cache"]
            )
            .is_empty()
    );
}

#[test]
fn writable_cache_is_still_persisted_and_reused() {
    let fixture = Fixture::new(false);
    let cold = fixture.run(&[]);
    let warm = fixture.run(&[]);
    assert_eq!(cold["summary"]["files"], 1);
    assert_eq!(warm["summary"], cold["summary"]);
    assert_eq!(warm["execution"]["cache_hits"], 1);
    assert!(fixture.cache_dir.is_dir());
}

#[test]
fn unavailable_cache_does_not_hide_invalid_revisions_or_output_failures() {
    let fixture = Fixture::new(true);
    fixture
        .command(&[
            "read",
            "--symbol",
            "lib.rs",
            "value",
            "--snapshot",
            "refs/heads/absent",
            "--error-format",
            "json",
        ])
        .assert()
        .failure();
    fixture
        .command(&["read", "--symbol", "lib.rs", "value"])
        .arg("--output")
        .arg(fixture.cache_dir.join("report.json"))
        .assert()
        .failure();
}
