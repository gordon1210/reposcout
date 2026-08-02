use super::*;

#[test]
fn reposcoutignore_excludes_matching_files() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("reposcout_test_ignore_{pid}_{nanos}"));
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
    let v: Value = serde_json::from_slice(&output).unwrap();

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
    assert_eq!(
        diagnostics["unsupported_samples"],
        serde_json::json!(["notes.unknown"])
    );
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

#[test]
fn invalid_health_exclude_glob_fails_before_scanning() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("sample.rs"), "fn sample() {}\n").unwrap();

    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--health-exclude",
        "[",
        "--no-cache",
        "--quiet",
        dir.path().to_str().unwrap(),
    ]);
    let output = cmd.assert().failure().get_output().stderr.clone();
    let error = String::from_utf8(output).unwrap();

    assert!(
        error.contains("invalid health exclude glob"),
        "error was: {error}"
    );
}
