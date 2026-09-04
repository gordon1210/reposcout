use super::*;

#[test]
fn agent_summary_is_a_bounded_standalone_json_contract() {
    let mut command = reposcout_command();
    command
        .args(["--agent-summary", &fixture()])
        .arg("--no-cache")
        .arg("--quiet");
    let stdout = command.assert().success().get_output().stdout.clone();
    assert!(stdout.len() <= 16 * 1024);
    assert!(stdout.ends_with(b"\n"));
    let report: Value = serde_json::from_slice(&stdout).unwrap();

    assert_eq!(report["schema_version"], "2.0");
    assert_eq!(report["report_kind"], "agent-summary");
    assert_eq!(report["projection"]["strategy_version"], 1);
    assert_eq!(report["projection"]["max_bytes"], 16 * 1024);
    assert_eq!(report["interpretation"]["profile"], "agent");
    assert_eq!(report["interpretation"]["analyzers"]["duplication"], false);
    assert_eq!(report["interpretation"]["analyzers"]["churn"], false);
    assert!(report.get("summary").is_none());
    assert!(report.get("files").is_none());
    assert!(report.get("duplicates").is_none());
    assert!(report.get("finding_catalog").is_none());
    assert!(report["signals"].get("duplication").is_none());
    assert!(report["coverage"].get("type2_analysis_partial").is_none());
    assert!(report["coverage"].get("churn_analysis_partial").is_none());
    assert!(report["coverage"].get("churn_deltas_omitted").is_none());
    assert!(report["coverage"].get("graph").is_none());
    assert!(report["coverage"]["primary"]["analyzed_files"].is_number());
    assert!(report["inventory"]["source"]["tokens"].is_number());
    assert!(
        report["signals"]["top_source_files"]["entries"]
            .as_array()
            .unwrap()
            .len()
            <= 3
    );
}

#[test]
fn agent_summary_respects_an_explicit_full_profile() {
    let report = run_json(&[
        "--agent-summary",
        "--profile",
        "full",
        "--top",
        "20",
        &fixture(),
    ]);

    assert_eq!(report["interpretation"]["profile"], "full");
    assert_eq!(report["interpretation"]["analyzers"]["duplication"], true);
    assert_eq!(report["interpretation"]["analyzers"]["churn"], true);
    assert!(report["signals"]["duplication"].is_object());
    assert!(report["coverage"]["type2_analysis_partial"].is_boolean());
    assert_eq!(report["coverage"]["churn_analysis_partial"], false);
    assert_eq!(report["coverage"]["churn_deltas_omitted"], 0);
    assert!(
        report["signals"]["duplication"]["top_blocks"]["shown"]
            .as_u64()
            .unwrap()
            <= 3
    );
}

#[test]
fn agent_summary_separates_direct_context_evidence_from_expansion() {
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
        "--agent-summary",
        "--focus",
        "focus.js",
        "--context-budget",
        "1000",
        "--context-max-files",
        "5",
        dir.path().to_str().unwrap(),
    ]);
    let context = &report["context"];
    let direct = context["direct_evidence"]["entries"].as_array().unwrap();

    assert_eq!(context["strategy_version"], 3);
    assert_eq!(context["budget"]["selected_files"], 5);
    assert_eq!(context["evidence"]["seed_files"], 1);
    assert_eq!(context["evidence"]["graph_eligible_seed_files"], 1);
    assert_eq!(context["evidence"]["graph_covered_seed_files"], 1);
    assert!(report["coverage"]["graph"].is_object());
    assert_eq!(
        context["direct_evidence"]["available_tokens"],
        direct
            .iter()
            .map(|entry| entry["tokens"].as_u64().unwrap())
            .sum::<u64>()
    );
    assert_eq!(
        context["expand_if_needed"]["available_tokens"],
        context["expand_if_needed"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["tokens"].as_u64().unwrap())
            .sum::<u64>()
    );
    assert!(direct.iter().any(|entry| {
        entry["path"] == "focus.js"
            && entry["evidence"]
                .as_array()
                .unwrap()
                .iter()
                .any(|evidence| evidence["role"] == "focus")
    }));
    assert!(direct.iter().any(|entry| entry["path"] == "dependency.js"));
    assert!(direct.iter().any(|entry| entry["path"] == "dependent.js"));
    assert!(direct.iter().any(|entry| entry["path"] == "focus.test.js"));
    assert!(
        context["expand_if_needed"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"] == "unrelated.js")
    );
}

#[test]
fn agent_summary_keeps_unseeded_context_as_bounded_expansion() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..6 {
        std::fs::write(
            dir.path().join(format!("source-{index}.js")),
            format!("export const value{index} = {index};\n"),
        )
        .unwrap();
    }

    let report = run_json(&[
        "--agent-summary",
        "--context-budget",
        "10000",
        "--context-max-files",
        "6",
        dir.path().to_str().unwrap(),
    ]);
    let context = &report["context"];
    let expansion = &context["expand_if_needed"];

    assert_eq!(context["evidence"]["seed_files"], 0);
    assert_eq!(context["direct_evidence"]["available"], 0);
    assert_eq!(context["direct_evidence"]["shown"], 0);
    assert_eq!(expansion["available"], context["budget"]["selected_files"]);
    assert_eq!(expansion["available"], 6);
    assert_eq!(expansion["shown"], 3);
    assert_eq!(expansion["omitted"], 3);
}

#[test]
fn agent_summary_keeps_oversized_focus_as_outline_first_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = String::from("export function large() {\n  let total = 0;\n");
    for index in 0..64 {
        writeln!(source, "  total += {index};").unwrap();
    }
    source.push_str("  return total;\n}\n");
    std::fs::write(dir.path().join("large.js"), source).unwrap();

    let report = run_json(&[
        "--agent-summary",
        "--focus",
        "large.js",
        "--context-budget",
        "1",
        "--context-max-files",
        "3",
        dir.path().to_str().unwrap(),
    ]);
    let context = &report["context"];
    let outlines = context["outline_only"]["entries"].as_array().unwrap();

    assert_eq!(context["evidence"]["seed_files"], 1);
    assert_eq!(context["direct_evidence"]["available"], 0);
    assert_eq!(context["outline_only"]["available"], 1);
    assert_eq!(context["outline_only"]["shown"], 1);
    assert!(outlines.iter().any(|entry| {
        entry["path"] == "large.js"
            && entry["evidence"]
                .as_array()
                .unwrap()
                .iter()
                .any(|evidence| evidence["role"] == "focus")
    }));
}

#[test]
fn agent_summary_rejects_unbounded_or_incompatible_rendering_requests() {
    for args in [
        vec!["--agent-summary", "--pretty", &fixture()],
        vec!["--agent-summary", "-f", "table", &fixture()],
        vec!["--agent-summary", "--graph", &fixture()],
    ] {
        let mut command = reposcout_command();
        command.args(args).arg("--no-cache").arg("--quiet");
        let stderr = command.assert().code(2).get_output().stderr.clone();
        let error = String::from_utf8(stderr).unwrap();
        assert!(error.contains("--agent-summary"), "error was: {error}");
    }
}

#[test]
fn agent_summary_is_rejected_on_analyzer_subcommands() {
    let mut command = reposcout_command();
    command.args([
        "tokens",
        "--agent-summary",
        &fixture(),
        "--no-cache",
        "--quiet",
    ]);
    let stderr = command.assert().code(2).get_output().stderr.clone();
    let error = String::from_utf8(stderr).unwrap();

    assert!(error.contains("available only on the default scan command"));
}
