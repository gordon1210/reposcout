use crate::lang;
use crate::model::{
    DuplicateBlock, FindingRecord, FunctionComplexity, LanguageStat, MetricDelta, ScanReport,
    Summary,
};
use serde_json::Value;
use std::path::Path;

/// Remove declaration objects from a context plan used in a compact summary.
/// Aggregate outline counts remain so consumers can decide whether the full
/// report is worth requesting.
pub(crate) fn strip_context_outline_details(context: &mut Value) {
    let Some(context) = context.as_object_mut() else {
        return;
    };
    let mut removed = false;
    for collection in ["files", "outline_only"] {
        let Some(entries) = context.get_mut(collection).and_then(Value::as_array_mut) else {
            continue;
        };
        for entry in entries {
            let Some(entry) = entry.as_object_mut() else {
                continue;
            };
            removed |= entry.remove("symbols").is_some();
        }
    }
    if removed {
        context.insert("outline_details_omitted".to_string(), Value::Bool(true));
    }
}

pub(crate) fn callable_cyclomatic_average(functions: &[FunctionComplexity]) -> Option<f64> {
    if functions.is_empty() {
        return None;
    }

    Some(
        functions
            .iter()
            .map(|function| function.cyclomatic as u64)
            .sum::<u64>() as f64
            / functions.len() as f64,
    )
}

pub(crate) fn file_cyclomatic_average(report: &ScanReport, path: &Path) -> Option<f64> {
    let complexity = report
        .files
        .iter()
        .find(|file| file.path.as_path() == path)?
        .complexity
        .as_ref()?;
    callable_cyclomatic_average(&complexity.functions)
}

pub(crate) fn human_risk_heading(report: &ScanReport) -> String {
    let version = report
        .summary
        .top_risks
        .iter()
        .map(|risk| risk.algorithm_version)
        .find(|version| *version > 0)
        .or_else(|| {
            report
                .analysis_profile
                .as_ref()?
                .findings
                .as_ref()
                .map(|findings| findings.risk_algorithm_version)
                .filter(|version| *version > 0)
        });
    match version {
        Some(version) => format!("Top risks · algorithm {version}"),
        None => "Top risks · algorithm unknown".to_string(),
    }
}

pub(crate) fn human_duplicate_projection(summary: &Summary) -> (&'static str, &[DuplicateBlock]) {
    if summary.assessment.production_duplication.is_some() {
        (
            "Top production duplicates",
            &summary.top_production_duplicates,
        )
    } else {
        ("Top duplicates", &summary.top_duplicates)
    }
}

/// Keep human reports source-first while preserving one honest rollup for the
/// non-source inventory retained in machine-readable summaries.
pub(crate) fn source_language_rollup(languages: &[LanguageStat]) -> Vec<LanguageStat> {
    let mut rows = Vec::new();
    let mut other = LanguageStat::default();
    let mut other_formats = 0usize;

    for language in languages {
        if language.source || lang::is_source_name(&language.name) {
            rows.push(language.clone());
        } else {
            other_formats += 1;
            other.files += language.files;
            other.bytes += language.bytes;
            other.loc += language.loc;
            other.sloc += language.sloc;
            other.comment_lines += language.comment_lines;
            other.tokens += language.tokens;
        }
    }
    if other_formats > 0 {
        other.name = format!("Other content ({other_formats} formats)");
        rows.push(other);
    }
    rows
}

pub(crate) struct MetricDeltaDisplay {
    pub baseline: String,
    pub current: String,
    pub delta: String,
}

pub(crate) fn metric_delta_display(metric: &MetricDelta) -> MetricDeltaDisplay {
    if matches!(
        metric.metric.as_str(),
        "files" | "tokens" | "sloc" | "cyclomatic_max" | "untested_source_files"
    ) {
        MetricDeltaDisplay {
            baseline: format!("{:.0}", metric.baseline),
            current: format!("{:.0}", metric.current),
            delta: format!("{:+.0}", metric.delta),
        }
    } else {
        MetricDeltaDisplay {
            baseline: format!("{:.2}", metric.baseline),
            current: format!("{:.2}", metric.current),
            delta: format!("{:+.2}", metric.delta),
        }
    }
}

pub(crate) fn metric_label(metric: &str) -> &str {
    if metric == "untested_source_files" {
        "source_files_without_matching_test"
    } else {
        metric
    }
}

