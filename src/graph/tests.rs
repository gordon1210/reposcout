use super::*;
use crate::model::FileReport;
use std::collections::BTreeMap;
use tempfile::tempdir;

fn file_report(path: &str) -> FileReport {
    FileReport {
        path: PathBuf::from(path),
        language: "Python".to_string(),
        bytes: 0,
        tokens: 0,
        loc: 0,
        sloc: 0,
        comment_lines: 0,
        comment_ratio: 0.0,
        line_metrics_approximate: false,
        complexity: None,
        imports: Vec::new(),
        markers: BTreeMap::new(),
        marker_occurrences: Vec::new(),
        churn: None,
        approximate: false,
        symbols: None,
        skip_hint: None,
        has_inline_tests: false,
    }
}

mod extraction;

mod language_resolvers;

mod javascript;

mod routing;
