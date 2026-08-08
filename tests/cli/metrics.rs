use super::*;

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
    assert!(text.contains("Work scope"), "missing work-scope section");
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
    assert!(
        text.contains(
            "Tip: global config active — a project config can sharpen this report further."
        )
    );
}

#[test]
fn no_project_config_suppresses_human_configuration_guidance() {
    let mut cmd = reposcout_command();
    cmd.args([
        "--only",
        "lines",
        "--no-project-config",
        "--no-cache",
        "--quiet",
        "-f",
        "table",
        &fixture(),
    ]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();

    assert!(!text.contains("Configuration tip"));
    assert!(!text.contains("Tip: no config found"));
    assert!(!text.contains("Tip: global config active"));
}

#[test]
fn narrow_human_tables_front_truncate_paths_without_wrapping_the_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir
        .path()
        .join("packages/application/src/components/navigation");
    std::fs::create_dir_all(&nested).unwrap();
    let source_path = nested.join("command-globe-scene.tsx");
    let mut source =
        String::from("export function analyze(input: number): number {\n  let total = 0;\n");
    for value in 0..30 {
        let _ = writeln!(source, "  if (input > {value}) total += 1;");
    }
    source.push_str("  return total;\n}\n");
    std::fs::write(&source_path, source).unwrap();

    let repo = git2::Repository::init(dir.path()).unwrap();
    commit_all(&repo, "add long path");

    let mut command = reposcout_command();
    command.args([
        "--no-cache",
        "--quiet",
        "-f",
        "table",
        "--top",
        "1",
        dir.path().to_str().unwrap(),
    ]);
    let output = command.assert().success().get_output().stdout.clone();
    let rendered = String::from_utf8(output).unwrap();

    assert!(rendered.contains("Complexity violations"));
    assert!(rendered.contains("Hotspots (churn × complexity)"));
    assert!(rendered.contains("Top risks"));
    assert!(
        rendered
            .lines()
            .any(|line| line.contains("command-globe-scene.tsx:1")),
        "complexity locations should keep the path tail and line on one row:\n{rendered}"
    );
    assert!(
        rendered
            .lines()
            .any(|line| line.contains("command-globe-scene.tsx") && line.contains("31.0")),
        "hotspots should keep the filename on one row:\n{rendered}"
    );
    let risk_row = rendered
        .lines()
        .find(|line| line.contains("command-globe-scene.tsx") && line.contains("0.12"))
        .unwrap_or_else(|| {
            panic!("risk rows should keep an identifying path suffix on one row:\n{rendered}")
        });
    assert!(risk_row.contains("no matching test file"));
    let risk_score = risk_row
        .split_whitespace()
        .nth(2)
        .and_then(|score| score.trim().parse::<f64>().ok())
        .unwrap_or_else(|| panic!("risk row should contain a numeric score: {risk_row}"));
    assert!(
        (0.10..=0.14).contains(&risk_score),
        "fixture risk score changed unexpectedly: {risk_row}"
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
    assert!(text.contains("## Work scope"), "missing work-scope heading");
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

#[test]
fn fail_on_accepts_and_evaluates_every_supported_metric_name() {
    let mut cmd = reposcout_command();
    cmd.args([
        "--no-cache",
        "--quiet",
        "-f",
        "json",
        "--fail-on",
        "max-cyclomatic>1000000,avg-cyclomatic>1000000,max-cognitive>1000000,avg-cognitive>1000000,min-mi<-1,min-maintainability<-1,avg-mi>1000000,avg-maintainability>1000000,duplicated-pct>101,tokens>1000000000,files>1000000,sloc>1000000000",
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
            .is_some_and(|a| !a.is_empty())
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
        let extension = Path::new(p).extension().and_then(|value| value.to_str());
        assert!(
            !extension.is_some_and(
                |value| value.eq_ignore_ascii_case("md") || value.eq_ignore_ascii_case("json")
            ),
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
fn json_is_compact_by_default_and_pretty_only_when_requested() {
    let mut compact = reposcout_command();
    compact.args([
        "-f",
        "json",
        "--only",
        "tokens",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);
    let compact =
        String::from_utf8(compact.assert().success().get_output().stdout.clone()).unwrap();
    assert_eq!(compact.trim_end().lines().count(), 1);
    serde_json::from_str::<Value>(&compact).unwrap();

    let mut pretty = reposcout_command();
    pretty.args([
        "-f",
        "json",
        "--pretty",
        "--only",
        "tokens",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);
    let pretty = String::from_utf8(pretty.assert().success().get_output().stdout.clone()).unwrap();
    assert!(pretty.lines().count() > 1);
    assert!(pretty.starts_with("{\n  \"schema_version\""));
    serde_json::from_str::<Value>(&pretty).unwrap();
}

#[test]
fn pretty_rejects_non_json_output() {
    let mut command = reposcout_command();
    command.args([
        "-f",
        "table",
        "--pretty",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);
    command
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("--pretty requires JSON output"));

    let mut baseline = reposcout_command();
    baseline.args([
        "-f",
        "table",
        "--baseline-ready",
        "--pretty",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);
    baseline
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "--baseline-ready requires JSON output",
        ));
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
fn context_fit_uses_readable_source_tokens_without_hiding_total_inventory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("catalog.txt"), "entry ".repeat(220_000)).unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--only",
        "tokens",
        dir.path().to_str().unwrap(),
    ]);

    assert!(report["summary"]["tokens"].as_u64().unwrap() > 200_000);
    assert!(report["summary"]["source"]["tokens"].as_u64().unwrap() < 200_000);
    assert_eq!(report["summary"]["assessment"]["fits_context"], true);
    assert!(
        report["summary"]["assessment"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason.as_str().unwrap().contains("source tokens")),
        "assessment should explain that context fit is source-based"
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
