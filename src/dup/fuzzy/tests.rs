use super::*;
use crate::dup::exact;
use std::path::PathBuf;

fn input(path: &str, content: &str) -> DupInput {
    DupInput {
        path: PathBuf::from(path),
        content: content.to_string(),
    }
}

fn alpha_window_hashes_reference(tokens: &[Token], window: usize) -> Vec<u64> {
    if window == 0 || tokens.len() < window {
        return Vec::new();
    }
    (0..=tokens.len() - window)
        .map(|start| {
            let mut previous = HashMap::default();
            (start..start + window).fold(0u64, |hash, index| {
                let component = if tokens[index].kind == TokenKind::Identifier {
                    previous
                        .insert(tokens[index].text.as_str(), index)
                        .map_or(0, |prior| (index - prior) as u64)
                } else {
                    tokens[index].shape_hash()
                };
                hash.wrapping_mul(ALPHA_BASE).wrapping_add(component)
            })
        })
        .collect()
}

#[test]
fn rolling_alpha_hash_matches_reference_for_every_window() {
    let inputs = vec![input(
        "sample.rs",
        "let alpha = beta + alpha; let beta = alpha + gamma; alpha = beta + alpha + gamma;",
    )];
    let prepared = prepare(&inputs, DetectionOptions::default());
    let tokens = &prepared[0].tokens;

    for window in 1..=tokens.len() {
        assert_eq!(
            alpha_window_hashes(tokens, window),
            alpha_window_hashes_reference(tokens, window),
            "window {window}"
        );
    }
}

