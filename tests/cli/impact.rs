use super::*;

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
