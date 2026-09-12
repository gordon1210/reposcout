#[path = "support/command.rs"]
#[allow(
    clippy::expect_used,
    reason = "integration tests intentionally fail immediately when fixtures or assertions are invalid"
)]
mod test_command;

use reposcout::{
    model::task_diagnostics::{
        TaskDiagnosticFormat as Format, TaskDiagnosticSeverity as Severity,
        TaskDiagnosticStatus as Status,
    },
    task_diagnostics::{self, TaskDiagnosticError, TaskDiagnosticLimits as Limits},
};
use serde_json::json;
use std::{
    io::{Cursor, Read},
    path::Path,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn rustc(path: &str, message: &str) -> String {
    json!({"reason":"compiler-message","message":{"message":message,"level":"error","code":{"code":"E0308"},"rendered":"ignored giant rendered body","spans":[{"file_name":path,"is_primary":true,"line_start":2,"column_start":3,"line_end":2,"column_end":8},{"file_name":"ignored.rs","is_primary":false,"line_start":1}]}}).to_string()
}
fn parse(
    input: &str,
    format: Format,
) -> Result<task_diagnostics::ParsedTaskDiagnostics, TaskDiagnosticError> {
    task_diagnostics::read_input(Cursor::new(input), format, Limits::for_safe(false))
}

#[test]
fn cargo_primary_spans_drop_rendered_and_deduplicate_stably() -> TestResult {
    let input = format!(
        "{}\n{}\n{{\"reason\":\"compiler-artifact\"}}",
        rustc("b.rs", "broken"),
        rustc("b.rs", "broken")
    );
    let parsed = parse(&input, Format::Auto)?;
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.evidence.parsed_records, 2);
    assert_eq!(parsed.evidence.deduplicated_records, 1);
    assert_eq!(parsed.evidence.ignored_records, 1);
    assert_eq!(parsed.records[0].message, "broken");
    assert_eq!(parsed.records[0].line, Some(2));
    assert_eq!(parsed.records[0].severity, Severity::Error);
    assert!(!serde_json::to_string(&parsed.records)?.contains("rendered"));
    Ok(())
}

#[test]
fn sarif_multiple_physical_locations_and_producer() -> TestResult {
    let input = json!({"version":"2.1.0","runs":[{"tool":{"driver":{"name":"lint"}},"results":[{"message":{"text":"broken"},"ruleId":"R1","level":"warning","locations":[{"physicalLocation":{"artifactLocation":{"uri":"a.rs"},"region":{"startLine":3}}},{"physicalLocation":{"artifactLocation":{"uri":"b.rs"},"region":{"startLine":4}}}]}]}]}).to_string();
    let parsed = parse(&input, Format::Auto)?;
    assert_eq!(parsed.records.len(), 2);
    assert_eq!(parsed.records[0].tool.as_deref(), Some("lint"));
    assert_eq!(parsed.records[0].code.as_deref(), Some("R1"));
    assert_eq!(parsed.records[0].severity, Severity::Warning);
    Ok(())
}

#[test]
fn malformed_prefix_keeps_usable_rustc_and_unknown_json_does_not_fallback() -> TestResult {
    let parsed = parse(
        &format!("{{broken\n{}", rustc("a.rs", "broken")),
        Format::Auto,
    )?;
    assert_eq!(parsed.evidence.parse_errors, 1);
    assert_eq!(parsed.evidence.status, "partial");
    assert!(matches!(
        parse("{broken", Format::RustcJson),
        Err(TaskDiagnosticError::MalformedInput)
    ));
    assert!(matches!(
        parse("{\"text\":\"a.rs:4 error\"}", Format::Auto),
        Err(TaskDiagnosticError::UnsupportedFormat)
    ));
    Ok(())
}

#[test]
fn sarif_truncation_retains_only_complete_results() -> TestResult {
    let input = r#"{"version":"2.1.0","runs":[{"tool":{"driver":{"name":"lint"}},"results":[{"message":{"text":"good"},"locations":[{"physicalLocation":{"artifactLocation":{"uri":"a.rs"}}}]},{"message":{"text":"cut"#;
    let parsed = parse(input, Format::Sarif)?;
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.evidence.parse_errors, 1);
    assert_eq!(parsed.records[0].tool.as_deref(), Some("lint"));
    Ok(())
}