#[test]
fn detailed_progress_exposes_every_type2_phase_and_final_counters() {
    let inputs = vec![
        input(
            "a.rs",
            "let alpha = beta; alpha = alpha + beta; return alpha;",
        ),
        input(
            "b.rs",
            "let gamma = delta; gamma = gamma + delta; return gamma;",
        ),
    ];
    let prepared = prepare(&inputs, DetectionOptions::default());
    let fast_groups = detect_prepared(&inputs, &prepared, 8, 0.85);
    let mut events = Vec::new();
    let mut capture = |progress| events.push(progress);

    let groups = detect_prepared_with_limits_and_progress_interval(
        &inputs,
        &prepared,
        8,
        0.85,
        Type2Limits::default(),
        Some(&mut capture),
        Duration::ZERO,
    )
    .groups;

    assert!(!groups.is_empty());
    assert_eq!(
        serde_json::to_value(&groups).unwrap(),
        serde_json::to_value(&fast_groups).unwrap()
    );
    let phases = events
        .iter()
        .map(|event| {
            serde_json::to_value(event).unwrap()["phase"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(phases.first().unwrap(), "started");
    assert_eq!(phases.last().unwrap(), "finished");
    for required in [
        "pool_started",
        "indexing",
        "planning_candidates",
        "candidate_search",
        "sorting_matches",
        "suppressing_overlaps",
        "materializing_groups",
        "pool_finished",
        "sorting_groups",
    ] {
        assert!(phases.iter().any(|phase| phase == required), "{required}");
    }

    let completed = events
        .iter()
        .rev()
        .find_map(|event| match event {
            Type2Progress::CandidateSearch {
                buckets_completed,
                buckets_total,
                seed_pairs_completed,
                seed_pairs_total,
                verification_tokens_compared,
                matches_buffered,
                ..
            } if buckets_completed == buckets_total => Some((
                *seed_pairs_completed,
                *seed_pairs_total,
                *verification_tokens_compared,
                *matches_buffered,
            )),
            _ => None,
        })
        .expect("completed candidate-search event");
    assert_eq!(completed.0, completed.1);
    assert!(completed.2 > 0);
    assert!(completed.3 > 0);
}

#[test]
fn bounded_seed_pair_totals_match_the_predecessor_policy() {
    assert_eq!(bounded_seed_pair_count(0), 0);
    assert_eq!(bounded_seed_pair_count(1), 0);
    assert_eq!(bounded_seed_pair_count(10), 45);
    assert_eq!(bounded_seed_pair_count(66), 2_144);
}

#[test]
fn ordinary_corpus_is_unchanged_by_default_type2_limits() {
    let inputs = vec![
        input(
            "a.rs",
            "let alpha = beta; alpha = alpha + beta; return alpha;",
        ),
        input(
            "b.rs",
            "let gamma = delta; gamma = gamma + delta; return gamma;",
        ),
    ];
    let prepared = prepare(&inputs, DetectionOptions::default());

    let bounded = detect_prepared_bounded(&inputs, &prepared, 8, 0.85);
    let unlimited =
        detect_prepared_with_limits(&inputs, &prepared, 8, 0.85, Type2Limits::unlimited());

    assert!(!bounded.diagnostics.truncated);
    assert_eq!(
        serde_json::to_value(&bounded.groups).unwrap(),
        serde_json::to_value(&unlimited.groups).unwrap()
    );
}

#[test]
fn repetitive_json_keeps_coverage_lookup_proportional_to_seed_work() {
    let inputs = (0..4)
        .map(|file| {
            let objects = (0..30)
                .map(|item| {
                    format!(
                        r#"{{"series_{file}_{item}":{},"label":"value_{file}_{item}"}}"#,
                        file * 1_000 + item
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            input(&format!("data-{file}.json"), &format!("[{objects}]"))
        })
        .collect::<Vec<_>>();
    let prepared = prepare(&inputs, DetectionOptions::default());
    let mut events = Vec::new();
    let mut capture = |progress| events.push(progress);

    detect_prepared_with_limits_and_progress_interval(
        &inputs,
        &prepared,
        12,
        0.70,
        Type2Limits::default(),
        Some(&mut capture),
        Duration::ZERO,
    );

    let (seed_pairs, coverage_checks) = events
        .iter()
        .rev()
        .find_map(|event| match event {
            Type2Progress::CandidateSearch {
                buckets_completed,
                buckets_total,
                seed_pairs_completed,
                covered_region_checks,
                ..
            } if buckets_completed == buckets_total => {
                Some((*seed_pairs_completed, *covered_region_checks))
            }
            _ => None,
        })
        .expect("completed candidate-search diagnostics");
    assert!(seed_pairs > 0);
    assert!(
        coverage_checks <= seed_pairs,
        "{coverage_checks} coverage checks for {seed_pairs} seed pairs"
    );
}

#[test]
fn repetitive_json_obeys_candidate_and_match_budgets() {
    let inputs = (0..4)
        .map(|file| {
            let objects = (0..30)
                .map(|item| {
                    format!(
                        r#"{{"series_{file}_{item}":{},"label":"value_{file}_{item}"}}"#,
                        file * 1_000 + item
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            input(&format!("data-{file}.json"), &format!("[{objects}]"))
        })
        .collect::<Vec<_>>();
    let prepared = prepare(&inputs, DetectionOptions::default());
    let mut events = Vec::new();
    let mut capture = |progress| events.push(progress);

    let detection = detect_prepared_with_limits_and_progress_interval(
        &inputs,
        &prepared,
        12,
        0.70,
        Type2Limits {
            max_seed_pairs_per_pool: 5_000,
            max_matches_per_pool: 300,
            max_overlap_checks_per_pool: u64::MAX,
            rare_first: true,
        },
        Some(&mut capture),
        Duration::ZERO,
    );

    assert!(detection.diagnostics.truncated);
    assert!(detection.diagnostics.seed_pairs_skipped > 0);
    let (seed_pairs_completed, matches_buffered) = events
        .iter()
        .rev()
        .find_map(|event| match event {
            Type2Progress::CandidateSearch {
                seed_pairs_completed,
                matches_buffered,
                ..
            } => Some((*seed_pairs_completed, *matches_buffered)),
            _ => None,
        })
        .expect("candidate-search diagnostics");
    assert!(seed_pairs_completed <= 5_000);
    assert!(matches_buffered <= 300);
    let pool_partial = events.iter().rev().find_map(|event| match event {
        Type2Progress::PoolFinished {
            analysis_partial,
            seed_pairs_skipped,
            match_limit_reached,
            ..
        } => Some((*analysis_partial, *seed_pairs_skipped, *match_limit_reached)),
        _ => None,
    });
    assert_eq!(
        pool_partial,
        Some((true, detection.diagnostics.seed_pairs_skipped, true))
    );
    let finished = events.iter().rev().find_map(|event| match event {
        Type2Progress::Finished {
            analysis_partial,
            pools_truncated,
            seed_pairs_skipped,
            match_limit_reached,
            ..
        } => Some((
            *analysis_partial,
            *pools_truncated,
            *seed_pairs_skipped,
            *match_limit_reached,
        )),
        _ => None,
    });
    assert_eq!(
        finished,
        Some((
            true,
            detection.diagnostics.pools_truncated,
            detection.diagnostics.seed_pairs_skipped,
            true,
        ))
    );
}

#[test]
fn compact_overlap_suppression_keeps_larger_and_distinct_matches() {
    let candidate = |first_start, second_start, len| CandidateMatch {
        first_file: 0,
        first_start,
        second_file: 1,
        second_start,
        len,
        lines: len,
        similarity: 0.9,
    };
    let large = candidate(0, 100, 100);
    let shifted = candidate(20, 120, 80);
    let distinct = candidate(200, 300, 60);

    let out = suppress_overlapping_matches(vec![shifted, distinct, large]);

    assert_eq!(out.len(), 2);
    assert!(out.iter().any(|item| item.len == 100));
    assert!(out.iter().any(|item| item.len == 60));
}

#[test]
fn overlap_suppression_stops_at_its_comparison_budget() {
    let matches = (0..50)
        .map(|index| CandidateMatch {
            first_file: 0,
            first_start: 0,
            second_file: 1,
            second_start: index * 20,
            len: 10,
            lines: 1,
            similarity: 0.9,
        })
        .collect();

    let outcome = suppress_overlapping_matches_with_limit(matches, 100);

    assert!(outcome.limit_reached);
    assert_eq!(outcome.stats.overlap_checks, 100);
    assert!(outcome.matches_skipped > 0);
    assert_eq!(
        outcome.stats.matches_completed + outcome.matches_skipped,
        50
    );
}

#[test]
fn finds_consistent_identifier_rename_without_exact_clone() {
    let inputs = vec![
        input(
            "a.rs",
            "let alpha = beta; alpha = alpha + beta; return alpha;",
        ),
        input(
            "b.rs",
            "let gamma = delta; gamma = gamma + delta; return gamma;",
        ),
    ];

    assert!(exact::detect(&inputs, 8).is_empty());
    assert!(!detect(&inputs, 8, 0.85).is_empty());
}

#[test]
fn inconsistent_identifier_mapping_is_rejected() {
    let inputs = vec![
        input("a.rs", "let out = left + left + left + left;"),
        input("b.rs", "let out = one + two + one + two;"),
    ];

    assert!(detect(&inputs, 6, 0.50).is_empty());
}

#[test]
fn unchanged_identifiers_also_reserve_the_bijection() {
    let inputs = vec![
        input("a.rs", "let out = x + y + x + y;"),
        input("b.rs", "let out = x + x + x + x;"),
    ];

    assert!(detect(&inputs, 6, 0.50).is_empty());
}

#[test]
fn exact_clone_is_not_reported_as_near() {
    let source = "let value = input + 1; return value;";
    let inputs = vec![input("a.rs", source), input("b.rs", source)];

    assert!(detect(&inputs, 6, 0.50).is_empty());
}

#[test]
fn string_and_number_shapes_do_not_match() {
    let inputs = vec![
        input("a.rs", "let value = \"42\"; return value;"),
        input("b.rs", "let value = 42; return value;"),
    ];

    assert!(detect(&inputs, 5, 0.50).is_empty());
}

#[test]
fn generic_signed_exponents_participate_in_type2_detection() {
    let inputs = vec![
        input("a.c", "double alpha = 1e-3 + 7; return alpha;"),
        input("b.c", "double beta = 2E+4 + 8; return beta;"),
    ];

    assert!(exact::detect(&inputs, 8).is_empty());
    assert!(!detect(&inputs, 8, 0.65).is_empty());
}

#[test]
fn adjacent_mapping_conflict_does_not_hide_valid_seed() {
    let inputs = vec![
        input(
            "a.rs",
            "wrong + wrong; let alpha = beta + alpha; return alpha;",
        ),
        input(
            "b.rs",
            "one + two; let gamma = delta + gamma; return gamma;",
        ),
    ];

    assert!(!detect(&inputs, 8, 0.75).is_empty());
}

#[test]
fn repetitive_type2_input_is_bounded_and_deterministic() {
    let inputs = vec![
        input("a.rs", &"let alpha = 1;\n".repeat(200)),
        input("b.rs", &"let beta = 1;\n".repeat(200)),
    ];
    let expected_tokens = prepare(&inputs, DetectionOptions::default())[0]
        .tokens
        .len();

    let first = detect(&inputs, 20, 0.85);
    let second = detect(&inputs, 20, 0.85);

    assert_eq!(first.len(), second.len());
    assert!(first.len() < 128, "{} groups", first.len());
    assert!(first.iter().any(|group| group.tokens == expected_tokens));
    assert_eq!(
        first
            .iter()
            .map(|group| (group.tokens, group.similarity))
            .collect::<Vec<_>>(),
        second
            .iter()
            .map(|group| (group.tokens, group.similarity))
            .collect::<Vec<_>>()
    );
}

#[test]
fn maximum_threshold_returns_without_linear_power_setup() {
    let inputs = vec![
        input("a.rs", "let alpha = 1;"),
        input("b.rs", "let beta = 1;"),
    ];

    assert!(detect(&inputs, usize::MAX, 0.85).is_empty());
}
