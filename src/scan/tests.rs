use super::aggregate::{
    build_assessment, production_duplication, production_duplication_is_complete,
    test_and_risk_summary,
};
use super::analyze_source;
use super::baseline::{baseline_delta, scan_profiles_compatible};
use super::duplicates::{top_duplicate_blocks, top_production_duplicate_blocks};
use super::file_analysis::apply_type2_diagnostics;
use super::report::scan_profile;
use super::rollup::dir_bucket;
use crate::config::{Config, Enabled};
use crate::dup::{DuplicateCoverage, fuzzy::Type2Diagnostics};
use crate::git::DiffScope;
use crate::model::{
    CloneGroup, CloneInstance, Duplication, LineRange, ProductionDuplication, ScanDiagnostics,
    Summary,
};
use std::path::{Path, PathBuf};

#[test]
fn type2_limits_mark_scan_diagnostics_partial() {
    let mut diagnostics = ScanDiagnostics::default();

    apply_type2_diagnostics(
        &mut diagnostics,
        Type2Diagnostics {
            truncated: true,
            pools_truncated: 2,
            candidate_buckets_skipped: 11,
            candidate_buckets_partially_selected: 1,
            seed_pairs_skipped: 99,
            match_limit_reached: true,
            suppression_limit_reached: true,
            matches_skipped_during_suppression: 7,
        },
    );

    assert!(diagnostics.type2_analysis_partial);
    assert_eq!(diagnostics.type2_pools_truncated, 2);
    assert_eq!(diagnostics.type2_candidate_buckets_skipped, 11);
    assert_eq!(diagnostics.type2_seed_pairs_skipped, 99);
    assert!(diagnostics.type2_match_limit_reached);
    assert!(diagnostics.type2_suppression_limit_reached);
    assert_eq!(diagnostics.type2_matches_skipped_during_suppression, 7);
}

#[test]
fn production_duplication_completeness_ignores_churn_only_truncation() {
    let clean = ScanDiagnostics::default();
    assert!(production_duplication_is_complete(true, &clean));
    assert!(!production_duplication_is_complete(false, &clean));

    let churn_only = ScanDiagnostics {
        scan_truncated: true,
        churn_analysis_partial: true,
        churn_deltas_omitted: 3,
        ..ScanDiagnostics::default()
    };
    assert!(production_duplication_is_complete(true, &churn_only));

    for incomplete in [
        ScanDiagnostics {
            type2_analysis_partial: true,
            ..ScanDiagnostics::default()
        },
        ScanDiagnostics {
            unreadable_files: 1,
            ..ScanDiagnostics::default()
        },
        ScanDiagnostics {
            walker_errors: 1,
            ..ScanDiagnostics::default()
        },
        ScanDiagnostics {
            oversized_files: 1,
            ..ScanDiagnostics::default()
        },
        ScanDiagnostics {
            files_omitted_by_limit: 1,
            ..ScanDiagnostics::default()
        },
        ScanDiagnostics {
            files_omitted_count_incomplete: true,
            ..ScanDiagnostics::default()
        },
        ScanDiagnostics {
            bytes_omitted_by_limit: 1,
            ..ScanDiagnostics::default()
        },
        ScanDiagnostics {
            duration_limit_reached: true,
            ..ScanDiagnostics::default()
        },
    ] {
        assert!(!production_duplication_is_complete(true, &incomplete));
    }
}

#[test]
fn dir_bucket_depth1_single_component() {
    assert_eq!(dir_bucket("src/model.rs", 1), "src");
}

#[test]
fn dir_bucket_depth2_nested() {
    assert_eq!(dir_bucket("src/metrics/tokens.rs", 2), "src/metrics");
}

#[test]
fn dir_bucket_root_file_is_dot() {
    assert_eq!(dir_bucket("README.md", 1), ".");
}

#[test]
fn dir_bucket_depth_clamps_to_available_components() {
    // depth=5 but only 2 parent components → clamp to "src/metrics"
    assert_eq!(dir_bucket("src/metrics/tokens.rs", 5), "src/metrics");
}