#[test]
fn text_position_forms_have_partial_confidence_and_unknown_severity() -> TestResult {
    let parsed = parse(
        "a.rs:2:3 unknown\n--> b.rs:4:5\nc.rs(6,7): warning: bad\nFile \"d.py\", line 8\nC:\\build\\a.rs:9:10 error",
        Format::Text,
    )?;
    assert_eq!(parsed.records.len(), 5);
    assert!(parsed.records.iter().all(|r| r.confidence == "partial"));
    assert!(
        parsed
            .records
            .iter()
            .any(|r| r.severity == Severity::Unknown)
    );
    assert!(
        parsed
            .records
            .iter()
            .any(|r| r.line == Some(9) && r.column == Some(10))
    );
    Ok(())
}

#[test]
fn ansi_unicode_and_bom_are_bounded() -> TestResult {
    let message = format!("\u{1b}[31m{}\u{1b}[0m\nsecret\u{7}", "🦀".repeat(600));
    let parsed = parse(
        &format!("\u{feff}{}", rustc("a.rs", &message)),
        Format::Auto,
    )?;
    assert_eq!(parsed.records[0].message.chars().count(), 512);
    assert!(!parsed.records[0].message.chars().any(char::is_control));
    Ok(())
}

#[test]
fn exact_byte_limit_reads_at_most_one_extra_byte() -> TestResult {
    struct Counting {
        count: usize,
    }
    impl Read for Counting {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            output.fill(b' ');
            self.count += output.len();
            Ok(output.len())
        }
    }
    let mut reader = Counting { count: 0 };
    let parsed = task_diagnostics::read_input(
        &mut reader,
        Format::Text,
        Limits {
            input_bytes: 8,
            records: 2,
            details: 1,
        },
    )?;
    assert_eq!(reader.count, 9);
    assert_eq!(parsed.evidence.bytes_read, 9);
    assert!(parsed.evidence.input_truncated);
    Ok(())
}

#[test]
fn record_and_detail_omissions_are_independent() -> TestResult {
    let root = tempfile::tempdir()?;
    let parsed = task_diagnostics::read_input(
        Cursor::new("a.rs:1\nb.rs:2\nc.rs:3"),
        Format::Text,
        Limits {
            input_bytes: 100,
            records: 2,
            details: 1,
        },
    )?;
    let result = task_diagnostics::resolve(parsed, root.path(), root.path(), &[]);
    assert_eq!(result.records.len(), 2);
    assert_eq!(result.evidence.diagnostics.len(), 1);
    assert_eq!(result.evidence.omitted_details, 1);
    assert!(result.evidence.records_truncated);
    assert!(!result.evidence.omitted_records_exact);
    Ok(())
}

