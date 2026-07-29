//! End-to-end integration tests that exercise the compiled `reposcout` binary
//! against a small multi-language fixture tree. Assertions target stable
//! behaviour (tokens, line metrics, language breakdown, markers, output
//! formats, analyzer selection and the `--fail-on` gate) so they remain valid
//! as the individual analyzers evolve.

use serde_json::Value;
#[path = "support/command.rs"]
mod test_command;
use test_command::{reposcout_command, test_global_config};

/// Absolute path to the bundled fixture tree.
fn fixture() -> String {
    format!("{}/tests/fixtures/sample", env!("CARGO_MANIFEST_DIR"))
}

/// Run reposcout with the given args plus `--no-cache --quiet`, returning parsed
/// JSON. Caching is disabled so tests never share state via `.reposcout/`.
fn run_json(args: &[&str]) -> Value {
    let mut cmd = reposcout_command();
    cmd.args(args);
    // Global flags must follow the (optional) subcommand, so append them last.
    cmd.arg("--no-cache").arg("--quiet");
    let output = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
}

fn language_names(v: &Value) -> Vec<String> {
    v["summary"]["languages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap().to_string())
        .collect()
}

fn write_health_scope_fixture(root: &std::path::Path) {
    let source_body = (0..24)
        .map(|value| format!("    total += {value};\n"))
        .collect::<String>();
    for (name, function) in [("first.rs", "first"), ("second.rs", "second")] {
        std::fs::write(
            root.join(name),
            format!(
                "pub fn {function}(input: i32) -> i32 {{\n    // TODO source marker\n    let mut total = input;\n{source_body}    total\n}}\n"
            ),
        )
        .unwrap();
    }

    let entries = (0..24)
        .map(|value| format!("  \"field_{value}\": {value},\n"))
        .collect::<String>();
    let json = format!("{{\n  \"note\": \"TODO content marker\",\n{entries}  \"end\": true\n}}\n");
    std::fs::write(root.join("first.json"), &json).unwrap();
    std::fs::write(root.join("second.json"), json).unwrap();
}

#[test]
fn full_scan_reports_core_metrics() {
    let v = run_json(&["-f", "json", &fixture()]);

    assert_eq!(v["schema_version"], "1.0");
    assert_eq!(v["encoding"], "o200k_base");
    assert!(v.get("report_kind").is_none());
    assert!(v.get("change_summary").is_none());

    let files = v["summary"]["files"].as_u64().unwrap();
    assert!(files >= 5, "expected >=5 fixture files, got {files}");
    assert!(v["summary"]["tokens"].as_u64().unwrap() > 0);
    assert!(v["summary"]["sloc"].as_u64().unwrap() > 0);

    // The per-file array length matches the summary count.
    assert_eq!(v["files"].as_array().unwrap().len() as u64, files);

    let langs = language_names(&v);
    for expected in ["Rust", "Python", "JavaScript", "Go"] {
        assert!(
            langs.contains(&expected.to_string()),
            "missing language {expected} in {langs:?}"
        );
    }
}

#[test]
fn default_health_metrics_are_source_first_but_inventory_remains_complete() {
    let dir = tempfile::tempdir().unwrap();
    write_health_scope_fixture(dir.path());

    let report = run_json(&["-f", "json", dir.path().to_str().unwrap()]);

    assert_eq!(report["summary"]["files"], 4);
    assert_eq!(report["summary"]["source"]["files"], 2);
    assert!(
        language_names(&report).contains(&"JSON".to_string()),
        "complete inventory must retain JSON"
    );
    assert_eq!(report["analysis_profile"]["health"]["scope"], "source");
    assert!(
        report["analysis_profile"]["health"]
            .get("includes")
            .is_none()
    );

    let json_file = report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "first.json")
        .unwrap();
    assert!(
        json_file.get("markers").is_none() || json_file["markers"].is_null(),
        "default JSON file must not carry marker results: {json_file}"
    );
    assert_eq!(report["summary"]["markers"]["TODO"], 2);

    let duplication = &report["summary"]["duplication"];
    assert!(duplication["analyzed_lines"].as_u64().unwrap() > 0);
    assert!(
        duplication["analyzed_lines"].as_u64().unwrap()
            < report["summary"]["loc"].as_u64().unwrap()
    );
    assert!(
        duplication["by_language"]
            .as_array()
            .unwrap()
            .iter()
            .all(|language| language["name"] != "JSON")
    );
    assert!(
        report["duplicates"]["file_coverage"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["format"] != "JSON")
    );
    assert!(
        report["summary"]["top_source_token_files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["path"].as_str().unwrap().ends_with(".rs"))
    );
}

#[test]
fn health_include_adds_one_content_format_to_markers_and_duplication() {
    let dir = tempfile::tempdir().unwrap();
    write_health_scope_fixture(dir.path());

    let report = run_json(&[
        "-f",
        "json",
        "--health-include",
        "json",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(
        report["analysis_profile"]["health"]["includes"],
        serde_json::json!(["JSON"])
    );
    assert_eq!(report["summary"]["markers"]["TODO"], 4);
    assert!(
        report["summary"]["duplication"]["by_language"]
            .as_array()
            .unwrap()
            .iter()
            .any(|language| {
                language["name"] == "JSON" && language["duplicated_lines"].as_u64().unwrap() > 0
            })
    );
    assert_eq!(
        report["summary"]["duplication"]["analyzed_lines"],
        report["summary"]["loc"]
    );
}

#[test]
fn project_config_can_opt_content_into_the_health_corpus() {
    let dir = tempfile::tempdir().unwrap();
    write_health_scope_fixture(dir.path());
    std::fs::write(
        dir.path().join("reposcout.toml"),
        "health_includes = [\"json\"]\n",
    )
    .unwrap();

    let report = run_json(&["-f", "json", dir.path().to_str().unwrap()]);

    assert_eq!(report["execution"]["config_mode"], "project");
    assert_eq!(
        report["analysis_profile"]["health"]["includes"],
        serde_json::json!(["JSON"])
    );
    assert!(
        report["summary"]["duplication"]["by_language"]
            .as_array()
            .unwrap()
            .iter()
            .any(|language| language["name"] == "JSON")
    );
}

#[test]
fn capabilities_are_machine_discoverable_without_scanning() {
    let mut cmd = reposcout_command();
    cmd.args(["capabilities", "-f", "json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(report["schema_version"], "1.0");
    assert_eq!(report["default_operation"], "scan");
    assert_eq!(report["default_invocation"], "reposcout [PATH]");
    assert_eq!(
        report["symbol_query_formats"],
        serde_json::json!(["table", "json", "markdown", "ndjson"])
    );
    assert!(
        report["symbol_kinds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kind| kind == "interface")
    );
    assert!(
        !report["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command == "scan"),
        "default scan must not be advertised as a literal subcommand"
    );
    assert!(
        report["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command == "locate")
    );
    assert!(
        report["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command == "cache")
    );
    assert!(
        report["first_class_languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|language| language == "PHP")
    );
    assert_eq!(
        report["health_scopes"],
        serde_json::json!(["source", "all"])
    );
    assert!(
        report["default_health_languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|language| language == "Dockerfile")
    );
    assert!(
        report["optional_health_formats"]
            .as_array()
            .unwrap()
            .iter()
            .any(|format| format == "JSON")
    );
    assert_eq!(
        report["machine_interfaces"],
        serde_json::json!(["cli-json", "cli-ndjson", "debug-log-ndjson"])
    );
    assert_eq!(
        report["daemon_profiles"],
        serde_json::json!(["lite", "full", "safe"])
    );
    assert_eq!(report["error_formats"], serde_json::json!(["text", "json"]));
    assert_eq!(report["type2_max_seed_pairs_per_pool"], 10_000_000);
    assert_eq!(report["type2_max_matches_per_pool"], 250_000);
    assert_eq!(report["type2_max_overlap_checks_per_pool"], 10_000_000);
    assert_eq!(report["change_summary"]["flag"], "--change-summary");
    assert_eq!(
        report["change_summary"]["requires_one_of"],
        serde_json::json!(["--since", "--staged", "--working"])
    );
    assert_eq!(
        report["change_summary"]["implies"],
        serde_json::json!(["summary", "context", "impact"])
    );
    assert_eq!(
        report["change_summary"]["formats"],
        serde_json::json!(["table", "json", "markdown", "ndjson"])
    );
    assert_eq!(report["change_summary"]["max_path_entries"], 100);
    assert_eq!(report["change_summary"]["max_gap_entries"], 25);
    assert_eq!(report["change_summary"]["max_validations"], 10);
}

#[test]
fn update_is_exposed_as_a_builtin_command() {
    let mut cmd = reposcout_command();
    cmd.args(["update", "--help"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let help = String::from_utf8(output).unwrap();

    assert!(help.contains("Usage: reposcout update"));
    assert!(help.contains("latest stable GitHub release"));
}

#[test]
fn update_refuses_an_executable_without_an_installer_receipt() {
    let config_home = tempfile::tempdir().unwrap();
    let mut cmd = reposcout_command();
    cmd.env("XDG_CONFIG_HOME", config_home.path()).arg("update");
    let stderr = cmd.assert().failure().get_output().stderr.clone();
    let error = String::from_utf8(stderr).unwrap();

    assert!(error.contains("release installer"));
    assert!(error.contains(
        "https://github.com/gordon1210/reposcout/releases/latest/download/reposcout-installer.sh"
    ));
}

#[test]
fn cache_clear_accepts_repository_subpaths_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let nested = dir.path().join("src/nested");
    std::fs::create_dir_all(&nested).unwrap();

    let mut command = reposcout_command();
    command.args(["cache", "clear", nested.to_str().unwrap()]);
    let stdout = command.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8(stdout).unwrap();

    assert!(stdout.contains("RepoScout cache is already empty for"));
    assert!(stdout.contains(dir.path().canonicalize().unwrap().to_str().unwrap()));
}

#[test]
fn json_error_format_covers_usage_and_runtime_failures() {
    let mut usage = reposcout_command();
    usage.args(["--not-a-real-option", "--error-format", "json"]);
    let stderr = usage.assert().failure().get_output().stderr.clone();
    let error: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(error["kind"], "error");
    assert_eq!(error["category"], "usage");
    assert_eq!(error["exit_code"], 2);

    let mut runtime = reposcout_command();
    runtime.args(["--graph-depth", "65", "--error-format", "json", &fixture()]);
    let stderr = runtime.assert().failure().get_output().stderr.clone();
    let error: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(error["kind"], "error");
    assert_eq!(error["category"], "runtime");
    assert_eq!(error["exit_code"], 1);
    assert!(error["message"].as_str().unwrap().contains("0 and 64"));
}

#[test]
fn debug_log_records_incremental_diagnostics_and_excludes_itself() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lib.rs"),
        "pub fn answer() -> usize { 42 }\n",
    )
    .unwrap();
    let log = dir.path().join("reposcout-debug.json");

    let mut command = reposcout_command();
    command.args([
        "-f",
        "json",
        "--summary",
        dir.path().to_str().unwrap(),
        "--debug-log",
        log.to_str().unwrap(),
        "--no-cache",
        "--quiet",
    ]);
    let stdout = command.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(report["diagnostics"]["discovered_files"], 1);
    assert_eq!(report["diagnostics"]["analyzed_files"], 1);

    let records = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(!records.is_empty());
    assert_eq!(records[0]["event"], "session_start");
    assert!(records.windows(2).all(|pair| {
        pair[0]["sequence"].as_u64().unwrap() < pair[1]["sequence"].as_u64().unwrap()
    }));
    assert!(records.iter().any(|record| {
        record["event"] == "stage_start" && record["data"]["stage"] == "discovery"
    }));
    assert!(records.iter().any(|record| {
        record["event"] == "stage_end" && record["data"]["stage"] == "discovery"
    }));
    assert!(records.iter().any(|record| {
        record["event"] == "discovery_progress" && record["data"]["latest_path"] == "lib.rs"
    }));
    assert!(
        records.iter().any(|record| {
            record["event"] == "file_start" && record["data"]["path"] == "lib.rs"
        })
    );
    assert!(records.iter().any(|record| {
        record["event"] == "file_end"
            && record["data"]["path"] == "lib.rs"
            && record["data"]["status"] == "analyzed"
    }));
    assert!(records.iter().any(|record| {
        record["event"] == "type2_progress" && record["data"]["phase"] == "started"
    }));
    assert!(records.iter().any(|record| {
        record["event"] == "type2_progress" && record["data"]["phase"] == "finished"
    }));
    assert!(records.iter().any(|record| {
        record["event"] == "session_end" && record["data"]["outcome"] == "completed"
    }));
}

#[test]
fn debug_log_records_runtime_errors_without_overwriting_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let log = dir.path().join("runtime-error.jsonl");

    let mut command = reposcout_command();
    command.args([
        missing.to_str().unwrap(),
        "--debug-log",
        log.to_str().unwrap(),
        "--quiet",
    ]);
    command.assert().failure();
    let records = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(records.iter().any(|record| {
        record["event"] == "runtime_error"
            && record["data"]["message"]
                .as_str()
                .unwrap()
                .contains("path not found")
    }));
    assert!(records.iter().any(|record| {
        record["event"] == "session_end" && record["data"]["outcome"] == "error"
    }));

    std::fs::write(&log, "keep me\n").unwrap();
    let mut existing = reposcout_command();
    existing.args([
        fixture().as_str(),
        "--debug-log",
        log.to_str().unwrap(),
        "--quiet",
    ]);
    existing.assert().failure();
    assert_eq!(std::fs::read_to_string(log).unwrap(), "keep me\n");

    let shared_output = dir.path().join("shared-output.json");
    let mut conflicting = reposcout_command();
    conflicting.args([
        "-f",
        "json",
        "-o",
        shared_output.to_str().unwrap(),
        "--debug-log",
        shared_output.to_str().unwrap(),
        fixture().as_str(),
    ]);
    let stderr = conflicting.assert().failure().get_output().stderr.clone();
    assert!(
        String::from_utf8(stderr)
            .unwrap()
            .contains("debug log path cannot also be the output path")
    );
    assert!(!shared_output.exists());
}

#[test]
fn debug_log_records_panics_before_the_normal_hook_runs() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("panic.jsonl");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "debug_log_panic_probe", "--nocapture"])
        .env("REPOSCOUT_DEBUG_LOG_PANIC_PROBE", &log)
        .output()
        .unwrap();
    assert!(!output.status.success());

    let records = std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let panic = records
        .iter()
        .find(|record| record["event"] == "panic")
        .expect("panic record");
    assert_eq!(panic["data"]["message"], "deliberate debug-log panic probe");
    assert!(panic["data"]["location"]["file"].is_string());
    assert!(!panic["data"]["backtrace"].as_str().unwrap().is_empty());
}

#[test]
fn debug_log_panic_probe() {
    let Some(log) = std::env::var_os("REPOSCOUT_DEBUG_LOG_PANIC_PROBE") else {
        return;
    };
    let _session = reposcout::debug_log::Session::start(Some(std::path::Path::new(&log))).unwrap();
    panic!("deliberate debug-log panic probe");
}

#[test]
fn debug_log_heartbeat_records_liveness_during_quiet_work() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("heartbeat.jsonl");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "debug_log_heartbeat_probe", "--nocapture"])
        .env("REPOSCOUT_DEBUG_LOG_HEARTBEAT_PROBE", &log)
        .output()
        .unwrap();
    assert!(output.status.success());

    let records = std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let heartbeat = records
        .iter()
        .find(|record| record["event"] == "heartbeat")
        .expect("heartbeat record");
    assert_eq!(heartbeat["data"]["last_event"], "session_start");
    assert!(heartbeat["data"]["quiet_for_ms"].as_u64().unwrap() >= 1_500);
    #[cfg(target_os = "linux")]
    {
        assert_eq!(heartbeat["data"]["memory"]["available"], true);
        assert!(heartbeat["data"]["memory"]["rss_bytes"].is_u64());
    }
}

#[test]
fn debug_log_heartbeat_probe() {
    let Some(log) = std::env::var_os("REPOSCOUT_DEBUG_LOG_HEARTBEAT_PROBE") else {
        return;
    };
    let _session = reposcout::debug_log::Session::start(Some(std::path::Path::new(&log))).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2_200));
}

#[test]
fn locate_returns_ranked_cross_language_declarations() {
    let report = run_json(&["locate", "load", &fixture(), "-f", "json"]);
    let matches = report["matches"].as_array().unwrap();

    assert_eq!(report["match_mode"], "ranked");
    assert!(
        matches
            .iter()
            .any(|item| { item["name"] == "loadJson" && item["language"] == "JavaScript" })
    );
    assert!(
        matches
            .iter()
            .any(|item| { item["name"] == "load_config" && item["language"] == "Python" })
    );
    assert!(report["first_class_files"].as_u64().unwrap() >= 5);
    assert_eq!(report["execution"]["cache_enabled"], false);

    let exact = run_json(&[
        "locate",
        "load_config",
        &fixture(),
        "--exact",
        "--language",
        "Python",
        "-f",
        "json",
    ]);
    assert_eq!(exact["total_matches"], 1);
    assert!(
        exact["matches"][0]["path"]
            .as_str()
            .unwrap()
            .ends_with("tests/fixtures/sample/util.py")
    );

    let case_mismatch = run_json(&[
        "locate",
        "LOAD_CONFIG",
        &fixture(),
        "--exact",
        "--language",
        "Python",
        "-f",
        "json",
    ]);
    assert_eq!(case_mismatch["total_matches"], 0);

    let mut ndjson = reposcout_command();
    ndjson.args([
        "locate",
        "load_config",
        &fixture(),
        "--exact",
        "--kind",
        "function",
        "--language",
        "Python",
        "-f",
        "ndjson",
        "--no-cache",
        "--quiet",
    ]);
    let output = ndjson.assert().success().get_output().stdout.clone();
    let header: Value = serde_json::from_slice(output.split(|byte| *byte == b'\n').next().unwrap())
        .expect("first NDJSON record should be a query header");
    assert_eq!(header["kind"], "symbol_query");
    assert_eq!(header["match_mode"], "exact");
    assert_eq!(header["filters"]["kind"], "function");
    assert_eq!(header["filters"]["language"], "Python");
    assert_eq!(header["returned_matches"], 1);
}

#[test]
fn full_scan_exposes_a_complete_versioned_finding_catalog() {
    let report = run_json(&["-f", "json", "--max-complexity", "1", &fixture()]);

    let catalog = &report["finding_catalog"];
    assert_eq!(catalog["version"], 1);
    let findings = catalog["findings"].as_array().unwrap();
    for kind in ["complexity", "marker", "duplication"] {
        assert!(
            findings.iter().any(|finding| finding["kind"] == kind),
            "missing {kind} finding: {findings:?}"
        );
    }
    assert!(findings.iter().all(|finding| {
        finding["fingerprint"]
            .as_str()
            .is_some_and(|fingerprint| !fingerprint.is_empty())
            && finding["primary_location"]["path"].is_string()
    }));
}

#[test]
fn output_file_inside_target_is_excluded_from_future_scans() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("source.rs"), "pub fn source() {}\n").unwrap();
    let output = dir.path().join("report.json");
    let target = dir.path().to_str().unwrap();
    let output_path = output.to_str().unwrap();

    for _ in 0..2 {
        let mut cmd = reposcout_command();
        cmd.args([
            "-f",
            "json",
            "--no-cache",
            "--quiet",
            "-o",
            output_path,
            target,
        ]);
        cmd.assert().success();
    }

    let report: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(report["summary"]["files"], 1);
    assert!(
        report["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["path"] != "report.json")
    );
}

