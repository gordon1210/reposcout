#![allow(
    clippy::unwrap_used,
    reason = "test fixtures must construct valid reports"
)]

use super::*;
use crate::model::{ReviewAnalysisStatus, ReviewComparison, ReviewRevisionCoverage};
use crate::scan::{ExplicitSourceBatch, ReviewCapture, ReviewRevision};
use std::collections::BTreeMap;
use std::time::Instant;

fn expired_capture() -> ReviewCapture {
    ReviewCapture {
        root: PathBuf::new(),
        deadline: Instant::now(),
        comparison: ReviewComparison::default(),
        changes: Vec::new(),
        total_changes: 0,
        revisions: std::array::from_fn(|_| ReviewRevision {
            tree: String::new(),
            sources: ExplicitSourceBatch {
                root: PathBuf::new(),
                files: BTreeMap::new(),
                failures: BTreeMap::new(),
            },
            coverage: ReviewRevisionCoverage::default(),
        }),
    }
}

#[test]
fn expired_capture_cannot_continue_into_mapping_or_graph_analysis() {
    let mut capture = expired_capture();
    let counter = TokenCounter::new("o200k_base").unwrap();
    let mapping = changes::describe(&capture, &counter, false);
    assert!(mapping.is_err());
    let graph = analysis::analyze(
        &mut capture,
        &[],
        &counter,
        crate::graph::GraphReadLimits::default(),
    );
    assert!(graph.is_err());
}

#[test]
fn expired_output_cannot_succeed_even_when_the_envelope_fits() {
    let mut report = ReviewContextReport {
        kind: "review_context".into(),
        schema_version: SCHEMA_VERSION.into(),
        strategy_version: 1,
        comparison: ReviewComparison::default(),
        encoding: "o200k_base".into(),
        token_budget: 65536,
        byte_budget: 1_048_576,
        context_budget: None,
        context_max_files: None,
        totals: ReviewContextTotals::default(),
        coverage: Vec::new(),
        limitations: Vec::new(),
        changes: Vec::new(),
        relations: Vec::new(),
        context: Vec::new(),
    };
    let options = ReviewContextOptions {
        base: "base".into(),
        head: "head".into(),
        merge_base: false,
        context: false,
        include_source: false,
        include_diff: false,
        token_budget: 65536,
        byte_budget: 1_048_576,
        format: Format::Json,
        pretty_json: false,
    };
    let counter = TokenCounter::new("o200k_base").unwrap();
    let result = project(&mut report, &options, &counter, Instant::now());
    assert!(result.is_err());
    assert!(
        project(
            &mut report,
            &options,
            &counter,
            Instant::now() + std::time::Duration::from_secs(60)
        )
        .is_ok()
    );
}

#[test]
fn historical_change_without_availability_deserializes_as_unknown() {
    let change: crate::model::ReviewContextChange = serde_json::from_value(serde_json::json!({
        "status": "modified", "base": null, "head": null,
        "hunks": 0, "hunks_omitted": 0, "diff_tokens": null
    }))
    .unwrap();
    assert_eq!(change.hunk_status, ReviewAnalysisStatus::Unavailable);
}
