use super::{PATH_WIDE, path_cell, render_scan_diagnostics};
use crate::model::ScanDiagnostics;

#[test]
fn partial_type2_analysis_is_visible_in_human_diagnostics() {
    let diagnostics = ScanDiagnostics {
        type2_analysis_partial: true,
        type2_pools_truncated: 1,
        type2_candidate_buckets_skipped: 12,
        type2_candidate_buckets_partially_selected: 1,
        type2_seed_pairs_skipped: 42,
        type2_match_limit_reached: true,
        ..ScanDiagnostics::default()
    };
    let mut out = String::new();

    render_scan_diagnostics(&mut out, &diagnostics, false);

    assert!(out.contains("Type-2 analysis"));
    assert!(out.contains("partial (safety limit reached)"));
    assert!(out.contains("Seed pairs skipped"));
    assert!(out.contains("42"));
    assert!(out.contains("Match buffer limit"));
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

    render_scan_diagnostics(&mut out, &diagnostics, false);

    assert!(out.contains("Known files omitted"));
    assert!(out.contains("at least 1"));
}

#[test]
fn long_table_paths_keep_the_filename_and_truncate_the_front() {
    let path = "packages/application/src/components/navigation/command-globe-scene.tsx";
    let rendered = path_cell(path, PATH_WIDE);

    assert!(rendered.starts_with('…'));
    assert!(rendered.ends_with("command-globe-scene.tsx"));
    assert!(rendered.chars().count() <= 48);
}
