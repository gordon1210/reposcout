use super::render_scan_diagnostics;
use crate::model::ScanDiagnostics;

#[test]
fn partial_type2_analysis_is_visible_in_markdown_diagnostics() {
    let diagnostics = ScanDiagnostics {
        type2_analysis_partial: true,
        type2_pools_truncated: 1,
        type2_seed_pairs_skipped: 42,
        type2_match_limit_reached: true,
        ..ScanDiagnostics::default()
    };
    let mut out = String::new();

    render_scan_diagnostics(&mut out, &diagnostics);

    assert!(out.contains("Type-2 analysis is **partial**"));
    assert!(out.contains("42 candidate seed pairs"));
    assert!(out.contains("match buffer limit was reached"));
}

#[test]
fn incomplete_omission_counts_are_labeled_as_lower_bounds() {
    let diagnostics = ScanDiagnostics {
        files_omitted_by_limit: 1,
        files_omitted_count_incomplete: true,
        scan_truncated: true,
        ..ScanDiagnostics::default()
    };
    let mut out = String::new();

    render_scan_diagnostics(&mut out, &diagnostics);

    assert!(out.contains("at least 1 (traversal stopped before an exact count)"));
}