pub(crate) fn human_test_signal(value: &str) -> String {
    value
        .replace("untested sources", "source files without a matching test")
        .replace("untested", "no matching test file")
}

pub(crate) fn finding_location(finding: &FindingRecord) -> String {
    let location = &finding.primary_location;
    if location.start_line == 0 {
        return location.path.display().to_string();
    }
    if location.end_line > location.start_line {
        format!(
            "{}:{}-{}",
            location.path.display(),
            location.start_line,
            location.end_line
        )
    } else {
        format!("{}:{}", location.path.display(), location.start_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FindingLocation;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn compact_context_removes_selected_and_outline_only_symbol_details() {
        let mut context = json!({
            "outline_symbols": 2,
            "files": [{"path": "src/lib.rs", "symbols": [{"name": "run"}]}],
            "outline_only": [{"path": "src/large.rs", "symbols": [{"name": "Large"}]}]
        });

        strip_context_outline_details(&mut context);

        assert_eq!(context["outline_details_omitted"], true);
        assert!(context["files"][0].get("symbols").is_none());
        assert!(context["outline_only"][0].get("symbols").is_none());
        assert_eq!(context["outline_symbols"], 2);
    }

    #[test]
    fn baseline_counts_and_ratios_have_stable_precision() {
        let count = MetricDelta {
            metric: "tokens".to_string(),
            baseline: 10.4,
            current: 12.6,
            delta: 2.2,
        };
        let count = metric_delta_display(&count);
        assert_eq!(
            (
                count.baseline.as_str(),
                count.current.as_str(),
                count.delta.as_str()
            ),
            ("10", "13", "+2")
        );

        let ratio = MetricDelta {
            metric: "duplicated_pct".to_string(),
            baseline: 10.456,
            current: 9.111,
            delta: -1.345,
        };
        let ratio = metric_delta_display(&ratio);
        assert_eq!(
            (
                ratio.baseline.as_str(),
                ratio.current.as_str(),
                ratio.delta.as_str()
            ),
            ("10.46", "9.11", "-1.34")
        );
    }

    #[test]
    fn callable_cyclomatic_average_requires_callable_facts() {
        let functions = [
            FunctionComplexity {
                cyclomatic: 2,
                ..FunctionComplexity::default()
            },
            FunctionComplexity {
                cyclomatic: 7,
                ..FunctionComplexity::default()
            },
        ];

        assert_eq!(callable_cyclomatic_average(&functions), Some(4.5));
        assert_eq!(callable_cyclomatic_average(&[]), None);
    }

    #[test]
    fn finding_locations_cover_file_line_and_range() {
        let mut finding = FindingRecord {
            primary_location: FindingLocation {
                path: PathBuf::from("src/lib.rs"),
                ..FindingLocation::default()
            },
            ..FindingRecord::default()
        };
        assert_eq!(finding_location(&finding), "src/lib.rs");

        finding.primary_location.start_line = 7;
        finding.primary_location.end_line = 7;
        assert_eq!(finding_location(&finding), "src/lib.rs:7");

        finding.primary_location.end_line = 11;
        assert_eq!(finding_location(&finding), "src/lib.rs:7-11");
    }

    #[test]
    fn coverage_heuristic_is_not_presented_as_coverage() {
        assert_eq!(
            metric_label("untested_source_files"),
            "source_files_without_matching_test"
        );
        assert_eq!(
            human_test_signal("untested sources +2 (now 4)"),
            "source files without a matching test +2 (now 4)"
        );
        assert_eq!(human_test_signal("untested"), "no matching test file");
    }

    #[test]
    fn language_rollup_keeps_source_rows_and_collapses_content() {
        let rows = source_language_rollup(&[
            LanguageStat {
                name: "Rust".to_string(),
                files: 2,
                tokens: 100,
                ..LanguageStat::default()
            },
            LanguageStat {
                name: "JSON".to_string(),
                files: 3,
                tokens: 200,
                ..LanguageStat::default()
            },
            LanguageStat {
                name: "Markdown".to_string(),
                files: 1,
                tokens: 50,
                ..LanguageStat::default()
            },
        ]);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "Rust");
        assert_eq!(rows[1].name, "Other content (2 formats)");
        assert_eq!(rows[1].files, 4);
        assert_eq!(rows[1].tokens, 250);
    }
}
