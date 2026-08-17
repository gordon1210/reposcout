use super::*;

#[test]
fn legacy_scan_reports_default_additive_agent_projections() {
    let report: ScanReport = serde_json::from_value(serde_json::json!({
        "schema_version": "1.0",
        "root": "/tmp/example",
        "target": "/tmp/example",
        "generated_at": "2026-07-29T00:00:00Z",
        "encoding": "o200k_base",
        "summary": serde_json::to_value(Summary::default()).unwrap(),
        "files": [],
        "duplicates": serde_json::to_value(Duplication::default()).unwrap()
    }))
    .expect("legacy scan report");

    assert!(report.change_summary.is_none());
    assert!(report.work_scope.is_none());
}

#[test]
fn scan_diagnostics_default_and_expose_partial_type2_analysis() {
    let legacy: ScanDiagnostics = serde_json::from_value(serde_json::json!({
        "discovered_files": 2,
        "analyzed_files": 2,
        "unsupported_files": 0,
        "unreadable_files": 0,
        "walker_errors": 0
    }))
    .expect("legacy scan diagnostics");
    assert!(!legacy.type2_analysis_partial);

    let diagnostics = ScanDiagnostics {
        oversized_files: 2,
        oversized_bytes: 8_388_608,
        files_omitted_by_limit: 3,
        files_omitted_count_incomplete: true,
        bytes_omitted_by_limit: 12_582_912,
        scan_truncated: true,
        duration_limit_reached: true,
        type2_analysis_partial: true,
        type2_pools_truncated: 1,
        type2_candidate_buckets_skipped: 12,
        type2_candidate_buckets_partially_selected: 1,
        type2_seed_pairs_skipped: 42,
        type2_match_limit_reached: true,
        type2_suppression_limit_reached: true,
        type2_matches_skipped_during_suppression: 7,
        ..ScanDiagnostics::default()
    };
    let json = serde_json::to_value(diagnostics).expect("scan diagnostics JSON");

    assert_eq!(json["type2_analysis_partial"], true);
    assert_eq!(json["oversized_files"], 2);
    assert_eq!(json["oversized_bytes"], 8_388_608u64);
    assert_eq!(json["files_omitted_by_limit"], 3);
    assert_eq!(json["files_omitted_count_incomplete"], true);
    assert_eq!(json["scan_truncated"], true);
    assert_eq!(json["duration_limit_reached"], true);
    assert_eq!(json["type2_seed_pairs_skipped"], 42);
    assert_eq!(json["type2_match_limit_reached"], true);
    assert_eq!(json["type2_suppression_limit_reached"], true);
    assert_eq!(json["type2_matches_skipped_during_suppression"], 7);
}

#[test]
fn partial_scan_profiles_default_new_nested_fields() {
    let json = serde_json::json!({
        "analyzers": { "tokens": true },
        "diff_scope": "full",
        "duplication": { "min_tokens": 50 }
    });

    let profile: ScanProfile = serde_json::from_value(json).expect("partial scan profile JSON");

    assert!(profile.analyzers.tokens);
    assert!(!profile.analyzers.complexity);
    assert_eq!(profile.diff_scope, "full");
    assert_eq!(profile.diff_base, None);
    assert_eq!(profile.health, None);
    assert_eq!(
        profile.duplication.expect("duplication profile").min_tokens,
        50
    );
}

#[test]
fn legacy_graph_blocks_default_parse_error_counts() {
    let graph: DepGraph = serde_json::from_value(serde_json::json!({
        "languages": [],
        "nodes": 0,
        "edges": 0,
        "cycles": [],
        "orphans": [],
        "top_depended": [],
        "most_dependent": [],
        "unresolved_imports": 0
    }))
    .unwrap();
    let impact: ImpactAnalysis = serde_json::from_value(serde_json::json!({
        "changed_files": [],
        "graph_changed_files": [],
        "direct_dependents": [],
        "transitive_dependents": [],
        "unresolved_imports": 0,
        "confidence": "none"
    }))
    .unwrap();

    assert_eq!(graph.parse_errors, 0);
    assert_eq!(impact.parse_errors, 0);
}

