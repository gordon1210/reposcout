#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration tests intentionally fail immediately when fixtures or assertions are invalid"
)]

#[path = "support/command.rs"]
mod test_command;

use reposcout::metrics::tokens::TokenCounter;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use test_command::reposcout_command;

fn fixture() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("network")).unwrap();
    fs::write(
        directory.path().join("network/http_client.rs"),
        r#"pub struct TransportEnvelope;

/// jitterbackoffcomment describes retry behavior.
pub fn dispatch_request(envelope: TransportEnvelope) -> usize {
    let internal_probe_token = "SECRET_BODY_NEVER_OUTPUT";
    std::mem::size_of_val(&envelope) + internal_probe_token.len()
}

pub fn path_only_candidate() -> usize {
    1
}
"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("a.rs"),
        "pub fn collide() -> usize { 1 }\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("b.rs"),
        "pub fn collide() -> usize { 2 }\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("terms.rs"),
        r"pub fn alpha_only() { let alpha_needlex = 1; }
pub fn beta_only() { let beta_needlex = 1; }
pub fn both_terms() {
    let alpha_needlex = 1;
    let beta_needlex = 2;
}
",
    )
    .unwrap();
    fs::write(
        directory.path().join("filtered.py"),
        "def filtered_target():\n    return 'filter_needlex'\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("filtered.rs"),
        "pub fn filtered_target() -> &'static str { \"filter_needlex\" }\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("unsupported.yaml"),
        "filter_needlex: true\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("parse_error.rs"),
        "pub fn partial_parse() { let parse_needlex = ;\n",
    )
    .unwrap();
    let long_code = format!(
        "pub fn bounded_code() {{\n    let visible_needlex = 1;\n    let padding = \"{}\";\n    let late_needlex = 2;\n}}\n",
        "a".repeat(5_000)
    );
    fs::write(directory.path().join("long.rs"), long_code).unwrap();
    directory
}