#[test]
fn baseline_file_inside_target_is_excluded_from_comparison_scan() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("source.rs"), "pub fn source() {}\n").unwrap();
    let baseline = dir.path().join("baseline.json");

    let mut save = reposcout_command();
    save.args([
        "-f",
        "json",
        "--no-cache",
        "--quiet",
        "-o",
        baseline.to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    save.assert().success();

    let report = run_json(&[
        "-f",
        "json",
        "--baseline",
        baseline.to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(report["summary"]["files"], 1);
    assert_eq!(report["baseline"]["regressed"], false);
}

#[test]
fn in_tree_baseline_is_excluded_from_diff_and_impact_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("source.js"), "export const value = 1;\n").unwrap();
    commit_all(&repo, "initial source");
    let baseline = dir.path().join("baseline.json");

    let mut save = reposcout_command();
    save.args([
        "-f",
        "json",
        "--working",
        "--baseline-ready",
        "--no-cache",
        "--quiet",
        "-o",
        baseline.to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    save.assert().success();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--impact",
        "--baseline",
        baseline.to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    assert!(
        report["impact"]["changed_files"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(report["impact"]["confidence"], "none");
}

#[test]
fn cli_excludes_extend_configuration_excludes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("reposcout.toml"),
        "excludes = [\"config_only.rs\"]\n",
    )
    .unwrap();
    for file in ["config_only.rs", "cli_only.rs", "kept.rs"] {
        std::fs::write(dir.path().join(file), "pub fn value() {}\n").unwrap();
    }

    let report = run_json(&[
        "-f",
        "json",
        "--exclude",
        "cli_only.rs",
        dir.path().to_str().unwrap(),
    ]);
    let paths = report["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(!paths.contains(&"config_only.rs"));
    assert!(!paths.contains(&"cli_only.rs"));
    assert!(paths.contains(&"kept.rs"));
}

#[test]
fn integration_cli_profile_caps_worker_threads() {
    let dir = tempfile::tempdir().unwrap();
    let mut command = reposcout_command();
    command.args(["config", dir.path().to_str().unwrap(), "-f", "json"]);
    let output = command.assert().success().get_output().stdout.clone();
    let inspection: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(inspection["sources"]["global"]["loaded"], true);
    assert_eq!(inspection["effective"]["jobs"], 2);
}

#[test]
fn config_command_reports_layered_sources_and_effective_values() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global.toml");
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(&global, "jobs = 2\ntop = 20\n\n[context]\nbudget = 8000\n").unwrap();
    std::fs::write(
        project.join("reposcout.toml"),
        "top = 4\n\n[context]\nmax_files = 6\n",
    )
    .unwrap();

    let mut command = reposcout_command();
    command.env("REPOSCOUT_GLOBAL_CONFIG", &global).args([
        "config",
        project.to_str().unwrap(),
        "-f",
        "json",
    ]);
    let output = command.assert().success().get_output().stdout.clone();
    let inspection: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(
        inspection["precedence"],
        serde_json::json!(["cli", "project", "global", "defaults"])
    );
    assert_eq!(
        inspection["sources"]["global"]["path"],
        global.to_str().unwrap()
    );
    assert_eq!(inspection["sources"]["global"]["loaded"], true);
    assert_eq!(
        inspection["sources"]["project"]["path"],
        project
            .join("reposcout.toml")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(inspection["effective"]["jobs"], 2);
    assert_eq!(inspection["effective"]["top"], 4);
    assert_eq!(inspection["effective"]["context_budget"], 8000);
    assert_eq!(inspection["effective"]["context_max_files"], 6);
}

#[test]
fn cli_overrides_project_and_global_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global.toml");
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        &global,
        "encoding = \"cl100k_base\"\n\n[context]\nenabled = true\nbudget = 100\n",
    )
    .unwrap();
    std::fs::write(
        project.join("reposcout.toml"),
        "encoding = \"o200k_base\"\n",
    )
    .unwrap();
    std::fs::write(project.join("sample.rs"), "pub fn sample() {}\n").unwrap();

    let mut command = reposcout_command();
    command.env("REPOSCOUT_GLOBAL_CONFIG", &global).args([
        "-f",
        "json",
        "--summary",
        "--encoding",
        "cl100k_base",
        "--no-context",
        "--no-cache",
        "--quiet",
        project.to_str().unwrap(),
    ]);
    let output = command.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(report["encoding"], "cl100k_base");
    assert!(report.get("context").is_none());
}

#[test]
fn context_plan_is_focus_aware_bounded_and_kept_in_summary_json() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("focus.js"),
        "import { dep } from './dependency';\nexport const focus = dep;\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("dependency.js"), "export const dep = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("dependent.js"),
        "import { focus } from './focus';\nexport const result = focus;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("focus.test.js"),
        "import { focus } from './focus';\nvoid focus;\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("unrelated.js"), "export const other = 1;\n").unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--summary",
        "--context",
        "--context-budget",
        "1000",
        "--context-max-files",
        "4",
        "--focus",
        "focus.js",
        dir.path().to_str().unwrap(),
    ]);
    let context = &report["context"];
    let selected = context["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert!(report.get("files").is_none());
    assert!(context["selected_tokens"].as_u64().unwrap() <= 1000);
    assert_eq!(selected.len(), 4);
    for expected in ["focus.js", "dependency.js", "dependent.js", "focus.test.js"] {
        assert!(
            selected.contains(&expected),
            "missing {expected}: {selected:?}"
        );
    }
    assert!(!selected.contains(&"unrelated.js"));
    assert!(
        context["graph_languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|language| language == "JavaScript")
    );
    assert!(context["files"].as_array().unwrap().iter().any(|file| {
        file["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "direct dependency of focus")
    }));
}

#[test]
fn context_plan_is_rendered_for_humans() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

    for (format, heading) in [
        ("table", "Agent context plan"),
        ("markdown", "## Agent context plan"),
    ] {
        let mut command = reposcout_command();
        command.args([
            "-f",
            format,
            "--context",
            "--no-cache",
            "--quiet",
            dir.path().to_str().unwrap(),
        ]);
        let output = command.assert().success().get_output().stdout.clone();
        let rendered = String::from_utf8(output).unwrap();
        assert!(
            rendered.contains(heading),
            "{format} output was: {rendered}"
        );
        assert!(
            rendered.contains("entrypoint"),
            "{format} omitted selection reasons"
        );
        assert!(
            rendered.contains("Selected symbol outlines"),
            "{format} omitted structural context"
        );
    }
}

#[test]
fn context_plan_projects_bounded_body_free_symbol_outlines() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lib.rs"),
        concat!(
            "pub struct Request { pub value: String }\n",
            "pub fn execute(request: Request) -> usize {\n",
            "    let secret_body = request.value.len();\n",
            "    secret_body\n",
            "}\n",
        ),
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--summary",
        "--context",
        "--only",
        "tokens",
        dir.path().to_str().unwrap(),
    ]);
    let context = &report["context"];
    let symbols = context["files"][0]["symbols"].as_array().unwrap();

    assert_eq!(context["strategy_version"], 2);
    assert_eq!(report["summary"]["symbols"]["functions"], 0);
    assert!(context["planning_ms"].is_number());
    assert_eq!(context["outline_symbols"], symbols.len());
    assert!(context["outline_bytes"].as_u64().unwrap() > 0);
    assert!(symbols.iter().any(|symbol| {
        symbol["name"] == "execute"
            && symbol["signature"]
                .as_str()
                .is_some_and(|signature| signature.contains("pub fn execute"))
    }));
    assert!(symbols.iter().all(|symbol| {
        !symbol["signature"]
            .as_str()
            .unwrap()
            .contains("secret_body")
            && !symbol["reasons"].as_array().unwrap().is_empty()
    }));
}

#[test]
fn working_context_uses_full_tree_without_widening_scoped_report_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::write(
        dir.path().join("src/dependency.ts"),
        "export const dependency = 1;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/changed.ts"),
        "import { dependency } from './dependency.js';\nexport const changed = dependency;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/direct.ts"),
        "import { changed } from './changed.js';\nexport const direct = changed;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/transitive.ts"),
        "import { direct } from './direct.js';\nexport const transitive = direct;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests/changed.test.ts"),
        "import { changed } from '../src/changed.js';\nvoid changed;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/unrelated.ts"),
        "export const unrelated = 1;\n",
    )
    .unwrap();
    commit_all(&repo, "initial context graph");
    std::fs::write(
        dir.path().join("src/changed.ts"),
        "import { dependency } from './dependency.js';\nexport const changed = dependency + 1;\n",
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--context",
        "--context-budget",
        "10000",
        "--context-max-files",
        "5",
        "--impact",
        "--review",
        dir.path().to_str().unwrap(),
    ]);
    let context = &report["context"];
    let selected = context["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(report["summary"]["files"], 1);
    assert_eq!(report["files"].as_array().unwrap().len(), 1);
    assert_eq!(report["files"][0]["path"], "src/changed.ts");
    assert_eq!(context["change_scope"], "working");
    assert_eq!(
        context["changed_files"],
        serde_json::json!(["src/changed.ts"])
    );
    assert_eq!(context["planning_diagnostics"]["analyzed_files"], 6);
    assert_eq!(context["planning_diagnostics"]["unreadable_files"], 0);
    for expected in [
        "src/changed.ts",
        "src/dependency.ts",
        "src/direct.ts",
        "src/transitive.ts",
        "tests/changed.test.ts",
    ] {
        assert!(
            selected.contains(&expected),
            "missing {expected}: {selected:?}"
        );
    }
    assert!(!selected.contains(&"src/unrelated.ts"));

    let evidence = |path: &str, role: &str| {
        context["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["path"] == path)
            .and_then(|file| {
                file["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|evidence| evidence["role"] == role)
            })
            .cloned()
            .unwrap_or_else(|| panic!("missing {role} evidence for {path}"))
    };
    assert_eq!(
        evidence("src/dependency.ts", "dependency")["confidence"],
        "high"
    );
    assert_eq!(evidence("src/direct.ts", "dependent")["distance"], 1);
    assert_eq!(
        evidence("src/transitive.ts", "dependent")["confidence"],
        "partial"
    );
    assert_eq!(
        evidence("tests/changed.test.ts", "matching-test")["confidence"],
        "partial"
    );
    assert!(report["review"].is_object());
    assert!(
        report["impact"]["direct_dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "src/direct.ts")
    );
    assert!(
        report["impact"]["transitive_dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "src/transitive.ts")
    );
}

#[test]
fn change_aware_context_accepts_a_deleted_file_seed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("changed.js"), "export const value = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("direct.js"),
        "import { value } from './changed.js';\nexport const direct = value;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("transitive.js"),
        "import { direct } from './direct.js';\nexport const transitive = direct;\n",
    )
    .unwrap();
    commit_all(&repo, "initial deleted context graph");
    std::fs::remove_file(dir.path().join("changed.js")).unwrap();

    let target = dir.path().join("changed.js");
    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--context",
        "--context-max-files",
        "2",
        target.to_str().unwrap(),
    ]);
    let context = &report["context"];
    let selected = context["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(report["summary"]["files"], 0);
    assert_eq!(context["changed_files"], serde_json::json!(["changed.js"]));
    assert!(selected.contains(&"direct.js"));
    assert!(selected.contains(&"transitive.js"));
}

#[test]
fn output_file_cannot_overwrite_a_scanned_file_target() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("source.rs");
    let original = "pub fn source() {}\n";
    std::fs::write(&file, original).unwrap();
    let path = file.to_str().unwrap();

    let mut cmd = reposcout_command();
    cmd.args(["-f", "json", "--no-cache", "--quiet", "-o", path, path]);
    let output = cmd.assert().code(1).get_output().stderr.clone();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("output path cannot be the scan target")
    );
    assert_eq!(std::fs::read_to_string(file).unwrap(), original);
}

#[test]
fn output_file_cannot_recreate_a_deleted_impact_target() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let file = dir.path().join("deleted.js");
    std::fs::write(&file, "export const value = 1;\n").unwrap();
    commit_all(&repo, "initial deleted target");
    std::fs::remove_file(&file).unwrap();
    let path = file.to_str().unwrap();

    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--working",
        "--impact",
        "--no-cache",
        "--quiet",
        "-o",
        path,
        path,
    ]);
    let output = cmd.assert().code(1).get_output().stderr.clone();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("output path cannot be the scan target")
    );
    assert!(!file.exists());
}

#[test]
fn markers_are_detected() {
    let v = run_json(&["-f", "json", &fixture()]);
    let markers = &v["summary"]["markers"];
    assert!(
        markers["TODO"].as_u64().unwrap_or(0) >= 1,
        "TODO not counted"
    );
    assert!(
        markers["FIXME"].as_u64().unwrap_or(0) >= 1,
        "FIXME not counted"
    );
    assert!(
        markers["HACK"].as_u64().unwrap_or(0) >= 1,
        "HACK not counted"
    );
}

#[test]
fn imports_are_reported_as_root_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("deps.js"),
        concat!(
            "import map from 'lodash/fp';\n",
            "import thing from '@scope/package/subpath';\n",
            "import { readFile } from 'node:fs/promises';\n",
            "import local from './local';\n",
        ),
    )
    .unwrap();
    std::fs::write(dir.path().join("local.js"), "export default 1;\n").unwrap();
    std::fs::write(
        dir.path().join("deps.py"),
        "import os.path\nfrom collections.abc import Iterable\nfrom . import local\n",
    )
    .unwrap();

    let report = run_json(&["metrics", "-f", "json", dir.path().to_str().unwrap()]);
    let files = report["files"].as_array().unwrap();
    let js = files.iter().find(|file| file["path"] == "deps.js").unwrap();
    let py = files.iter().find(|file| file["path"] == "deps.py").unwrap();
    assert_eq!(
        js["imports"],
        serde_json::json!(["lodash", "@scope/package", "node:fs"])
    );
    assert_eq!(py["imports"], serde_json::json!(["os", "collections"]));
}

#[test]
fn tokens_subcommand_limits_analyzers() {
    let v = run_json(&["tokens", "-f", "json", &fixture()]);
    assert!(v["summary"]["tokens"].as_u64().unwrap() > 0);
    // Complexity analyzer is disabled, so no file carries a complexity block.
    for f in v["files"].as_array().unwrap() {
        assert!(f.get("complexity").is_none() || f["complexity"].is_null());
        assert!(f.get("symbols").is_none() || f["symbols"].is_null());
    }
}

#[test]
fn only_flag_selects_single_analyzer() {
    let v = run_json(&["--only", "markers", "-f", "json", &fixture()]);
    // markers on, tokens off.
    assert_eq!(v["summary"]["tokens"].as_u64().unwrap(), 0);
    assert!(v["summary"]["markers"]["TODO"].as_u64().unwrap_or(0) >= 1);
}

