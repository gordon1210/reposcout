#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests intentionally fail immediately for invalid fixtures or assertions"
)]

#[path = "support/command.rs"]
mod test_command;

use reposcout::metrics::tokens::TokenCounter;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_command::reposcout_command;

const MODULE: &str = "from .handlers import quote as active_quote\r\n\r\nROUTES = {\"POST /shipping/quote\": active_quote}\r\nLABEL = \"Grüße 🚚\"\r\n\r\ndef dispatch():\r\n\treturn ROUTES[\"POST /shipping/quote\"]()\r\n";

struct Fixture {
    directory: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        fs::create_dir(&root).unwrap();
        Self { directory, root }
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn command(&self) -> assert_cmd::Command {
        let mut command = reposcout_command();
        command
            .arg("read")
            .arg(&self.root)
            .arg("--no-project-config")
            .env("XDG_CACHE_HOME", self.directory.path().join("cache"));
        command
    }

    fn json(&self, arguments: &[&str]) -> Value {
        let stdout = self
            .command()
            .args(arguments)
            .args(["--format", "json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice(&stdout).unwrap()
    }
}

fn delivered<'a>(report: &'a Value, result: &Value) -> &'a Value {
    let source_id = result["source"]
        .as_u64()
        .expect("complete result has source");
    report["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["id"] == source_id)
        .expect("source reference resolves")
}

fn captured_file(report: &Value, result_index: usize) -> &Value {
    let file_id = &report["results"][result_index]["file"];
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| &file["id"] == file_id)
        .expect("result's captured file reference resolves")
}

fn assert_whole_file(report: &Value, result_index: usize, expected: &str) {
    let result = &report["results"][result_index];
    assert_eq!(result["status"], "complete");
    assert_eq!(result["selection"], "file");
    assert!(result["definition"].is_null());
    let source = delivered(report, result);
    assert_eq!(source["content"], expected);
    assert_eq!(source["span"]["start_byte"], 0);
    assert_eq!(source["span"]["end_byte"], expected.len());
    assert_eq!(source["file"], result["file"]);
}

fn assert_no_sources(report: &Value) {
    assert!(
        report
            .get("sources")
            .is_none_or(|sources| sources.as_array().is_some_and(Vec::is_empty))
    );
}

