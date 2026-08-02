use super::*;

#[test]
fn ordinary_detection_reports_complete_type2_analysis() {
    let inputs = vec![
        input("a.rs", "let alpha = beta; alpha = alpha + beta;"),
        input("b.rs", "let gamma = delta; gamma = gamma + delta;"),
    ];

    let result = analyze(&inputs, 8, 1, 0.85, DetectionOptions::default());

    assert_eq!(result.type2_diagnostics, fuzzy::Type2Diagnostics::default());
}

fn type2_content_fingerprint(content: &str) -> String {
    let inputs = vec![DupInput {
        path: PathBuf::from("sample.rs"),
        content: content.to_string(),
    }];
    let prepared = prepare(&inputs, DetectionOptions::default());
    let tokens = prepared[0].tokens.len();
    let mut groups = vec![CloneGroup {
        lines: content.lines().count(),
        tokens,
        similarity: 0.9,
        format: "Rust".to_string(),
        fingerprint: String::new(),
        instances: vec![CloneInstance {
            path: PathBuf::from("sample.rs"),
            start_line: 1,
            end_line: content.lines().count(),
            start_token: 1,
            end_token: tokens,
            ..CloneInstance::default()
        }],
    }];
    assign_group_fingerprints(
        &mut groups,
        "type2",
        &prepared,
        &inputs,
        DetectionOptions::default(),
    );
    groups.remove(0).fingerprint
}

#[test]
fn type2_family_fingerprints_preserve_identifier_relationships() {
    let repeated = type2_content_fingerprint("fn score(a: i32) { let b = a + a; }");
    let renamed = type2_content_fingerprint("fn total(x: i32) { let y = x + x; }");
    let distinct = type2_content_fingerprint("fn total(x: i32) { let y = x + y; }");

    assert_eq!(repeated, renamed, "alpha-renames must keep family identity");
    assert_ne!(
        repeated, distinct,
        "different identifier relationships need different identities"
    );
}

fn group(instances: &[(&str, usize, usize)], tokens: usize) -> CloneGroup {
    CloneGroup {
        lines: 1,
        tokens,
        similarity: 1.0,
        format: "Rust".to_string(),
        fingerprint: String::new(),
        instances: instances
            .iter()
            .map(|(path, start, end)| CloneInstance {
                path: PathBuf::from(path),
                start_line: *start,
                end_line: *end,
                ..CloneInstance::default()
            })
            .collect(),
    }
}

#[test]
fn duplicate_coverage_unions_line_and_token_ranges() {
    let mut first = group(&[("a.rs", 2, 4), ("b.rs", 1, 2)], 30);
    first.instances[0].start_token = 2;
    first.instances[0].end_token = 6;
    first.instances[1].start_token = 1;
    first.instances[1].end_token = 4;
    let mut near = group(&[("a.rs", 4, 6), ("b.rs", 2, 3)], 25);
    near.instances[0].start_token = 5;
    near.instances[0].end_token = 8;
    near.instances[1].start_token = 3;
    near.instances[1].end_token = 5;
    let duplication = Duplication {
        exact: vec![first],
        near: vec![near],
        ..Duplication::default()
    };

    let coverage = DuplicateCoverage::from_duplication(&duplication);

    assert_eq!(coverage.covered_lines(Path::new("a.rs")), 5);
    assert_eq!(coverage.covered_lines(Path::new("b.rs")), 3);
    assert_eq!(coverage.total_lines(), 8);
    assert_eq!(
        coverage.covered_lines_excluding(
            Path::new("a.rs"),
            &[
                LineRange { start: 3, end: 4 },
                LineRange { start: 4, end: 5 },
            ],
        ),
        2
    );
    assert_eq!(coverage.covered_tokens(Path::new("a.rs")), 7);
    assert_eq!(coverage.covered_tokens(Path::new("b.rs")), 5);
    assert_eq!(coverage.total_tokens(), 12);
}

#[test]
fn interval_coverage_merges_large_ranges_without_per_index_storage() {
    let mut coverage = IntervalSet::default();
    coverage.insert(0..1_000_000);
    coverage.insert(500_000..1_500_000);
    coverage.insert(1_500_000..2_000_000);

    assert_eq!(coverage.len(), 2_000_000);
    assert_eq!(coverage.intervals, vec![0..2_000_000]);
}

#[test]
fn strict_trailing_newline_uses_precise_endpoint_without_phantom_coverage() {
    let inputs = vec![input("a.rs", "shared\n"), input("b.rs", "shared\n")];
    let result = analyze(
        &inputs,
        2,
        1,
        0.85,
        DetectionOptions {
            mode: DuplicationMode::Strict,
            ..DetectionOptions::default()
        },
    );
    let group = result.duplication.exact.first().expect("exact clone");

    assert_eq!(group.lines, 1);
    assert!(
        group
            .instances
            .iter()
            .all(|instance| instance.end_line == 2)
    );
    assert!(
        group
            .instances
            .iter()
            .all(|instance| instance.end_column == 1)
    );
    assert_eq!(result.coverage.covered_lines(Path::new("a.rs")), 1);
    assert_eq!(result.coverage.covered_lines(Path::new("b.rs")), 1);
    assert_eq!(result.coverage.total_lines(), 2);
}

