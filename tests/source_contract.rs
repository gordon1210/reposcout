#![cfg(unix)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests intentionally fail immediately for invalid fixtures or assertions"
)]

use reposcout::config::Config;
use reposcout::metrics::tokens::TokenCounter;
use reposcout::model::{SourceQueryReport, SourceQueryStatus};
use reposcout::query::{SourceQueryOptions, SourceQueryTarget, SourceSelector, read_source};
use reposcout::report::Format;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

fn options(path: &str, names: &[&str]) -> SourceQueryOptions {
    SourceQueryOptions {
        targets: names
            .iter()
            .map(|name| SourceQueryTarget {
                path: PathBuf::from(path),
                selector: SourceSelector::Symbol((*name).to_string()),
                expected_hash: None,
            })
            .collect(),
        token_budget: 2_048,
        byte_budget: 4_096,
        format: Format::Json,
        pretty_json: false,
    }
}

fn config() -> Config {
    Config {
        use_cache: false,
        encoding: "cl100k_base".to_string(),
        ..Config::default()
    }
}

fn assert_exact_sources(report: &SourceQueryReport, original: &str) {
    for source in &report.sources {
        assert_eq!(
            source.content,
            original[source.span.start_byte..source.span.end_byte]
        );
    }
    for result in report
        .results
        .iter()
        .filter(|result| result.status == SourceQueryStatus::Complete)
    {
        let definition = result.definition.as_ref().unwrap();
        let span = definition.source_span.unwrap();
        let source = report
            .sources
            .iter()
            .find(|source| Some(source.id) == result.source)
            .unwrap();
        assert!(source.span.start_byte <= span.start_byte && source.span.end_byte >= span.end_byte);
    }
}

#[test]
fn overlapping_definitions_share_complete_source_while_large_definition_is_omitted() {
    let directory = tempfile::tempdir().unwrap();
    let source = format!(
        "class Container:\n    def run(self):\n        return 7\n\ndef giant():\n    return {:?}\n",
        "oversized_value_".repeat(800)
    );
    fs::write(directory.path().join("example.py"), &source).unwrap();
    let query = options("example.py", &["Container", "Container.run", "giant"]);
    let output = read_source(directory.path(), &config(), &[], &query).unwrap();

    assert_eq!(output.report.results.len(), 3);
    assert_eq!(output.report.results[0].status, SourceQueryStatus::Complete);
    assert_eq!(output.report.results[1].status, SourceQueryStatus::Complete);
    assert_eq!(
        output.report.results[2].status,
        SourceQueryStatus::BudgetOmitted
    );
    assert_eq!(
        output.report.results[0].source,
        output.report.results[1].source
    );
    assert_eq!(output.report.sources.len(), 1);
    assert!(!output.rendered.contains("oversized_value_"));
    assert!(output.rendered.len() <= query.byte_budget);
    assert!(
        TokenCounter::new("cl100k_base")
            .unwrap()
            .count(&output.rendered)
            <= query.token_budget
    );
    assert_exact_sources(&output.report, &source);
}

#[test]
fn same_line_siblings_remain_ambiguous_while_nested_line_selection_is_precise() {
    let directory = tempfile::tempdir().unwrap();
    let source =
        "fn first() {} fn second() {}\nfn outer() {\n    fn inner() { let value = 1; }\n}\n";
    fs::write(directory.path().join("lib.rs"), source).unwrap();
    let mut query = options("lib.rs", &[]);
    query.targets = [1, 3]
        .into_iter()
        .map(|line| SourceQueryTarget {
            path: PathBuf::from("lib.rs"),
            selector: SourceSelector::Line(line),
            expected_hash: None,
        })
        .collect();
    let output = read_source(directory.path(), &config(), &[], &query).unwrap();

    assert_eq!(
        output.report.results[0].status,
        SourceQueryStatus::Ambiguous
    );
    assert_eq!(output.report.results[0].total_candidates, 2);
    assert_eq!(output.report.results[1].status, SourceQueryStatus::Complete);
    assert_eq!(output.report.sources.len(), 1);
    assert_eq!(
        output.report.sources[0].content,
        "fn inner() { let value = 1; }"
    );
    assert_exact_sources(&output.report, source);
}

