use super::{add_responsive_path_rows, new_table, path_cell, render_scan_diagnostics, right_align};
use crate::model::ScanDiagnostics;
use unicode_width::UnicodeWidthStr;

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
    let rendered = path_cell(path, 48);

    assert!(rendered.starts_with('…'));
    assert!(rendered.ends_with("command-globe-scene.tsx"));
    assert!(rendered.chars().count() <= 48);
}

#[test]
fn path_truncation_respects_display_width_for_wide_unicode() {
    let rendered = path_cell("packages/界面/组件/navigation.rs", 18);

    assert!(rendered.starts_with('…'));
    assert!(rendered.ends_with("navigation.rs"));
    assert!(UnicodeWidthStr::width(rendered.as_str()) <= 18);
}

#[test]
fn tables_fill_the_available_terminal_width() {
    let mut table = new_table(vec!["Label", "Value"]);
    table.set_width(72);
    table.add_row(vec!["Files", "42"]);

    let rendered = table.to_string();

    assert!(
        rendered.lines().all(|line| line.chars().count() == 72),
        "every table line should use the shared width:\n{rendered}"
    );
}

#[test]
fn narrow_tables_shrink_paths_before_metric_headers() {
    let mut table = new_table(vec!["File", "Commits", "Cyclo", "Avg/fn", "Score"]);
    table.set_width(60);
    add_responsive_path_rows(
        &mut table,
        vec![vec![
            "packages/application/src/components/navigation.rs".to_string(),
            "12".to_string(),
            "34".to_string(),
            "5.7".to_string(),
            "408".to_string(),
        ]],
        0,
        &[(0, 1)],
    );
    right_align(&mut table, &[1, 2, 3, 4]);

    let rendered = table.to_string();
    let header = rendered
        .lines()
        .find(|line| line.contains("File"))
        .expect("table header should be present");

    assert!(header.contains("Commits"));
    assert!(header.contains("Cyclo"));
    assert!(header.contains("Avg/fn"));
    assert!(header.contains("Score"));
    assert!(rendered.lines().all(|line| line.chars().count() == 60));
    assert_eq!(
        rendered.lines().count(),
        5,
        "the path must remain on one row"
    );
    assert!(rendered.contains('…'));
    assert!(rendered.contains("navigation.rs"));
}

#[test]
fn wider_tables_reveal_more_of_the_same_path() {
    let path = "packages/application/src/components/navigation/command-globe-scene.tsx";
    let mut narrow = new_table(vec!["File", "Tokens"]);
    narrow.set_width(60);
    add_responsive_path_rows(
        &mut narrow,
        vec![vec![path.to_string(), "123".to_string()]],
        0,
        &[(0, 1)],
    );
    let mut wide = new_table(vec!["File", "Tokens"]);
    wide.set_width(120);
    add_responsive_path_rows(
        &mut wide,
        vec![vec![path.to_string(), "123".to_string()]],
        0,
        &[(0, 1)],
    );

    let narrow = narrow.to_string();
    let wide = wide.to_string();
    assert!(narrow.contains('…'));
    assert!(narrow.contains("command-globe-scene.tsx"));
    assert!(!narrow.contains(path));
    assert!(wide.contains(path));
}
