use super::*;

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
    assert!(report.get("work_scope").is_none());

    let summary = run_json(&["-f", "json", "--summary", &fixture()]);
    assert!(summary.get("finding_catalog").is_none());
    assert!(summary["work_scope"].is_object());
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
fn focused_baseline_accepts_legacy_profile_without_health_metadata() {
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

    let mut baseline: Value = serde_json::from_slice(&std::fs::read(&tmp_path).unwrap()).unwrap();
    baseline["analysis_profile"]
        .as_object_mut()
        .unwrap()
        .remove("health");
    std::fs::write(&tmp_path, serde_json::to_vec(&baseline).unwrap()).unwrap();

    let report = run_json(&["tokens", "-f", "json", "--baseline", &tmp_path, &fix]);
    assert_eq!(report["baseline"]["regressed"], false);
}

#[test]
fn focused_legacy_baseline_rejects_new_health_path_excludes() {
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

    let mut baseline: Value = serde_json::from_slice(&std::fs::read(&tmp_path).unwrap()).unwrap();
    baseline["analysis_profile"]
        .as_object_mut()
        .unwrap()
        .remove("health");
    std::fs::write(&tmp_path, serde_json::to_vec(&baseline).unwrap()).unwrap();

    let mut compare = reposcout_command();
    compare.args([
        "tokens",
        "-f",
        "json",
        "--health-exclude",
        "app.js",
        "--baseline",
        &tmp_path,
        "--no-cache",
        "--quiet",
        &fix,
    ]);
    let error = String::from_utf8(compare.assert().code(1).get_output().stderr.clone()).unwrap();
    assert!(
        error.contains("baseline analyzer profile does not match"),
        "error was: {error}"
    );
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
fn baseline_rejects_mismatched_duplication_artifact_policy() {
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
        "--dup-include-artifacts",
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
