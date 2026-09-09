#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration tests intentionally fail immediately when fixtures or assertions are invalid"
)]

#[path = "support/command.rs"]
mod test_command;

use clap::Parser;
use reposcout::cli::{Cli, Command};
use reposcout::metrics::tokens::TokenCounter;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use test_command::reposcout_command;

const SOURCE: &str = r#"#[derive(Debug)]
pub struct Widget {
    value: i32,
}

impl Widget {
    pub fn compute(&self) -> i32 {
        let secret = "SECRET_BODY_LITERAL_7";
        self.value + secret.len() as i32
    }
}

pub fn helper(input: i32) -> i32 {
    input * 2
}
"#;

fn fixture() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("lib.rs"), SOURCE).unwrap();
    directory
}

fn run_json(root: &Path, arguments: &[&str]) -> Value {
    let mut command = reposcout_command();
    command.arg("read").arg(root).args(arguments);
    let output = command.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
}

fn assert_no_sources(report: &Value) {
    assert!(
        report
            .get("sources")
            .is_none_or(|sources| sources.as_array().is_some_and(Vec::is_empty))
    );
}

#[test]
fn capabilities_match_the_read_cli_defaults_limits_and_language_matrix() {
    let capability = reposcout::query::capabilities().source_query.unwrap();
    assert!(capability.available);
    assert_eq!(capability.platforms, ["unix"]);
    let parsed =
        Cli::try_parse_from(["reposcout", "read", "--symbol", "lib.rs", "helper"]).unwrap();
    let Command::Read(read) = parsed.command.unwrap() else {
        panic!("expected read command");
    };
    assert_eq!(read.budget, capability.default_tokens);
    assert_eq!(read.max_output_bytes, capability.default_bytes);
    assert_eq!(capability.formats, ["table", "json", "markdown", "ndjson"]);
    assert_eq!(
        capability.selectors,
        ["--symbol FILE SYMBOL", "--line FILE LINE", "--outline FILE"]
    );
    assert_eq!(capability.snapshot, "worktree");
    assert_eq!(capability.hash_algorithm, "sha256");
    assert_eq!(
        (
            capability.min_tokens,
            capability.max_tokens,
            capability.min_bytes,
            capability.max_bytes,
        ),
        (256, 65_536, 1_024, 1_048_576)
    );
    assert_eq!(capability.max_targets, 32);
    assert_eq!(capability.max_candidates, 8);
    assert_eq!(capability.max_outline_declarations, 100);
    assert_eq!(capability.max_input_file_bytes, 8 * 1_024 * 1_024);
    assert_eq!(capability.max_input_total_bytes, 32 * 1_024 * 1_024);
    let matrix = capability
        .languages
        .iter()
        .map(|language| {
            (
                language.language.as_str(),
                language
                    .kinds
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matrix,
        [
            ("Rust", vec!["enum", "function", "method", "trait", "type"]),
            ("Python", vec!["class", "function", "method"]),
            ("JavaScript", vec!["class", "function", "method"]),
            (
                "TypeScript",
                vec!["class", "enum", "function", "interface", "method", "type"]
            ),
            (
                "TSX",
                vec!["class", "enum", "function", "interface", "method", "type"]
            ),
            ("Go", vec!["function", "method", "type"]),
            (
                "PHP",
                vec!["class", "enum", "function", "interface", "method", "trait"]
            ),
            (
                "GDScript",
                vec!["class", "constant", "enum", "method", "property", "signal"]
            ),
            ("Godot Shader", vec!["function"]),
        ]
    );

    let mut command = reposcout_command();
    let stdout = command
        .args(["capabilities", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(
        report["source_query"],
        serde_json::to_value(&capability).unwrap()
    );
}

#[test]
fn reads_explicit_symbols_and_lines_in_one_batch() {
    let directory = fixture();
    let report = run_json(
        directory.path(),
        &[
            "--symbol",
            "lib.rs",
            "Widget.compute",
            "--line",
            "lib.rs",
            "13",
            "--format",
            "json",
        ],
    );

    assert_eq!(report["kind"], "source_query");
    assert_eq!(report["mode"], "source");
    assert_eq!(report["requested_targets"], 2);
    assert_eq!(report["omitted_targets"], 0);
    assert_eq!(report["results"][0]["status"], "complete");
    assert_eq!(report["results"][1]["status"], "complete");
    let delivered = report["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| source["content"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(
        delivered
            .iter()
            .any(|source| source.contains("pub fn compute"))
    );
    assert!(
        delivered
            .iter()
            .any(|source| source.contains("pub fn helper"))
    );
}

#[test]
fn reports_ambiguous_symbols_without_delivering_a_guessed_body() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("ambiguous.rs"),
        "fn one() { fn nested() {} nested(); }\nfn two() { fn nested() {} nested(); }\n",
    )
    .unwrap();

    let report = run_json(
        directory.path(),
        &["--symbol", "ambiguous.rs", "nested", "--format", "json"],
    );

    assert_eq!(report["results"][0]["status"], "ambiguous");
    assert!(report["results"][0]["source"].is_null());
    assert_eq!(report["results"][0]["total_candidates"], 2);
    assert_no_sources(&report);
}

#[test]
fn rejects_stale_content_for_every_selection_of_the_hashed_file() {
    let directory = fixture();
    let absolute_source = directory.path().join("lib.rs");
    let absolute_source = absolute_source.to_string_lossy().into_owned();
    let report = run_json(
        directory.path(),
        &[
            "--symbol",
            "lib.rs",
            "Widget.compute",
            "--line",
            &absolute_source,
            "13",
            "--expect-hash",
            "lib.rs",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "--format",
            "json",
        ],
    );

    assert_eq!(report["results"][0]["status"], "stale");
    assert_eq!(report["results"][1]["status"], "stale");
    assert_no_sources(&report);
}

#[test]
fn outline_is_body_free_and_cannot_be_combined_with_source_selectors() {
    let directory = fixture();
    let report = run_json(
        directory.path(),
        &["--outline", "lib.rs", "--format", "json"],
    );

    assert_eq!(report["mode"], "outline");
    assert_eq!(report["results"][0]["status"], "outline");
    assert_no_sources(&report);
    assert!(!report.to_string().contains("SECRET_BODY_LITERAL_7"));

    let mut command = reposcout_command();
    command
        .arg("read")
        .arg(directory.path())
        .args(["--outline", "lib.rs", "--symbol", "lib.rs", "helper"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn oversized_inputs_are_reported_without_source() {
    let directory = fixture();
    let report = run_json(
        directory.path(),
        &[
            "--symbol",
            "lib.rs",
            "helper",
            "--max-file-bytes",
            "64",
            "--format",
            "json",
        ],
    );

    assert_eq!(report["results"][0]["status"], "oversized");
    assert_no_sources(&report);
}

#[test]
fn complete_rendered_output_obeys_token_and_byte_budgets() {
    let directory = fixture();
    fs::write(
        directory.path().join("large.rs"),
        format!(
            "pub fn huge() -> usize {{\n    let payload = \"{}\";\n    payload.len()\n}}\n",
            "large_definition_payload_".repeat(300)
        ),
    )
    .unwrap();
    let mut command = reposcout_command();
    command.arg("read").arg(directory.path()).args([
        "--symbol",
        "lib.rs",
        "helper",
        "--symbol",
        "large.rs",
        "huge",
        "--budget",
        "256",
        "--max-output-bytes",
        "1024",
        "--encoding",
        "cl100k_base",
        "--format",
        "json",
    ]);
    let stdout = command.assert().success().get_output().stdout.clone();
    let rendered = std::str::from_utf8(&stdout).unwrap();
    let report: Value = serde_json::from_slice(&stdout).unwrap();

    assert!(stdout.len() <= 1024);
    assert!(stdout.ends_with(b"\n"));
    assert!(TokenCounter::new("cl100k_base").unwrap().count(rendered) <= 256);
    assert!(
        report["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["status"] == "budget-omitted")
    );
    assert!(!rendered.contains("large_definition_payload_"));
}

#[test]
fn human_and_ndjson_formats_return_the_same_selected_source() {
    let directory = fixture();
    for format in ["table", "markdown"] {
        let mut command = reposcout_command();
        let stdout = command
            .arg("read")
            .arg(directory.path())
            .args(["--symbol", "lib.rs", "helper", "--format", format])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let rendered = std::str::from_utf8(&stdout).unwrap();
        assert!(rendered.contains("pub fn helper"), "{format}");
        assert!(rendered.ends_with('\n'), "{format}");
    }

    let mut command = reposcout_command();
    let stdout = command
        .arg("read")
        .arg(directory.path())
        .args(["--symbol", "lib.rs", "helper", "--format", "ndjson"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rendered = std::str::from_utf8(&stdout).unwrap();
    assert!(rendered.ends_with('\n'));
    assert_eq!(rendered.lines().count(), 1);
    let report: Value = serde_json::from_str(rendered.trim_end()).unwrap();
    assert_eq!(report["kind"], "source_query");
    assert_eq!(report["results"][0]["status"], "complete");
}

#[test]
fn pretty_and_structured_error_formats_remain_automation_safe() {
    let directory = fixture();
    let mut pretty = reposcout_command();
    pretty
        .arg("read")
        .arg(directory.path())
        .args(["--symbol", "lib.rs", "helper", "--format", "json"])
        .arg("--pretty");
    let stdout = pretty.assert().success().get_output().stdout.clone();
    assert!(
        std::str::from_utf8(&stdout)
            .unwrap()
            .contains("\n  \"kind\"")
    );
    serde_json::from_slice::<Value>(&stdout).unwrap();

    let mut invalid = reposcout_command();
    let stderr = invalid
        .arg("read")
        .arg(directory.path())
        .args(["--symbol", "lib.rs", "helper", "--format", "ndjson"])
        .arg("--pretty")
        .args(["--error-format", "json"])
        .assert()
        .failure()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let error: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(error["kind"], "error");
    assert_eq!(error["category"], "usage");
}

#[test]
fn ordinary_scan_output_remains_body_free() {
    let directory = fixture();
    let mut command = reposcout_command();
    let stdout = command
        .arg(directory.path())
        .args(["--format", "json", "--profile", "agent", "--quiet"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert!(
        !std::str::from_utf8(&stdout)
            .unwrap()
            .contains("SECRET_BODY_LITERAL_7")
    );
}

#[test]
fn output_cannot_replace_the_root_or_a_selected_source() {
    let directory = fixture();
    let source = directory.path().join("lib.rs");

    let mut selected = reposcout_command();
    selected
        .arg("read")
        .arg(directory.path())
        .args(["--symbol", "lib.rs", "helper", "--output"])
        .arg(&source)
        .assert()
        .failure();
    assert_eq!(fs::read_to_string(&source).unwrap(), SOURCE);

    let mut root = reposcout_command();
    root.arg("read")
        .arg(directory.path())
        .args(["--symbol", "lib.rs", "helper", "--output"])
        .arg(directory.path())
        .assert()
        .failure();
    assert_eq!(fs::read_to_string(&source).unwrap(), SOURCE);

    let mut debug_log = reposcout_command();
    debug_log
        .arg("read")
        .arg(directory.path())
        .args(["--symbol", "lib.rs", "helper", "--debug-log"])
        .arg(&source)
        .assert()
        .failure();
    assert_eq!(fs::read_to_string(&source).unwrap(), SOURCE);
}

#[test]
fn absolute_source_paths_are_accepted_only_inside_the_query_root() {
    let directory = fixture();
    let source = directory.path().join("lib.rs");
    let source_text = source.to_string_lossy().into_owned();
    let report = run_json(
        directory.path(),
        &["--symbol", &source_text, "helper", "--format", "json"],
    );
    assert_eq!(report["results"][0]["status"], "complete");

    let outside = tempfile::tempdir().unwrap();
    let outside_source = outside.path().join("outside.rs");
    fs::write(&outside_source, "fn outside() {}\n").unwrap();
    let outside_text = outside_source.to_string_lossy().into_owned();
    let report = run_json(
        directory.path(),
        &["--symbol", &outside_text, "outside", "--format", "json"],
    );
    assert_eq!(report["results"][0]["status"], "invalid-path");
    assert_no_sources(&report);
}

#[test]
fn unmatched_and_conflicting_hashes_are_usage_errors() {
    let directory = fixture();
    let hash_a = "0000000000000000000000000000000000000000000000000000000000000000";
    let hash_b = "1111111111111111111111111111111111111111111111111111111111111111";

    let mut unmatched = reposcout_command();
    unmatched
        .arg("read")
        .arg(directory.path())
        .args([
            "--symbol",
            "lib.rs",
            "helper",
            "--expect-hash",
            "other.rs",
            hash_a,
        ])
        .assert()
        .failure()
        .code(2);

    let mut conflicting = reposcout_command();
    conflicting
        .arg("read")
        .arg(directory.path())
        .args([
            "--symbol",
            "lib.rs",
            "helper",
            "--expect-hash",
            "lib.rs",
            hash_a,
            "--expect-hash",
            "lib.rs",
            hash_b,
        ])
        .assert()
        .failure()
        .code(2);

    let mut selectorless = reposcout_command();
    selectorless
        .arg("read")
        .arg(directory.path())
        .assert()
        .failure()
        .code(2);
}

#[test]
fn selector_and_budget_bounds_are_usage_errors() {
    let directory = fixture();

    for symbol in [String::new(), "x".repeat(1_025)] {
        let mut command = reposcout_command();
        command
            .arg("read")
            .arg(directory.path())
            .arg("--symbol")
            .arg("lib.rs")
            .arg(symbol)
            .assert()
            .failure()
            .code(2);
    }

    let mut invalid_hash = reposcout_command();
    let stderr = invalid_hash
        .arg("read")
        .arg(directory.path())
        .args([
            "--symbol",
            "lib.rs",
            "helper",
            "--expect-hash",
            "lib.rs",
            "not-a-sha256",
            "--error-format",
            "json",
        ])
        .assert()
        .failure()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let error: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(error["category"], "usage");

    let mut too_many = reposcout_command();
    too_many.arg("read").arg(directory.path());
    for index in 0..33 {
        too_many
            .arg("--symbol")
            .arg("lib.rs")
            .arg(format!("symbol_{index}"));
    }
    too_many.assert().failure().code(2);

    for (flag, value) in [("--budget", "255"), ("--max-output-bytes", "1023")] {
        let mut command = reposcout_command();
        command
            .arg("read")
            .arg(directory.path())
            .args(["--symbol", "lib.rs", "helper", flag, value])
            .assert()
            .failure()
            .code(2);
    }
}
