use super::*;

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

    assert_eq!(report["work_scope"]["basis"], serde_json::json!(["diff"]));
    assert_eq!(report["work_scope"]["seeds"]["changes"]["total"], 1);
    assert!(report["work_scope"].get("context").is_none());
    assert!(report["work_scope"].get("impact").is_none());
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
    index.add_path(Path::new("work.py")).unwrap();
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
    let block = (0..30).fold(String::new(), |mut block, index| {
        let _ = writeln!(block, "const value{index} = input + {index};");
        block
    });
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
fn deep_review_excludes_health_filtered_files_from_duplication() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let block = (0..30).fold(String::new(), |mut block, index| {
        let _ = writeln!(block, "const value{index} = input + {index};");
        block
    });
    std::fs::write(dir.path().join("original.js"), &block).unwrap();
    std::fs::write(
        dir.path().join("copy.js"),
        "export const initiallyUnique = true;\n",
    )
    .unwrap();
    commit_all(&repo, "initial duplication exclusion fixture");
    std::fs::write(dir.path().join("copy.js"), &block).unwrap();

    let report = run_json(&[
        "--working",
        "--review=deep",
        "--only",
        "duplication",
        "--health-exclude",
        "copy.js",
        "-f",
        "json",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(report["review"]["counts"]["new"], 0);
    assert!(
        report["review"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["finding"]["kind"] != "duplication")
    );
}

#[test]
fn deep_review_excludes_build_artifacts_unless_explicitly_included() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let chunk_dir = dir.path().join("static/js");
    std::fs::create_dir_all(&chunk_dir).unwrap();
    let block = (0..30).fold(String::new(), |mut block, index| {
        let _ = writeln!(block, "const value{index} = input + {index};");
        block
    });
    std::fs::write(dir.path().join("original.js"), &block).unwrap();
    let chunk = chunk_dir.join("main.a1b2c3.chunk.js");
    std::fs::write(&chunk, "export const initiallyUnique = true;\n").unwrap();
    commit_all(&repo, "initial build artifact review fixture");
    std::fs::write(&chunk, &block).unwrap();

    let default = run_json(&[
        "--working",
        "--review=deep",
        "--only",
        "duplication",
        "-f",
        "json",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(default["review"]["counts"]["new"], 0);

    let included = run_json(&[
        "--working",
        "--review=deep",
        "--only",
        "duplication",
        "--dup-include-artifacts",
        "-f",
        "json",
        dir.path().to_str().unwrap(),
    ]);
    assert!(
        included["review"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| {
                finding["state"] == "new" && finding["finding"]["kind"] == "duplication"
            }),
        "review findings were: {}",
        included["review"]["findings"]
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
            index.add_path(Path::new("new.py")).unwrap();
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
