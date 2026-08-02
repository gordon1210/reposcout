//! Composite file-risk scoring shared by summaries and `explain`.

use crate::model::{FileReport, RiskEntry, RiskExplanation};
use crate::numeric::usize_to_f64;

const SLOC_HALF_SATURATION: f64 = 1_000.0;
const CYCLOMATIC_HALF_SATURATION: f64 = 100.0;
const CHURN_HALF_SATURATION: f64 = 20.0;
const REASON_THRESHOLD: f64 = 0.66;
pub const ALGORITHM_VERSION: u32 = 5;

#[must_use]
pub fn explain(file: &FileReport, no_matching_test_file: bool) -> RiskExplanation {
    let sloc = file.sloc;
    let cyclomatic = file
        .complexity
        .as_ref()
        .map_or(0, |complexity| complexity.cyclomatic);
    let churn_commits = file.churn.as_ref().map_or(0, |churn| churn.commits);
    let sloc_metric = usize_to_f64(sloc);
    let churn_metric = usize_to_f64(churn_commits);
    let size_factor = half_saturation(sloc_metric, SLOC_HALF_SATURATION);
    let complexity_factor = half_saturation(f64::from(cyclomatic), CYCLOMATIC_HALF_SATURATION);
    let churn_factor = half_saturation(churn_metric, CHURN_HALF_SATURATION);
    let score = (0.40 * size_factor + 0.40 * complexity_factor + 0.20 * churn_factor).min(1.0);
    // Retained in the stable JSON contract. Filename matching is useful
    // navigation evidence, but it is not measured coverage and must not alter
    // the risk score.
    let untested_multiplier = 1.0;

    let mut reasons = Vec::new();
    if sloc_metric / SLOC_HALF_SATURATION >= REASON_THRESHOLD {
        reasons.push("large".to_string());
    }
    if f64::from(cyclomatic) / CYCLOMATIC_HALF_SATURATION >= REASON_THRESHOLD {
        reasons.push("complex".to_string());
    }
    if churn_metric / CHURN_HALF_SATURATION >= REASON_THRESHOLD {
        reasons.push("high churn".to_string());
    }
    if no_matching_test_file {
        reasons.push("no matching test file".to_string());
    }
    if reasons.is_empty() && score > 0.0 {
        reasons.push("combined signals".to_string());
    }

    RiskExplanation {
        algorithm_version: ALGORITHM_VERSION,
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

fn half_saturation(value: f64, anchor: f64) -> f64 {
    value / (value + anchor)
}

#[must_use]
pub fn entry(file: &FileReport, no_matching_test_file: bool) -> Option<RiskEntry> {
    let risk = explain(file, no_matching_test_file);
    (risk.score > 0.0).then(|| RiskEntry {
        path: file.path.to_string_lossy().into_owned(),
        algorithm_version: ALGORITHM_VERSION,
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
    use super::{ALGORITHM_VERSION, entry, explain};
    use crate::model::{Churn, Complexity, FileReport};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn saturation_anchors_are_half_saturation_points() {
        let risk = explain(&file_with("src/anchor.rs", 1_000, 100, 20), false);

        assert!((risk.size_factor - 0.5).abs() < f64::EPSILON);
        assert!((risk.complexity_factor - 0.5).abs() < f64::EPSILON);
        assert!((risk.churn_factor - 0.5).abs() < f64::EPSILON);
        assert!((risk.score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn risk_remains_monotonic_above_the_former_saturation_anchors() {
        let first = explain(&file_with("src/first.rs", 1_000, 100, 20), false);
        let second = explain(&file_with("src/second.rs", 2_000, 200, 40), false);
        let third = explain(&file_with("src/third.rs", 4_000, 400, 80), false);

        assert!(first.score < second.score);
        assert!(second.score < third.score);
        assert!(third.score < 1.0);
    }

    #[test]
    fn compact_and_detailed_risk_evidence_identify_the_algorithm() {
        let file = representative_file();
        let explanation = explain(&file, false);
        let compact = entry(&file, false).expect("risk entry");

        assert_eq!(ALGORITHM_VERSION, 5);
        assert_eq!(explanation.algorithm_version, ALGORITHM_VERSION);
        assert_eq!(compact.algorithm_version, ALGORITHM_VERSION);
        assert_eq!(compact.sloc, explanation.sloc);
        assert_eq!(compact.cyclomatic, explanation.cyclomatic);
        assert_eq!(compact.churn_commits, explanation.churn_commits);
    }

    #[test]
    fn missing_test_match_is_informational_and_does_not_change_risk() {
        let file = representative_file();
        let matched = explain(&file, false);
        let unmatched = explain(&file, true);

        assert!((matched.score - (1.0 / 3.0)).abs() < f64::EPSILON);
        assert!((unmatched.score - matched.score).abs() < f64::EPSILON);
        assert!((unmatched.untested_multiplier - 1.0).abs() < f64::EPSILON);
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
        file_with("src/example.rs", 500, 50, 10)
    }

    fn file_with(path: &str, sloc: usize, cyclomatic: u32, commits: usize) -> FileReport {
        FileReport {
            path: PathBuf::from(path),
            language: "Rust".to_string(),
            bytes: 0,
            tokens: 0,
            loc: sloc,
            sloc,
            comment_lines: 0,
            comment_ratio: 0.0,
            line_metrics_approximate: false,
            complexity: Some(Complexity {
                cyclomatic,
                ..Complexity::default()
            }),
            imports: Vec::new(),
            markers: BTreeMap::new(),
            marker_occurrences: Vec::new(),
            churn: Some(Churn {
                commits,
                ..Churn::default()
            }),
            approximate: false,
            symbols: None,
            skip_hint: None,
            has_inline_tests: false,
        }
    }
}
