use super::*;

#[test]
fn full_scan_reports_core_metrics() {
    let v = run_json(&["-f", "json", &fixture()]);

    assert_eq!(v["schema_version"], "1.0");
    assert_eq!(v["encoding"], "o200k_base");
    assert!(v.get("report_kind").is_none());
    assert!(v.get("change_summary").is_none());
    assert_eq!(v["work_scope"]["strategy_version"], 2);
    assert_eq!(v["work_scope"]["basis"], serde_json::json!(["repository"]));
    assert_eq!(
        v["work_scope"]["inventory"]["source_files"],
        v["summary"]["source"]["files"]
    );
    assert_eq!(
        v["work_scope"]["inventory"]["source_tokens"],
        v["summary"]["source"]["tokens"]
    );
    assert_eq!(
        v["work_scope"]["production_duplication"],
        v["summary"]["assessment"]["production_duplication"]
    );

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
            .all(|file| {
                Path::new(file["path"].as_str().unwrap())
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
            })
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
fn health_excludes_win_after_scope_and_format_includes_without_removing_inventory() {
    let dir = tempfile::tempdir().unwrap();
    write_health_scope_fixture(dir.path());
    std::fs::write(
        dir.path().join("reposcout.toml"),
        concat!(
            "health_includes = [\"json\"]\n",
            "health_excludes = [\"second.rs\"]\n",
        ),
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--health-exclude",
        "second.json",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(
        report["summary"]["files"], 5,
        "inventory retains the four fixture files and project config"
    );
    assert_eq!(report["summary"]["source"]["files"], 2);
    assert_eq!(report["summary"]["markers"]["TODO"], 2);
    assert_eq!(
        report["analysis_profile"]["health"]["includes"],
        serde_json::json!(["JSON"])
    );
    assert_eq!(
        report["analysis_profile"]["health"]["excludes"],
        serde_json::json!(["second.json", "second.rs"])
    );
    assert_eq!(report["summary"]["test_presence"]["source_files"], 1);

    for excluded in ["second.rs", "second.json"] {
        let file = report["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["path"] == excluded)
            .unwrap();
        assert!(
            file.get("markers").is_none() || file["markers"].is_null(),
            "{excluded} must not carry marker health results"
        );
    }
    let excluded_source = report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "second.rs")
        .unwrap();
    assert!(
        excluded_source.get("complexity").is_none() || excluded_source["complexity"].is_null(),
        "health-excluded source must not carry complexity health results"
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
    assert_eq!(report["health_exclude_flag"], "--health-exclude");
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
    assert_eq!(report["work_scope"]["strategy_version"], 2);
    assert_eq!(report["work_scope"]["max_path_entries"], 25);
    assert_eq!(report["work_scope"]["max_components"], 10);
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
        .args(["--exact", "core::debug_log_panic_probe", "--nocapture"])
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
    let _session = reposcout::debug_log::Session::start(Some(Path::new(&log))).unwrap();
    panic!("deliberate debug-log panic probe");
}

#[test]
fn debug_log_heartbeat_records_liveness_during_quiet_work() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("heartbeat.jsonl");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "core::debug_log_heartbeat_probe", "--nocapture"])
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
    let _session = reposcout::debug_log::Session::start(Some(Path::new(&log))).unwrap();
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