#[test]
fn only_flag_is_rejected_with_analyzer_subcommand() {
    let mut cmd = reposcout_command();
    cmd.args([
        "tokens",
        "--only",
        "markers",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);

    let output = cmd.assert().failure().get_output().stderr.clone();
    let error = String::from_utf8(output).unwrap();
    assert!(
        error.contains("--only cannot be used with an analyzer subcommand"),
        "error was: {error}"
    );
}

#[test]
fn encoding_override_is_reflected() {
    let v = run_json(&[
        "tokens",
        "-f",
        "json",
        "--encoding",
        "cl100k_base",
        &fixture(),
    ]);
    assert_eq!(v["encoding"], "cl100k_base");
    assert!(v["summary"]["tokens"].as_u64().unwrap() > 0);
}

#[test]
fn table_output_has_headers() {
    let mut cmd = reposcout_command();
    cmd.args([
        "--no-cache",
        "--quiet",
        "-f",
        "table",
        "--max-complexity",
        "1",
        &fixture(),
    ]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("reposcout"), "missing title");
    assert!(text.contains("Overview"), "missing Overview section");
    assert!(
        text.contains("Function complexity"),
        "missing function rule"
    );
    assert!(text.contains("Complexity violations"), "missing violations");
    assert!(
        text.contains("Source languages"),
        "missing source languages section"
    );
    assert!(text.contains("Other content (1 formats)"));
    assert!(text.contains("Top source files by tokens"));
    assert!(
        text.contains("Avg/fn"),
        "missing per-file complexity average"
    );
    assert!(
        text.lines().any(|line| {
            line.contains("tests/fixtures/sample/math.rs")
                && line.contains("12")
                && line.contains("3.0")
        }),
        "math.rs hotspot must show cyclomatic total 12 and callable average 3.0"
    );
}

#[test]
fn markdown_output_has_headings() {
    let mut cmd = reposcout_command();
    cmd.args([
        "--no-cache",
        "--quiet",
        "-f",
        "markdown",
        "--max-complexity",
        "1",
        &fixture(),
    ]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("# reposcout"), "missing H1");
    assert!(text.contains("## Overview"), "missing Overview heading");
    assert!(
        text.contains("## Function complexity"),
        "missing function rule heading"
    );
    assert!(
        text.contains("## Complexity violations"),
        "missing violation heading"
    );
    assert!(text.contains("## Source languages"));
    assert!(text.contains("Other content (1 formats)"));
    assert!(text.contains("## Top source files by tokens"));
}

#[test]
fn duplication_metric_is_bounded() {
    // The fixture contains a real duplicated block, so this exercises the
    // union-based line accounting. The percentage must stay within [0, 100]
    // and duplicated lines can never exceed the eligible physical lines.
    let v = run_json(&["-f", "json", &fixture()]);
    let dup = &v["summary"]["duplication"];
    let pct = dup["duplicated_pct"].as_f64().unwrap();
    assert!(
        (0.0..=100.0).contains(&pct),
        "duplicated_pct out of range: {pct}"
    );
    let dup_lines = dup["duplicated_lines"].as_u64().unwrap();
    let analyzed_lines = dup["analyzed_lines"].as_u64().unwrap();
    assert!(
        dup_lines <= analyzed_lines,
        "duplicated_lines {dup_lines} exceeds analyzed lines {analyzed_lines}"
    );
}

#[test]
fn fail_on_triggers_exit_code_two() {
    // The fixture has >2 files, so this condition is met and must exit 2.
    let mut cmd = reposcout_command();
    cmd.args([
        "--no-cache",
        "--quiet",
        "-f",
        "json",
        "--fail-on",
        "files>2",
        &fixture(),
    ]);
    cmd.assert().code(2);
}

#[test]
fn fail_on_rejects_metric_from_disabled_analyzer() {
    let mut cmd = reposcout_command();
    cmd.args([
        "tokens",
        "-f",
        "json",
        "--fail-on",
        "max-cyclomatic<1",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);

    let output = cmd.assert().code(1).get_output().stderr.clone();
    let error = String::from_utf8(output).unwrap();
    assert!(
        error.contains("max-cyclomatic requires the complexity analyzer"),
        "error was: {error}"
    );
}

#[test]
fn fail_on_passes_when_condition_not_met() {
    let mut cmd = reposcout_command();
    cmd.args([
        "--no-cache",
        "--quiet",
        "-f",
        "json",
        "--fail-on",
        "files>100000",
        &fixture(),
    ]);
    cmd.assert().success();
}

fn basenames(v: &Value) -> Vec<String> {
    v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            f["path"]
                .as_str()
                .unwrap()
                .rsplit('/')
                .next()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
fn lockfiles_are_excluded_by_default() {
    let v = run_json(&["-f", "json", &fixture()]);
    assert!(
        !basenames(&v).contains(&"package-lock.json".to_string()),
        "lockfile should be excluded by default"
    );
}

#[test]
fn lockfiles_included_with_flag() {
    let v = run_json(&["-f", "json", "--include-lockfiles", &fixture()]);
    assert!(
        basenames(&v).contains(&"package-lock.json".to_string()),
        "lockfile should appear with --include-lockfiles"
    );
}

#[test]
fn per_function_complexity_is_surfaced() {
    let v = run_json(&["-f", "json", &fixture()]);
    let c = &v["summary"]["complexity"];

    // Functions were counted across first-class-language files.
    let func_count = c["functions"].as_u64().unwrap();
    assert!(
        func_count >= 4,
        "expected several functions, got {func_count}"
    );

    // top_functions is populated and sorted by cyclomatic (descending).
    let top = v["summary"]["top_functions"].as_array().unwrap();
    assert!(!top.is_empty(), "top_functions should not be empty");

    let mut prev = u64::MAX;
    for f in top {
        assert!(!f["name"].as_str().unwrap().is_empty());
        assert!(f["path"].as_str().is_some());
        assert!(f["line"].as_u64().unwrap() >= 1);
        let cc = f["cyclomatic"].as_u64().unwrap();
        assert!(
            cc <= prev,
            "top_functions must be sorted by cyclomatic desc"
        );
        prev = cc;
    }

    // The summary maximum matches the worst individual function (function-based,
    // not a whole-file aggregate).
    let max = c["cyclomatic_max"].as_u64().unwrap();
    assert_eq!(
        max,
        top[0]["cyclomatic"].as_u64().unwrap(),
        "cyclomatic_max should equal the top function's cyclomatic value"
    );

    // Per-file function detail is still present in the file array.
    let has_functions = v["files"].as_array().unwrap().iter().any(|f| {
        f["complexity"]
            .get("functions")
            .and_then(|x| x.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    });
    assert!(has_functions, "expected per-file functions[] in JSON");
}

#[test]
fn complexity_rule_flags_functions_over_configured_maximum() {
    let v = run_json(&[
        "complexity",
        "-f",
        "json",
        "--max-complexity",
        "1",
        &fixture(),
    ]);
    let c = &v["summary"]["complexity"];
    let violations = v["summary"]["complexity_violations"]
        .as_array()
        .expect("complexity_violations must be an array");

    assert_eq!(c["cyclomatic_threshold"], 1);
    assert!(c["functions_over_threshold"].as_u64().unwrap() > 0);
    assert!(!violations.is_empty());
    for finding in violations {
        assert!(finding["cyclomatic"].as_u64().unwrap() > 1);
        assert!(!finding["name"].as_str().unwrap().is_empty());
        assert!(finding["line"].as_u64().unwrap() > 0);
    }
}

#[test]
fn non_code_files_have_no_complexity() {
    let v = run_json(&["-f", "json", &fixture()]);
    let md = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"].as_str().unwrap().ends_with("notes.md"))
        .expect("notes.md should be scanned");

    assert_eq!(md["language"], "Markdown");
    assert!(
        md["complexity"].is_null(),
        "markdown must not get complexity metrics, got {:?}",
        md["complexity"]
    );
    assert_eq!(
        md["approximate"], false,
        "non-code files are not 'approximate', they simply have no complexity"
    );

    // Non-code files must never appear as churn×complexity hotspots.
    for h in v["summary"]["top_hotspots"].as_array().unwrap() {
        let p = h["path"].as_str().unwrap();
        assert!(
            !p.ends_with(".md") && !p.ends_with(".json"),
            "non-code file leaked into hotspots: {p}"
        );
    }
}

#[test]
fn metric_conformance_survives_the_full_json_pipeline() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lines.rs"),
        "fn value() {\n    let marker = \"/*\";\n    let answer = 42;\n    answer;\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("comprehension.py"),
        "def positives(values):\n    return [value for value in values if value > 0]\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("modern.js"),
        "function sample(input = {}) {\n  input ||= {};\n  return input?.first?.second ?? null;\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("fallback.c"),
        "const char *marker = \"/*\";\nint answer = 42;\n",
    )
    .unwrap();

    let report = run_json(&["-f", "json", dir.path().to_str().unwrap()]);
    let files = report["files"].as_array().unwrap();
    let file = |name: &str| {
        files
            .iter()
            .find(|file| file["path"].as_str() == Some(name))
            .unwrap()
    };

    assert_eq!(file("lines.rs")["sloc"], 5);
    assert_eq!(file("lines.rs")["comment_lines"], 0);
    assert!(file("lines.rs").get("line_metrics_approximate").is_none());
    assert_eq!(file("fallback.c")["sloc"], 2);
    assert_eq!(file("fallback.c")["line_metrics_approximate"], true);
    assert_eq!(report["summary"]["line_metrics_approximate_files"], 1);

    assert_eq!(
        file("comprehension.py")["complexity"]["functions"][0]["cyclomatic"],
        3
    );
    assert_eq!(
        file("modern.js")["complexity"]["functions"][0]["cyclomatic"],
        6
    );
}

#[test]
fn summary_flag_omits_heavy_arrays() {
    let full = run_json(&["-f", "json", &fixture()]);
    assert!(full.get("files").is_some(), "full output has files[]");
    assert!(
        full.get("duplicates").is_some(),
        "full output has duplicates"
    );

    let brief = run_json(&["-f", "json", "--summary", &fixture()]);
    assert!(
        brief.get("files").is_none(),
        "--summary must drop the files[] array"
    );
    assert!(
        brief.get("duplicates").is_none(),
        "--summary must drop the duplicates array"
    );
    // The aggregate summary (the whole point) is retained.
    assert!(brief["summary"]["files"].as_u64().unwrap() >= 5);
    assert!(brief["summary"]["tokens"].as_u64().unwrap() > 0);
    assert_eq!(brief["schema_version"], "1.0");
}

#[test]
fn summary_json_retains_explicit_directory_and_graph_queries() {
    let report = run_json(&[
        "-f",
        "json",
        "--summary",
        "--by-dir",
        "--graph",
        "--only",
        "tokens,imports",
        &fixture(),
    ]);

    assert!(
        report["directories"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "compact output erased the explicitly requested directory rollup"
    );
    assert!(
        report["graph"].is_object(),
        "compact output erased the explicitly requested graph"
    );
    assert!(report.get("files").is_none());
    assert!(report.get("duplicates").is_none());
    assert!(report["execution"]["graph_fact_files"].as_u64().unwrap() >= 5);
}

#[test]
fn agent_profile_skips_expensive_cross_file_analyzers_and_reports_partial_evidence() {
    let report = run_json(&["-f", "json", "--summary", "--profile", "agent", &fixture()]);

    assert_eq!(report["execution"]["profile"], "agent");
    assert_eq!(report["execution"]["cache_enabled"], false);
    for stage in [
        "discovery",
        "file_analysis",
        "cross_file",
        "planning_universe",
        "report_assembly",
        "total",
    ] {
        assert!(
            report["execution"]["stage_ms"][stage].is_number(),
            "missing execution stage {stage}: {}",
            report["execution"]
        );
    }
    assert_eq!(
        report["analysis_profile"]["analyzers"]["duplication"],
        false
    );
    assert_eq!(report["analysis_profile"]["analyzers"]["churn"], false);
    assert_eq!(report["summary"]["assessment"]["fits_context_known"], true);
    assert_eq!(
        report["summary"]["assessment"]["cleanup_worth_complete"],
        false
    );
    assert!(
        report["summary"]["assessment"]["unavailable_signals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|signal| signal == "duplication")
    );
}

#[test]
fn safe_profile_ignores_repository_configuration_and_applies_guardrails() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("reposcout.toml"),
        concat!(
            "jobs = 999999\n",
            "use_cache = false\n",
            "include_hidden = true\n",
            "respect_gitignore = false\n",
            "exclude_lockfiles = false\n",
            "min_dup_tokens = 1\n",
            "min_dup_lines = 1\n",
            "churn_max_commits = 0\n",
            "max_file_bytes = 999999999999\n",
            "max_total_bytes = 999999999999\n",
            "max_files = 999999999\n",
            "max_git_blob_bytes = 999999999999\n",
            "max_scan_seconds = 999999999\n",
            "[context]\n",
            "enabled = true\n",
            "budget = 999999\n",
            "max_files = 999999\n",
        ),
    )
    .unwrap();

    let mut command = reposcout_command();
    command.args([
        "config",
        dir.path().to_str().unwrap(),
        "-f",
        "json",
        "--profile",
        "safe",
    ]);
    let output = command.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(report["config_mode"], "user");
    assert_eq!(report["sources"]["project"]["ignored"], true);
    assert_eq!(report["sources"]["project"]["loaded"], false);
    assert_eq!(report["effective"]["execution_profile"], "safe");
    assert_eq!(report["effective"]["jobs"], 2);
    assert_eq!(report["effective"]["use_cache"], true);
    assert_eq!(report["effective"]["include_hidden"], false);
    assert_eq!(report["effective"]["respect_gitignore"], false);
    assert_eq!(report["effective"]["load_repository_ignores"], false);
    assert_eq!(report["effective"]["exclude_lockfiles"], true);
    assert_eq!(report["effective"]["min_dup_tokens"], 50);
    assert_eq!(report["effective"]["min_dup_lines"], 3);
    assert_eq!(report["effective"]["churn_max_commits"], 1000);
    assert_eq!(report["effective"]["max_file_bytes"], 4 * 1024 * 1024);
    assert_eq!(report["effective"]["max_total_bytes"], 128 * 1024 * 1024);
    assert_eq!(report["effective"]["max_files"], 20_000);
    assert_eq!(report["effective"]["max_git_blob_bytes"], 4 * 1024 * 1024);
    assert_eq!(report["effective"]["max_scan_seconds"], 120);
    assert_eq!(report["effective"]["context_budget"], 32000);
    assert_eq!(report["effective"]["context_max_files"], 25);
    assert_eq!(report["effective"]["health_scope"], "source");
    assert_eq!(
        report["effective"]["health_includes"],
        serde_json::json!([])
    );
    assert_eq!(report["effective"]["analyzers"]["duplication"], false);
    assert_eq!(report["effective"]["analyzers"]["churn"], false);
}

#[test]
fn clone_groups_expose_similarity() {
    // The fixture has both an exact clone (dup_twin.rs ↔ math.rs) and a near
    // clone. Exact groups must report similarity 1.0; near groups must report
    // a value in [threshold, 1.0).
    let v = run_json(&["-f", "json", &fixture()]);
    let dups = &v["duplicates"];

    let exact = dups["exact"].as_array().unwrap();
    assert!(!exact.is_empty(), "fixture must yield an exact clone");
    for g in exact {
        assert_eq!(
            g["similarity"].as_f64().unwrap(),
            1.0,
            "exact clones are 100% similar"
        );
    }

    let near = dups["near"].as_array().unwrap();
    assert!(!near.is_empty(), "fixture must yield a near clone");
    for g in near {
        let sim = g["similarity"].as_f64().unwrap();
        assert!(
            (0.85..1.0).contains(&sim),
            "near-dup similarity {sim} must be within [threshold, 1.0)"
        );
    }
}

#[test]
fn duplicate_findings_and_union_coverage_are_precise() {
    let v = run_json(&[
        "-f",
        "json",
        "--dup-mode",
        "weak",
        "--dup-format-scope",
        "exact",
        "--dup-snippets",
        &fixture(),
    ]);
    let duplication = &v["summary"]["duplication"];
    let duplicated_tokens = duplication["duplicated_tokens"].as_u64().unwrap();
    let analyzed_tokens = duplication["analyzed_tokens"].as_u64().unwrap();
    assert!(duplicated_tokens <= analyzed_tokens);
    assert!((0.0..=100.0).contains(&duplication["duplicated_tokens_pct"].as_f64().unwrap()));
    assert!(duplication["by_language"].is_array());

    let findings = v["duplicates"]["findings"]
        .as_array()
        .expect("detailed findings");
    assert!(!findings.is_empty());
    let finding = &findings[0];
    assert_eq!(finding["id"].as_str().unwrap().len(), 32);
    assert_eq!(finding["family_id"].as_str().unwrap().len(), 32);
    for side in ["fragment_a", "fragment_b"] {
        let fragment = &finding[side];
        assert!(fragment["start_line"].as_u64().unwrap() >= 1);
        assert!(fragment["start_column"].as_u64().unwrap() >= 1);
        assert!(fragment["end_byte"].as_u64().unwrap() > fragment["start_byte"].as_u64().unwrap());
        assert!(
            fragment["end_token"].as_u64().unwrap() >= fragment["start_token"].as_u64().unwrap()
        );
        assert!(fragment["snippet"].is_string());
    }
    assert!(
        !v["duplicates"]["file_coverage"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !v["summary"]["top_duplicate_findings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn duplicate_details_flag_expands_human_reports() {
    for format in ["table", "markdown"] {
        let mut cmd = reposcout_command();
        cmd.args([
            "--no-cache",
            "--quiet",
            "-f",
            format,
            "--dup-details",
            &fixture(),
        ]);
        let out = cmd.assert().success().get_output().stdout.clone();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Duplicate findings"));
        assert!(text.contains("Duplication by language"));
    }
}

#[test]
fn top_duplicates_are_ranked_and_consistent() {
    let v = run_json(&["-f", "json", &fixture()]);
    let top = v["summary"]["top_duplicates"].as_array().unwrap();
    assert!(!top.is_empty(), "top_duplicates must surface real clones");

    let mut prev = u64::MAX;
    for b in top {
        let lines = b["lines"].as_u64().unwrap();
        let copies = b["copies"].as_u64().unwrap();
        let removable = b["duplicated_lines"].as_u64().unwrap();
        assert!(copies >= 2, "a duplicate needs at least two copies");
        assert_eq!(
            removable,
            lines * (copies - 1),
            "duplicated_lines must equal lines * (copies - 1)"
        );
        assert!(
            !b["locations"].as_array().unwrap().is_empty(),
            "each block must list where it occurs"
        );
        assert!(
            removable <= prev,
            "top_duplicates must be sorted by removable lines desc"
        );
        prev = removable;
    }
}

#[test]
fn short_clones_are_filtered_out() {
    // With the default min_dup_lines (3), no reported group may span fewer
    // than 3 lines in any instance — single-line "clones" are noise.
    let v = run_json(&["-f", "json", &fixture()]);
    let dups = &v["duplicates"];
    for key in ["exact", "near"] {
        for g in dups[key].as_array().unwrap() {
            let max_span = g["instances"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["end_line"].as_u64().unwrap() - i["start_line"].as_u64().unwrap() + 1)
                .max()
                .unwrap();
            assert!(
                max_span >= 3,
                "{key} group spans only {max_span} lines; should be filtered"
            );
        }
    }
}

#[test]
fn summary_flag_keeps_top_duplicates() {
    // top_duplicates lives in the summary specifically so agents still get
    // actionable duplication data in the compact --summary output.
    let brief = run_json(&["-f", "json", "--summary", &fixture()]);
    assert!(
        brief.get("duplicates").is_none(),
        "--summary drops the full duplicates array"
    );
    assert!(
        !brief["summary"]["top_duplicates"]
            .as_array()
            .unwrap()
            .is_empty(),
        "--summary must retain summary.top_duplicates"
    );
}

#[test]
fn symbols_and_skip_candidates_present() {
    let v = run_json(&["-f", "json", &fixture()]);

    // (a) summary.symbols.functions is present and > 0
    let func_count = v["summary"]["symbols"]["functions"]
        .as_u64()
        .expect("summary.symbols.functions should be a number");
    assert!(
        func_count > 0,
        "expected summary.symbols.functions > 0, got {func_count}"
    );

    // (b) summary.skip_candidates is present and is an array
    assert!(
        v["summary"]["skip_candidates"].is_array(),
        "summary.skip_candidates must be an array"
    );
}

#[test]
fn summary_has_test_presence_top_risks_assessment() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        concat!(
            "pub fn classify(value: i32) -> bool { value > 0 }\n",
            "#[cfg(test)] mod tests {\n",
            "    #[test] fn classifies() { assert!(super::classify(1)); }\n",
            "}\n",
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests/lib_test.rs"),
        "#[test] fn integration_smoke() { assert_eq!(1 + 1, 2); }\n",
    )
    .unwrap();
    let v = run_json(&["-f", "json", dir.path().to_str().unwrap()]);

    // test_presence: object with the four required keys
    let tp = &v["summary"]["test_presence"];
    assert!(tp.is_object(), "summary.test_presence must be an object");
    assert!(
        tp["test_files"].is_number(),
        "test_presence.test_files must be a number"
    );
    assert!(
        tp["source_files"].is_number(),
        "test_presence.source_files must be a number"
    );
    assert!(
        tp["untested_source_files"].is_number(),
        "test_presence.untested_source_files must be a number"
    );
    assert!(
        tp["untested_samples"].is_array(),
        "test_presence.untested_samples must be an array"
    );
    assert!(tp["test_files"].as_u64().unwrap() >= 1);
    assert!(tp["source_files"].as_u64().unwrap() >= 1);

    // top_risks: array (may be empty if no complexity data, but must exist)
    assert!(
        v["summary"]["top_risks"].is_array(),
        "summary.top_risks must be an array"
    );

    // assessment: boolean fits_context, numeric token_budget, valid cleanup_worth
    let a = &v["summary"]["assessment"];
    assert!(
        a["fits_context"].is_boolean(),
        "assessment.fits_context must be a boolean"
    );
    assert!(
        a["token_budget"].is_number(),
        "assessment.token_budget must be a number"
    );
    let cw = a["cleanup_worth"]
        .as_str()
        .expect("assessment.cleanup_worth must be a string");
    assert!(
        matches!(cw, "low" | "medium" | "high"),
        "assessment.cleanup_worth must be low/medium/high, got {cw}"
    );
}

#[test]
fn test_presence_does_not_cross_package_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    for package in ["web", "server"] {
        std::fs::create_dir_all(dir.path().join(format!("packages/{package}/src"))).unwrap();
        std::fs::write(
            dir.path().join(format!("packages/{package}/src/user.ts")),
            "export const user = 1;\n",
        )
        .unwrap();
    }
    std::fs::create_dir_all(dir.path().join("packages/web/tests")).unwrap();
    std::fs::write(
        dir.path().join("packages/web/tests/user.test.ts"),
        "export const userTest = 1;\n",
    )
    .unwrap();

    let report = run_json(&["-f", "json", dir.path().to_str().unwrap()]);
    let presence = &report["summary"]["test_presence"];
    assert_eq!(presence["source_files"], 2);
    assert_eq!(presence["test_files"], 1);
    assert_eq!(presence["untested_source_files"], 1);
    assert_eq!(
        presence["untested_samples"],
        serde_json::json!(["packages/server/src/user.ts"])
    );
}

#[test]
fn rust_cli_integration_tests_cover_the_binary_entrypoint() {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::write(
        dir.path().join("src/main.rs"),
        "fn main() { println!(\"ready\"); }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests/cli.rs"),
        "#[test]\nfn reports_help() { assert!(true); }\n",
    )
    .unwrap();

    let report = run_json(&["-f", "json", dir.path().to_str().unwrap()]);
    assert_eq!(report["summary"]["test_presence"]["source_files"], 1);
    assert_eq!(
        report["summary"]["test_presence"]["untested_source_files"],
        0
    );

    let mut explain = reposcout_command();
    explain.args([
        "explain",
        dir.path().join("src/main.rs").to_str().unwrap(),
        "-f",
        "json",
        "--no-cache",
        "--quiet",
    ]);
    let explained: Value =
        serde_json::from_slice(&explain.assert().success().get_output().stdout).unwrap();
    assert_eq!(explained["testing"]["tested"], true);
    assert!(
        explained["testing"]["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "tests/cli.rs")
    );
}

#[test]
fn inline_rust_tests_do_not_inflate_source_duplication_assessment() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("reposcout.toml"),
        "min_dup_tokens = 8\nmin_dup_lines = 3\nnear_dup_min_similarity = 1.0\n",
    )
    .unwrap();
    let tests = r#"
#[cfg(test)]
mod tests {
    #[test]
    fn accepts_known_values() {
        let values = [2, 3, 5, 7, 11, 13];
        let total = values.iter().sum::<i32>();
        assert_eq!(total, 41);
        assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
"#;
    std::fs::write(
        dir.path().join("src/first.rs"),
        format!("pub const FIRST_PRIME: u64 = 104729;\n{tests}"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/second.rs"),
        format!(
            "pub fn second_signal(input: &str) -> usize {{ input.bytes().filter(|byte| *byte == b'z').count() }}\n{tests}"
        ),
    )
    .unwrap();

    let report = run_json(&["-f", "json", dir.path().to_str().unwrap()]);
    assert!(
        report["summary"]["duplication"]["duplicated_pct"]
            .as_f64()
            .unwrap()
            > 15.0,
        "fixture must retain raw duplication evidence"
    );
    assert!(
        report["summary"]["assessment"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .all(|reason| !reason
                .as_str()
                .unwrap()
                .starts_with("high source duplication")),
        "test-only clones must not influence the production cleanup verdict"
    );
}

#[test]
fn tiny_single_file_scan_is_not_labeled_high_risk() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tiny.rs");
    std::fs::write(&file, "pub fn answer() -> u8 { 42 }\n").unwrap();

    let report = run_json(&["-f", "json", file.to_str().unwrap()]);
    let risk = &report["summary"]["top_risks"][0];
    assert!(
        risk["score"].as_f64().unwrap() < 0.1,
        "tiny file should have a low absolute risk score: {risk:?}"
    );
    let reasons = risk["reasons"].as_array().unwrap();
    for misleading in ["large", "complex", "high churn"] {
        assert!(
            !reasons.iter().any(|reason| reason == misleading),
            "tiny file must not be labeled {misleading}: {risk:?}"
        );
    }
}

#[test]
fn large_complex_file_receives_strong_absolute_risk_without_coverage_claims() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("risky.rs");
    let mut source = String::from("pub fn risky(input: i32) -> i32 {\n    let mut value = 0;\n");
    for index in 0..70 {
        source.push_str(&format!("    if input > {index} {{ value += 1; }}\n"));
    }
    for index in 0..700 {
        source.push_str(&format!("    let _padding_{index} = {index};\n"));
    }
    source.push_str("    value\n}\n");
    std::fs::write(&file, source).unwrap();

    let report = run_json(&["--only", "complexity", "-f", "json", file.to_str().unwrap()]);
    let risk = &report["summary"]["top_risks"][0];
    let score = risk["score"].as_f64().unwrap();
    assert!(score >= 0.6, "risk was: {risk:?}");
    assert!(
        score < 0.7,
        "test matching should be a modest signal: {risk:?}"
    );
    let reasons = risk["reasons"].as_array().unwrap();
    assert!(reasons.iter().any(|reason| reason == "large"));
    assert!(reasons.iter().any(|reason| reason == "complex"));
    assert!(
        reasons
            .iter()
            .any(|reason| reason == "no matching test file")
    );
}

#[test]
fn by_dir_produces_directories_array() {
    // Path before --by-dir so clap doesn't try to parse the path as DEPTH.
    let v = run_json(&["-f", "json", &fixture(), "--by-dir"]);

    let dirs = v["directories"]
        .as_array()
        .expect("directories must be an array when --by-dir is used");
    assert!(
        !dirs.is_empty(),
        "directories must be non-empty for a fixture with subdirectories"
    );

    for d in dirs {
        assert!(
            d["path"].is_string(),
            "each directory entry must have a 'path' string"
        );
        assert!(d["files"].is_number(), "each entry must have 'files'");
        assert!(d["tokens"].is_number(), "each entry must have 'tokens'");
        assert!(d["sloc"].is_number(), "each entry must have 'sloc'");
        assert!(
            d["cyclomatic_avg"].is_number(),
            "each entry must have 'cyclomatic_avg'"
        );
        assert!(d["mi_avg"].is_number(), "each entry must have 'mi_avg'");
        assert!(
            d["duplicated_lines"].is_number(),
            "each entry must have 'duplicated_lines'"
        );
        assert!(
            d["untested_source_files"].is_number(),
            "each entry must have 'untested_source_files'"
        );
    }
}

#[test]
fn without_by_dir_directories_is_absent() {
    // Without --by-dir, the 'directories' key must be absent (skip_serializing_if empty).
    let v = run_json(&["-f", "json", &fixture()]);
    assert!(
        v.get("directories").is_none(),
        "directories must be absent when --by-dir is not passed"
    );
}

#[test]
fn by_dir_depth2_produces_finer_buckets() {
    // With depth=2, we should get at least as many buckets as depth=1
    // (never fewer) and paths should contain at most 2 components.
    let v = run_json(&["-f", "json", &fixture(), "--by-dir=2"]);

    let dirs = v["directories"]
        .as_array()
        .expect("directories must be an array");
    assert!(!dirs.is_empty(), "depth=2 must still produce results");
    for d in dirs {
        let path = d["path"].as_str().unwrap();
        // Root-level files go to "." (0 slashes); deeper entries have at most
        // depth-1 slashes (e.g. "a/b" has 1 slash for depth=2).
        let slash_count = path.chars().filter(|&c| c == '/').count();
        assert!(
            slash_count <= 1 || path == ".",
            "depth=2 bucket '{path}' has too many components"
        );
    }
}

#[test]
fn baseline_compare_identical_yields_no_regression() {
    let fix = fixture();
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let tmp_path = tmp.path().to_str().unwrap().to_string();

    // Save a baseline report to the temp file.
    let mut cmd = reposcout_command();
    cmd.args(["-f", "json", "--no-cache", "--quiet", "-o", &tmp_path, &fix]);
    cmd.assert().success();

    // Compare the same scan against the baseline; regressions must be absent.
    let v = run_json(&["-f", "json", "--baseline", &tmp_path, &fix]);
    assert!(
        v["baseline"].is_object(),
        "expected baseline object in report"
    );
    assert!(
        v["baseline"]["metrics"].is_array(),
        "expected metrics array in baseline"
    );
    assert_eq!(
        v["baseline"]["regressed"], false,
        "identical scan must not regress"
    );
}

#[test]
fn summary_json_can_be_used_as_a_baseline() {
    let fix = fixture();
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let tmp_path = tmp.path().to_str().unwrap().to_string();

    let mut save = reposcout_command();
    save.args([
        "-f",
        "json",
        "--summary",
        "--no-cache",
        "--quiet",
        "-o",
        &tmp_path,
        &fix,
    ]);
    save.assert().success();

    let report = run_json(&["-f", "json", "--baseline", &tmp_path, &fix]);
    assert_eq!(report["baseline"]["regressed"], false);
}

#[test]
fn baseline_ready_output_is_compact_and_finding_complete() {
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let tmp_path = tmp.path().to_str().unwrap().to_string();

    let mut save = reposcout_command();
    save.args([
        "--baseline-ready",
        "--context",
        "--max-complexity",
        "1",
        "--no-cache",
        "--quiet",
        "-o",
        &tmp_path,
        &fixture(),
    ]);
    save.assert().success();

    let report: Value = serde_json::from_slice(&std::fs::read(&tmp_path).unwrap()).unwrap();
    assert_eq!(report["finding_catalog"]["version"], 1);
    assert!(
        report["finding_catalog"]["findings"]
            .as_array()
            .unwrap()
            .len()
            > 3
    );
    assert!(report.get("files").is_none());
    assert!(report.get("duplicates").is_none());
    assert!(report.get("graph").is_none());
    assert!(report.get("context").is_none());

    let summary = run_json(&["-f", "json", "--summary", &fixture()]);
    assert!(summary.get("finding_catalog").is_none());
}

#[test]
fn baseline_reports_new_and_resolved_findings() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("work.py");
    std::fs::write(
        &source,
        "# TODO remove\ndef work(value):\n    return value\n",
    )
    .unwrap();
    let baseline = tempfile::NamedTempFile::new().unwrap();

    let mut save = reposcout_command();
    save.args([
        "--baseline-ready",
        "--max-complexity",
        "1",
        "--no-cache",
        "--quiet",
        "-o",
        baseline.path().to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    save.assert().success();

    std::fs::write(
        &source,
        "def work(value):\n    if value:\n        return 1\n    return 0\n",
    )
    .unwrap();
    let report = run_json(&[
        "-f",
        "json",
        "--max-complexity",
        "1",
        "--baseline",
        baseline.path().to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);

    let changes = &report["baseline"]["finding_changes"];
    assert_eq!(changes["comparison"], "complete");
    assert!(changes["counts"]["new"].as_u64().unwrap() >= 1);
    assert!(changes["counts"]["resolved"].as_u64().unwrap() >= 1);
    let states = changes["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| change["state"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(states.contains(&"new"));
    assert!(states.contains(&"resolved"));
}

#[test]
fn baseline_fingerprints_survive_line_movement_and_report_worsening() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("work.py");
    std::fs::write(
        &source,
        "# TODO keep\ndef work(value):\n    if value:\n        return 1\n    return 0\n",
    )
    .unwrap();
    let baseline = tempfile::NamedTempFile::new().unwrap();

    let mut save = reposcout_command();
    save.args([
        "--baseline-ready",
        "--max-complexity",
        "1",
        "--no-cache",
        "--quiet",
        "-o",
        baseline.path().to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    save.assert().success();

    std::fs::write(
        &source,
        "\n\n# TODO keep\ndef work(value):\n    if value:\n        return 1\n    if value > 10:\n        return 2\n    return 0\n",
    )
    .unwrap();
    let report = run_json(&[
        "-f",
        "json",
        "--max-complexity",
        "1",
        "--baseline",
        baseline.path().to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);

    let changes = &report["baseline"]["finding_changes"];
    assert_eq!(changes["counts"]["new"], 0);
    assert_eq!(changes["counts"]["resolved"], 0);
    assert_eq!(changes["counts"]["worsened"], 1);
    let change = changes["changes"].as_array().unwrap().first().unwrap();
    assert_eq!(change["state"], "worsened");
    assert_eq!(change["after"]["kind"], "complexity");
}

#[test]
fn baseline_rejects_mismatched_analyzer_profiles() {
    let fix = fixture();
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let tmp_path = tmp.path().to_str().unwrap().to_string();

    let mut save = reposcout_command();
    save.args(["-f", "json", "--no-cache", "--quiet", "-o", &tmp_path, &fix]);
    save.assert().success();

    let mut compare = reposcout_command();
    compare.args([
        "tokens",
        "-f",
        "json",
        "--baseline",
        &tmp_path,
        "--no-cache",
        "--quiet",
        &fix,
    ]);
    let output = compare.assert().code(1).get_output().stderr.clone();
    let error = String::from_utf8(output).unwrap();
    assert!(
        error.contains("baseline analyzer profile does not match"),
        "error was: {error}"
    );
}

#[test]
fn focused_baseline_reports_only_available_metrics() {
    let fix = fixture();
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let tmp_path = tmp.path().to_str().unwrap().to_string();

    let mut save = reposcout_command();
    save.args([
        "tokens",
        "-f",
        "json",
        "--no-cache",
        "--quiet",
        "-o",
        &tmp_path,
        &fix,
    ]);
    save.assert().success();

    let report = run_json(&["tokens", "-f", "json", "--baseline", &tmp_path, &fix]);
    let names = report["baseline"]["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|metric| metric["metric"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["files", "tokens", "sloc", "untested_source_files"]
    );
    assert_eq!(report["baseline"]["regressed"], false);
}

#[test]
fn baseline_rejects_mismatched_token_encoding() {
    let fix = fixture();
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let tmp_path = tmp.path().to_str().unwrap().to_string();

    let mut save = reposcout_command();
    save.args(["-f", "json", "--no-cache", "--quiet", "-o", &tmp_path, &fix]);
    save.assert().success();

    let mut compare = reposcout_command();
    compare.args([
        "-f",
        "json",
        "--encoding",
        "cl100k_base",
        "--baseline",
        &tmp_path,
        "--no-cache",
        "--quiet",
        &fix,
    ]);
    let output = compare.assert().code(1).get_output().stderr.clone();
    let error = String::from_utf8(output).unwrap();
    assert!(
        error.contains("baseline token encoding does not match"),
        "error was: {error}"
    );
}

#[test]
fn baseline_rejects_mismatched_target_scope() {
    let fix = fixture();
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let tmp_path = tmp.path().to_str().unwrap().to_string();

    let mut save = reposcout_command();
    save.args(["-f", "json", "--no-cache", "--quiet", "-o", &tmp_path, &fix]);
    save.assert().success();

    let file = format!("{fix}/app.js");
    let mut compare = reposcout_command();
    compare.args([
        "-f",
        "json",
        "--baseline",
        &tmp_path,
        "--no-cache",
        "--quiet",
        &file,
    ]);
    let output = compare.assert().code(1).get_output().stderr.clone();
    let error = String::from_utf8(output).unwrap();
    assert!(
        error.contains("baseline target scope does not match"),
        "error was: {error}"
    );
}

#[test]
fn baseline_rejects_mismatched_duplication_settings() {
    let fix = fixture();
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let tmp_path = tmp.path().to_str().unwrap().to_string();

    let mut save = reposcout_command();
    save.args(["-f", "json", "--no-cache", "--quiet", "-o", &tmp_path, &fix]);
    save.assert().success();

    let mut compare = reposcout_command();
    compare.args([
        "-f",
        "json",
        "--dup-mode",
        "weak",
        "--baseline",
        &tmp_path,
        "--no-cache",
        "--quiet",
        &fix,
    ]);
    let output = compare.assert().code(1).get_output().stderr.clone();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("baseline analyzer profile does not match")
    );
}

#[test]
fn baseline_rejects_mismatched_health_file_scope() {
    let fix = fixture();
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let tmp_path = tmp.path().to_str().unwrap().to_string();

    let mut save = reposcout_command();
    save.args(["-f", "json", "--no-cache", "--quiet", "-o", &tmp_path, &fix]);
    save.assert().success();

    let mut compare = reposcout_command();
    compare.args([
        "-f",
        "json",
        "--health-scope",
        "all",
        "--baseline",
        &tmp_path,
        "--no-cache",
        "--quiet",
        &fix,
    ]);
    let output = compare.assert().code(1).get_output().stderr.clone();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("baseline analyzer profile does not match")
    );
}

#[test]
fn diff_baselines_compare_resolved_tree_identity() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("source.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    commit_all(&repo, "first tree");
    let baseline = tempfile::NamedTempFile::new().unwrap();

    let mut save = reposcout_command();
    save.args([
        "-f",
        "json",
        "--since",
        "HEAD",
        "--no-cache",
        "--quiet",
        "-o",
        baseline.path().to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    save.assert().success();

    let same_tree = run_json(&[
        "-f",
        "json",
        "--since",
        "HEAD^{tree}",
        "--baseline",
        baseline.path().to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(same_tree["baseline"]["regressed"], false);

    std::fs::write(dir.path().join("source.rs"), "pub fn value() -> u8 { 2 }\n").unwrap();
    commit_all(&repo, "second tree");
    let mut compare = reposcout_command();
    compare.args([
        "-f",
        "json",
        "--since",
        "HEAD",
        "--baseline",
        baseline.path().to_str().unwrap(),
        "--no-cache",
        "--quiet",
        dir.path().to_str().unwrap(),
    ]);
    let error = String::from_utf8(compare.assert().failure().get_output().stderr.clone()).unwrap();
    assert!(
        error.contains("baseline diff base tree does not match"),
        "error was: {error}"
    );
}

#[test]
fn baselines_without_analysis_profiles_are_rejected_after_scope_semantics_changed() {
    let fix = fixture();
    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let tmp_path = tmp.path().to_str().unwrap().to_string();

    let mut save = reposcout_command();
    save.args(["-f", "json", "--no-cache", "--quiet", "-o", &tmp_path, &fix]);
    save.assert().success();
    let mut legacy: Value = serde_json::from_slice(&std::fs::read(&tmp_path).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("analysis_profile");
    std::fs::write(&tmp_path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

    let mut full = reposcout_command();
    full.args([
        "-f",
        "json",
        "--baseline",
        &tmp_path,
        "--no-cache",
        "--quiet",
        &fix,
    ]);
    let output = full.assert().code(1).get_output().stderr.clone();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("baseline lacks analyzer profile metadata")
    );

    let mut focused = reposcout_command();
    focused.args([
        "tokens",
        "-f",
        "json",
        "--baseline",
        &tmp_path,
        "--no-cache",
        "--quiet",
        &fix,
    ]);
    let output = focused.assert().code(1).get_output().stderr.clone();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("baseline lacks analyzer profile metadata")
    );
}

#[test]
fn review_filters_complexity_findings_to_changed_function_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let source = dir.path().join("work.py");
    std::fs::write(
        &source,
        concat!(
            "def changed(value):\n",
            "    if value:\n",
            "        return 1\n",
            "    return 0\n\n",
            "def untouched(value):\n",
            "    if value:\n",
            "        return 1\n",
            "    return 0\n",
        ),
    )
    .unwrap();
    commit_all(&repo, "initial review fixture");
    std::fs::write(
        &source,
        concat!(
            "def changed(value):\n",
            "    if value:\n",
            "        return value + 1\n",
            "    return 0\n\n",
            "def untouched(value):\n",
            "    if value:\n",
            "        return 1\n",
            "    return 0\n",
        ),
    )
    .unwrap();

    let report = run_json(&[
        "--working",
        "--review",
        "--only",
        "complexity",
        "--max-complexity",
        "1",
        "-f",
        "json",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(report["review"]["mode"], "lines");
    let findings = report["review"]["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1, "review findings were: {findings:?}");
    assert!(
        findings[0]["finding"]["message"]
            .as_str()
            .unwrap()
            .contains("changed")
    );
}

#[test]
fn deep_review_reports_new_and_resolved_findings_against_git_base() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let source = dir.path().join("work.py");
    std::fs::write(
        &source,
        "# TODO remove\ndef work(value):\n    return value\n",
    )
    .unwrap();
    commit_all(&repo, "initial deep review fixture");
    std::fs::write(
        &source,
        "def work(value):\n    if value:\n        return 1\n    return 0\n",
    )
    .unwrap();

    let report = run_json(&[
        "--working",
        "--review=deep",
        "--only",
        "complexity,markers",
        "--max-complexity",
        "1",
        "-f",
        "json",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(report["review"]["mode"], "deep");
    assert_eq!(report["review"]["counts"]["new"], 1);
    assert_eq!(report["review"]["counts"]["resolved"], 1);
    let states = report["review"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["state"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(states.contains(&"new"));
    assert!(states.contains(&"resolved"));
}

#[test]
fn staged_review_analyzes_index_content_not_unstaged_worktree_content() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let source = dir.path().join("work.py");
    let simple = "def work(value):\n    return value\n";
    std::fs::write(&source, simple).unwrap();
    commit_all(&repo, "initial staged review fixture");

    std::fs::write(
        &source,
        "def work(value):\n    if value:\n        return 1\n    return 0\n",
    )
    .unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("work.py")).unwrap();
    index.write().unwrap();
    std::fs::write(&source, simple).unwrap();

    let report = run_json(&[
        "--staged",
        "--review=deep",
        "--only",
        "complexity",
        "--max-complexity",
        "1",
        "-f",
        "json",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(report["review"]["counts"]["new"], 1);
    assert_eq!(
        report["review"]["findings"][0]["finding"]["kind"],
        "complexity"
    );
}

#[test]
fn review_detects_changed_code_duplicated_from_an_unchanged_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let block = (0..30)
        .map(|index| format!("const value{index} = input + {index};\n"))
        .collect::<String>();
    std::fs::write(dir.path().join("original.js"), &block).unwrap();
    commit_all(&repo, "initial duplication review fixture");
    std::fs::write(dir.path().join("copy.js"), &block).unwrap();

    let report = run_json(&[
        "--working",
        "--review",
        "--only",
        "duplication",
        "-f",
        "json",
        dir.path().to_str().unwrap(),
    ]);

    let findings = report["review"]["findings"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding["finding"]["kind"] == "duplication"),
        "review findings were: {findings:?}"
    );
}

#[test]
fn fail_on_review_exits_two_for_changed_line_findings() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let source = dir.path().join("work.py");
    std::fs::write(
        &source,
        "def work(value):\n    if value:\n        return 1\n    return 0\n",
    )
    .unwrap();
    commit_all(&repo, "initial review gate fixture");
    std::fs::write(
        &source,
        "def work(value):\n    if value:\n        return value + 1\n    return 0\n",
    )
    .unwrap();

    let mut cmd = reposcout_command();
    cmd.args([
        "--working",
        "--review",
        "--fail-on-review",
        "--only",
        "complexity",
        "--max-complexity",
        "1",
        "--no-cache",
        "--quiet",
        dir.path().to_str().unwrap(),
    ]);
    cmd.assert().code(2);
}

#[test]
fn review_requires_a_diff_scope() {
    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--review",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);
    let error = String::from_utf8(cmd.assert().failure().get_output().stderr.clone()).unwrap();

    assert!(error.contains("--review requires"), "error was: {error}");
}

#[test]
fn deep_review_does_not_treat_a_renamed_finding_as_new_or_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let old = dir.path().join("old.py");
    let new = dir.path().join("new.py");
    std::fs::write(
        &old,
        "def risky(value):\n    if value == 1:\n        return 1\n    if value == 2:\n        return 2\n    return 0\n",
    )
    .unwrap();
    commit_all(&repo, "rename base");
    std::fs::rename(&old, &new).unwrap();

    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--working",
        "--review=deep",
        "--max-complexity",
        "1",
        "--no-cache",
        "--quiet",
        dir.path().to_str().unwrap(),
    ]);
    let report: Value =
        serde_json::from_slice(&cmd.assert().success().get_output().stdout.clone()).unwrap();

    assert_eq!(report["review"]["counts"]["new"], 0);
    assert_eq!(report["review"]["counts"]["resolved"], 0);
}

#[test]
fn deep_review_applies_reposcoutignore_to_both_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join(".reposcoutignore"), "ignored.py\n").unwrap();
    std::fs::write(dir.path().join("ignored.py"), "# TODO hidden\nVALUE = 1\n").unwrap();
    std::fs::write(dir.path().join("kept.py"), "VALUE = 1\n").unwrap();
    commit_all(&repo, "ignored base");
    std::fs::write(dir.path().join("ignored.py"), "# FIXME hidden\nVALUE = 2\n").unwrap();

    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--working",
        "--review=deep",
        "--no-cache",
        "--quiet",
        dir.path().to_str().unwrap(),
    ]);
    let report: Value =
        serde_json::from_slice(&cmd.assert().success().get_output().stdout.clone()).unwrap();

    assert!(report["review"]["findings"].as_array().unwrap().is_empty());
}

#[test]
fn deep_review_supports_unborn_working_and_staged_repositories() {
    for staged in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("new.py"), "# TODO new file\nVALUE = 1\n").unwrap();
        if staged {
            let mut index = repo.index().unwrap();
            index.add_path(std::path::Path::new("new.py")).unwrap();
            index.write().unwrap();
        }

        let scope = if staged { "--staged" } else { "--working" };
        let report = run_json(&[
            "-f",
            "json",
            scope,
            "--review=deep",
            dir.path().to_str().unwrap(),
        ]);
        assert_eq!(report["review"]["counts"]["new"], 1);
        assert_eq!(report["review"]["findings"][0]["finding"]["kind"], "marker");
    }
}

#[test]
fn review_renderers_surface_the_same_deep_states() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let source = dir.path().join("work.py");
    std::fs::write(&source, "# TODO old\nVALUE = 1\n").unwrap();
    commit_all(&repo, "renderer base");
    std::fs::write(&source, "# FIXME new\nVALUE = 2\n").unwrap();
    let path = dir.path().to_str().unwrap();

    for (format, expected) in [
        ("table", "Changed-line review"),
        ("markdown", "## Changed-line review"),
    ] {
        let mut cmd = reposcout_command();
        cmd.args([
            "-f",
            format,
            "--working",
            "--review=deep",
            "--no-cache",
            "--quiet",
            path,
        ]);
        let text = String::from_utf8(cmd.assert().success().get_output().stdout.clone()).unwrap();
        assert!(text.contains(expected), "{format} output was: {text}");
        assert!(text.contains("new"), "{format} omitted the new state");
        assert!(
            text.contains("resolved"),
            "{format} omitted the resolved state"
        );
    }

    let mut ndjson = reposcout_command();
    ndjson.args([
        "-f",
        "ndjson",
        "--working",
        "--review=deep",
        "--no-cache",
        "--quiet",
        path,
    ]);
    let text = String::from_utf8(ndjson.assert().success().get_output().stdout.clone()).unwrap();
    let rows = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(rows[0].get("review").is_some());
    assert_eq!(
        rows.iter()
            .filter(|row| row["kind"] == "review_finding")
            .count(),
        2
    );

    let mut sarif = reposcout_command();
    sarif.args([
        "-f",
        "sarif",
        "--working",
        "--review=deep",
        "--no-cache",
        "--quiet",
        path,
    ]);
    let report: Value =
        serde_json::from_slice(&sarif.assert().success().get_output().stdout.clone()).unwrap();
    let states = report["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| result["baselineState"].as_str().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(states, std::collections::HashSet::from(["new", "absent"]));
}

#[test]
fn explain_json_combines_file_findings_tests_and_graph_context() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::write(
        dir.path().join("src/work.js"),
        "import { dep } from './dep';\n// TODO explain me\nexport function work(value) { if (value) { return dep; } return 0; }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/dep.js"), "export const dep = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("src/consumer.js"),
        "import { work } from './work';\nexport const result = work(1);\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests/work.test.js"),
        "import { work } from '../src/work';\nwork(1);\n",
    )
    .unwrap();
    commit_all(&repo, "explain fixture");

    let mut cmd = reposcout_command();
    cmd.args([
        "explain",
        dir.path().join("src/work.js").to_str().unwrap(),
        "-f",
        "json",
        "--max-complexity",
        "1",
        "--no-cache",
        "--quiet",
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(report["path"], "src/work.js");
    assert_eq!(report["discovery"]["status"], "analyzed");
    assert_eq!(report["file"]["language"], "JavaScript");
    assert!(report["risk"]["score"].is_number());
    assert_eq!(report["testing"]["tested"], true);
    assert!(
        report["testing"]["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "tests/work.test.js")
    );
    assert!(
        report["graph"]["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "src/dep.js")
    );
    assert!(
        report["graph"]["dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "src/consumer.js")
    );
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["kind"] == "marker")
    );
}

#[test]
fn explain_reports_the_exact_reposcoutignore_rule() {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("ignored.py"),
        "def ignored():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("kept.py"), "def kept():\n    return 1\n").unwrap();
    std::fs::write(dir.path().join(".reposcoutignore"), "ignored.py\n").unwrap();

    let mut cmd = reposcout_command();
    cmd.args([
        "explain",
        dir.path().join("ignored.py").to_str().unwrap(),
        "-f",
        "json",
        "--no-cache",
        "--quiet",
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(report["discovery"]["status"], "ignored");
    assert_eq!(report["discovery"]["rule"]["pattern"], "ignored.py");
    assert!(
        report["discovery"]["rule"]["source"]
            .as_str()
            .unwrap()
            .ends_with(".reposcoutignore")
    );
    assert!(report.get("file").is_none());
}

#[test]
fn explain_rejects_sarif_with_a_clear_error() {
    let mut cmd = reposcout_command();
    cmd.args(["explain", &fixture(), "-f", "sarif"]);
    let error = String::from_utf8(cmd.assert().failure().get_output().stderr.clone()).unwrap();

    assert!(
        error.contains("does not support SARIF"),
        "error was: {error}"
    );
}

#[test]
fn explain_reports_a_nested_missing_file_without_failing() {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("kept.py"), "VALUE = 1\n").unwrap();
    let missing = dir.path().join("not/created/missing.py");

    let mut cmd = reposcout_command();
    cmd.args([
        "explain",
        missing.to_str().unwrap(),
        "-f",
        "json",
        "--no-cache",
        "--quiet",
    ]);
    let report: Value =
        serde_json::from_slice(&cmd.assert().success().get_output().stdout.clone()).unwrap();

    assert_eq!(report["path"], "not/created/missing.py");
    assert_eq!(report["discovery"]["status"], "missing");
    assert_eq!(report["testing"]["classification"], "unavailable");
}

#[cfg(unix)]
#[test]
fn explain_does_not_follow_a_symlink_outside_the_repository() {
    let dir = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("kept.py"), "VALUE = 1\n").unwrap();
    std::fs::write(external.path().join("outside.py"), "# TODO outside\n").unwrap();
    let link = dir.path().join("outside.py");
    std::os::unix::fs::symlink(external.path().join("outside.py"), &link).unwrap();

    let mut cmd = reposcout_command();
    cmd.args([
        "explain",
        link.to_str().unwrap(),
        "-f",
        "json",
        "--no-cache",
        "--quiet",
    ]);
    let report: Value =
        serde_json::from_slice(&cmd.assert().success().get_output().stdout.clone()).unwrap();

    assert_eq!(
        report["root"],
        dir.path().canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(report["path"], "outside.py");
    assert_eq!(report["discovery"]["status"], "ignored");
    assert_eq!(report["discovery"]["rule"]["kind"], "symlink");
    assert_eq!(report["testing"]["classification"], "unavailable");
    assert!(report.get("file").is_none());
}

#[cfg(unix)]
#[test]
fn machine_formats_fail_on_non_utf8_report_paths() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(OsString::from_vec(b"bad-\xff.rs".to_vec()));
    if let Err(error) = std::fs::write(path, "pub fn value() {}\n") {
        #[cfg(target_os = "macos")]
        if error.raw_os_error() == Some(92) {
            eprintln!("skipping: filesystem rejects non-UTF-8 filenames");
            return;
        }
        panic!("failed to create non-UTF-8 fixture: {error}");
    }

    for args in [
        vec!["-f", "json"],
        vec!["-f", "json", "--summary"],
        vec!["-f", "json", "--baseline-ready"],
        vec!["-f", "ndjson"],
        vec!["-f", "sarif"],
    ] {
        let mut cmd = reposcout_command();
        cmd.args(args)
            .arg("--no-cache")
            .arg("--quiet")
            .arg(dir.path());
        let assertion = cmd.assert().failure();
        let output = assertion.get_output();
        assert!(output.stdout.is_empty(), "machine output must be atomic");
        let error = String::from_utf8(output.stderr.clone()).unwrap();
        assert!(error.contains("not valid UTF-8"), "error was: {error}");
    }
}

#[cfg(unix)]
#[test]
fn human_and_sarif_renderers_escape_repository_paths() {
    let dir = tempfile::tempdir().unwrap();
    let unusual = dir.path().join("bad\n|`name.py");
    std::fs::write(
        &unusual,
        "def value(flag):\n    if flag:\n        return 1\n    return 0\n",
    )
    .unwrap();

    let mut table = reposcout_command();
    table.args([
        "-f",
        "table",
        "--no-cache",
        "--quiet",
        dir.path().to_str().unwrap(),
    ]);
    let table = String::from_utf8(table.assert().success().get_output().stdout.clone()).unwrap();
    assert!(table.contains("bad\\n|`name.py"));
    assert!(!table.contains("bad\n|`name.py"));

    let mut markdown = reposcout_command();
    markdown.args([
        "-f",
        "markdown",
        "--no-cache",
        "--quiet",
        dir.path().to_str().unwrap(),
    ]);
    let markdown =
        String::from_utf8(markdown.assert().success().get_output().stdout.clone()).unwrap();
    assert!(markdown.contains("bad\\n\\|"));
    assert!(!markdown.contains("bad\n|`name.py"));

    let sarif_source = dir.path().join("a b#.py");
    std::fs::write(
        sarif_source,
        "def risky(flag):\n    if flag:\n        return 1\n    return 0\n",
    )
    .unwrap();
    let mut sarif = reposcout_command();
    sarif.args([
        "-f",
        "sarif",
        "--max-complexity",
        "1",
        "--no-cache",
        "--quiet",
        dir.path().to_str().unwrap(),
    ]);
    let sarif: Value =
        serde_json::from_slice(&sarif.assert().success().get_output().stdout.clone()).unwrap();
    assert!(
        sarif["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| {
                result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
                    == "a%20b%23.py"
            })
    );
}

#[test]
fn staged_flag_succeeds_with_valid_json() {
    let repo_root = env!("CARGO_MANIFEST_DIR");
    let v = run_json(&["-f", "json", "--staged", repo_root]);
    // Even with zero staged files the report structure must be intact.
    assert!(
        v["summary"]["files"].is_number(),
        "summary.files must be a number"
    );
}

#[test]
fn since_bogus_ref_fails() {
    let repo_root = env!("CARGO_MANIFEST_DIR");
    let mut cmd = reposcout_command();
    cmd.args([
        "--no-cache",
        "--quiet",
        "-f",
        "json",
        "--since",
        "definitely-not-a-ref-zzz",
        repo_root,
    ]);
    cmd.assert().failure();
}

#[test]
fn graph_flag_produces_graph_object() {
    let v = run_json(&["-f", "json", "--graph", &fixture()]);
    let g = &v["graph"];
    assert!(
        g.is_object(),
        "expected graph object when --graph is passed"
    );
    assert!(g["nodes"].as_u64().is_some(), "graph.nodes must be present");
    assert!(g["edges"].as_u64().is_some(), "graph.edges must be present");
    assert!(g["cycles"].is_array(), "graph.cycles must be an array");
    assert!(g["orphans"].is_array(), "graph.orphans must be an array");
    assert!(
        g["top_depended"].is_array(),
        "graph.top_depended must be an array"
    );
    assert!(
        g["most_dependent"].is_array(),
        "graph.most_dependent must be an array"
    );
    assert!(
        g["unresolved_imports"].as_u64().is_some(),
        "graph.unresolved_imports must be present"
    );
    // The fixture contains app.js and util.py — both are JS/Python graph files.
    assert!(
        g["nodes"].as_u64().unwrap() >= 2,
        "expected at least 2 nodes (app.js + util.py)"
    );
}

#[test]
fn graph_absent_without_flag() {
    let v = run_json(&["-f", "json", &fixture()]);
    assert!(
        v["graph"].is_null(),
        "graph field must be absent when --graph is not passed"
    );
}

#[test]
fn graph_table_shows_section_header() {
    let mut cmd = reposcout_command();
    cmd.args([
        "--no-cache",
        "--quiet",
        "-f",
        "table",
        "--graph",
        &fixture(),
    ]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("Dependency graph"),
        "table output must contain 'Dependency graph' section"
    );
}

#[test]
fn graph_query_exposes_bounded_adjacency_and_tsconfig_edges() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/core")).unwrap();
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{
          "compilerOptions": {
            "baseUrl": ".",
            "paths": { "@core/*": ["src/core/*"] }
          }
        }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/app.ts"),
        "import { service } from '@core/service';\nexport { service };\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/core/service.ts"),
        "import { db } from './db';\nexport const service = db;\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/core/db.ts"), "export const db = 1;\n").unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--only",
        "imports",
        "--graph-focus",
        "src/core/service.ts",
        "--graph-direction",
        "dependents",
        "--graph-depth",
        "1",
        dir.path().to_str().unwrap(),
    ]);
    let graph = &report["graph"];

    assert_eq!(graph["nodes"], 2);
    assert_eq!(graph["edges"], 1);
    assert_eq!(graph["direction"], "dependents");
    assert_eq!(graph["depth"], 1);
    assert_eq!(graph["config_files"], serde_json::json!(["tsconfig.json"]));
    assert_eq!(
        graph["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["src/app.ts", "src/core/service.ts"]
    );
    assert_eq!(graph["edge_list"][0]["source"], "src/app.ts");
    assert_eq!(graph["edge_list"][0]["target"], "src/core/service.ts");
    assert_eq!(graph["edge_list"][0]["resolver"], "tsconfig-paths");
}

