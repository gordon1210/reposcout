use super::*;

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
        assert!(
            (g["similarity"].as_f64().unwrap() - 1.0).abs() < f64::EPSILON,
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
    assert!(
        brief["summary"]["assessment"]["production_duplication"].is_object(),
        "--summary must retain explicit production duplication evidence"
    );
}

#[test]
fn production_duplicates_are_actionable_in_compact_and_human_reports() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".reposcout.toml"),
        "min_dup_tokens = 8\nmin_dup_lines = 3\nnear_dup_min_similarity = 1.0\n",
    )
    .unwrap();
    let duplicate = r"
pub fn repeated_business_rule(values: &[i32]) -> i32 {
    let mut total = 0;
    for value in values {
        if *value > 0 {
            total += value * 2;
        } else {
            total -= value.abs();
        }
    }
    total
}
";
    std::fs::write(dir.path().join("first.rs"), duplicate).unwrap();
    std::fs::write(dir.path().join("second.rs"), duplicate).unwrap();

    let report = run_json(&["-f", "json", "--summary", dir.path().to_str().unwrap()]);
    let production = &report["summary"]["assessment"]["production_duplication"];
    assert_eq!(production["corpus"], "production-source");
    assert!(production["duplicated_lines"].as_u64().unwrap() > 0);
    assert!(production["analyzed_lines"].as_u64().unwrap() > 0);
    assert_eq!(production["complete"], true);
    assert!(
        !report["summary"]["top_production_duplicates"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    for format in ["table", "markdown"] {
        let mut cmd = reposcout_command();
        cmd.args([
            "--no-cache",
            "--quiet",
            "-f",
            format,
            dir.path().to_str().unwrap(),
        ]);
        let output = cmd.assert().success().get_output().stdout.clone();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("Production duplication"));
        assert!(text.contains("Top production duplicates"));
        assert!(text.contains("Top risks · algorithm 5"));
    }
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
    let tests = r"
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
";
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
    let production = &report["summary"]["assessment"]["production_duplication"];
    assert_eq!(production["corpus"], "production-source");
    assert_eq!(production["duplicated_lines"], 0);
    assert_eq!(production["duplicated_pct"], 0.0);
    assert_eq!(production["complete"], true);
    assert!(
        report["summary"].get("top_production_duplicates").is_none(),
        "inline-test-only families must not enter the production projection"
    );
    assert_eq!(&report["work_scope"]["production_duplication"], production);
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
fn large_complex_file_uses_continuous_risk_without_coverage_claims() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("risky.rs");
    let mut source = String::from("pub fn risky(input: i32) -> i32 {\n    let mut value = 0;\n");
    for index in 0..70 {
        let _ = writeln!(source, "    if input > {index} {{ value += 1; }}");
    }
    for index in 0..700 {
        let _ = writeln!(source, "    let _padding_{index} = {index};");
    }
    source.push_str("    value\n}\n");
    std::fs::write(&file, source).unwrap();

    let report = run_json(&["--only", "complexity", "-f", "json", file.to_str().unwrap()]);
    let risk = &report["summary"]["top_risks"][0];
    let score = risk["score"].as_f64().unwrap();
    assert_eq!(risk["algorithm_version"], 5);
    assert!(score > 0.340, "risk was: {risk:?}");
    assert!(
        score < 0.341,
        "continuous half-saturation score changed unexpectedly: {risk:?}"
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
