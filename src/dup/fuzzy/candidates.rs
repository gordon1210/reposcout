use super::{
    ALPHA_BASE, Bijection, CHANGED_LITERAL_CREDIT, CandidateMatch, CandidateRun, CandidateStats,
    CloneGroup, CountingVerification, DiagonalKey, Duration, HashMap, HashSet, Instant,
    MAX_PREVIOUS_PER_WINDOW, MergedIntervals, NoVerification, Occurrence, PROGRESS_CHECK_EVERY,
    PlanDiagnostics, PoolScope, PreparedFile, RENAMED_IDENTIFIER_CREDIT, SuppressionOutcome,
    SuppressionStats, Token, TokenKind, Type2Progress, VerificationObserver, predecessor_indices,
};

#[expect(
    clippy::too_many_arguments,
    reason = "each field maps directly to the public indexing progress event and keeping them explicit prevents stale derived progress"
)]
pub(super) fn indexing_progress(
    scope: PoolScope<'_>,
    operation: &'static str,
    files_completed: usize,
    files_total: usize,
    current_file: Option<String>,
    current_units_completed: usize,
    current_units_total: usize,
    windows_indexed: usize,
    windows_total: usize,
    fingerprint_buckets: usize,
    started: Instant,
) -> Type2Progress {
    let elapsed = started.elapsed();
    Type2Progress::Indexing {
        pool: scope.name.to_string(),
        pool_index: scope.index,
        pools_total: scope.total,
        operation,
        files_completed,
        files_total,
        current_file,
        current_units_completed,
        current_units_total,
        windows_indexed,
        windows_total,
        fingerprint_buckets,
        phase_elapsed_ms: duration_ms(elapsed),
        windows_per_second: per_second(windows_indexed, elapsed),
    }
}

pub(super) fn candidate_planning_progress(
    scope: PoolScope<'_>,
    fingerprint_buckets_completed: usize,
    fingerprint_buckets_total: usize,
    plan: PlanDiagnostics,
    started: Instant,
) -> Type2Progress {
    let elapsed = started.elapsed();
    Type2Progress::PlanningCandidates {
        pool: scope.name.to_string(),
        pool_index: scope.index,
        pools_total: scope.total,
        fingerprint_buckets_completed,
        fingerprint_buckets_total,
        candidate_buckets: plan.candidate_buckets,
        candidate_buckets_selected: plan.candidate_buckets_selected,
        candidate_buckets_skipped: plan.candidate_buckets_skipped,
        candidate_buckets_partially_selected: plan.candidate_buckets_partially_selected,
        seed_pairs_total: plan.seed_pairs_total,
        seed_pairs_selected: plan.seed_pairs_selected,
        seed_pairs_skipped: plan.seed_pairs_skipped(),
        phase_elapsed_ms: duration_ms(elapsed),
        buckets_per_second: per_second(fingerprint_buckets_completed, elapsed),
    }
}

pub(super) fn candidate_progress(
    scope: PoolScope<'_>,
    stats: CandidateStats,
    buckets_total: usize,
    current_bucket_occurrences: usize,
    seed_pairs_total: u64,
    started: Instant,
) -> Type2Progress {
    let elapsed = started.elapsed();
    Type2Progress::CandidateSearch {
        pool: scope.name.to_string(),
        pool_index: scope.index,
        pools_total: scope.total,
        buckets_completed: stats.buckets_completed,
        buckets_total,
        current_bucket_occurrences,
        seed_pairs_completed: stats.seed_pairs_completed,
        seed_pairs_total,
        seeds_verified: stats.seeds_verified,
        seeds_skipped_same_file_overlap: stats.seeds_skipped_same_file_overlap,
        seeds_skipped_covered: stats.seeds_skipped_covered,
        covered_region_checks: stats.covered_region_checks,
        verification_tokens_compared: stats.verification_tokens_compared,
        qualified_matches: stats.qualified_matches,
        duplicate_regions_rejected: stats.duplicate_regions_rejected,
        exact_matches_rejected: stats.exact_matches_rejected,
        matches_buffered: stats.matches_buffered,
        phase_elapsed_ms: duration_ms(elapsed),
        seed_pairs_per_second: per_second_u64(stats.seed_pairs_completed, elapsed),
    }
}