#[test]
fn graph_json_exposes_package_and_python_absolute_resolver_provenance() {
    let dir = tempfile::tempdir().unwrap();
    for directory in ["apps/web", "apps/tools", "packages/core/src", "src/domain"] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    std::fs::write(
        dir.path().join("packages/core/package.json"),
        r#"{
          "name": "@acme/core",
          "exports": { ".": "./src/index.js" }
        }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/web/main.ts"),
        "import '@acme/core';\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("packages/core/src/index.ts"),
        "export const core = 1;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/tools/main.py"),
        "from domain.service import VALUE\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/domain/service.py"), "VALUE = 1\n").unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--only",
        "imports",
        "--graph",
        dir.path().to_str().unwrap(),
    ]);
    let graph = &report["graph"];

    assert!(
        graph["config_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "packages/core/package.json")
    );
    assert!(graph["edge_list"].as_array().unwrap().iter().any(|edge| {
        edge["source"] == "apps/web/main.ts"
            && edge["target"] == "packages/core/src/index.ts"
            && edge["resolver"] == "package-exports"
    }));
    assert!(graph["edge_list"].as_array().unwrap().iter().any(|edge| {
        edge["source"] == "apps/tools/main.py"
            && edge["target"] == "src/domain/service.py"
            && edge["resolver"] == "python-src-root"
    }));
}