#[test]
fn pre_detail_duplication_json_still_deserializes() {
    let json = r#"{
            "exact": [{
                "lines": 3,
                "tokens": 20,
                "similarity": 1.0,
                "instances": [
                    {"path": "a.rs", "start_line": 1, "end_line": 3},
                    {"path": "b.rs", "start_line": 5, "end_line": 7}
                ]
            }],
            "near": []
        }"#;

    let duplication: Duplication = serde_json::from_str(json).expect("old duplication JSON");

    assert_eq!(duplication.exact.len(), 1);
    assert_eq!(duplication.exact[0].format, "");
    assert_eq!(duplication.exact[0].instances[0].start_byte, 0);
    assert!(duplication.findings.is_empty());
    assert!(duplication.file_coverage.is_empty());
}

#[test]
fn pre_detail_summary_json_defaults_new_duplication_fields() {
    let json = serde_json::json!({
        "files": 0,
        "bytes": 0,
        "tokens": 0,
        "loc": 0,
        "sloc": 0,
        "comment_lines": 0,
        "comment_ratio": 0.0,
        "languages": [],
        "complexity": {
            "cyclomatic_total": 0,
            "cyclomatic_avg": 0.0,
            "cyclomatic_max": 0,
            "cognitive_total": 0,
            "cognitive_avg": 0.0,
            "cognitive_max": 0,
            "mi_avg": 0.0,
            "mi_min": 0.0,
            "functions": 0,
            "approximate_files": 0
        },
        "duplication": {
            "exact_groups": 0,
            "near_groups": 0,
            "duplicated_lines": 0,
            "duplicated_pct": 0.0
        },
        "markers": {},
        "top_token_files": [],
        "top_hotspots": [],
        "top_functions": [],
        "top_duplicates": []
    });

    let summary: Summary = serde_json::from_value(json).expect("old summary JSON");

    assert_eq!(summary.source.files, 0);
    assert!(summary.top_source_token_files.is_empty());
    assert_eq!(summary.duplication.analyzed_lines, 0);
    assert_eq!(summary.duplication.analyzed_tokens, 0);
    assert!(summary.duplication.by_language.is_empty());
    assert!(summary.top_production_duplicates.is_empty());
    assert!(summary.top_duplicate_findings.is_empty());
    assert!(summary.assessment.production_duplication.is_none());
    assert_eq!(summary.complexity.cyclomatic_threshold, 0);
    assert_eq!(summary.complexity.functions_over_threshold, 0);
    assert!(summary.complexity_violations.is_empty());
}

#[test]
fn legacy_risk_evidence_defaults_the_algorithm_version() {
    let entry: RiskEntry = serde_json::from_value(serde_json::json!({
        "path": "src/lib.rs",
        "score": 0.5,
        "sloc": 100,
        "cyclomatic": 20,
        "churn_commits": 3,
        "untested": false,
        "reasons": []
    }))
    .expect("legacy risk entry");
    let explanation: RiskExplanation = serde_json::from_value(serde_json::json!({
        "score": 0.5,
        "sloc": 100,
        "cyclomatic": 20,
        "churn_commits": 3,
        "size_factor": 0.1,
        "complexity_factor": 0.2,
        "churn_factor": 0.15,
        "untested": false,
        "untested_multiplier": 1.0,
        "reasons": []
    }))
    .expect("legacy risk explanation");

    assert_eq!(entry.algorithm_version, 0);
    assert_eq!(explanation.algorithm_version, 0);
}

#[test]
fn legacy_test_presence_defaults_framework_evidence() {
    let presence: TestPresence = serde_json::from_value(serde_json::json!({
        "test_files": 2
    }))
    .expect("legacy test presence");

    assert!(presence.frameworks.is_empty());
    assert_eq!(presence.test_files, 2);
}

#[test]
fn precise_clone_coordinates_serialize_a_real_zero_byte_offset() {
    let instance = CloneInstance {
        path: PathBuf::from("sample.rs"),
        start_line: 1,
        end_line: 3,
        start_column: 1,
        end_column: 2,
        start_byte: 0,
        end_byte: 24,
        start_token: 1,
        end_token: 8,
    };

    let value = serde_json::to_value(instance).expect("serialize clone instance");

    assert_eq!(value["start_byte"], 0);
    assert_eq!(value["start_token"], 1);
}