#[test]
fn dir_bucket_depth1_on_deep_path() {
    assert_eq!(dir_bucket("src/metrics/tokens.rs", 1), "src");
}

#[test]
fn dir_bucket_normalises_backslash() {
    assert_eq!(dir_bucket("src\\metrics\\tokens.rs", 1), "src");
}

#[test]
fn baseline_delta_detects_regression() {
    let mut base = Summary::default();
    let mut cur = Summary::default();
    base.duplication.duplicated_pct = 5.0;
    cur.duplication.duplicated_pct = 9.0;
    base.complexity.mi_avg = 70.0;
    cur.complexity.mi_avg = 60.0;

    let delta = baseline_delta(
        &base,
        "2020-01-01T00:00:00Z",
        &cur,
        &scan_profile(&Config::default(), None),
    );

    let dup_delta = delta
        .metrics
        .iter()
        .find(|m| m.metric == "duplicated_pct")
        .expect("duplicated_pct metric must be present");
    assert!(
        (dup_delta.delta - 4.0).abs() < f64::EPSILON,
        "expected delta 4.0, got {}",
        dup_delta.delta
    );

    assert!(delta.regressed, "expected regressed == true");
    assert!(
        delta.regressions.iter().any(|r| r.contains("duplication")),
        "expected a duplication regression message"
    );
    assert!(
        delta
            .regressions
            .iter()
            .any(|r| r.contains("maintainability")),
        "expected a maintainability regression message"
    );
}

#[test]
fn baseline_delta_no_regression_on_identical() {
    let base = Summary::default();
    let cur = Summary::default();
    let delta = baseline_delta(
        &base,
        "2020-01-01T00:00:00Z",
        &cur,
        &scan_profile(&Config::default(), None),
    );
    assert!(!delta.regressed, "identical summaries must not regress");
}

#[test]
fn diff_profiles_require_the_same_resolved_base_tree() {
    let cfg = Config {
        diff_scope: Some(DiffScope::Since("main".to_string())),
        ..Config::default()
    };
    let first = scan_profile(&cfg, Some("tree-a".to_string()));
    let alias = scan_profile(&cfg, Some("tree-a".to_string()));
    let different = scan_profile(&cfg, Some("tree-b".to_string()));

    assert!(scan_profiles_compatible(&first, &alias));
    assert!(!scan_profiles_compatible(&first, &different));
}

#[test]
fn baseline_profiles_require_matching_resource_limits() {
    let first = scan_profile(&Config::default(), None);
    let changed = scan_profile(
        &Config {
            max_file_bytes: Config::default().max_file_bytes / 2,
            ..Config::default()
        },
        None,
    );

    assert!(!scan_profiles_compatible(&first, &changed));
}

#[test]
fn assessment_treats_test_filename_matching_as_informational() {
    let summary = Summary {
        test_presence: Some(crate::model::TestPresence {
            source_files: 4,
            untested_source_files: 3,
            ..crate::model::TestPresence::default()
        }),
        ..Summary::default()
    };
    let evidence_at_threshold = ProductionDuplication {
        corpus: "production-source".to_string(),
        duplicated_lines: 15,
        analyzed_lines: 100,
        duplicated_pct: 15.0,
        complete: true,
    };

    let assessment_at_threshold = build_assessment(
        &summary,
        Some(evidence_at_threshold.clone()),
        Enabled::default(),
    );
    assert!(
        !assessment_at_threshold
            .reasons
            .iter()
            .any(|reason| reason.contains("source duplication"))
    );

    let above_threshold = build_assessment(
        &summary,
        Some(ProductionDuplication {
            duplicated_lines: 16,
            duplicated_pct: 15.1,
            ..evidence_at_threshold
        }),
        Enabled::default(),
    );
    assert!(
        above_threshold
            .reasons
            .iter()
            .any(|reason| reason == "high source duplication (15.1%)")
    );
    assert!(
        !above_threshold
            .reasons
            .iter()
            .any(|reason| reason == "many source files have no matching test file")
    );
    let evidence = above_threshold
        .production_duplication
        .expect("production duplication evidence");
    assert_eq!(evidence.corpus, "production-source");
    assert_eq!(evidence.duplicated_lines, 16);
    assert_eq!(evidence.analyzed_lines, 100);
    assert!(evidence.complete);
}