#[test]
fn graph_json_exposes_rust_and_go_module_resolver_provenance() {
    let dir = tempfile::tempdir().unwrap();
    for directory in ["src", "cmd", "internal/store"] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    std::fs::write(
        dir.path().join("Cargo.toml"),
        r#"[package]
name = "mixed-rust"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub mod service;\n").unwrap();
    std::fs::write(dir.path().join("src/service.rs"), "pub fn run() {}\n").unwrap();
    std::fs::write(dir.path().join("go.mod"), "module example.com/mixed\n").unwrap();
    std::fs::write(
        dir.path().join("cmd/main.go"),
        "package main\n\nimport \"example.com/mixed/internal/store\"\n\nfunc main() { store.Open() }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("internal/store/store.go"),
        "package store\n\nfunc Open() {}\n",
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--only",
        "imports",
        "--graph",
        dir.path().to_str().unwrap(),
    ]);
    let graph = &report["graph"];

    assert_eq!(graph["languages"], serde_json::json!(["Go", "Rust"]));
    assert_eq!(
        graph["config_files"],
        serde_json::json!(["Cargo.toml", "go.mod"])
    );
    assert!(graph["edge_list"].as_array().unwrap().iter().any(|edge| {
        edge["source"] == "src/lib.rs"
            && edge["target"] == "src/service.rs"
            && edge["resolver"] == "rust-mod"
    }));
    assert!(graph["edge_list"].as_array().unwrap().iter().any(|edge| {
        edge["source"] == "cmd/main.go"
            && edge["target"] == "internal/store/store.go"
            && edge["resolver"] == "go-module"
    }));
}