#[test]
fn exact_paths_scope_uris_and_untrusted_paths() -> TestResult {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path().canonicalize()?;
    std::fs::create_dir(root.join("src"))?;
    std::fs::write(root.join("src/a b.rs"), "fn a() {}")?;
    std::fs::write(root.join("other.rs"), "fn other() {}")?;
    let input = [
        "src/a b.rs".to_owned(),
        "a b.rs".to_owned(),
        "other.rs".to_owned(),
        "../other.rs".to_owned(),
        "https://bad/a.rs".to_owned(),
        format!("file://{}/src/a%20b.rs", root.display()),
        "a%2Fb.rs".to_owned(),
    ];
    let lines = input
        .iter()
        .map(|path| rustc(path, path))
        .collect::<Vec<_>>()
        .join("\n");
    let result = task_diagnostics::resolve(
        parse(&lines, Format::RustcJson)?,
        &root,
        &root.join("src"),
        &["src/a b.rs".into(), "other.rs".into()],
    );
    assert_eq!(result.evidence.resolved_records, 3);
    assert_eq!(result.evidence.out_of_scope_records, 1);
    assert_eq!(result.evidence.unresolved_records, 3);
    assert!(
        result
            .records
            .iter()
            .filter(|r| r.status == Status::Resolved)
            .all(|r| r.path.as_deref() == Some("src/a b.rs"))
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_never_become_seeds() -> TestResult {
    let root = tempfile::tempdir()?;
    std::fs::write(root.path().join("real.rs"), "fn real() {}")?;
    std::os::unix::fs::symlink(root.path().join("real.rs"), root.path().join("alias.rs"))?;
    let result = task_diagnostics::resolve(
        parse(&rustc("alias.rs", "bad"), Format::Auto)?,
        root.path(),
        root.path(),
        &["alias.rs".into()],
    );
    assert_eq!(result.evidence.unresolved_records, 1);
    assert_eq!(
        result.records[0].reason.as_deref(),
        Some("symlink-or-unavailable-path")
    );
    Ok(())
}

#[test]
fn explicit_file_input_and_nonregular_rejection() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("input.log");
    std::fs::write(&path, "a.rs:5")?;
    assert_eq!(
        task_diagnostics::load_file(&path, Format::Auto, Limits::for_safe(false))?
            .records
            .len(),
        1
    );
    assert!(matches!(
        task_diagnostics::load_file(root.path(), Format::Text, Limits::for_safe(false)),
        Err(TaskDiagnosticError::NotRegularFile)
    ));
    assert!(matches!(
        task_diagnostics::load_file(
            Path::new("/definitely-missing-reposcout-test.log"),
            Format::Text,
            Limits::for_safe(false)
        ),
        Err(TaskDiagnosticError::Io(_))
    ));
    Ok(())
}

#[test]
fn safe_limits_and_supported_empty_runs_are_explicit() -> TestResult {
    let safe = Limits::for_safe(true);
    assert_eq!(
        (safe.input_bytes, safe.records, safe.details),
        (1024 * 1024, 250, 50)
    );
    let normal = Limits::for_safe(false);
    assert_eq!(
        (normal.input_bytes, normal.records, normal.details),
        (8 * 1024 * 1024, 1000, 100)
    );
    for (input, format) in [
        ("", Format::RustcJson),
        ("{\"version\":\"2.1.0\",\"runs\":[]}", Format::Sarif),
        (
            "{\"reason\":\"build-finished\",\"success\":true}",
            Format::RustcJson,
        ),
    ] {
        let parsed = parse(input, format)?;
        assert!(parsed.records.is_empty());
        assert_eq!(parsed.evidence.parse_errors, 0);
    }
    Ok(())
}

#[test]
fn sarif_record_cap_preserves_late_version_and_tool() -> TestResult {
    let item = json!({"message":{"text":"bad"},"locations":[{"physicalLocation":{"artifactLocation":{"uri":"a.rs"}}},{"physicalLocation":{"artifactLocation":{"uri":"b.rs"}}}]});
    let input = json!({"version":"2.1.0","runs":[{"results":[item],"tool":{"driver":{"name":"late tool"}}}]}).to_string();
    let parsed = task_diagnostics::read_input(
        Cursor::new(input),
        Format::Sarif,
        Limits {
            input_bytes: 4096,
            records: 1,
            details: 1,
        },
    )?;
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.records[0].tool.as_deref(), Some("late tool"));
    assert!(parsed.evidence.records_truncated);
    Ok(())
}

#[test]
fn file_target_and_normalized_aliases_preserve_stable_ids() -> TestResult {
    let fixture = tempfile::tempdir()?;
    std::fs::write(fixture.path().join("a.rs"), "fn a() {}")?;
    std::fs::write(fixture.path().join("b.rs"), "fn b() {}")?;
    let first = rustc("a.rs", "a");
    let second = rustc("b.rs", "b");
    let inventory = vec!["a.rs".into(), "b.rs".into()];
    let a = task_diagnostics::resolve(
        parse(&format!("{first}\n{second}"), Format::Auto)?,
        fixture.path(),
        &fixture.path().join("a.rs"),
        &inventory,
    );
    let b = task_diagnostics::resolve(
        parse(&format!("{second}\n{first}"), Format::Auto)?,
        fixture.path(),
        &fixture.path().join("a.rs"),
        &inventory,
    );
    assert_eq!(a.records, b.records);
    assert_eq!(a.evidence.resolved_records, 1);
    assert_eq!(a.evidence.out_of_scope_records, 1);
    Ok(())
}