pub(super) fn suppression_progress(
    scope: PoolScope<'_>,
    stats: SuppressionStats,
    matches_total: usize,
    started: Instant,
) -> Type2Progress {
    let elapsed = started.elapsed();
    Type2Progress::SuppressingOverlaps {
        pool: scope.name.to_string(),
        pool_index: scope.index,
        pools_total: scope.total,
        matches_completed: stats.matches_completed,
        matches_total,
        overlap_checks: stats.overlap_checks,
        matches_kept: stats.matches_kept,
        phase_elapsed_ms: duration_ms(elapsed),
        matches_per_second: per_second(stats.matches_completed, elapsed),
    }
}

pub(super) fn materialization_progress(
    scope: PoolScope<'_>,
    matches_completed: usize,
    matches_total: usize,
    groups_created: usize,
    started: Instant,
) -> Type2Progress {
    let elapsed = started.elapsed();
    Type2Progress::MaterializingGroups {
        pool: scope.name.to_string(),
        pool_index: scope.index,
        pools_total: scope.total,
        matches_completed,
        matches_total,
        groups_created,
        phase_elapsed_ms: duration_ms(elapsed),
        matches_per_second: per_second(matches_completed, elapsed),
    }
}

pub(super) fn window_count(tokens: usize, window: usize) -> usize {
    if window == 0 {
        0
    } else {
        tokens
            .checked_sub(window)
            .map_or(0, |remaining| remaining.saturating_add(1))
    }
}

pub(super) fn bounded_seed_pair_count(occurrences: usize) -> u64 {
    let rights = occurrences.saturating_sub(1) as u128;
    let maximum = MAX_PREVIOUS_PER_WINDOW as u128;
    let count = if rights <= maximum {
        rights.saturating_mul(rights + 1) / 2
    } else {
        maximum.saturating_mul(maximum + 1) / 2 + (rights - maximum).saturating_mul(maximum)
    };
    u64::try_from(count).unwrap_or(u64::MAX)
}

pub(super) fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub(super) fn per_second(completed: usize, elapsed: Duration) -> f64 {
    per_second_u64(u64::try_from(completed).unwrap_or(u64::MAX), elapsed)
}