#[test]
fn rolling_power_matches_linear_reference_and_handles_maximum_window() {
    for window in 0..128 {
        let expected = (1..window).fold(1u64, |power, _| power.wrapping_mul(ROLLING_BASE));
        assert_eq!(rolling_power(window), expected, "window {window}");
    }

    let _ = rolling_power(usize::MAX);
}

#[test]
fn longer_same_start_instance_wins_overlap_pruning() {
    let block = group(&[("a.rs", 10, 12), ("a.rs", 10, 16), ("b.rs", 2, 8)], 60);

    let out = prune_overlaps(vec![block]);

    assert_eq!(out.len(), 1);
    assert_eq!(out[0].instances.len(), 2);
    assert_eq!(out[0].instances[0].end_line, 16);
    assert_eq!(out[0].lines, 7);
}

#[test]
fn dedup_uses_precise_ranges_not_only_lines() {
    let mut first = group(&[("a.rs", 1, 1), ("b.rs", 1, 1)], 10);
    first.instances[0].start_byte = 1;
    first.instances[0].end_byte = 5;
    let mut second = first.clone();
    second.instances[0].start_byte = 8;
    second.instances[0].end_byte = 12;

    assert_eq!(dedup_groups(vec![first, second]).len(), 2);
}

#[test]
fn contained_group_is_suppressed_only_for_the_same_copy_set() {
    let large = group(&[("a.rs", 1, 10), ("b.rs", 1, 10)], 100);
    let small = group(&[("a.rs", 2, 4), ("b.rs", 2, 4)], 20);
    let extra_copy = group(&[("a.rs", 2, 4), ("b.rs", 2, 4), ("c.rs", 2, 4)], 20);

    let out = suppress_contained(vec![large, small, extra_copy]);

    assert_eq!(out.len(), 2);
    assert!(out.iter().any(|candidate| candidate.instances.len() == 3));
}

fn input(path: &str, content: &str) -> DupInput {
    DupInput {
        path: PathBuf::from(path),
        content: content.to_string(),
    }
}

#[test]
fn weak_mode_ignores_comment_only_differences() {
    let inputs = vec![
        input(
            "a.rs",
            "fn value() { let item = 1; /* alpha */ return item + 2; }",
        ),
        input(
            "b.rs",
            "fn value() { let item = 1; /* beta */ return item + 2; }",
        ),
    ];

    let mild = analyze(&inputs, 15, 1, 0.85, DetectionOptions::default());
    let weak = analyze(
        &inputs,
        15,
        1,
        0.85,
        DetectionOptions {
            mode: DuplicationMode::Weak,
            ..DetectionOptions::default()
        },
    );

    assert!(mild.duplication.exact.is_empty());
    assert!(!weak.duplication.exact.is_empty());
}

#[test]
fn near_instances_can_have_different_physical_spans() {
    let inputs = vec![
        input(
            "a.rs",
            "fn first() {\n let alpha = 1;\n return alpha + 2;\n}",
        ),
        input(
            "b.rs",
            "fn second() {\n /* note\n    continued */\n let beta = 1;\n return beta + 2;\n}",
        ),
    ];
    let result = analyze(
        &inputs,
        8,
        1,
        0.75,
        DetectionOptions {
            mode: DuplicationMode::Weak,
            ..DetectionOptions::default()
        },
    );

    let finding = result
        .duplication
        .findings
        .iter()
        .find(|finding| finding.kind == "type2")
        .expect("Type-2 finding");
    assert_ne!(finding.lines_a, finding.lines_b);
    assert_eq!(
        finding.removable_lines,
        finding.lines_a.min(finding.lines_b)
    );
}

#[test]
fn snippets_are_opt_in_and_bounded() {
    let source = "fn shared() {\n let value = 1;\n return value + 2;\n}";
    let inputs = vec![input("a.rs", source), input("b.rs", source)];
    let result = analyze(
        &inputs,
        8,
        1,
        0.85,
        DetectionOptions {
            report_snippets: true,
            ..DetectionOptions::default()
        },
    );

    let finding = result.duplication.findings.first().expect("finding");
    assert!(finding.fragment_a.snippet.is_some());
    assert!(finding.fragment_b.snippet.is_some());
}

#[test]
fn duplication_progress_reports_every_expensive_phase_in_order() {
    let source = "fn shared() {\n let value = 1;\n return value + 2;\n}";
    let inputs = vec![input("a.rs", source), input("b.rs", source)];
    let mut stages = Vec::new();

    analyze_with_progress(&inputs, 8, 1, 0.85, DetectionOptions::default(), |stage| {
        stages.push(stage);
    });

    assert_eq!(
        stages,
        vec![
            DetectionStage::Tokenizing,
            DetectionStage::ExactClones,
            DetectionStage::Type2Clones,
            DetectionStage::Finalizing,
        ]
    );
}