#[test]
fn controls_and_encoded_escapes_cannot_resolve_to_clean_paths() -> TestResult {
    let fixture = tempfile::tempdir()?;
    std::fs::write(fixture.path().join("a.rs"), "fn a() {}")?;
    let input = [
        "a\0.rs",
        "file:a%00.rs",
        "file:%2e%2e/a.rs",
        "file://evil/a.rs",
        "a.rs:42",
    ]
    .iter()
    .map(|path| rustc(path, "bad"))
    .collect::<Vec<_>>()
    .join("\n");
    let result = task_diagnostics::resolve(
        parse(&input, Format::Auto)?,
        fixture.path(),
        fixture.path(),
        &["a.rs".into()],
    );
    assert_eq!(result.evidence.resolved_records, 0);
    assert_eq!(result.evidence.unresolved_records, 5);
    assert!(!serde_json::to_string(&result.evidence)?.contains("\\u0000"));
    Ok(())
}

#[test]
fn debug_configuration_never_exposes_diagnostic_payload() -> TestResult {
    let parsed = parse(&rustc("SECRET_PATH.rs", "SECRET_MESSAGE"), Format::Auto)?;
    let debug = format!("{parsed:?}");
    assert!(debug.contains("parsed_records"));
    assert!(!debug.contains("SECRET"));
    Ok(())
}

#[test]
fn relative_sarif_uris_decode_once_and_preserve_literal_rustc_percent_paths() -> TestResult {
    let fixture = tempfile::tempdir()?;
    std::fs::write(fixture.path().join("a b.rs"), "fn a() {}")?;
    std::fs::write(fixture.path().join("a%20b.rs"), "fn b() {}")?;
    let inventory = vec!["a b.rs".into(), "a%20b.rs".into()];
    let input = json!({"version":"2.1.0","runs":[{"results":[{"message":{"text":"bad"},"locations":[{"physicalLocation":{"artifactLocation":{"uri":"a%20b.rs"}}}]}]}]}).to_string();
    let sarif = task_diagnostics::resolve(
        parse(&input, Format::Auto)?,
        fixture.path(),
        fixture.path(),
        &inventory,
    );
    let rustc = task_diagnostics::resolve(
        parse(&rustc("a%20b.rs", "bad"), Format::Auto)?,
        fixture.path(),
        fixture.path(),
        &inventory,
    );
    assert_eq!(sarif.records[0].path.as_deref(), Some("a b.rs"));
    assert_eq!(rustc.records[0].path.as_deref(), Some("a%20b.rs"));
    Ok(())
}

#[test]
fn invalid_utf8_cannot_fabricate_a_replacement_character_path() -> TestResult {
    let fixture = tempfile::tempdir()?;
    std::fs::write(fixture.path().join("a�.rs"), "fn replacement() {}")?;
    std::fs::write(fixture.path().join("good.rs"), "fn good() {}")?;
    let bytes = b"a\xff.rs:1\ngood.rs:2";
    let parsed =
        task_diagnostics::read_input(Cursor::new(bytes), Format::Text, Limits::for_safe(false))?;
    let resolved = task_diagnostics::resolve(
        parsed,
        fixture.path(),
        fixture.path(),
        &["a�.rs".into(), "good.rs".into()],
    );
    assert_eq!(resolved.evidence.parse_errors, 1);
    assert_eq!(resolved.evidence.resolved_records, 1);
    assert_eq!(resolved.records[0].path.as_deref(), Some("good.rs"));
    Ok(())
}