pub(super) fn per_second_u64(completed: u64, elapsed: Duration) -> f64 {
    let seconds = elapsed.as_secs_f64();
    if seconds > 0.0 {
        crate::numeric::u64_to_f64(completed) / seconds
    } else {
        0.0
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the candidate hot path explicitly separates corpus input, mutable deduplication state, and hard work limits"
)]
pub(super) fn detect_candidates_fast(
    prepared: &[PreparedFile],
    occurrences: &[Occurrence],
    min_tokens: usize,
    threshold: f64,
    matches: &mut Vec<CandidateMatch>,
    seen_regions: &mut HashSet<(usize, usize, usize, usize, usize)>,
    covered_diagonals: &mut HashMap<DiagonalKey, MergedIntervals>,
    seed_pair_limit: u64,
    match_limit: usize,
) -> CandidateRun {
    if matches.len() >= match_limit {
        return CandidateRun {
            match_limit_reached: true,
            ..CandidateRun::default()
        };
    }
    let mut verification = NoVerification;
    let mut seed_pairs_completed = 0u64;
    for right in 1..occurrences.len() {
        let b = occurrences[right];
        for left in predecessor_indices(right, MAX_PREVIOUS_PER_WINDOW) {
            if seed_pairs_completed >= seed_pair_limit {
                return CandidateRun {
                    seed_pairs_completed,
                    match_limit_reached: false,
                };
            }
            seed_pairs_completed = seed_pairs_completed.saturating_add(1);
            let a = occurrences[left];
            if same_file_seed_overlaps(a, b, min_tokens)
                || seed_is_covered(covered_diagonals, a, b, min_tokens, &mut verification)
            {
                continue;
            }
            let Some((a_start, b_start, len, similarity)) =
                maximal_qualified_match(prepared, a, b, min_tokens, threshold, &mut verification)
            else {
                continue;
            };
            let region = canonical_region_key(a.file, a_start, b.file, b_start, len);
            if seen_regions.contains(&region) {
                continue;
            }
            if originals_equal(
                prepared,
                a.file,
                a_start,
                b.file,
                b_start,
                len,
                &mut verification,
            ) {
                remember_region(covered_diagonals, a.file, a_start, b.file, b_start, len);
                continue;
            }
            seen_regions.insert(region);
            remember_region(covered_diagonals, a.file, a_start, b.file, b_start, len);
            matches.push(candidate_match(
                prepared, a.file, a_start, b.file, b_start, len, similarity,
            ));
            if matches.len() >= match_limit {
                return CandidateRun {
                    seed_pairs_completed,
                    match_limit_reached: seed_pairs_completed < seed_pair_limit,
                };
            }
        }
    }
    CandidateRun {
        seed_pairs_completed,
        match_limit_reached: false,
    }
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the progress-aware candidate loop keeps every bounded counter and mutable result store explicit for auditable work accounting"
)]
pub(super) fn detect_candidates(
    prepared: &[PreparedFile],
    occurrences: &[Occurrence],
    min_tokens: usize,
    threshold: f64,
    matches: &mut Vec<CandidateMatch>,
    seen_regions: &mut HashSet<(usize, usize, usize, usize, usize)>,
    covered_diagonals: &mut HashMap<DiagonalKey, MergedIntervals>,
    stats: &mut CandidateStats,
    progress: &mut impl FnMut(CandidateStats),
    seed_pair_limit: u64,
    match_limit: usize,
) -> CandidateRun {
    if matches.len() >= match_limit {
        return CandidateRun {
            match_limit_reached: true,
            ..CandidateRun::default()
        };
    }
    let starting_seed_pairs = stats.seed_pairs_completed;
    for right in 1..occurrences.len() {
        let b = occurrences[right];
        for left in predecessor_indices(right, MAX_PREVIOUS_PER_WINDOW) {
            let bucket_seed_pairs = stats
                .seed_pairs_completed
                .saturating_sub(starting_seed_pairs);
            if bucket_seed_pairs >= seed_pair_limit {
                return CandidateRun {
                    seed_pairs_completed: bucket_seed_pairs,
                    match_limit_reached: false,
                };
            }
            let a = occurrences[left];
            stats.seed_pairs_completed = stats.seed_pairs_completed.saturating_add(1);
            if stats
                .seed_pairs_completed
                .is_multiple_of(PROGRESS_CHECK_EVERY as u64)
            {
                progress(*stats);
            }
            if same_file_seed_overlaps(a, b, min_tokens) {
                stats.seeds_skipped_same_file_overlap =
                    stats.seeds_skipped_same_file_overlap.saturating_add(1);
                continue;
            }
            let mut covered_region_checks = stats.covered_region_checks;
            let baseline = *stats;
            let mut report_coverage = |covered_region_checks| {
                progress(CandidateStats {
                    covered_region_checks,
                    ..baseline
                });
            };
            let mut coverage_verification = CountingVerification {
                completed: &mut covered_region_checks,
                progress: &mut report_coverage,
            };
            let covered = seed_is_covered(
                covered_diagonals,
                a,
                b,
                min_tokens,
                &mut coverage_verification,
            );
            stats.covered_region_checks = covered_region_checks;
            if covered {
                stats.seeds_skipped_covered = stats.seeds_skipped_covered.saturating_add(1);
                continue;
            }
            stats.seeds_verified = stats.seeds_verified.saturating_add(1);
            let mut compared = stats.verification_tokens_compared;
            let baseline = *stats;
            let mut report_verification = |verification_tokens_compared| {
                progress(CandidateStats {
                    verification_tokens_compared,
                    ..baseline
                });
            };
            let mut verification = CountingVerification {
                completed: &mut compared,
                progress: &mut report_verification,
            };
            let qualified =
                maximal_qualified_match(prepared, a, b, min_tokens, threshold, &mut verification);
            stats.verification_tokens_compared = compared;
            let Some((a_start, b_start, len, similarity)) = qualified else {
                continue;
            };
            stats.qualified_matches = stats.qualified_matches.saturating_add(1);
            let region = canonical_region_key(a.file, a_start, b.file, b_start, len);
            if seen_regions.contains(&region) {
                stats.duplicate_regions_rejected =
                    stats.duplicate_regions_rejected.saturating_add(1);
                continue;
            }
            let mut compared = stats.verification_tokens_compared;
            let baseline = *stats;
            let mut report_originals = |verification_tokens_compared| {
                progress(CandidateStats {
                    verification_tokens_compared,
                    ..baseline
                });
            };
            let mut verification = CountingVerification {
                completed: &mut compared,
                progress: &mut report_originals,
            };
            let is_exact = originals_equal(
                prepared,
                a.file,
                a_start,
                b.file,
                b_start,
                len,
                &mut verification,
            );
            stats.verification_tokens_compared = compared;
            if is_exact {
                stats.exact_matches_rejected = stats.exact_matches_rejected.saturating_add(1);
                remember_region(covered_diagonals, a.file, a_start, b.file, b_start, len);
                continue;
            }
            seen_regions.insert(region);
            remember_region(covered_diagonals, a.file, a_start, b.file, b_start, len);
            matches.push(candidate_match(
                prepared, a.file, a_start, b.file, b_start, len, similarity,
            ));
            stats.matches_buffered = matches.len();
            if matches.len() >= match_limit {
                let bucket_seed_pairs = stats
                    .seed_pairs_completed
                    .saturating_sub(starting_seed_pairs);
                return CandidateRun {
                    seed_pairs_completed: bucket_seed_pairs,
                    match_limit_reached: bucket_seed_pairs < seed_pair_limit,
                };
            }
        }
    }
    CandidateRun {
        seed_pairs_completed: stats
            .seed_pairs_completed
            .saturating_sub(starting_seed_pairs),
        match_limit_reached: false,
    }
}