#[test]
fn php_is_first_class_across_scan_context_tests_and_composer_graphs() {
    let dir = tempfile::tempdir().unwrap();
    for directory in ["src/Http", "src/Service", "tests/Service"] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{
          "autoload": { "psr-4": { "App\\": "src/" } },
          "autoload-dev": { "psr-4": { "Tests\\": "tests/" } }
        }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/Service/UserService.php"),
        r#"<?php
namespace App\Service;
use Psr\Log\LoggerInterface;

final class UserService {
    public function find(bool $active): int {
        $TODO = "TODO in a string";
        // TODO real marker
        return $active ? 1 : 0;
    }
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/Http/Controller.php"),
        "<?php\nnamespace App\\Http;\nuse App\\Service\\UserService;\nfinal class Controller {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests/Service/UserServiceTest.php"),
        "<?php\nnamespace Tests\\Service;\nuse App\\Service\\UserService;\nfinal class UserServiceTest {}\n",
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--graph",
        "--context",
        "--focus",
        "src/Service/UserService.php",
        dir.path().to_str().unwrap(),
    ]);
    let service = report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "src/Service/UserService.php")
        .unwrap();

    assert_eq!(service["language"], "PHP");
    assert_eq!(service["approximate"], false);
    assert!(service.get("line_metrics_approximate").is_none());
    assert_eq!(service["markers"]["TODO"], 1);
    assert_eq!(service["imports"], serde_json::json!(["Psr"]));
    assert_eq!(service["symbols"]["types"], 1);
    assert_eq!(service["symbols"]["functions"], 1);
    assert_eq!(service["complexity"]["functions"][0]["name"], "find");
    assert_eq!(service["complexity"]["functions"][0]["cyclomatic"], 2);
    assert!(
        !report["summary"]["test_presence"]["untested_samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "src/Service/UserService.php")
    );
    assert!(
        report["context"]["graph_languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|language| language == "PHP")
    );
    assert_eq!(
        report["graph"]["config_files"],
        serde_json::json!(["composer.json"])
    );
    assert!(
        report["graph"]["edge_list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| {
                edge["source"] == "src/Http/Controller.php"
                    && edge["target"] == "src/Service/UserService.php"
                    && edge["resolver"] == "composer-psr-4"
            })
    );
}

#[test]
fn dot_and_mermaid_formats_export_the_graph_without_external_tools() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.ts"), "import './b';\n").unwrap();
    std::fs::write(dir.path().join("b.ts"), "export const b = 1;\n").unwrap();

    for (format, expected) in [("dot", "digraph reposcout"), ("mermaid", "flowchart LR")] {
        let mut cmd = reposcout_command();
        cmd.args([
            "--only",
            "imports",
            "--no-cache",
            "--quiet",
            "-f",
            format,
            dir.path().to_str().unwrap(),
        ]);
        let output = cmd.assert().success().get_output().stdout.clone();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains(expected), "output was: {rendered}");
        assert!(rendered.contains("a.ts"));
        assert!(rendered.contains("b.ts"));
    }

    let inferred = dir.path().join("focused.mmd");
    let mut cmd = reposcout_command();
    cmd.args([
        "--only",
        "imports",
        "--no-cache",
        "--quiet",
        "--graph-focus",
        "a.ts",
        "-o",
        inferred.to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    cmd.assert().success();
    let rendered = std::fs::read_to_string(inferred).unwrap();
    assert!(rendered.starts_with("flowchart LR"));
    assert!(rendered.contains("class n0 focus"));
}

#[test]
fn ndjson_output_has_summary_and_file_lines() {
    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "ndjson",
        "--context",
        "--graph",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.split('\n').filter(|l| !l.is_empty()).collect();
    assert!(
        lines.len() >= 2,
        "expected at least 2 NDJSON lines, got {}",
        lines.len()
    );
    for line in &lines {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("NDJSON line is not valid JSON: {e}\nline: {line}"));
    }
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(
        first["kind"], "summary",
        "first line must have kind=summary"
    );
    assert!(
        first["diagnostics"].is_object(),
        "summary line must carry scan diagnostics"
    );
    assert!(
        first["context"].is_object(),
        "summary line must carry a requested context plan"
    );
    assert!(
        first["graph"]["files"].is_array(),
        "full NDJSON summary must carry deterministic graph files"
    );
    for field in [
        "schema_version",
        "root",
        "target",
        "generated_at",
        "encoding",
        "analysis_profile",
    ] {
        assert!(!first[field].is_null(), "summary line omitted {field}");
    }
    let has_file_line = lines.iter().any(|l| {
        serde_json::from_str::<serde_json::Value>(l)
            .map(|v| v["kind"] == "file")
            .unwrap_or(false)
    });
    assert!(has_file_line, "at least one line must have kind=file");
    let finding = lines
        .iter()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value["kind"] == "finding")
        .expect("full NDJSON output must append duplicate findings");
    assert!(matches!(
        finding["finding_kind"].as_str(),
        Some("exact" | "type2")
    ));
}

