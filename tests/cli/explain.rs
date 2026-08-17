use super::*;

#[test]
fn explain_json_combines_file_findings_tests_and_graph_context() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"devDependencies":{"vitest":"latest"}}"#,
    )
    .unwrap();
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
    assert_eq!(report["risk"]["algorithm_version"], 5);
    assert!(report["risk"].get("untested").is_none());
    assert!(report["risk"].get("untested_multiplier").is_none());
    assert_eq!(report["repository"]["source_files"], 4);
    assert_eq!(report["repository"]["test_files"], 1);
    assert_eq!(report["testing"]["classification"], "source");
    assert_eq!(report["testing"]["frameworks"][0]["name"], "vitest");
    assert!(report["testing"].get("tested").is_none());
    assert!(report["testing"].get("matches").is_none());
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
fn explain_without_a_configured_runner_keeps_source_inventory_separate() {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )
    .unwrap();

    let mut cmd = reposcout_command();
    cmd.args([
        "explain",
        dir.path().join("src/lib.rs").to_str().unwrap(),
        "-f",
        "json",
        "--no-cache",
        "--quiet",
    ]);
    let report: Value =
        serde_json::from_slice(&cmd.assert().success().get_output().stdout).unwrap();

    assert_eq!(report["repository"]["source_files"], 1);
    assert!(report["repository"].get("test_files").is_none());
    assert_eq!(report["testing"]["classification"], "unavailable");
    assert!(report["risk"]["score"].is_number());
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