pub(super) fn candidate_match(
    prepared: &[PreparedFile],
    a_file: usize,
    a_start: usize,
    b_file: usize,
    b_start: usize,
    len: usize,
    similarity: f64,
) -> CandidateMatch {
    let ((first_file, first_start), (second_file, second_start)) =
        if (a_file, a_start) <= (b_file, b_start) {
            ((a_file, a_start), (b_file, b_start))
        } else {
            ((b_file, b_start), (a_file, a_start))
        };
    CandidateMatch {
        first_file,
        first_start,
        second_file,
        second_start,
        len,
        lines: token_line_span(prepared, first_file, first_start, len).max(token_line_span(
            prepared,
            second_file,
            second_start,
            len,
        )),
        similarity,
    }
}

pub(super) fn token_line_span(
    prepared: &[PreparedFile],
    file: usize,
    start: usize,
    len: usize,
) -> usize {
    let tokens = &prepared[file].tokens;
    crate::dup::token_line_span(&tokens[start], &tokens[start + len - 1])
}

/// Reduce shifted variants while matches are still compact token coordinates.
/// Building full clone instances first is exceptionally costly on repetitive
/// corpora because most candidates are discarded by this same overlap rule.
#[cfg(test)]
pub(super) fn suppress_overlapping_matches(
    mut matches: Vec<CandidateMatch>,
) -> Vec<CandidateMatch> {
    sort_candidate_matches(&mut matches);
    let mut ignore_progress = |_| {};
    suppress_sorted_overlapping_matches(matches, u64::MAX, &mut ignore_progress).retained
}

pub(super) fn suppress_overlapping_matches_with_limit(
    mut matches: Vec<CandidateMatch>,
    max_overlap_checks: u64,
) -> SuppressionOutcome {
    sort_candidate_matches(&mut matches);
    let mut ignore_progress = |_| {};
    suppress_sorted_overlapping_matches(matches, max_overlap_checks, &mut ignore_progress)
}

pub(super) fn sort_candidate_matches(matches: &mut [CandidateMatch]) {
    matches.sort_by(|a, b| {
        b.len
            .cmp(&a.len)
            .then_with(|| b.lines.cmp(&a.lines))
            .then_with(|| b.similarity.total_cmp(&a.similarity))
            .then_with(|| {
                (a.first_file, a.first_start, a.second_file, a.second_start).cmp(&(
                    b.first_file,
                    b.first_start,
                    b.second_file,
                    b.second_start,
                ))
            })
    });
}