#[test]
fn broken_stdout_pipe_exits_successfully_without_noise() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..512 {
        std::fs::write(
            dir.path().join(format!("bounded-{index:03}.txt")),
            format!("bounded pipe fixture {index}\n"),
        )
        .unwrap();
    }
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("reposcout"))
        .env("REPOSCOUT_GLOBAL_CONFIG", test_global_config())
        .args([
            "tokens",
            "-f",
            "ndjson",
            "--no-cache",
            "--quiet",
            dir.path().to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take().unwrap());

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "status was {}", output.status);
    assert!(
        output.stderr.is_empty(),
        "stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sarif_output_is_valid_2_1_0() {
    let v = run_json(&["-f", "sarif", "--max-complexity", "1", &fixture()]);
    assert_eq!(v["version"], "2.1.0");
    assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "reposcout");
    let rules = v["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .expect("rules must be an array");
    assert!(!rules.is_empty(), "rules must be non-empty");
    assert!(
        v["runs"][0]["results"].is_array(),
        "results must be an array"
    );
    assert_eq!(v["runs"][0]["columnKind"], "unicodeCodePoints");
    let duplicate = v["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["ruleId"] == "reposcout/duplicate-code")
        .expect("duplicate SARIF result");
    assert!(duplicate["locations"][0]["physicalLocation"]["region"]["startColumn"].is_number());
    assert!(
        duplicate["relatedLocations"][0]["physicalLocation"]["region"]["endColumn"].is_number()
    );
    let complexity = v["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["ruleId"] == "reposcout/high-complexity-function")
        .expect("per-function complexity SARIF result");
    assert_eq!(complexity["level"], "warning");
    assert_eq!(complexity["properties"]["maximum"], 1);
    assert!(complexity["properties"]["cyclomatic"].as_u64().unwrap() > 1);
}

#[test]
fn reposcoutignore_excludes_matching_files() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("reposcout_test_ignore_{}_{}", pid, nanos));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("keep.py"), "def keep():\n    return 1\n").unwrap();
    std::fs::write(dir.join("drop.py"), "def drop():\n    return 2\n").unwrap();
    std::fs::write(dir.join(".reposcoutignore"), "drop.py\n").unwrap();

    let dir_str = dir.to_str().unwrap().to_string();
    let output = {
        let mut cmd = reposcout_command();
        cmd.args(["-f", "json", "--no-cache", "--quiet", &dir_str]);
        cmd.assert().success().get_output().stdout.clone()
    };
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();

    let paths: Vec<String> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_string())
        .collect();

    assert!(
        paths.iter().any(|p| p.ends_with("keep.py")),
        "keep.py should be in the scan results, got: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.ends_with("drop.py")),
        "drop.py should be excluded by .reposcoutignore, got: {paths:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_keeps_focused_and_full_file_reports_separate() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sample.rs"),
        "// TODO: cached analysis must stay complete\nuse std::collections::HashMap;\nfn score(values: &[i32]) -> i32 { if values.is_empty() { 0 } else { values[0] } }\n",
    )
    .unwrap();
    let path = dir.path().to_str().unwrap().to_string();

    let mut focused = reposcout_command();
    focused.args(["tokens", "-f", "json", "--quiet", &path]);
    focused.assert().success();

    let mut full = reposcout_command();
    full.args(["-f", "json", "--quiet", &path]);
    let output = full.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    assert!(
        report["summary"]["complexity"]["functions"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "a focused cache entry must not erase full-scan complexity"
    );
    assert!(
        report["summary"]["markers"]["TODO"].as_u64().unwrap_or(0) > 0,
        "a focused cache entry must not erase full-scan markers"
    );
    assert!(
        !report["files"][0]["imports"].as_array().unwrap().is_empty(),
        "a focused cache entry must not erase full-scan imports"
    );
}

#[test]
fn standalone_file_uses_its_basename_as_report_path() {
    let file = tempfile::Builder::new()
        .prefix("reposcout-standalone-")
        .suffix(".rs")
        .tempfile()
        .unwrap();
    std::fs::write(file.path(), "fn example() {}\n").unwrap();
    let path = file.path().to_str().unwrap().to_string();
    let basename = file
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let report = run_json(&["-f", "json", &path]);

    assert_eq!(report["files"][0]["path"], basename);
}

#[test]
fn directory_duplication_uses_the_same_coverage_as_the_global_summary() {
    let report = run_json(&["-f", "json", &fixture(), "--by-dir=2"]);
    let global = report["summary"]["duplication"]["duplicated_lines"]
        .as_u64()
        .unwrap();
    let directories = report["directories"].as_array().unwrap();
    let by_directory: u64 = directories
        .iter()
        .map(|directory| directory["duplicated_lines"].as_u64().unwrap())
        .sum();

    assert_eq!(
        by_directory, global,
        "directory totals must partition the same physical clone coverage"
    );

    assert_eq!(
        directories.len(),
        1,
        "fixture should have one depth-2 bucket"
    );
    let directory_avg = directories[0]["cyclomatic_avg"].as_f64().unwrap();
    let global_avg = report["summary"]["complexity"]["cyclomatic_avg"]
        .as_f64()
        .unwrap();
    assert!(
        (directory_avg - global_avg).abs() < f64::EPSILON,
        "directory complexity must use the same per-function semantics"
    );
}

#[test]
fn diagnostics_explain_unsupported_and_unreadable_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ok.rs"), "fn okay() {}\n").unwrap();
    std::fs::write(dir.path().join("notes.unknown"), "not a language\n").unwrap();
    std::fs::write(dir.path().join("binary.rs"), [0xff, 0xfe, 0xfd]).unwrap();
    let path = dir.path().to_str().unwrap().to_string();

    let report = run_json(&["-f", "json", &path]);
    let diagnostics = &report["diagnostics"];

    assert_eq!(diagnostics["discovered_files"], 3);
    assert_eq!(diagnostics["analyzed_files"], 1);
    assert_eq!(diagnostics["unsupported_files"], 1);
    assert_eq!(diagnostics["unreadable_files"], 1);
}

#[test]
fn diagnostics_explain_files_skipped_by_resource_limits() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("small.rs"), "fn okay() {}\n").unwrap();
    std::fs::write(dir.path().join("large.rs"), "x".repeat(128)).unwrap();
    let path = dir.path().to_str().unwrap().to_string();

    let report = run_json(&["-f", "json", &path, "--max-file-bytes", "32"]);
    let diagnostics = &report["diagnostics"];

    assert_eq!(diagnostics["discovered_files"], 2);
    assert_eq!(diagnostics["analyzed_files"], 1);
    assert_eq!(diagnostics["oversized_files"], 1);
    assert_eq!(diagnostics["oversized_bytes"], 128);
    assert_eq!(diagnostics["scan_truncated"], true);
    assert_eq!(
        report["analysis_profile"]["resources"]["max_file_bytes"],
        32
    );
}

#[test]
fn invalid_config_fails_instead_of_falling_back_to_defaults() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("reposcout.toml"),
        "unknown_setting = true\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("sample.rs"), "fn sample() {}\n").unwrap();
    let path = dir.path().to_str().unwrap().to_string();

    let mut cmd = reposcout_command();
    cmd.args(["-f", "json", "--no-cache", "--quiet", &path]);
    let output = cmd.assert().failure().get_output().stderr.clone();
    let error = String::from_utf8(output).unwrap();
    assert!(
        error.contains("failed to parse config"),
        "error was: {error}"
    );
    assert!(error.contains("reposcout.toml"), "error was: {error}");
}

fn commit_all(repo: &git2::Repository, message: &str) {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("reposcout tests", "tests@example.com").unwrap();

    if let Ok(head) = repo.head()
        && let Ok(parent) = head.peel_to_commit()
    {
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        )
        .unwrap();
    } else {
        repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
            .unwrap();
    }
}

#[test]
fn impact_reports_direct_and_transitive_dependents_from_full_topology() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("changed.js"), "export const value = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("direct.js"),
        "import { value } from './changed';\nexport const direct = value;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("transitive.js"),
        "import { direct } from './direct';\nexport const transitive = direct;\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("outside.js"), "export const outside = 1;\n").unwrap();
    commit_all(&repo, "initial graph");
    std::fs::write(dir.path().join("changed.js"), "export const value = 2;\n").unwrap();
    std::fs::write(dir.path().join("outside.js"), "export const outside = 2;\n").unwrap();
    let path = dir.path().join("changed.js").to_str().unwrap().to_string();

    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--working",
        "--impact",
        "--no-cache",
        "--quiet",
        &path,
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    let impact = &report["impact"];

    assert_eq!(
        impact["changed_files"],
        serde_json::json!(["changed.js"]),
        "impact changes must stay scoped to the requested target"
    );
    assert!(
        impact["direct_dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "direct.js"),
        "direct dependent missing: {impact:?}"
    );
    assert!(
        impact["transitive_dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "transitive.js"),
        "transitive dependent missing: {impact:?}"
    );
    assert_eq!(impact["confidence"], "high");
}

#[test]
fn change_summary_is_a_bounded_agent_profile_decision_report() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("changed.js"), "export const value = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("direct.js"),
        "import { value } from './changed';\nexport const direct = value;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("transitive.js"),
        "import { direct } from './direct';\nexport const transitive = direct;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("changed.test.js"),
        "import { value } from './changed';\nif (value !== 1) throw new Error('failed');\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("compose.yaml"), "services: {}\n").unwrap();
    commit_all(&repo, "initial change summary fixture");
    std::fs::write(dir.path().join("changed.js"), "export const value = 2;\n").unwrap();
    std::fs::write(
        dir.path().join("compose.yaml"),
        "services:\n  app:\n    image: example\n",
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(report["report_kind"], "change-summary");
    assert_eq!(report["execution"]["profile"], "agent");
    assert_eq!(report["change_summary"]["scope"], "working");
    assert_eq!(report["change_summary"]["executive"]["changed_files"], 2);
    assert_eq!(
        report["change_summary"]["executive"]["graph_eligible_changed_files"],
        1
    );
    assert_eq!(
        report["change_summary"]["executive"]["known_direct_dependents"],
        2
    );
    assert_eq!(
        report["change_summary"]["executive"]["known_transitive_dependents"],
        1
    );
    assert_eq!(report["change_summary"]["executive"]["matching_tests"], 1);
    assert!(
        report["change_summary"]["tests"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["path"] == "changed.test.js"
                    && entry["matched_sources"] == serde_json::json!(["changed.js"])
                    && entry["confidence"] == "partial"
            })
    );
    assert!(
        report.get("summary").is_none()
            && report.get("files").is_none()
            && report.get("context").is_none()
            && report.get("impact").is_none(),
        "dedicated projection leaked ordinary report blocks: {report}"
    );
    assert_eq!(
        report["change_summary"]["changed"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["changed.js", "compose.yaml"]
    );
}

#[test]
fn change_summary_keeps_known_matching_tests_outside_the_context_file_cap() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("changed.ts"), "export const value = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("changed.test.ts"),
        "import { value } from './changed';\nif (value < 0) throw new Error('failed');\n",
    )
    .unwrap();
    commit_all(&repo, "initial matching test fixture");
    std::fs::write(dir.path().join("changed.ts"), "export const value = 2;\n").unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        "--context-max-files",
        "1",
        dir.path().to_str().unwrap(),
    ]);
    let summary = &report["change_summary"];

    assert_eq!(summary["tests"]["total"], 1);
    assert_eq!(summary["tests"]["files"][0]["path"], "changed.test.ts");
    assert_eq!(
        summary["tests"]["files"][0]["matched_sources"],
        serde_json::json!(["changed.ts"])
    );
}

#[test]
fn change_summary_separates_clean_observed_scope_from_repository_blind_spots() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path().join("unrelated")).unwrap();
    std::fs::write(dir.path().join("changed.js"), "export const value = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("direct.js"),
        "import { value } from './changed';\nexport const direct = value;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("unrelated/broken.js"),
        "export function broken( {\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("unrelated/unresolved.js"),
        "import missing from './does-not-exist';\nexport const value = missing;\n",
    )
    .unwrap();
    commit_all(&repo, "initial graph with distant parser gap");
    std::fs::write(dir.path().join("changed.js"), "export const value = 2;\n").unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        "--context-max-files",
        "2",
        dir.path().to_str().unwrap(),
    ]);
    let coverage = &report["change_summary"]["coverage"];

    assert_eq!(coverage["observed_scope_confidence"], "high");
    assert_eq!(coverage["discovery_completeness"], "partial");
    assert_eq!(
        report["change_summary"]["executive"]["confidence"],
        "partial"
    );
    assert_eq!(coverage["relevant_gaps"]["parse_errors"], 0);
    assert!(
        coverage["outside_known_scope_gaps"]["parse_errors"]
            .as_u64()
            .unwrap()
            > 0,
        "distant parser gap was not retained: {coverage}"
    );
    assert!(
        coverage["outside_known_scope_gaps"]["unresolved_imports"]
            .as_u64()
            .unwrap()
            > 0,
        "distant unresolved import was not retained: {coverage}"
    );
    assert!(
        coverage["gaps"].as_array().unwrap().iter().any(|gap| {
            gap["path"] == "unrelated/broken.js" && gap["scope"] == "outside-known-scope"
        }),
        "distant parser gap was not attributed: {coverage}"
    );
    assert!(coverage["gaps"].as_array().unwrap().iter().any(|gap| {
        gap["path"] == "unrelated/unresolved.js"
            && gap["scope"] == "outside-known-scope"
            && gap["unresolved_imports"].as_u64().unwrap() > 0
    }));
}

#[test]
fn change_summary_marks_a_changed_parser_gap_as_relevant() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("changed.js"),
        "export function changed() { return 1; }\n",
    )
    .unwrap();
    commit_all(&repo, "initial valid source");
    std::fs::write(
        dir.path().join("changed.js"),
        "import missing from './does-not-exist';\nexport function changed( {\n",
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        dir.path().to_str().unwrap(),
    ]);
    let coverage = &report["change_summary"]["coverage"];

    assert_eq!(coverage["observed_scope_confidence"], "partial");
    assert!(
        coverage["relevant_gaps"]["parse_errors"].as_u64().unwrap() > 0,
        "changed parser gap was not relevant: {coverage}"
    );
    assert!(
        coverage["relevant_gaps"]["unresolved_imports"]
            .as_u64()
            .unwrap()
            > 0,
        "changed unresolved import was not relevant: {coverage}"
    );
    assert!(
        coverage["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap["path"] == "changed.js" && gap["scope"] == "changed")
    );
}

#[test]
fn change_summary_attributes_relevant_resolver_configuration_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("tsconfig.json"),
        "{ this is not valid JSON }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("changed.ts"),
        "export const value: number = 1;\n",
    )
    .unwrap();
    commit_all(&repo, "initial invalid resolver config");
    std::fs::write(
        dir.path().join("changed.ts"),
        "export const value: number = 2;\n",
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        dir.path().to_str().unwrap(),
    ]);
    let coverage = &report["change_summary"]["coverage"];

    assert!(
        coverage["relevant_gaps"]["config_errors"].as_u64().unwrap() > 0,
        "resolver config error was not relevant: {coverage}"
    );
    assert!(coverage["gaps"].as_array().unwrap().iter().any(|gap| {
        gap["path"] == "tsconfig.json"
            && gap["scope"] == "known-impact"
            && gap["config_errors"].as_u64().unwrap() > 0
    }));
}

#[test]
fn change_summary_keeps_deleted_first_class_files_as_impact_seeds() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("changed.js"), "export const value = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("importer.js"),
        "import { value } from './changed';\nexport const imported = value;\n",
    )
    .unwrap();
    commit_all(&repo, "initial deleted source fixture");
    std::fs::remove_file(dir.path().join("changed.js")).unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        dir.path().to_str().unwrap(),
    ]);
    let summary = &report["change_summary"];

    assert_eq!(summary["changed"]["files"][0]["path"], "changed.js");
    assert_eq!(summary["changed"]["files"][0]["graph_eligible"], true);
    assert_eq!(summary["changed"]["files"][0]["graph_covered"], true);
    assert_eq!(summary["impact"]["direct_total"], 1);
    assert!(
        summary["impact"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"] == "importer.js" && entry["distance"] == 1)
    );
}