fn run_find(root: &Path, query: &str, arguments: &[&str]) -> Value {
    let output = reposcout_command()
        .arg("find")
        .arg(query)
        .arg(root)
        .args(arguments)
        .arg("--format")
        .arg("json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
}

fn matched_fields(report: &Value) -> Vec<&str> {
    report["hits"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|hit| hit["matched_fields"].as_array().unwrap())
        .filter_map(|evidence| evidence["field"].as_str())
        .collect()
}

#[test]
fn capabilities_match_the_find_defaults_fields_and_limits() {
    let directory = fixture();
    let capability = reposcout::query::capabilities().find_query.unwrap();
    let report = run_find(directory.path(), "collide", &[]);

    assert_eq!(capability.command, "find");
    assert_eq!(capability.formats, ["table", "json", "markdown", "ndjson"]);
    assert_eq!(capability.match_modes, ["all", "any"]);
    assert_eq!(capability.default_match_mode, "all");
    assert_eq!(
        capability.fields,
        ["name", "path", "signature", "comment", "code"]
    );
    assert_eq!(report["match_mode"], capability.default_match_mode);
    assert_eq!(report["limit"], capability.default_limit);
    assert_eq!(report["token_budget"], capability.default_tokens);
    assert_eq!(report["byte_budget"], capability.default_bytes);
    assert_eq!(capability.max_limit, 100);
    assert_eq!(
        (capability.max_query_chars, capability.max_query_terms),
        (512, 16)
    );
    assert_eq!(
        (
            capability.min_tokens,
            capability.max_tokens,
            capability.min_bytes,
            capability.max_bytes,
        ),
        (256, 65_536, 1_024, 1_048_576)
    );
    assert_eq!(capability.max_definitions_per_file, 2_048);
    assert_eq!(capability.max_terms_per_field, 128);
    assert_eq!(capability.max_term_chars, 128);
    assert_eq!(capability.max_code_bytes_per_definition, 4_096);
    assert_eq!(capability.max_comment_bytes_per_definition, 2_048);
    assert_eq!(capability.max_comment_nodes_per_file, 4_096);
    assert_eq!(capability.max_syntax_nodes_per_file, 100_000);
}

#[test]
fn all_search_fields_are_attributed_without_returning_source_bodies() {
    let directory = fixture();
    let cases = [
        ("dispatch_request", "name"),
        ("http_client.rs", "path"),
        ("TRANSPORT envelope", "signature"),
        ("jitterbackoffcomment", "comment"),
        ("internal_probe_token", "code"),
    ];
    for (query, expected_field) in cases {
        let report = run_find(directory.path(), query, &[]);
        assert!(
            matched_fields(&report).contains(&expected_field),
            "missing {expected_field} evidence for {query}: {report:?}"
        );
        assert_eq!(report["kind"], "find_query");
        assert_eq!(report["match_mode"], "all");
        assert!(report["returned_matches"].as_u64().unwrap() > 0);
        assert!(!report.to_string().contains("SECRET_BODY_NEVER_OUTPUT"));
    }
}

#[test]
fn match_modes_filters_and_stable_ties_have_deterministic_results() {
    let directory = fixture();
    let all = run_find(directory.path(), "alpha beta", &["--match", "all"]);
    assert_eq!(all["returned_matches"], 1);
    assert_eq!(all["hits"][0]["name"], "both_terms");

    let any = run_find(directory.path(), "alpha beta", &["--match", "any"]);
    assert_eq!(any["returned_matches"], 3);

    let filtered = run_find(
        directory.path(),
        "filter_needlex",
        &["--language", "Python", "--kind", "function"],
    );
    assert_eq!(filtered["returned_matches"], 1);
    assert_eq!(filtered["hits"][0]["path"], "filtered.py");
    assert_eq!(filtered["hits"][0]["language"], "Python");

    let first = run_find(directory.path(), "collide", &[]);
    let second = run_find(directory.path(), "collide", &[]);
    assert_eq!(first, second);
    assert_eq!(first["hits"][0]["path"], "a.rs");
    assert_eq!(first["hits"][1]["path"], "b.rs");
    assert_eq!(first["hits"][0]["score"], first["hits"][1]["score"]);
}

#[test]
fn coverage_and_output_omissions_are_independent_and_hard_bounded() {
    let directory = fixture();
    let coverage = run_find(directory.path(), "late_needlex", &[]);
    assert_eq!(coverage["returned_matches"], 0);
    assert!(coverage["coverage"]["unsupported_files"].as_u64().unwrap() >= 1);
    assert!(coverage["coverage"]["parse_error_files"].as_u64().unwrap() >= 1);
    assert!(
        coverage["coverage"]["field_truncated_files"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert!(
        coverage["coverage"]["truncated_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["field"] == "code")
    );

    let limited = run_find(directory.path(), "pub", &["--match", "any", "--limit", "1"]);
    assert_eq!(limited["returned_matches"], 1);
    assert!(limited["limit_omitted"].as_u64().unwrap() > 0);
    assert_eq!(limited["budget_omitted"], 0);

    let output = reposcout_command()
        .arg("find")
        .arg("pub")
        .arg(directory.path())
        .args([
            "--match",
            "any",
            "--budget",
            "256",
            "--max-output-bytes",
            "1024",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(output.len() <= 1_024);
    let rendered = std::str::from_utf8(&output).unwrap();
    assert!(TokenCounter::new("o200k_base").unwrap().count(rendered) <= 256);
    let budgeted: Value = serde_json::from_slice(&output).unwrap();
    assert!(budgeted["budget_omitted"].as_u64().unwrap() > 0);
}

#[test]
fn coverage_includes_unknown_extensions_without_counting_recognized_unsupported_twice() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("code.rs"), "fn found() {}\n").unwrap();
    fs::write(directory.path().join("data.yaml"), "value: true\n").unwrap();
    fs::write(directory.path().join("unknown.xyz"), "unindexed\n").unwrap();
    fs::write(directory.path().join("unreadable.rs"), [0xff, 0xfe]).unwrap();

    let report = run_find(directory.path(), "found", &[]);
    assert_eq!(report["coverage"]["files_total"], 4);
    assert_eq!(report["coverage"]["files_inspected"], 1);
    assert_eq!(report["coverage"]["unsupported_files"], 2);
    assert_eq!(report["coverage"]["unavailable_files"], 1);
    assert_eq!(report["returned_matches"], 1);
}

#[test]
fn exact_filename_matches_even_when_parent_paths_exhaust_lexical_terms() {
    let directory = tempfile::tempdir().unwrap();
    let mut parent = directory.path().to_path_buf();
    for index in 0..70 {
        parent.push(format!("segment{index}"));
    }
    fs::create_dir_all(&parent).unwrap();
    fs::write(parent.join("needle.rs"), "fn unrelated() {}\n").unwrap();

    let report = run_find(directory.path(), "needle.rs", &[]);
    assert_eq!(report["total_matches"], 1);
    assert_eq!(report["returned_matches"], 1);
    assert_eq!(report["coverage"]["field_truncated_files"], 1);
    let evidence = report["hits"][0]["matched_fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["field"] == "path")
        .unwrap();
    assert_eq!(evidence["exact"], true);
    assert_eq!(evidence["terms"], serde_json::json!(["needle", "rs"]));
}

#[test]
fn health_exclusions_do_not_remove_navigation_candidates() {
    let directory = fixture();
    let report = run_find(
        directory.path(),
        "internal_probe_token",
        &["--health-exclude", "network/**"],
    );
    assert!(
        report["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hit| hit["path"] == "network/http_client.rs")
    );
}

#[test]
fn read_handoff_is_hash_checked_and_becomes_stale_after_edit() {
    let directory = fixture();
    let report = run_find(directory.path(), "dispatch_request", &["--limit", "1"]);
    let read = &report["hits"][0]["read"];
    let path = read["path"].as_str().unwrap();
    let symbol = read["selector"]["value"].as_str().unwrap();
    let hash = read["expected_hash"].as_str().unwrap();

    let fresh = reposcout_command()
        .arg("read")
        .arg(directory.path())
        .args([
            "--snapshot",
            "worktree",
            "--symbol",
            path,
            symbol,
            "--expect-hash",
            path,
            hash,
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let fresh: Value = serde_json::from_slice(&fresh).unwrap();
    assert_eq!(fresh["results"][0]["status"], "complete");

    fs::write(
        directory.path().join(path),
        "pub fn dispatch_request() -> usize { 99 }\n",
    )
    .unwrap();
    let stale = reposcout_command()
        .arg("read")
        .arg(directory.path())
        .args([
            "--snapshot",
            "worktree",
            "--symbol",
            path,
            symbol,
            "--expect-hash",
            path,
            hash,
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stale: Value = serde_json::from_slice(&stale).unwrap();
    assert_eq!(stale["results"][0]["status"], "stale");
    assert!(
        stale
            .get("sources")
            .is_none_or(|sources| sources.as_array().is_some_and(Vec::is_empty))
    );
}

#[test]
fn edited_content_invalidates_cached_lexical_facts_without_changing_locate() {
    let directory = fixture();
    let path = directory.path().join("cache.rs");
    fs::write(&path, "pub fn cached() { let cache_before_needlex = 1; }\n").unwrap();
    assert_eq!(
        run_find(directory.path(), "cache_before_needlex", &[])["returned_matches"],
        1
    );

    fs::write(&path, "pub fn cached() { let cache_after_needlex = 1; }\n").unwrap();
    assert_eq!(
        run_find(directory.path(), "cache_before_needlex", &[])["returned_matches"],
        0
    );
    assert_eq!(
        run_find(directory.path(), "cache_after_needlex", &[])["returned_matches"],
        1
    );

    reposcout_command()
        .args(["locate", "cached", directory.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicates::str::contains("cached"));
}