pub(super) fn suppress_sorted_overlapping_matches(
    matches: Vec<CandidateMatch>,
    max_overlap_checks: u64,
    progress: &mut impl FnMut(SuppressionStats),
) -> SuppressionOutcome {
    let matches_total = matches.len();
    let mut retained = Vec::with_capacity(matches_total);
    let mut by_pair: HashMap<(usize, usize), Vec<usize>> = HashMap::default();
    let mut stats = SuppressionStats::default();
    for candidate in matches {
        let pair = (candidate.first_file, candidate.second_file);
        let mut redundant = false;
        if let Some(indices) = by_pair.get(&pair) {
            for index in indices {
                if stats.overlap_checks >= max_overlap_checks {
                    return SuppressionOutcome {
                        retained,
                        stats,
                        limit_reached: true,
                        matches_skipped: matches_total.saturating_sub(stats.matches_completed),
                    };
                }
                stats.overlap_checks = stats.overlap_checks.saturating_add(1);
                if stats
                    .overlap_checks
                    .is_multiple_of(PROGRESS_CHECK_EVERY as u64)
                {
                    progress(stats);
                }
                if candidate_matches_overlap(retained[*index], candidate) {
                    redundant = true;
                    break;
                }
            }
        }
        if !redundant {
            let index = retained.len();
            retained.push(candidate);
            by_pair.entry(pair).or_default().push(index);
            stats.matches_kept = retained.len();
        }
        stats.matches_completed = stats.matches_completed.saturating_add(1);
        if stats.matches_completed.is_multiple_of(PROGRESS_CHECK_EVERY) {
            progress(stats);
        }
    }
    SuppressionOutcome {
        retained,
        stats,
        limit_reached: false,
        matches_skipped: 0,
    }
}

pub(super) fn candidate_matches_overlap(a: CandidateMatch, b: CandidateMatch) -> bool {
    token_ranges_overlap(a.first_start, a.len, b.first_start, b.len)
        && token_ranges_overlap(a.second_start, a.len, b.second_start, b.len)
}

pub(super) fn token_ranges_overlap(
    a_start: usize,
    a_len: usize,
    b_start: usize,
    b_len: usize,
) -> bool {
    a_start < b_start + b_len && b_start < a_start + a_len
}

/// Rolling parameterized fingerprints for every window. Identifiers are
/// represented by the distance to their previous occurrence inside the
/// window, or zero when first seen. This is rename-invariant and equivalent to
/// alpha-canonical names, but all windows are computed in linear time.
pub(super) fn alpha_window_hashes(tokens: &[Token], window: usize) -> Vec<u64> {
    if window == 0 || tokens.len() < window {
        return Vec::new();
    }

    let mut previous = vec![None; tokens.len()];
    let mut next = vec![None; tokens.len()];
    let mut last_identifier: HashMap<&str, usize> = HashMap::default();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Identifier {
            continue;
        }
        if let Some(prior) = last_identifier.insert(&token.text, index) {
            previous[index] = Some(prior);
            next[prior] = Some(index);
        }
    }

    let mut powers = Vec::with_capacity(window);
    powers.push(1u64);
    for exponent in 1..window {
        powers.push(powers[exponent - 1].wrapping_mul(ALPHA_BASE));
    }
    let window_power = powers[window - 1];

    let component = |index: usize, start: usize| match previous[index] {
        Some(prior) if prior >= start => (index - prior) as u64,
        _ if tokens[index].kind == TokenKind::Identifier => 0,
        _ => tokens[index].shape_hash(),
    };

    let mut hash = (0..window).fold(0u64, |hash, index| {
        hash.wrapping_mul(ALPHA_BASE)
            .wrapping_add(component(index, 0))
    });
    let mut hashes = Vec::with_capacity(tokens.len() - window + 1);
    hashes.push(hash);

    for start in 1..=tokens.len() - window {
        let outgoing = start - 1;
        let incoming = start + window - 1;
        hash = hash
            .wrapping_sub(component(outgoing, start - 1).wrapping_mul(window_power))
            .wrapping_mul(ALPHA_BASE)
            .wrapping_add(component(incoming, start));

        if let Some(next_occurrence) = next[outgoing]
            && next_occurrence < incoming
        {
            let distance = (next_occurrence - outgoing) as u64;
            let exponent = incoming - next_occurrence;
            hash = hash.wrapping_sub(distance.wrapping_mul(powers[exponent]));
        }
        hashes.push(hash);
    }
    hashes
}