#[test]
fn many_same_line_siblings_have_bounded_candidates_and_exact_ambiguity_count() {
    let directory = tempfile::tempdir().unwrap();
    let source = (0..256).fold(String::new(), |mut source, index| {
        write!(source, "fn sibling_{index}() {{}} ").unwrap();
        source
    });
    fs::write(directory.path().join("lib.rs"), source).unwrap();
    let mut query = options("lib.rs", &[]);
    query.targets.push(SourceQueryTarget {
        path: PathBuf::from("lib.rs"),
        selector: SourceSelector::Line(1),
        expected_hash: None,
    });
    let output = read_source(directory.path(), &config(), &[], &query).unwrap();
    let result = &output.report.results[0];
    assert_eq!(result.status, SourceQueryStatus::Ambiguous);
    assert_eq!(result.total_candidates, 256);
    assert_eq!(result.candidates.len(), 8);
    assert_eq!(result.omitted_candidates, 248);
    assert!(output.report.sources.is_empty());
}

#[test]
fn captured_hash_and_source_are_stable_warm_and_change_together_after_an_edit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("lib.rs");
    let original = "fn café() -> &'static str {\r\n    \"first\"\r\n}\r\n";
    fs::write(&path, original).unwrap();
    let mut cfg = config();
    cfg.use_cache = true;
    let mut query = options("lib.rs", &["café"]);
    let cold = read_source(directory.path(), &cfg, &[], &query).unwrap();
    let warm = read_source(directory.path(), &cfg, &[], &query).unwrap();
    assert_eq!(cold.rendered, warm.rendered);
    assert_exact_sources(&cold.report, original);
    query.targets[0].expected_hash = cold.report.files[0].sha256.clone();
    let replacement = original.replace("first", "second");
    fs::write(&path, &replacement).unwrap();
    let stale = read_source(directory.path(), &cfg, &[], &query).unwrap();
    assert_eq!(stale.report.results[0].status, SourceQueryStatus::Stale);
    assert!(stale.report.sources.is_empty());
    assert_ne!(stale.report.files[0].sha256, cold.report.files[0].sha256);
    query.targets[0].expected_hash = None;
    let changed = read_source(directory.path(), &cfg, &[], &query).unwrap();
    assert_exact_sources(&changed.report, &replacement);
    assert!(changed.report.sources[0].content.contains("second"));
}

#[test]
fn shared_query_applies_one_file_expectation_to_every_target_and_rejects_conflicts() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("lib.rs"),
        "fn first() {}\nfn second() {}\n",
    )
    .unwrap();
    let mut query = options("lib.rs", &["first", "second"]);
    query.targets[0].expected_hash = Some("0".repeat(64));
    let output = read_source(directory.path(), &config(), &[], &query).unwrap();
    assert!(
        output
            .report
            .results
            .iter()
            .all(|result| result.status == SourceQueryStatus::Stale)
    );
    assert!(output.report.sources.is_empty());
    query.targets[1].expected_hash = Some("f".repeat(64));
    assert!(read_source(directory.path(), &config(), &[], &query).is_err());
}

#[test]
fn source_formatting_preserves_machine_bytes_and_contains_human_controls_and_fences() {
    let directory = tempfile::tempdir().unwrap();
    let source = "fn example() {\r\n\tlet value = \"```\u{1b}[31m\";\r\n}\r\n";
    fs::write(directory.path().join("lib.rs"), source).unwrap();
    let mut query = options("lib.rs", &["example"]);
    for format in [
        Format::Json,
        Format::Ndjson,
        Format::Table,
        Format::Markdown,
    ] {
        query.format = format;
        let output = read_source(directory.path(), &config(), &[], &query).unwrap();
        assert_exact_sources(&output.report, source);
        assert!(output.rendered.ends_with('\n'));
        assert!(output.rendered.len() <= query.byte_budget);
        assert!(!output.rendered.contains('\u{1b}'));
        if matches!(format, Format::Json | Format::Ndjson) {
            let decoded: SourceQueryReport = serde_json::from_str(&output.rendered).unwrap();
            assert_exact_sources(&decoded, source);
        } else {
            assert!(output.rendered.contains("\\u{d}"));
            assert!(output.rendered.contains("\\u{1b}"));
        }
        if format == Format::Markdown {
            assert!(output.rendered.contains("\n````\n"));
        }
    }
}
