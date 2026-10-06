#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration tests intentionally fail immediately when fixtures or assertions are invalid"
)]

#[path = "support/command.rs"]
mod test_command;

use reposcout::metrics::tokens::TokenCounter;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use test_command::reposcout_command;

fn run_json(root: &Path, arguments: &[&str]) -> Value {
    let mut command = reposcout_command();
    let output = command
        .arg("read")
        .arg(root)
        .args(arguments)
        .args(["--no-project-config", "--no-cache", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
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

fn assert_range(report: &Value, index: usize, start: usize, end: usize, content: &str) {
    let result = &report["results"][index];
    assert_eq!(result["status"], "complete");
    assert_eq!(result["selection"], "range");
    assert_eq!(
        result["requested_range"],
        json!({"start": start, "end": end})
    );
    assert!(result["definition"].is_null());
    assert_eq!(delivered(report, result)["content"], content);
}

fn assert_no_sources(report: &Value) {
    assert!(
        report
            .get("sources")
            .is_none_or(|sources| sources.as_array().is_some_and(Vec::is_empty))
    );
}

#[test]
fn sparse_range_preserves_physical_bytes_without_inventing_a_definition() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("notes.md"),
        "# Note\r\nprice = \"€\"\r\n\r\nunrelated",
    )
    .unwrap();

    let report = run_json(directory.path(), &["--range", "notes.md", "2", "3"]);

    assert_eq!(report["requested_targets"], 1);
    assert_range(&report, 0, 2, 3, "price = \"€\"\r\n\r\n");
    assert_eq!(report["sources"].as_array().unwrap().len(), 1);
    assert_eq!(report["sources"][0]["content"], "price = \"€\"\r\n\r\n");
    assert_eq!(
        report["sources"][0]["span"],
        json!({"start_byte": 8, "end_byte": 25, "start_line": 2, "end_line": 3})
    );
    assert_eq!(report["results"][0]["source"], report["sources"][0]["id"]);
    assert_eq!(report["files"][0]["path"], "notes.md");
}

#[test]
fn physical_ranges_never_clip_past_eof_or_invent_an_empty_final_line() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("empty.md"), "").unwrap();
    fs::write(directory.path().join("terminated.md"), "x\n").unwrap();
    fs::write(directory.path().join("unterminated.md"), "x\ny").unwrap();
    let report = run_json(
        directory.path(),
        &[
            "--range",
            "empty.md",
            "1",
            "1",
            "--range",
            "terminated.md",
            "1",
            "1",
            "--range",
            "terminated.md",
            "2",
            "2",
            "--range",
            "terminated.md",
            "1",
            "2",
            "--range",
            "unterminated.md",
            "2",
            "2",
            "--range",
            "unterminated.md",
            "3",
            "3",
        ],
    );

    assert_range(&report, 1, 1, 1, "x\n");
    assert_range(&report, 4, 2, 2, "y");
    for index in [0, 2, 3, 5] {
        assert_eq!(report["results"][index]["status"], "not-found");
        assert!(report["results"][index]["source"].is_null());
    }
    assert_eq!(report["sources"].as_array().unwrap().len(), 2);
    assert_eq!(
        delivered(&report, &report["results"][4])["span"],
        json!({"start_byte": 2, "end_byte": 3, "start_line": 2, "end_line": 2})
    );
}

#[test]
fn overlapping_ranges_share_bytes_but_disjoint_ranges_do_not_expand_into_the_gap() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("notes.md"),
        "one\ntwo\nthree\nfour\nPRIVATE_GAP\nsix\n",
    )
    .unwrap();
    let report = run_json(
        directory.path(),
        &[
            "--range", "notes.md", "2", "3", "--range", "notes.md", "3", "4", "--range",
            "notes.md", "6", "6",
        ],
    );

    assert_range(&report, 0, 2, 3, "two\nthree\nfour\n");
    assert_range(&report, 1, 3, 4, "two\nthree\nfour\n");
    assert_range(&report, 2, 6, 6, "six\n");
    assert_eq!(
        report["results"][0]["source"],
        report["results"][1]["source"]
    );
    assert_eq!(report["sources"].as_array().unwrap().len(), 2);
    assert_eq!(
        delivered(&report, &report["results"][0])["span"],
        json!({"start_byte": 4, "end_byte": 19, "start_line": 2, "end_line": 4})
    );
    assert!(!report.to_string().contains("PRIVATE_GAP"));
}

#[test]
fn range_targets_follow_existing_selector_groups_and_retain_intent_in_shared_chunks() {
    let directory = tempfile::tempdir().unwrap();
    let source = "from helper import answer\n\ndef run():\n    return answer()\n";
    fs::write(directory.path().join("api.py"), source).unwrap();
    let report = run_json(
        directory.path(),
        &[
            "--range", "api.py", "1", "1", "--file", "api.py", "--line", "api.py", "4", "--symbol",
            "api.py", "run", "--range", "api.py", "3", "4",
        ],
    );

    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 5);
    assert_eq!(results[0]["selection"], "exact-qualified-name");
    assert_eq!(results[1]["selection"], "innermost-line");
    assert_eq!(results[2]["selection"], "file");
    assert_range(&report, 3, 1, 1, source);
    assert_range(&report, 4, 3, 4, source);
    assert_eq!(report["sources"].as_array().unwrap().len(), 1);
    for (index, result) in results.iter().enumerate() {
        assert_eq!(result["target"], index + 1);
        assert_eq!(result["status"], "complete");
        assert_eq!(result["source"], results[0]["source"]);
    }
    assert_eq!(results[0]["definition"]["name"], "run");
    assert!(results[2]["definition"].is_null());
}