pub(super) fn alpha_window_hashes_with_progress(
    tokens: &[Token],
    window: usize,
    mut progress: Option<&mut dyn FnMut(&'static str, usize, usize)>,
) -> Vec<u64> {
    if window == 0 || tokens.len() < window {
        return Vec::new();
    }

    let mut previous = vec![None; tokens.len()];
    let mut next = vec![None; tokens.len()];
    let mut last_identifier: HashMap<&str, usize> = HashMap::default();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind == TokenKind::Identifier
            && let Some(prior) = last_identifier.insert(&token.text, index)
        {
            previous[index] = Some(prior);
            next[prior] = Some(index);
        }
        if (index + 1).is_multiple_of(PROGRESS_CHECK_EVERY)
            && let Some(progress) = progress.as_deref_mut()
        {
            progress("linking_identifiers", index + 1, tokens.len());
        }
    }

    let mut powers = Vec::with_capacity(window);
    powers.push(1u64);
    for exponent in 1..window {
        powers.push(powers[exponent - 1].wrapping_mul(ALPHA_BASE));
        if (exponent + 1).is_multiple_of(PROGRESS_CHECK_EVERY)
            && let Some(progress) = progress.as_deref_mut()
        {
            progress("building_alpha_powers", exponent + 1, window);
        }
    }
    let window_power = powers[window - 1];

    let component = |index: usize, start: usize| match previous[index] {
        Some(prior) if prior >= start => (index - prior) as u64,
        _ if tokens[index].kind == TokenKind::Identifier => 0,
        _ => tokens[index].shape_hash(),
    };

    let mut hash = 0u64;
    for index in 0..window {
        hash = hash
            .wrapping_mul(ALPHA_BASE)
            .wrapping_add(component(index, 0));
        if (index + 1).is_multiple_of(PROGRESS_CHECK_EVERY)
            && let Some(progress) = progress.as_deref_mut()
        {
            progress("hashing_initial_window", index + 1, window);
        }
    }
    let mut hashes = Vec::with_capacity(tokens.len() - window + 1);
    hashes.push(hash);

    for start in 1..=tokens.len() - window {
        let outgoing = start - 1;
        let incoming = start + window - 1;
        hash = hash
            .wrapping_sub(component(outgoing, start - 1).wrapping_mul(window_power))
            .wrapping_mul(ALPHA_BASE)
            .wrapping_add(component(incoming, start));

        if let Some(next_occurrence) = next[outgoing]
            && next_occurrence < incoming
        {
            let distance = (next_occurrence - outgoing) as u64;
            let exponent = incoming - next_occurrence;
            hash = hash.wrapping_sub(distance.wrapping_mul(powers[exponent]));
        }
        hashes.push(hash);
        if (start + 1).is_multiple_of(PROGRESS_CHECK_EVERY)
            && let Some(progress) = progress.as_deref_mut()
        {
            progress(
                "hashing_alpha_windows",
                start + 1,
                tokens.len() - window + 1,
            );
        }
    }
    hashes
}

pub(super) fn seed_is_covered(
    covered: &HashMap<DiagonalKey, MergedIntervals>,
    a: Occurrence,
    b: Occurrence,
    len: usize,
    verification: &mut impl VerificationObserver,
) -> bool {
    let (key, start) = diagonal_key(a.file, a.start, b.file, b.start);
    covered.get(&key).is_some_and(|regions| {
        verification.compared();
        regions.covers(start, start.saturating_add(len))
    })
}

pub(super) fn remember_region(
    covered: &mut HashMap<DiagonalKey, MergedIntervals>,
    a_file: usize,
    a_start: usize,
    b_file: usize,
    b_start: usize,
    len: usize,
) {
    let (key, start) = diagonal_key(a_file, a_start, b_file, b_start);
    covered
        .entry(key)
        .or_default()
        .insert(start, start.saturating_add(len));
}

pub(super) fn diagonal_key(
    a_file: usize,
    a_start: usize,
    b_file: usize,
    b_start: usize,
) -> (DiagonalKey, usize) {
    let ((first_file, first_start), (second_file, second_start)) =
        if (a_file, a_start) <= (b_file, b_start) {
            ((a_file, a_start), (b_file, b_start))
        } else {
            ((b_file, b_start), (a_file, a_start))
        };
    (
        DiagonalKey {
            first_file,
            second_file,
            token_delta: second_start as i128 - first_start as i128,
        },
        first_start,
    )
}

