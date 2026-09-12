#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "bounded integration fixtures fail immediately on invalid setup or assertions"
)]

#[path = "support/command.rs"]
mod test_command;

use reposcout::metrics::tokens::TokenCounter;
use serde_json::Value;
use std::fs;
use tempfile::TempDir;
use test_command::reposcout_command;

fn fixture() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("task.rs"), "pub struct Request { pub value: usize }\npub fn execute(input: Request) -> usize { input.value + 314159 }\npub fn unrelated() -> usize { 271828 }\n").unwrap();
    root
}

#[test]
fn plan_cli_formats_share_bounded_source_opt_in() {
    let root = fixture();
    let counter = TokenCounter::new("o200k_base").unwrap();
    for format in ["json", "ndjson", "table", "markdown"] {
        for source in [false, true] {
            let mut command = reposcout_command();
            command.arg("plan").arg(root.path()).args([
                "--symbol",
                "task.rs",
                "execute",
                "-f",
                format,
                "--budget",
                "4096",
                "--max-output-bytes",
                "16384",
                "--quiet",
            ]);
            if source {
                command.arg("--source");
            }
            let output = command.assert().success().get_output().stdout.clone();
            let text = String::from_utf8(output).unwrap();
            assert!(text.len() <= 16_384);
            assert!(counter.count(&text) <= 4096);
            assert_eq!(text.contains("314159"), source, "{format}: {text}");
            assert!(!text.contains("271828"));
            if matches!(format, "json" | "ndjson") {
                let value: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(value["selected_files"], 1);
                assert!(
                    value["selected"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["name"] == "Request" && item["role"] == "environment")
                );
                assert_eq!(value.get("source").is_some(), source);
            }
        }
    }
}

#[test]
fn diagnostic_location_can_seed_definition_plan_and_source() {
    let root = fixture();
    let log = root.path().join("diagnostics.txt");
    fs::write(&log, "task.rs:2:55: error: unexpected offset\n").unwrap();
    let output = reposcout_command()
        .arg(root.path())
        .arg("--task-diagnostics")
        .arg(&log)
        .args(["--profile", "agent", "--summary", "-f", "json", "--quiet"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    let diagnostic = &value["context"]["task_evidence"]["diagnostics"][0];
    assert_eq!(diagnostic["status"], "resolved");
    let line = diagnostic["line"].as_u64().unwrap().to_string();
    let output = reposcout_command()
        .arg("plan")
        .arg(root.path())
        .args([
            "--line",
            diagnostic["path"].as_str().unwrap(),
            &line,
            "--source",
            "-f",
            "json",
            "--quiet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert!(
        value["selected"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "execute" && item["role"] == "direct")
    );
    assert!(String::from_utf8(output).unwrap().contains("314159"));
}

#[test]
fn query_errors_are_structured_and_protected_paths_remain_unchanged() {
    let root = fixture();
    let path = root.path().join("task.rs");
    let before = fs::read(&path).unwrap();
    for arguments in [
        vec![
            "plan",
            "--symbol",
            "task.rs",
            "execute",
            "--max-definitions",
            "0",
        ],
        vec![
            "consumers",
            "--symbol",
            "task.rs",
            "execute",
            "--depth",
            "999",
        ],
        vec!["find", "execute", "--limit", "0"],
    ] {
        let output = reposcout_command()
            .args(&arguments)
            .arg(root.path())
            .args(["--error-format", "json", "--quiet"])
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone();
        let error: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(error["kind"], "error");
        assert_eq!(error["category"], "runtime");
        assert_eq!(error["exit_code"], 1);
        assert!(!error["message"].as_str().unwrap().is_empty());
    }
    for output_flag in ["--output", "--debug-log"] {
        reposcout_command()
            .arg("plan")
            .arg(root.path())
            .args(["--symbol", "task.rs", "execute", output_flag])
            .arg(&path)
            .assert()
            .failure();
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