#[test]
fn change_summary_handles_non_graph_changes_without_inventing_impact() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("README.md"), "# Before\n").unwrap();
    std::fs::write(dir.path().join("compose.yaml"), "services: {}\n").unwrap();
    commit_all(&repo, "initial non-graph files");
    std::fs::write(dir.path().join("README.md"), "# After\n").unwrap();
    std::fs::write(
        dir.path().join("compose.yaml"),
        "services:\n  app:\n    image: example\n",
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        dir.path().to_str().unwrap(),
    ]);
    let summary = &report["change_summary"];

    assert_eq!(
        summary["coverage"]["observed_scope_confidence"],
        "not-applicable"
    );
    assert_eq!(
        summary["coverage"]["test_mapping_confidence"],
        "not-applicable"
    );
    assert_eq!(summary["impact"]["direct_total"], 0);
    assert_eq!(summary["impact"]["transitive_total"], 0);
    assert_eq!(
        summary["reading_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        ["README.md", "compose.yaml"].into_iter().collect()
    );
    assert!(
        summary["validations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "project-configuration"
                && entry["target"] == "compose.yaml")
    );
}

#[test]
fn change_summary_returns_a_minimal_success_for_an_empty_diff() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    commit_all(&repo, "clean repository");

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        dir.path().to_str().unwrap(),
    ]);
    let summary = &report["change_summary"];

    assert_eq!(summary["executive"]["changed_files"], 0);
    assert_eq!(summary["changed"]["total"], 0);
    assert_eq!(summary["changed"]["omitted"], 0);
    assert_eq!(
        summary["coverage"]["observed_scope_confidence"],
        "not-applicable"
    );
    assert_eq!(
        summary["coverage"]["test_mapping_confidence"],
        "not-applicable"
    );
    assert!(summary["reading_order"].as_array().unwrap().is_empty());
    assert!(summary["validations"].as_array().unwrap().is_empty());
}

#[test]
fn change_summary_honors_explicit_full_and_safe_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("main.rs"), "pub fn value() -> i32 { 1 }\n").unwrap();
    commit_all(&repo, "initial profile fixture");
    std::fs::write(dir.path().join("main.rs"), "pub fn value() -> i32 { 2 }\n").unwrap();
    let path = dir.path().to_str().unwrap();

    let full = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        "--profile",
        "full",
        path,
    ]);
    assert_eq!(full["execution"]["profile"], "full");
    assert_eq!(full["analysis_profile"]["analyzers"]["duplication"], true);
    assert_eq!(full["analysis_profile"]["analyzers"]["churn"], true);

    let safe = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        "--profile",
        "safe",
        path,
    ]);
    assert_eq!(safe["execution"]["profile"], "safe");
    assert_eq!(safe["analysis_profile"]["analyzers"]["duplication"], false);
    assert_eq!(safe["analysis_profile"]["analyzers"]["churn"], false);
}

#[test]
fn change_summary_supports_human_and_machine_formats() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("main.rs"), "pub fn value() -> i32 { 1 }\n").unwrap();
    commit_all(&repo, "initial renderer fixture");
    std::fs::write(dir.path().join("main.rs"), "pub fn value() -> i32 { 2 }\n").unwrap();
    let path = dir.path().to_str().unwrap();

    for (format, expected) in [
        ("table", "RepoScout change summary (working)"),
        ("markdown", "# RepoScout change summary"),
    ] {
        let mut cmd = reposcout_command();
        cmd.args([
            "-f",
            format,
            "--working",
            "--change-summary",
            "--no-cache",
            "--quiet",
            path,
        ]);
        let stdout = cmd.assert().success().get_output().stdout.clone();
        let rendered = String::from_utf8(stdout).unwrap();
        assert!(
            rendered.contains(expected),
            "{format} output was: {rendered}"
        );
        assert!(
            !rendered.contains("Top risks"),
            "general health rankings leaked into {format}: {rendered}"
        );
    }

    let mut ndjson = reposcout_command();
    ndjson.args([
        "-f",
        "ndjson",
        "--working",
        "--change-summary",
        "--no-cache",
        "--quiet",
        path,
    ]);
    let stdout = ndjson.assert().success().get_output().stdout.clone();
    let rendered = String::from_utf8(stdout).unwrap();
    assert_eq!(rendered.lines().count(), 1);
    let record: Value = serde_json::from_str(rendered.trim()).unwrap();
    assert_eq!(record["report_kind"], "change-summary");
}

#[test]
fn change_summary_human_formats_escape_repository_control_characters() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let filename = "bad\n#heading.md";
    std::fs::write(dir.path().join(filename), "# Before\n").unwrap();
    commit_all(&repo, "initial control-character fixture");
    std::fs::write(dir.path().join(filename), "# After\n").unwrap();
    let path = dir.path().to_str().unwrap();

    for format in ["table", "markdown"] {
        let mut cmd = reposcout_command();
        cmd.args([
            "-f",
            format,
            "--working",
            "--change-summary",
            "--no-cache",
            "--quiet",
            path,
        ]);
        let stdout = cmd.assert().success().get_output().stdout.clone();
        let rendered = String::from_utf8(stdout).unwrap();

        assert!(rendered.contains("bad\\n#heading.md"));
        assert!(!rendered.contains("bad\n#heading.md"));
    }
}

#[test]
fn change_summary_rejects_findings_and_graph_only_formats_before_scanning() {
    for format in ["sarif", "dot", "mermaid"] {
        let mut cmd = reposcout_command();
        cmd.args([
            "-f",
            format,
            "--working",
            "--change-summary",
            "--error-format",
            "json",
            "/definitely/not/a/repository",
        ]);
        let stderr = cmd.assert().failure().get_output().stderr.clone();
        let error: Value = serde_json::from_slice(&stderr).unwrap();
        assert_eq!(error["category"], "usage");
        assert_eq!(error["exit_code"], 2);
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("supports table, JSON, Markdown, or NDJSON"),
            "{format} error was: {error}"
        );
    }
}

#[test]
fn change_summary_enforces_one_aggregate_path_budget() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    for index in 0..125 {
        std::fs::write(
            dir.path().join(format!("note-{index:03}.md")),
            format!("# Before {index}\n"),
        )
        .unwrap();
    }
    commit_all(&repo, "initial bounded path fixture");
    for index in 0..125 {
        std::fs::write(
            dir.path().join(format!("note-{index:03}.md")),
            format!("# After {index}\n"),
        )
        .unwrap();
    }

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        dir.path().to_str().unwrap(),
    ]);
    let summary = &report["change_summary"];
    let serialized_path_entries = summary["changed"]["files"].as_array().unwrap().len()
        + summary["reading_order"].as_array().unwrap().len()
        + summary["impact"]["files"].as_array().unwrap().len()
        + summary["tests"]["files"].as_array().unwrap().len()
        + summary["coverage"]["gaps"].as_array().unwrap().len()
        + summary["validations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry.get("target").is_some())
            .count();

    assert_eq!(summary["changed"]["total"], 125);
    assert_eq!(summary["changed"]["shown"], 100);
    assert_eq!(summary["changed"]["omitted"], 25);
    assert_eq!(summary["reading_order_total"], 125);
    assert_eq!(summary["reading_order_shown"], 0);
    assert_eq!(summary["reading_order_omitted"], 125);
    assert!(
        serialized_path_entries <= 100,
        "aggregate path budget exceeded: {serialized_path_entries}"
    );
}

#[test]
fn change_summary_bounds_detailed_graph_gaps_but_preserves_totals() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("changed.js"), "export const value = 1;\n").unwrap();
    for index in 0..30 {
        let directory = dir.path().join(format!("unrelated-{index:02}"));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("broken.js"), "export function broken( {\n").unwrap();
    }
    commit_all(&repo, "initial graph gap fixture");
    std::fs::write(dir.path().join("changed.js"), "export const value = 2;\n").unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        "--context-max-files",
        "1",
        dir.path().to_str().unwrap(),
    ]);
    let coverage = &report["change_summary"]["coverage"];

    assert_eq!(coverage["gaps"].as_array().unwrap().len(), 25);
    assert_eq!(coverage["gaps_omitted"], 5);
    assert_eq!(coverage["outside_known_scope_gaps"]["parse_errors"], 30);
}

#[test]
fn change_summary_exposes_safe_profile_resource_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("changed.rs"),
        "pub fn value() -> i32 { 1 }\n",
    )
    .unwrap();
    commit_all(&repo, "initial bounded source");
    std::fs::write(
        dir.path().join("changed.rs"),
        format!("pub fn value() -> i32 {{ 2 }}\n// {}\n", "x".repeat(256)),
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--change-summary",
        "--profile",
        "safe",
        "--max-file-bytes",
        "64",
        dir.path().to_str().unwrap(),
    ]);
    let summary = &report["change_summary"];

    assert_eq!(report["diagnostics"]["scan_truncated"], true);
    assert_eq!(report["diagnostics"]["oversized_files"], 1);
    assert!(
        summary["executive"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "scan-truncated")
    );
    assert_ne!(
        summary["coverage"]["discovery_completeness"], "high",
        "truncated scan claimed complete discovery"
    );
}

#[test]
fn change_summary_excludes_its_output_file_from_the_change_scope() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("changed.rs"),
        "pub fn value() -> i32 { 1 }\n",
    )
    .unwrap();
    commit_all(&repo, "initial output exclusion fixture");
    std::fs::write(
        dir.path().join("changed.rs"),
        "pub fn value() -> i32 { 2 }\n",
    )
    .unwrap();
    let output = dir.path().join("change-summary.json");

    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--working",
        "--change-summary",
        "-o",
        output.to_str().unwrap(),
        "--no-cache",
        "--quiet",
        dir.path().to_str().unwrap(),
    ]);
    cmd.assert().success();
    let report: Value =
        serde_json::from_slice(&std::fs::read(&output).expect("change summary output")).unwrap();

    assert_eq!(report["change_summary"]["changed"]["total"], 1);
    assert_eq!(
        report["change_summary"]["changed"]["files"][0]["path"],
        "changed.rs"
    );
}

#[test]
fn change_summary_is_materially_smaller_than_detailed_change_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let source_body = |index: usize, value: usize| {
        format!(
            "export function value{index}(input: number): number {{\n  if (input > {value}) {{\n    return input + {value};\n  }}\n  return {value};\n}}\n"
        )
    };
    for index in 0..8 {
        std::fs::write(
            dir.path().join(format!("module-{index}.ts")),
            source_body(index, 1),
        )
        .unwrap();
    }
    for index in 0..2 {
        std::fs::write(
            dir.path().join(format!("module-{index}.test.ts")),
            format!(
                "import {{ value{index} }} from './module-{index}';\nif (value{index}(1) < 0) throw new Error('failed');\n"
            ),
        )
        .unwrap();
    }
    for (name, content) in [
        ("README.md", "# Before\n"),
        ("CHANGELOG.md", "# Changelog\n"),
        ("compose.yaml", "services: {}\n"),
        ("Makefile", "check:\n\ttrue\n"),
        ("package.json", "{\"private\":true}\n"),
        ("tsconfig.json", "{\"compilerOptions\":{}}\n"),
        ("notes.txt", "before\n"),
    ] {
        std::fs::write(dir.path().join(name), content).unwrap();
    }
    commit_all(&repo, "initial representative change fixture");
    for index in 0..8 {
        std::fs::write(
            dir.path().join(format!("module-{index}.ts")),
            source_body(index, 2),
        )
        .unwrap();
    }
    for index in 0..2 {
        std::fs::write(
            dir.path().join(format!("module-{index}.test.ts")),
            format!(
                "import {{ value{index} }} from './module-{index}';\nif (value{index}(2) < 0) throw new Error('failed');\n"
            ),
        )
        .unwrap();
    }
    for (name, content) in [
        ("README.md", "# After\n"),
        ("CHANGELOG.md", "# Changelog\n\n- Changed\n"),
        ("compose.yaml", "services:\n  app:\n    image: example\n"),
        ("Makefile", "check:\n\t@true\n"),
        ("package.json", "{\"private\":true,\"type\":\"module\"}\n"),
        ("tsconfig.json", "{\"compilerOptions\":{\"strict\":true}}\n"),
        ("notes.txt", "after\n"),
    ] {
        std::fs::write(dir.path().join(name), content).unwrap();
    }
    let path = dir.path().to_str().unwrap();

    let mut detailed = reposcout_command();
    detailed.args([
        "-f",
        "json",
        "--working",
        "--summary",
        "--context",
        "--impact",
        "--profile",
        "agent",
        "--no-cache",
        "--quiet",
        path,
    ]);
    let detailed_bytes = detailed.assert().success().get_output().stdout.clone();

    let mut concise = reposcout_command();
    concise.args([
        "-f",
        "json",
        "--working",
        "--change-summary",
        "--no-cache",
        "--quiet",
        path,
    ]);
    let concise_bytes = concise.assert().success().get_output().stdout.clone();
    let concise_report: Value = serde_json::from_slice(&concise_bytes).unwrap();

    assert_eq!(concise_report["change_summary"]["changed"]["total"], 17);
    assert_eq!(concise_report["change_summary"]["changed"]["shown"], 17);
    assert!(
        concise_bytes.len() * 100 <= detailed_bytes.len() * 40,
        "expected at least 60% fewer bytes; detailed={}, concise={}",
        detailed_bytes.len(),
        concise_bytes.len()
    );
}

#[test]
fn impact_reports_python_from_current_package_importers() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::create_dir(dir.path().join("pkg")).unwrap();
    std::fs::write(dir.path().join("pkg/__init__.py"), "").unwrap();
    std::fs::write(dir.path().join("pkg/changed.py"), "VALUE = 1\n").unwrap();
    std::fs::write(
        dir.path().join("pkg/consumer.py"),
        "from . import changed\nRESULT = changed.VALUE\n",
    )
    .unwrap();
    commit_all(&repo, "initial python graph");
    std::fs::write(dir.path().join("pkg/changed.py"), "VALUE = 2\n").unwrap();

    let target = dir.path().join("pkg/changed.py");
    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--working",
        "--impact",
        "--no-cache",
        "--quiet",
        target.to_str().unwrap(),
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(
        report["impact"]["direct_dependents"],
        serde_json::json!(["pkg/consumer.py"])
    );
    assert_eq!(report["impact"]["confidence"], "high");
}

#[test]
fn impact_reports_importers_of_a_deleted_graph_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("changed.js"), "export const value = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("direct.js"),
        "import { value } from './changed';\nexport const direct = value;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("transitive.js"),
        "import { direct } from './direct';\nexport const transitive = direct;\n",
    )
    .unwrap();
    commit_all(&repo, "initial graph");
    std::fs::remove_file(dir.path().join("changed.js")).unwrap();
    let path = dir.path().join("changed.js").to_str().unwrap().to_string();

    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--working",
        "--impact",
        "--no-cache",
        "--quiet",
        &path,
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    let impact = &report["impact"];

    assert!(
        impact["direct_dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "direct.js"),
        "deleted file's direct importer missing: {impact:?}"
    );
    assert!(
        impact["transitive_dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "transitive.js"),
        "deleted file's transitive importer missing: {impact:?}"
    );
}

#[test]
fn impact_accepts_a_deleted_target_whose_parent_directories_are_gone() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let changed = dir.path().join("removed/nested/changed.js");
    std::fs::create_dir_all(changed.parent().unwrap()).unwrap();
    std::fs::write(&changed, "export const value = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("direct.js"),
        "import { value } from './removed/nested/changed';\nexport const direct = value;\n",
    )
    .unwrap();
    commit_all(&repo, "initial nested graph");
    std::fs::remove_dir_all(dir.path().join("removed")).unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--impact",
        changed.to_str().unwrap(),
    ]);
    assert!(
        report["impact"]["direct_dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "direct.js")
    );
}

#[test]
fn impact_is_partial_when_a_changed_graph_file_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("changed.js"), "export const value = 1;\n").unwrap();
    commit_all(&repo, "initial graph");
    std::fs::write(dir.path().join("changed.js"), [0xff, 0xfe, 0xfd]).unwrap();
    let path = dir.path().join("changed.js").to_str().unwrap().to_string();

    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--working",
        "--impact",
        "--no-cache",
        "--quiet",
        &path,
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(report["impact"]["confidence"], "partial");
}

#[test]
fn impact_is_partial_when_an_unchanged_graph_file_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("changed.js"), "export const value = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("importer.js"),
        "import { value } from './changed';\nexport const imported = value;\n",
    )
    .unwrap();
    commit_all(&repo, "initial graph");
    std::fs::write(dir.path().join("changed.js"), "export const value = 2;\n").unwrap();
    std::fs::write(dir.path().join("importer.js"), [0xff, 0xfe, 0xfd]).unwrap();
    let path = dir.path().join("changed.js").to_str().unwrap().to_string();

    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--working",
        "--impact",
        "--no-cache",
        "--quiet",
        &path,
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let report: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(report["impact"]["confidence"], "partial");
}

#[test]
fn impact_requires_a_diff_scope() {
    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--impact",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);
    let output = cmd.assert().failure().get_output().stderr.clone();
    let error = String::from_utf8(output).unwrap();
    assert!(error.contains("--impact requires"), "error was: {error}");
}

#[test]
fn change_summary_requires_a_diff_scope_before_scanning() {
    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--change-summary",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);
    let output = cmd.assert().failure().get_output().stderr.clone();
    let error = String::from_utf8(output).unwrap();

    assert!(
        error.contains("--change-summary requires exactly one of --since, --staged, or --working"),
        "error was: {error}"
    );
}
