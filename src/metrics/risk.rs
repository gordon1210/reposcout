//! Composite file-risk scoring shared by summaries and `explain`.

use crate::model::{FileReport, RiskEntry, RiskExplanation};

const SLOC_SATURATION: f64 = 1_000.0;
const CYCLOMATIC_SATURATION: f64 = 100.0;
const CHURN_SATURATION: f64 = 20.0;

pub fn explain(file: &FileReport, no_matching_test_file: bool) -> RiskExplanation {
    let sloc = file.sloc;
    let cyclomatic = file
        .complexity
        .as_ref()
        .map(|complexity| complexity.cyclomatic)
        .unwrap_or(0);
    let churn_commits = file.churn.as_ref().map(|churn| churn.commits).unwrap_or(0);
    let size_factor = (sloc as f64 / SLOC_SATURATION).min(1.0);
    let complexity_factor = (cyclomatic as f64 / CYCLOMATIC_SATURATION).min(1.0);
    let churn_factor = (churn_commits as f64 / CHURN_SATURATION).min(1.0);
    let score = (0.40 * size_factor + 0.40 * complexity_factor + 0.20 * churn_factor).min(1.0);
    // Retained in the stable JSON contract. Filename matching is useful
    // navigation evidence, but it is not measured coverage and must not alter
    // the risk score.
    let untested_multiplier = 1.0;

    let mut reasons = Vec::new();
    if size_factor >= 0.66 {
        reasons.push("large".to_string());
    }
    if complexity_factor >= 0.66 {
        reasons.push("complex".to_string());
    }
    if churn_factor >= 0.66 {
        reasons.push("high churn".to_string());
    }
    if no_matching_test_file {
        reasons.push("no matching test file".to_string());
    }
    if reasons.is_empty() && score > 0.0 {
        reasons.push("combined signals".to_string());
    }

    RiskExplanation {
        score,
        sloc,
        cyclomatic,
        churn_commits,
        size_factor,
        complexity_factor,
        churn_factor,
        untested: no_matching_test_file,
        untested_multiplier,
        reasons,
    }
}

pub fn entry(file: &FileReport, no_matching_test_file: bool) -> Option<RiskEntry> {
    let risk = explain(file, no_matching_test_file);
    (risk.score > 0.0).then(|| RiskEntry {
        path: file.path.to_string_lossy().into_owned(),
        score: risk.score,
        sloc: risk.sloc,
        cyclomatic: risk.cyclomatic,
        churn_commits: risk.churn_commits,
        untested: risk.untested,
        reasons: risk.reasons,
    })
}

#[cfg(test)]
mod tests {
    use super::explain;
    use crate::model::{Churn, Complexity, FileReport};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn missing_test_match_is_informational_and_does_not_change_risk() {
        let file = representative_file();
        let matched = explain(&file, false);
        let unmatched = explain(&file, true);

        assert_eq!(matched.score, 0.5);
        assert_eq!(unmatched.score, matched.score);
        assert_eq!(unmatched.untested_multiplier, 1.0);
        assert_eq!(unmatched.reasons, ["no matching test file"]);
    }

    #[test]
    fn missing_test_match_never_claims_measured_coverage() {
        let risk = explain(&representative_file(), true);

        assert!(!risk.reasons.iter().any(|reason| reason == "untested"));
        assert!(
            risk.reasons
                .iter()
                .any(|reason| reason == "no matching test file")
        );
    }

    fn representative_file() -> FileReport {
        FileReport {
            path: PathBuf::from("src/example.rs"),
            language: "Rust".to_string(),
            bytes: 0,
            tokens: 0,
            loc: 500,
            sloc: 500,
            comment_lines: 0,
            comment_ratio: 0.0,
            line_metrics_approximate: false,
            complexity: Some(Complexity {
                cyclomatic: 50,
                ..Complexity::default()
            }),
            imports: Vec::new(),
            markers: BTreeMap::new(),
            marker_occurrences: Vec::new(),
            churn: Some(Churn {
                commits: 10,
                ..Churn::default()
            }),
            approximate: false,
            symbols: None,
            skip_hint: None,
            has_inline_tests: false,
        }
    }
}