#[test]
fn explicit_file_read_preserves_module_bindings_unicode_and_line_endings() {
    let fixture = Fixture::new();
    fixture.write("routes.py", MODULE);
    fixture.write("empty.py", "");
    fixture.write("without_newline.json", "{\"label\":\"Grüße 🚚\"}");
    let report = fixture.json(&[
        "--file",
        "routes.py",
        "--file",
        "empty.py",
        "--file",
        "without_newline.json",
    ]);

    assert_eq!(report["mode"], "source");
    assert_eq!(report["requested_targets"], 3);
    assert_eq!(report["omitted_targets"], 0);
    assert_whole_file(&report, 0, MODULE);
    assert_whole_file(&report, 1, "");
    assert_whole_file(&report, 2, "{\"label\":\"Grüße 🚚\"}");
    assert_eq!(
        delivered(&report, &report["results"][0])["span"]["start_line"],
        1
    );
    assert_eq!(
        delivered(&report, &report["results"][0])["span"]["end_line"],
        7
    );
    assert_eq!(
        delivered(&report, &report["results"][1])["span"]["start_line"],
        1
    );
    assert_eq!(
        delivered(&report, &report["results"][1])["span"]["end_line"],
        1
    );
    assert_eq!(captured_file(&report, 0)["snapshot"]["kind"], "worktree");
    assert_eq!(
        captured_file(&report, 0)["sha256"].as_str().unwrap().len(),
        64
    );
    assert_eq!(
        captured_file(&report, 1)["sha256"],
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn mixed_selectors_keep_definition_semantics_and_share_full_file_source() {
    let fixture = Fixture::new();
    fixture.write("routes.py", MODULE);
    fixture.write("empty.py", "");
    let report = fixture.json(&[
        "--file",
        "routes.py",
        "--line",
        "routes.py",
        "7",
        "--symbol",
        "routes.py",
        "dispatch",
        "--file",
        "routes.py",
        "--file",
        "empty.py",
        "--file",
        "empty.py",
    ]);

    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 6);
    assert_eq!(report["files"].as_array().unwrap().len(), 2);
    assert_eq!(report["sources"].as_array().unwrap().len(), 2);
    assert_eq!(results[0]["selection"], "exact-qualified-name");
    assert_eq!(results[1]["selection"], "innermost-line");
    for result in &results[..2] {
        assert_eq!(result["status"], "complete");
        assert_eq!(result["definition"]["name"], "dispatch");
    }
    for index in 2..4 {
        assert_whole_file(&report, index, MODULE);
        assert_eq!(results[index]["source"], results[0]["source"]);
    }
    assert_eq!(results[1]["source"], results[0]["source"]);
    for index in 4..6 {
        assert_whole_file(&report, index, "");
    }
    assert_eq!(results[4]["source"], results[5]["source"]);
    for (index, result) in results.iter().enumerate() {
        assert_eq!(result["target"], index + 1);
    }

    let definition = fixture.json(&["--symbol", "routes.py", "dispatch"]);
    let body = delivered(&definition, &definition["results"][0]);
    assert_eq!(definition["results"][0]["definition"]["name"], "dispatch");
    assert!(!body["content"].as_str().unwrap().contains("from .handlers"));
    assert!(!body["content"].as_str().unwrap().contains("ROUTES ="));
}

#[test]
fn file_read_does_not_require_successful_declaration_extraction() {
    let fixture = Fixture::new();
    fixture.write(
        "routes.json",
        "{\"POST /shipping/quote\": \"handlers.quote\"}\n",
    );
    fixture.write(
        "broken.py",
        "from handlers import quote\ndef broken(:\n    return quote()\n",
    );
    fixture.write("unknown.reposcout_unknown", "important binding\n");
    let report = fixture.json(&[
        "--file",
        "routes.json",
        "--file",
        "broken.py",
        "--file",
        "unknown.reposcout_unknown",
    ]);

    assert_whole_file(
        &report,
        0,
        "{\"POST /shipping/quote\": \"handlers.quote\"}\n",
    );
    assert_whole_file(
        &report,
        1,
        "from handlers import quote\ndef broken(:\n    return quote()\n",
    );
    assert_eq!(captured_file(&report, 0)["extraction"], "unsupported");
    assert_eq!(captured_file(&report, 1)["extraction"], "parse-errors");
    assert_eq!(report["results"][2]["status"], "unsupported");
    assert!(report["results"][2]["source"].is_null());
    assert_eq!(report["sources"].as_array().unwrap().len(), 2);
}

#[test]
fn whole_file_budget_omission_preserves_a_small_independent_file_in_every_format() {
    let fixture = Fixture::new();
    fixture.write(
        "large.json",
        &format!("{{\"payload\":\"{}\"}}\n", "LARGE_PAYLOAD_".repeat(800)),
    );
    fixture.write(
        "small.py",
        "from handlers import quote\nROUTES = {\"quote\": quote}\n",
    );

    for encoding in ["o200k_base", "cl100k_base"] {
        let counter = TokenCounter::new(encoding).unwrap();
        for format in ["json", "ndjson", "table", "markdown"] {
            let stdout = fixture
                .command()
                .args([
                    "--file",
                    "large.json",
                    "--file",
                    "small.py",
                    "--budget",
                    "2048",
                    "--max-output-bytes",
                    "4096",
                    "--encoding",
                    encoding,
                    "--format",
                    format,
                ])
                .assert()
                .success()
                .get_output()
                .stdout
                .clone();
            let rendered = std::str::from_utf8(&stdout).unwrap();
            assert!(stdout.len() <= 4096, "{format}/{encoding}");
            assert!(counter.count(rendered) <= 2048, "{format}/{encoding}");
            assert!(stdout.ends_with(b"\n"));
            assert!(
                !rendered.contains("LARGE_PAYLOAD_"),
                "no partial file: {format}/{encoding}"
            );
            if matches!(format, "json" | "ndjson") {
                let report: Value = serde_json::from_slice(&stdout).unwrap();
                assert_eq!(report["encoding"], encoding);
                assert_eq!(report["results"][0]["status"], "budget-omitted");
                assert_whole_file(
                    &report,
                    1,
                    "from handlers import quote\nROUTES = {\"quote\": quote}\n",
                );
            } else {
                assert!(rendered.contains("budget-omitted"));
                assert!(rendered.contains("from handlers import quote"));
                assert!(rendered.contains("ROUTES = {\"quote\": quote}"));
            }
        }
    }
}

#[test]
fn whole_file_token_limit_keeps_small_file_when_byte_budget_has_room() {
    let fixture = Fixture::new();
    let content = format!(
        "{{\"payload\":\"{}\"}}\n",
        "LARGE_TOKEN_PAYLOAD_".repeat(800)
    );
    assert!(content.len() < 32_768);
    fixture.write("large.json", &content);
    fixture.write("small.py", "from handlers import quote\n");

    for encoding in ["o200k_base", "cl100k_base"] {
        let counter = TokenCounter::new(encoding).unwrap();
        assert!(counter.count(&content) > 1024);
        let stdout = fixture
            .command()
            .args([
                "--file",
                "large.json",
                "--file",
                "small.py",
                "--budget",
                "1024",
                "--max-output-bytes",
                "65536",
                "--encoding",
                encoding,
                "--format",
                "json",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let rendered = std::str::from_utf8(&stdout).unwrap();
        let report: Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(report["results"][0]["status"], "budget-omitted");
        assert!(report["results"][0]["source"].is_null());
        assert_whole_file(&report, 1, "from handlers import quote\n");
        assert!(!rendered.contains("LARGE_TOKEN_PAYLOAD_"));
        assert!(counter.count(rendered) <= 1024);
        assert!(stdout.len() <= 65_536);
        assert!(stdout.ends_with(b"\n"));
    }
}

#[test]
fn file_hash_handoff_is_stable_warm_and_fails_closed_after_binding_edits() {
    let fixture = Fixture::new();
    fixture.write("routes.py", MODULE);
    let cold = fixture.json(&["--file", "routes.py"]);
    let warm = fixture.json(&["--file", "routes.py"]);
    assert_eq!(cold, warm);
    let expected_hash = cold["files"][0]["sha256"].as_str().unwrap();
    let pinned = fixture.json(&[
        "--file",
        "routes.py",
        "--expect-hash",
        "routes.py",
        expected_hash,
    ]);
    assert_whole_file(&pinned, 0, MODULE);

    let edited = MODULE.replace("handlers import", "replacement import");
    fixture.write("routes.py", &edited);
    let stale = fixture.json(&[
        "--file",
        "routes.py",
        "--symbol",
        "routes.py",
        "dispatch",
        "--line",
        "routes.py",
        "7",
        "--expect-hash",
        "routes.py",
        expected_hash,
    ]);
    for result in stale["results"].as_array().unwrap() {
        assert_eq!(result["status"], "stale");
    }
    assert_no_sources(&stale);
    assert_ne!(stale["files"][0]["sha256"], cold["files"][0]["sha256"]);
    let refreshed = fixture.json(&["--file", "routes.py"]);
    assert_whole_file(&refreshed, 0, &edited);
}

fn stage(repo: &git2::Repository, path: &str) {
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(path)).unwrap();
    index.write().unwrap();
}

fn first_commit(repo: &git2::Repository) -> String {
    let tree_id = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
        .unwrap();
    tree_id.to_string()
}

#[test]
fn file_snapshot_reads_keep_base_index_and_worktree_bindings_distinct() {
    let fixture = Fixture::new();
    let repo = git2::Repository::init(&fixture.root).unwrap();
    let base = "from base import quote\nROUTES = {\"quote\": quote}\n";
    let indexed = "from staged import quote\nROUTES = {\"quote\": quote}\n";
    let current = "from live import quote\nROUTES = {\"quote\": quote}\n";
    fixture.write("src/routes.py", base);
    stage(&repo, "src/routes.py");
    let tree = first_commit(&repo);
    fixture.write("src/routes.py", indexed);
    stage(&repo, "src/routes.py");
    fixture.write("src/routes.py", current);

    let mut hashes = Vec::new();
    for (snapshot, expected) in [
        (tree.as_str(), base),
        ("index", indexed),
        ("worktree", current),
    ] {
        let report = fixture.json(&["--file", "src/routes.py", "--snapshot", snapshot]);
        assert_whole_file(&report, 0, expected);
        hashes.push(report["files"][0]["sha256"].as_str().unwrap().to_string());
        if snapshot == tree {
            assert_eq!(report["files"][0]["snapshot"]["kind"], "tree");
            assert_eq!(report["files"][0]["snapshot"]["revision"], tree);
        } else {
            assert_eq!(report["files"][0]["snapshot"]["kind"], snapshot);
        }
    }
    assert_ne!(hashes[0], hashes[1]);
    assert_ne!(hashes[1], hashes[2]);
    assert_ne!(hashes[0], hashes[2]);
}

#[test]
fn pinned_file_reads_ignore_removed_or_symlinked_live_parents_but_reject_git_symlinks() {
    let fixture = Fixture::new();
    let repo = git2::Repository::init(&fixture.root).unwrap();
    fixture.write("src/routes.py", MODULE);
    stage(&repo, "src/routes.py");
    std::os::unix::fs::symlink("src/routes.py", fixture.root.join("alias.py")).unwrap();
    stage(&repo, "alias.py");
    let tree = first_commit(&repo);
    let original = fixture.json(&["--file", "src/routes.py", "--snapshot", &tree]);
    let hash = original["files"][0]["sha256"].as_str().unwrap();
    let saved = fixture.directory.path().join("saved-src");
    fs::rename(fixture.root.join("src"), &saved).unwrap();
    assert_eq!(
        fixture.json(&["--file", "src/routes.py", "--snapshot", &tree]),
        original
    );
    std::os::unix::fs::symlink(&saved, fixture.root.join("src")).unwrap();
    for snapshot in [tree.as_str(), "index"] {
        let report = fixture.json(&[
            "--file",
            "src/routes.py",
            "--snapshot",
            snapshot,
            "--expect-hash",
            "src/routes.py",
            hash,
        ]);
        assert_whole_file(&report, 0, MODULE);
        let alias = fixture.json(&["--file", "alias.py", "--snapshot", snapshot]);
        assert_eq!(alias["results"][0]["status"], "not-regular-file");
        assert_no_sources(&alias);
    }
    let live = fixture.json(&["--file", "src/routes.py"]);
    assert_eq!(live["results"][0]["status"], "not-regular-file");
    assert_no_sources(&live);
}

#[test]
fn file_selection_preserves_path_and_ignore_policy() {
    let fixture = Fixture::new();
    fixture.write("src/routes.py", MODULE);
    fixture.write("ignored.py", "from secret import quote\n");
    fixture.write(".reposcoutignore", "ignored.py\n");
    let outside = fixture.directory.path().join("outside.py");
    fs::write(&outside, "from outside import quote\n").unwrap();
    let report = fixture.json(&[
        "--file",
        "ignored.py",
        "--file",
        "src/routes.py",
        "--file",
        "../outside.py",
        "--file",
        outside.to_str().unwrap(),
        "--exclude",
        "src/routes.py",
        "--no-ignore",
    ]);
    let statuses = report["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| result["status"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        ["excluded", "excluded", "invalid-path", "invalid-path"]
    );
    assert_no_sources(&report);

    let absolute = fixture.root.join("ignored.py");
    let ignored = fixture.json(&["--file", absolute.to_str().unwrap()]);
    assert_eq!(ignored["results"][0]["status"], "excluded");
    assert_no_sources(&ignored);
}

#[test]
fn file_input_limits_are_distinct_from_output_budget_omissions() {
    let fixture = Fixture::new();
    fixture.write("routes.py", MODULE);
    let report = fixture.json(&["--file", "routes.py", "--max-file-bytes", "16"]);
    assert_eq!(report["results"][0]["status"], "oversized");
    assert_ne!(report["results"][0]["status"], "budget-omitted");
    assert_no_sources(&report);

    fixture.write("binary.json", "{\"payload\":\"before\0after\"}\n");
    let binary = fixture.json(&["--file", "binary.json"]);
    assert_eq!(binary["results"][0]["status"], "binary");
    assert_no_sources(&binary);
}

#[test]
fn file_selectors_preserve_outline_exclusivity_target_cap_and_output_safety() {
    let fixture = Fixture::new();
    fixture.write("routes.py", MODULE);
    fixture
        .command()
        .args(["--outline", "routes.py", "--file", "routes.py"])
        .assert()
        .failure()
        .code(2);
    let mut too_many = fixture.command();
    for _ in 0..33 {
        too_many.args(["--file", "routes.py"]);
    }
    too_many.assert().failure().code(2);

    let source_path = fixture.root.join("routes.py");
    for flag in ["--output", "--debug-log"] {
        fixture
            .command()
            .args(["--file", "routes.py", flag])
            .arg(&source_path)
            .assert()
            .failure();
        assert_eq!(fs::read_to_string(&source_path).unwrap(), MODULE);
    }
}
