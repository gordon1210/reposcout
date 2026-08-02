use super::*;

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
    assert_eq!(report["work_scope"]["basis"], serde_json::json!(["diff"]));
    assert_eq!(report["work_scope"]["diff_scope"], "working");
    assert_eq!(report["work_scope"]["seeds"]["changes"]["total"], 2);
    assert!(
        report["work_scope"]["context"]["selected_files"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(report["work_scope"]["impact"]["seed_files"], 2);
    assert_eq!(
        report["work_scope"]["impact"]["graph_eligible_seed_files"],
        1
    );
    assert_eq!(
        report["work_scope"]["impact"]["graph_covered_seed_files"],
        1
    );
    assert_eq!(report["work_scope"]["impact"]["matching_tests"], 1);
    assert_eq!(report["work_scope"]["impact"]["matching_tests_known"], true);
    assert_eq!(report["work_scope"]["structure"]["graph_files"], 4);
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
    assert_eq!(
        full["work_scope"]["production_duplication"]["corpus"],
        "production-source"
    );
    assert_eq!(
        full["work_scope"]["production_duplication"]["complete"],
        true
    );

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
    assert!(safe["work_scope"].get("production_duplication").is_none());
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
    assert!(record["work_scope"].is_object());

    let output_path = dir.path().join("change-summary.ndjson");
    let mut ndjson_file = reposcout_command();
    ndjson_file.args([
        "-f",
        "ndjson",
        "--working",
        "--change-summary",
        "--no-cache",
        "--quiet",
        "-o",
        output_path.to_str().unwrap(),
        path,
    ]);
    ndjson_file.assert().success();
    let rendered = std::fs::read_to_string(output_path).unwrap();
    assert!(
        rendered.ends_with('\n'),
        "NDJSON files must end with a record delimiter"
    );
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
    assert_eq!(
        report["work_scope"]["confidence"]["primary"]["oversized_files"],
        1
    );
    assert_eq!(
        report["work_scope"]["confidence"]["primary"]["truncated"],
        true
    );
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
    // Both JSON contracts are compact by default; the dedicated decision
    // projection must still cost no more than half of detailed change output.
    assert!(
        concise_bytes.len() * 2 <= detailed_bytes.len(),
        "expected at least 50% fewer bytes; detailed={}, concise={}",
        detailed_bytes.len(),
        concise_bytes.len()
    );
}