#[test]
fn partial_production_duplication_remains_explicit_observed_evidence() {
    let evidence = ProductionDuplication {
        corpus: "production-source".to_string(),
        duplicated_lines: 20,
        analyzed_lines: 100,
        duplicated_pct: 20.0,
        complete: false,
    };

    let assessment = build_assessment(&Summary::default(), Some(evidence), Enabled::default());
    let projected = assessment
        .production_duplication
        .expect("partial production duplication");

    assert!(!projected.complete);
    assert!((projected.duplicated_pct - 20.0).abs() < f64::EPSILON);
    assert!(!assessment.cleanup_worth_complete);
    assert!(
        assessment
            .reasons
            .iter()
            .any(|reason| reason == "high observed source duplication (20.0%; partial evidence)")
    );
}

#[test]
fn equal_risk_scores_use_the_path_as_a_stable_tie_breaker() {
    let cfg = Config::default();
    let health_policy = cfg.health_policy().unwrap();
    let source = "fn choose(value: bool) -> bool { if value { true } else { false } }\n";
    let second =
        analyze_source(Path::new("src/zeta.rs"), source, &cfg, &health_policy, None).unwrap();
    let first = analyze_source(
        Path::new("src/alpha.rs"),
        source,
        &cfg,
        &health_policy,
        None,
    )
    .unwrap();

    let (_, ranked, _) = test_and_risk_summary(
        &[second, first],
        &cfg,
        &health_policy,
        vec![crate::model::TestFramework {
            name: "cargo-test".to_string(),
            evidence: "Cargo.toml".to_string(),
        }],
    );
    let paths = ranked
        .iter()
        .map(|risk| risk.path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(paths, ["src/alpha.rs", "src/zeta.rs"]);
}

#[test]
fn assessment_uses_microsoft_maintainability_bands() {
    let mut summary = Summary::default();
    summary.complexity.functions = 1;

    summary.complexity.mi_avg = 20.0;
    assert!(
        build_assessment(&summary, None, Enabled::default())
            .reasons
            .iter()
            .all(|reason| !reason.contains("maintainability"))
    );

    summary.complexity.mi_avg = 15.0;
    assert!(
        build_assessment(&summary, None, Enabled::default())
            .reasons
            .iter()
            .any(|reason| reason == "moderate maintainability (MI avg 15)")
    );

    summary.complexity.mi_avg = 5.0;
    assert!(
        build_assessment(&summary, None, Enabled::default())
            .reasons
            .iter()
            .any(|reason| reason == "low maintainability (MI avg 5)")
    );
}

#[test]
fn assessment_does_not_claim_context_fit_when_tokens_are_disabled() {
    let assessment = build_assessment(
        &Summary::default(),
        None,
        Enabled {
            tokens: false,
            complexity: true,
            imports: false,
            markers: false,
            duplication: false,
            churn: false,
            lines: true,
        },
    );

    assert!(!assessment.fits_context_known);
    assert!(!assessment.fits_context);
    assert!(!assessment.cleanup_worth_complete);
    assert_eq!(
        assessment.unavailable_signals,
        ["tokens", "duplication", "churn"]
    );
    assert!(
        assessment
            .reasons
            .iter()
            .any(|reason| reason.contains("context fit unavailable"))
    );
}

#[test]
fn source_duplication_excludes_test_files() {
    let cfg = Config::default();
    let health_policy = cfg.health_policy().unwrap();
    let content = "fn example() {}\n".repeat(10);
    let source = analyze_source(
        Path::new("src/example.rs"),
        &content,
        &cfg,
        &health_policy,
        None,
    )
    .unwrap();
    let test = analyze_source(
        Path::new("tests/example.rs"),
        &content,
        &cfg,
        &health_policy,
        None,
    )
    .unwrap();
    let split_test_module = analyze_source(
        Path::new("src/example/tests.rs"),
        &content,
        &cfg,
        &health_policy,
        None,
    )
    .unwrap();
    let instance = |path: &str, end_line: usize| CloneInstance {
        path: path.into(),
        start_line: 1,
        end_line,
        start_column: 1,
        end_column: 2,
        ..CloneInstance::default()
    };
    let duplication = Duplication {
        exact: vec![CloneGroup {
            instances: vec![
                instance("src/example.rs", 2),
                instance("tests/example.rs", 10),
                instance("src/example/tests.rs", 10),
            ],
            ..CloneGroup::default()
        }],
        ..Duplication::default()
    };
    let coverage = DuplicateCoverage::from_duplication(&duplication);

    let evidence = production_duplication(
        &[source, test, split_test_module],
        &coverage,
        &std::collections::BTreeMap::new(),
        &health_policy,
        true,
    );

    assert_eq!(evidence.duplicated_lines, 2);
    assert_eq!(evidence.analyzed_lines, 10);
    assert!((evidence.duplicated_pct - 20.0).abs() < f64::EPSILON);
    assert!(evidence.complete);
}

#[test]
fn source_analysis_limits_first_class_markers_to_comments() {
    let cfg = Config::default();
    let health_policy = cfg.health_policy().unwrap();
    let report = analyze_source(
        Path::new("src/example.rs"),
        "const TODO: &str = \"TODO\";\n// TODO real work\n",
        &cfg,
        &health_policy,
        None,
    )
    .unwrap();

    assert_eq!(report.markers.get("TODO"), Some(&1));
    assert_eq!(report.marker_occurrences.len(), 1);
    assert_eq!(report.marker_occurrences[0].line, 2);
}

#[test]
fn baseline_regression_describes_test_matching_heuristic() {
    let baseline = Summary::default();
    let current = Summary {
        test_presence: Some(crate::model::TestPresence {
            untested_source_files: 1,
            ..crate::model::TestPresence::default()
        }),
        ..Summary::default()
    };

    let delta = baseline_delta(
        &baseline,
        "2020-01-01T00:00:00Z",
        &current,
        &scan_profile(&Config::default(), None),
    );

    assert!(
        delta
            .regressions
            .iter()
            .any(|reason| reason == "sources without matching tests +1 (now 1)")
    );
}

#[test]
fn top_duplicate_locations_use_occupied_end_lines() {
    let instance = |path: &str| CloneInstance {
        path: path.into(),
        start_line: 1,
        end_line: 2,
        start_column: 1,
        end_column: 1,
        start_byte: 0,
        end_byte: 7,
        ..CloneInstance::default()
    };
    let duplication = Duplication {
        exact: vec![CloneGroup {
            lines: 1,
            tokens: 3,
            similarity: 1.0,
            instances: vec![instance("a.rs"), instance("b.rs")],
            ..CloneGroup::default()
        }],
        ..Duplication::default()
    };

    let blocks = top_duplicate_blocks(&duplication, 10, 1);
    assert_eq!(blocks[0].locations, ["a.rs:1-1", "b.rs:1-1"]);
}

#[test]
fn compact_duplicates_suppress_nested_windows_but_keep_new_contiguous_lines() {
    let group = |lines: usize, ranges: &[(&str, usize, usize)]| CloneGroup {
        lines,
        tokens: lines * 3,
        similarity: 1.0,
        instances: ranges
            .iter()
            .map(|(path, start_line, end_line)| CloneInstance {
                path: (*path).into(),
                start_line: *start_line,
                end_line: *end_line,
                start_column: 1,
                end_column: 2,
                start_byte: start_line * 10,
                end_byte: end_line * 10,
                ..CloneInstance::default()
            })
            .collect(),
        ..CloneGroup::default()
    };
    let duplication = Duplication {
        exact: vec![
            group(10, &[("a.rs", 1, 10), ("b.rs", 1, 10)]),
            group(6, &[("a.rs", 8, 13), ("b.rs", 8, 13)]),
            group(5, &[("c.rs", 1, 5), ("d.rs", 1, 5)]),
        ],
        near: vec![group(8, &[("a.rs", 2, 9), ("b.rs", 2, 9)])],
        ..Duplication::default()
    };
    let coverage_before = DuplicateCoverage::from_duplication(&duplication);

    let blocks = top_duplicate_blocks(&duplication, 10, 3);

    assert_eq!(duplication.exact.len(), 3);
    assert_eq!(duplication.near.len(), 1);
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].locations, ["a.rs:1-10", "b.rs:1-10"]);
    assert_eq!(blocks[1].locations, ["a.rs:8-13", "b.rs:8-13"]);
    assert_eq!(blocks[2].locations, ["c.rs:1-5", "d.rs:1-5"]);
    assert_eq!(
        DuplicateCoverage::from_duplication(&duplication).total_lines(),
        coverage_before.total_lines()
    );
}