#[test]
fn cli_file_input_implies_context_excludes_input_and_keeps_source_bodyfree() -> TestResult {
    let fixture = tempfile::tempdir()?;
    std::fs::write(
        fixture.path().join("lib.rs"),
        "fn broken() { let private_body = 1; }\n",
    )?;
    let input = fixture.path().join("diagnostics.json");
    std::fs::write(&input, rustc("lib.rs", "failure"))?;
    let output = test_command::reposcout_command()
        .args([
            "--profile",
            "agent",
            "--no-cache",
            "--quiet",
            "-f",
            "json",
            "--task-diagnostics",
        ])
        .arg(&input)
        .arg(fixture.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output)?;
    assert_eq!(value["context"]["task_evidence"]["resolved_records"], 1);
    assert_eq!(value["context"]["task_evidence"]["format"], "rustc-json");
    let files = value["files"].as_array().ok_or("missing inventory files")?;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "lib.rs");
    assert!(!String::from_utf8(output)?.contains("private_body"));
    Ok(())
}

#[test]
fn cli_stdin_safe_limits_and_agent_summary_keep_coverage() -> TestResult {
    let fixture = tempfile::tempdir()?;
    std::fs::write(fixture.path().join("lib.rs"), "fn broken() {}\n")?;
    let input = "lib.rs:1:1 error: repeated\n".repeat(251);
    let output = test_command::reposcout_command()
        .args([
            "--profile",
            "safe",
            "--no-cache",
            "--quiet",
            "--agent-summary",
            "--task-diagnostics",
            "-",
            "--task-diagnostics-format",
            "text",
        ])
        .arg(fixture.path())
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(output.len() <= 16 * 1024);
    let value: serde_json::Value = serde_json::from_slice(&output)?;
    let evidence = &value["context"]["task_evidence"];
    assert_eq!(evidence["parsed_records"], 250);
    assert_eq!(evidence["deduplicated_records"], 249);
    assert_eq!(evidence["resolved_records"], 1);
    assert_eq!(evidence["records_truncated"], true);
    assert_eq!(value["context"]["direct_evidence"]["shown"], 1);
    Ok(())
}

#[test]
fn cli_malformed_structured_stdin_and_incompatible_flags_fail() -> TestResult {
    let fixture = tempfile::tempdir()?;
    std::fs::write(fixture.path().join("lib.rs"), "fn sample() {}")?;
    test_command::reposcout_command()
        .args([
            "--task-diagnostics",
            "-",
            "--task-diagnostics-format",
            "rustc-json",
            "-f",
            "json",
        ])
        .arg(fixture.path())
        .write_stdin("{broken")
        .assert()
        .failure();
    test_command::reposcout_command()
        .args(["--task-diagnostics-format", "text"])
        .arg(fixture.path())
        .assert()
        .failure();
    test_command::reposcout_command()
        .args(["--task-diagnostics", "-", "--no-context"])
        .arg(fixture.path())
        .write_stdin("lib.rs:1")
        .assert()
        .failure();
    Ok(())
}

#[test]
fn cli_input_output_and_debug_collisions_preserve_input_and_logs_redact_payload() -> TestResult {
    let fixture = tempfile::tempdir()?;
    std::fs::write(fixture.path().join("lib.rs"), "fn sample() {}")?;
    let input = fixture.path().join("diagnostics.json");
    let payload = rustc("lib.rs", "SECRET_DIAGNOSTIC_MESSAGE");
    std::fs::write(&input, &payload)?;
    for flag in ["--output", "--debug-log"] {
        test_command::reposcout_command()
            .args(["--task-diagnostics"])
            .arg(&input)
            .arg(flag)
            .arg(&input)
            .arg(fixture.path())
            .assert()
            .failure();
        assert_eq!(std::fs::read_to_string(&input)?, payload);
    }
    let debug = fixture.path().join("debug.ndjson");
    test_command::reposcout_command()
        .args([
            "--profile",
            "agent",
            "--no-cache",
            "--quiet",
            "-f",
            "json",
            "--task-diagnostics",
        ])
        .arg(&input)
        .arg("--debug-log")
        .arg(&debug)
        .arg(fixture.path())
        .assert()
        .success();
    assert!(!std::fs::read_to_string(debug)?.contains("SECRET_DIAGNOSTIC_MESSAGE"));
    Ok(())
}