pub(super) fn normalize_threshold(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(super) fn maximal_qualified_match(
    prepared: &[PreparedFile],
    a: Occurrence,
    b: Occurrence,
    seed_len: usize,
    threshold: f64,
    verification: &mut impl VerificationObserver,
) -> Option<(usize, usize, usize, f64)> {
    let left_tokens = &prepared.get(a.file)?.tokens;
    let right_tokens = &prepared.get(b.file)?.tokens;
    let same_file = a.file == b.file;
    let distance = a.start.abs_diff(b.start);
    if same_file && (distance == 0 || seed_len > distance) {
        return None;
    }

    let mut mapping = Bijection::default();
    let mut score = 0.0;
    for (left, right) in left_tokens[a.start..a.start + seed_len]
        .iter()
        .zip(&right_tokens[b.start..b.start + seed_len])
    {
        let credit = accept_pair(left, right, &mut mapping);
        verification.compared();
        score += credit?;
    }
    if score / crate::numeric::usize_to_f64(seed_len) < threshold {
        return None;
    }

    let mut best = (
        a.start,
        b.start,
        seed_len,
        score / crate::numeric::usize_to_f64(seed_len),
    );
    let mut a_start = a.start;
    let mut b_start = b.start;
    let mut len = seed_len;

    while a_start > 0 && b_start > 0 {
        if same_file && len + 1 > distance {
            break;
        }
        let credit = accept_pair(
            &left_tokens[a_start - 1],
            &right_tokens[b_start - 1],
            &mut mapping,
        );
        verification.compared();
        let Some(credit) = credit else {
            break;
        };
        score += credit;
        a_start -= 1;
        b_start -= 1;
        len += 1;
        let similarity = score / crate::numeric::usize_to_f64(len);
        if similarity >= threshold {
            best = (a_start, b_start, len, similarity);
        }
    }

    while a_start + len < left_tokens.len() && b_start + len < right_tokens.len() {
        if same_file && len + 1 > distance {
            break;
        }
        let credit = accept_pair(
            &left_tokens[a_start + len],
            &right_tokens[b_start + len],
            &mut mapping,
        );
        verification.compared();
        let Some(credit) = credit else {
            break;
        };
        score += credit;
        len += 1;
        let similarity = score / crate::numeric::usize_to_f64(len);
        if similarity >= threshold {
            best = (a_start, b_start, len, similarity);
        }
    }

    Some(best)
}

/// Validate one shape-compatible pair and return its similarity credit. Equal
/// identifier spellings still reserve both sides of the bijection.
pub(super) fn accept_pair<'a>(
    left: &'a Token,
    right: &'a Token,
    mapping: &mut Bijection<'a>,
) -> Option<f64> {
    if !left.shape_eq(right) {
        return None;
    }
    match (left.kind, right.kind) {
        (TokenKind::Identifier, TokenKind::Identifier) => {
            if !mapping.accept_identifier(&left.text, &right.text) {
                return None;
            }
            Some(if left.text == right.text {
                1.0
            } else {
                RENAMED_IDENTIFIER_CREDIT
            })
        }
        (TokenKind::Literal(_), TokenKind::Literal(_)) => Some(if left.text == right.text {
            1.0
        } else {
            CHANGED_LITERAL_CREDIT
        }),
        _ => left.exact_eq(right).then_some(1.0),
    }
}

pub(super) fn originals_equal(
    prepared: &[PreparedFile],
    a_file: usize,
    a_start: usize,
    b_file: usize,
    b_start: usize,
    len: usize,
    verification: &mut impl VerificationObserver,
) -> bool {
    prepared[a_file].tokens[a_start..a_start + len]
        .iter()
        .zip(&prepared[b_file].tokens[b_start..b_start + len])
        .all(|(a, b)| {
            let equal = a.exact_eq(b);
            verification.compared();
            equal
        })
}

pub(super) fn same_file_seed_overlaps(a: Occurrence, b: Occurrence, len: usize) -> bool {
    a.file == b.file && a.start.abs_diff(b.start) < len
}

pub(super) fn canonical_region_key(
    a_file: usize,
    a_start: usize,
    b_file: usize,
    b_start: usize,
    len: usize,
) -> (usize, usize, usize, usize, usize) {
    if (a_file, a_start) <= (b_file, b_start) {
        (a_file, a_start, b_file, b_start, len)
    } else {
        (b_file, b_start, a_file, a_start, len)
    }
}

pub(super) fn group_sort_key(group: &CloneGroup) -> (String, usize, usize) {
    group
        .instances
        .first()
        .map(|instance| {
            (
                instance.path.to_string_lossy().into_owned(),
                instance.start_byte,
                instance.end_byte,
            )
        })
        .unwrap_or_default()
}