#[test]
fn production_duplicate_projection_excludes_test_only_families() {
    let group = |lines: usize, paths: &[&str]| CloneGroup {
        lines,
        tokens: lines * 3,
        similarity: 1.0,
        instances: paths
            .iter()
            .map(|path| CloneInstance {
                path: (*path).into(),
                start_line: 1,
                end_line: lines,
                start_column: 1,
                end_column: 2,
                start_byte: 0,
                end_byte: lines * 10,
                ..CloneInstance::default()
            })
            .collect(),
        ..CloneGroup::default()
    };
    let duplication = Duplication {
        exact: vec![
            group(12, &["tests/first.rs", "tests/second.rs"]),
            group(10, &["src/mixed.rs", "tests/mixed.rs"]),
            group(8, &["src/first.rs", "src/second.rs"]),
            group(6, &["src/inline.rs", "src/inline-copy.rs"]),
        ],
        ..Duplication::default()
    };
    let test_regions = std::collections::BTreeMap::from([
        (
            PathBuf::from("src/inline.rs"),
            vec![LineRange { start: 1, end: 6 }],
        ),
        (
            PathBuf::from("src/inline-copy.rs"),
            vec![LineRange { start: 1, end: 6 }],
        ),
    ]);
    let health_policy = Config::default().health_policy().unwrap();

    let blocks =
        top_production_duplicate_blocks(&duplication, 10, 3, &test_regions, &health_policy);

    assert_eq!(duplication.exact.len(), 4);
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        blocks[0].locations,
        ["src/mixed.rs:1-10", "tests/mixed.rs:1-10"]
    );
    assert_eq!(
        blocks[1].locations,
        ["src/first.rs:1-8", "src/second.rs:1-8"]
    );
}

#[test]
fn production_duplicate_projection_requires_actionable_non_test_span() {
    let instance = |path: &str| CloneInstance {
        path: path.into(),
        start_line: 1,
        end_line: 8,
        start_column: 1,
        end_column: 2,
        start_byte: 0,
        end_byte: 80,
        ..CloneInstance::default()
    };
    let duplication = Duplication {
        exact: vec![CloneGroup {
            lines: 8,
            tokens: 24,
            similarity: 1.0,
            instances: vec![instance("src/first.rs"), instance("src/second.rs")],
            ..CloneGroup::default()
        }],
        ..Duplication::default()
    };
    let test_regions = std::collections::BTreeMap::from([
        (
            PathBuf::from("src/first.rs"),
            vec![
                LineRange { start: 1, end: 3 },
                LineRange { start: 5, end: 8 },
            ],
        ),
        (
            PathBuf::from("src/second.rs"),
            vec![
                LineRange { start: 1, end: 3 },
                LineRange { start: 5, end: 8 },
            ],
        ),
    ]);
    let health_policy = Config::default().health_policy().unwrap();

    let blocks =
        top_production_duplicate_blocks(&duplication, 10, 3, &test_regions, &health_policy);

    assert!(blocks.is_empty());
}