#[test]
fn omitted_overlapping_range_keeps_prior_source_and_admits_later_small_range() {
    let directory = tempfile::tempdir().unwrap();
    let source = format!("first\n{}\nlast\n", "LARGE_OMITTED_BODY_".repeat(1500));
    fs::write(directory.path().join("notes.md"), source).unwrap();
    let mut command = reposcout_command();
    let stdout = command
        .arg("read")
        .arg(directory.path())
        .args([
            "--range",
            "notes.md",
            "1",
            "1",
            "--range",
            "notes.md",
            "1",
            "2",
            "--range",
            "notes.md",
            "3",
            "3",
            "--no-project-config",
            "--no-cache",
            "--format",
            "json",
            "--budget",
            "2048",
            "--max-output-bytes",
            "4096",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&stdout).unwrap();

    assert_range(&report, 0, 1, 1, "first\n");
    assert_eq!(report["results"][1]["status"], "budget-omitted");
    assert_eq!(
        report["results"][1]["requested_range"],
        json!({"start": 1, "end": 2})
    );
    assert!(report["results"][1]["source"].is_null());
    assert_range(&report, 2, 3, 3, "last\n");
    assert_eq!(report["sources"].as_array().unwrap().len(), 2);
    assert_eq!(
        delivered(&report, &report["results"][0])["span"],
        json!({"start_byte": 0, "end_byte": 6, "start_line": 1, "end_line": 1})
    );
    let rendered = std::str::from_utf8(&stdout).unwrap();
    assert!(!rendered.contains("LARGE_OMITTED_BODY_"));
    assert!(stdout.len() <= 4096);
    assert!(TokenCounter::new("o200k_base").unwrap().count(rendered) <= 2048);
}

#[test]
fn range_reads_preserve_extraction_independence_and_full_file_input_limits() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("broken.py"),
        "from helpers import answer\ndef broken(:\n    return answer()\n",
    )
    .unwrap();
    let report = run_json(directory.path(), &["--range", "broken.py", "1", "1"]);
    assert_range(&report, 0, 1, 1, "from helpers import answer\n");
    assert_eq!(report["files"][0]["extraction"], "parse-errors");

    let oversized = run_json(
        directory.path(),
        &["--range", "broken.py", "1", "1", "--max-file-bytes", "16"],
    );
    assert_eq!(oversized["results"][0]["status"], "oversized");
    assert_no_sources(&oversized);
}

#[test]
fn sparse_snapshot_reads_use_full_blob_hashes_and_ignore_dirty_unselected_lines() {
    let directory = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(directory.path()).unwrap();
    let source = "from helpers import answer\n\ndef run():\n    return 1\n";
    fs::write(directory.path().join("api.py"), source).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("api.py")).unwrap();
    index.write().unwrap();
    let tree = index.write_tree().unwrap().to_string();
    let captured = run_json(
        directory.path(),
        &["--range", "api.py", "1", "1", "--snapshot", &tree],
    );
    let hash = captured["files"][0]["sha256"].as_str().unwrap();
    fs::write(
        directory.path().join("api.py"),
        source.replace("return 1", "return 2"),
    )
    .unwrap();

    let pinned = run_json(
        directory.path(),
        &[
            "--range",
            "api.py",
            "1",
            "1",
            "--snapshot",
            &tree,
            "--expect-hash",
            "api.py",
            hash,
        ],
    );
    assert_range(&pinned, 0, 1, 1, "from helpers import answer\n");
    assert_eq!(
        pinned["files"][0]["snapshot"],
        json!({"kind": "tree", "revision": tree})
    );
    assert_eq!(pinned["files"][0]["sha256"], hash);
    let stale = run_json(
        directory.path(),
        &[
            "--range",
            "api.py",
            "1",
            "1",
            "--expect-hash",
            "api.py",
            hash,
        ],
    );
    assert_eq!(stale["results"][0]["status"], "stale");
    assert_no_sources(&stale);
}

#[test]
fn malformed_range_bounds_conflicts_and_excess_targets_are_usage_errors() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("api.py"), "import helpers\n").unwrap();
    for arguments in [
        vec!["--range", "api.py", "0", "1"],
        vec!["--range", "api.py", "2", "1"],
        vec!["--range", "api.py", "1", "18446744073709551616"],
        vec!["--range", "api.py", "1", "1", "--outline", "api.py"],
    ] {
        let mut command = reposcout_command();
        let stderr = command
            .arg("read")
            .arg(directory.path())
            .args(arguments)
            .args(["--error-format", "json"])
            .assert()
            .failure()
            .code(2)
            .get_output()
            .stderr
            .clone();
        let error: Value = serde_json::from_slice(&stderr).unwrap();
        assert_eq!(error["category"], "usage");
    }
    let mut too_many = reposcout_command();
    too_many.arg("read").arg(directory.path());
    for _ in 0..33 {
        too_many.args(["--range", "api.py", "1", "1"]);
    }
    too_many.assert().failure().code(2);
    for unsupported in ["plan", "consumers"] {
        reposcout_command()
            .arg(unsupported)
            .arg(directory.path())
            .args(["--range", "api.py", "1", "1"])
            .assert()
            .failure()
            .code(2);
    }
}

#[test]
fn range_output_and_debug_log_cannot_replace_the_selected_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = "from helpers import answer\n\ndef run():\n    return answer()\n";
    let selected = directory.path().join("api.py");
    fs::write(&selected, source).unwrap();
    for flag in ["--output", "--debug-log"] {
        reposcout_command()
            .arg("read")
            .arg(directory.path())
            .args(["--range", "api.py", "1", "1", flag])
            .arg(&selected)
            .assert()
            .failure();
        assert_eq!(fs::read_to_string(&selected).unwrap(), source);
    }
}
