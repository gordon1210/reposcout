//! Type-2 clone detection with identifier-renaming validation.
//!
//! Candidate discovery uses rolling hashes over token *shapes*. Every retained
//! pair is then verified with a pair-local two-way identifier mapping. Literal
//! categories are preserved separately and exact clones are omitted.
//!
//! ## Contract (frozen)
//! `detect(inputs, min_tokens, min_similarity) -> Vec<CloneGroup>`

mod plan;
mod progress;

use crate::dup::{
    DetectionOptions, DupInput, PreparedFile, Token, TokenKind, clone_instance,
    predecessor_indices, prepare, roll_window, rolling_power, window_hash,
};
use crate::model::CloneGroup;
use plan::{CandidatePlan, PlanDiagnostics};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use progress::ProgressReporter;
pub(crate) use progress::Type2Progress;

const RENAMED_IDENTIFIER_CREDIT: f64 = 0.80;
const CHANGED_LITERAL_CREDIT: f64 = 0.70;
const MAX_PREVIOUS_PER_WINDOW: usize = 64;
const ALPHA_BASE: u64 = 0x517c_c1b7_2722_0a95;
const TYPE2_PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
const PROGRESS_CHECK_EVERY: usize = 16_384;
pub const MAX_SEED_PAIRS_PER_POOL: u64 = 10_000_000;
pub const MAX_MATCHES_PER_POOL: usize = 250_000;
pub const MAX_OVERLAP_CHECKS_PER_POOL: u64 = 10_000_000;

#[derive(Debug, Clone, Copy)]
struct Type2Limits {
    max_seed_pairs_per_pool: u64,
    max_matches_per_pool: usize,
    max_overlap_checks_per_pool: u64,
    rare_first: bool,
}

impl Default for Type2Limits {
    fn default() -> Self {
        Self {
            max_seed_pairs_per_pool: MAX_SEED_PAIRS_PER_POOL,
            max_matches_per_pool: MAX_MATCHES_PER_POOL,
            max_overlap_checks_per_pool: MAX_OVERLAP_CHECKS_PER_POOL,
            rare_first: true,
        }
    }
}

impl Type2Limits {
    fn unlimited() -> Self {
        Self {
            max_seed_pairs_per_pool: u64::MAX,
            max_matches_per_pool: usize::MAX,
            max_overlap_checks_per_pool: u64::MAX,
            rare_first: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Type2Diagnostics {
    pub truncated: bool,
    pub pools_truncated: usize,
    pub candidate_buckets_skipped: usize,
    pub candidate_buckets_partially_selected: usize,
    pub seed_pairs_skipped: u64,
    pub match_limit_reached: bool,
    pub suppression_limit_reached: bool,
    pub matches_skipped_during_suppression: usize,
}

impl Type2Diagnostics {
    fn record_pool(&mut self, outcome: PoolOutcome) {
        let seed_pairs_skipped = outcome
            .plan
            .seed_pairs_total
            .saturating_sub(outcome.seed_pairs_completed);
        let truncated = seed_pairs_skipped > 0
            || outcome.match_limit_reached
            || outcome.suppression_limit_reached;
        self.truncated |= truncated;
        self.pools_truncated += usize::from(truncated);
        self.candidate_buckets_skipped += outcome.plan.candidate_buckets_skipped;
        self.candidate_buckets_partially_selected +=
            outcome.plan.candidate_buckets_partially_selected;
        self.seed_pairs_skipped = self.seed_pairs_skipped.saturating_add(seed_pairs_skipped);
        self.match_limit_reached |= outcome.match_limit_reached;
        self.suppression_limit_reached |= outcome.suppression_limit_reached;
        self.matches_skipped_during_suppression = self
            .matches_skipped_during_suppression
            .saturating_add(outcome.matches_skipped_during_suppression);
    }
}

pub(crate) struct Type2Detection {
    pub groups: Vec<CloneGroup>,
    pub diagnostics: Type2Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Occurrence {
    file: usize,
    start: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DiagonalKey {
    first_file: usize,
    second_file: usize,
    token_delta: i128,
}

/// Covered token ranges for one file-pair diagonal.
///
/// Ranges remain disjoint and merged, so containment needs only the nearest
/// predecessor instead of a linear scan through every earlier match.
#[derive(Default)]
struct MergedIntervals {
    by_start: BTreeMap<usize, usize>,
}

impl MergedIntervals {
    fn covers(&self, start: usize, end: usize) -> bool {
        self.by_start
            .range(..=start)
            .next_back()
            .is_some_and(|(_, covered_end)| *covered_end >= end)
    }

    fn insert(&mut self, start: usize, end: usize) {
        let mut merged_start = start;
        let mut merged_end = end;

        if let Some((&previous_start, &previous_end)) = self.by_start.range(..=start).next_back()
            && previous_end >= start
        {
            merged_start = previous_start;
            merged_end = merged_end.max(previous_end);
            self.by_start.remove(&previous_start);
        }

        loop {
            let next = self
                .by_start
                .range(merged_start..)
                .next()
                .map(|(&next_start, &next_end)| (next_start, next_end));
            let Some((next_start, next_end)) = next else {
                break;
            };
            if next_start > merged_end {
                break;
            }
            merged_end = merged_end.max(next_end);
            self.by_start.remove(&next_start);
        }

        self.by_start.insert(merged_start, merged_end);
    }
}

#[derive(Debug, Clone, Copy)]
struct CandidateMatch {
    first_file: usize,
    first_start: usize,
    second_file: usize,
    second_start: usize,
    len: usize,
    lines: usize,
    similarity: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct CandidateStats {
    buckets_completed: usize,
    seed_pairs_completed: u64,
    seeds_verified: u64,
    seeds_skipped_same_file_overlap: u64,
    seeds_skipped_covered: u64,
    covered_region_checks: u64,
    verification_tokens_compared: u64,
    qualified_matches: u64,
    duplicate_regions_rejected: u64,
    exact_matches_rejected: u64,
    matches_buffered: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct PoolOutcome {
    plan: PlanDiagnostics,
    seed_pairs_completed: u64,
    match_limit_reached: bool,
    suppression_limit_reached: bool,
    matches_skipped_during_suppression: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct CandidateRun {
    seed_pairs_completed: u64,
    match_limit_reached: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct SuppressionStats {
    matches_completed: usize,
    overlap_checks: u64,
    matches_kept: usize,
}

struct SuppressionOutcome {
    retained: Vec<CandidateMatch>,
    stats: SuppressionStats,
    limit_reached: bool,
    matches_skipped: usize,
}

#[derive(Debug, Clone, Copy)]
struct PoolScope<'a> {
    name: &'a str,
    index: usize,
    total: usize,
}

trait VerificationObserver {
    fn compared(&mut self);
}

struct NoVerification;

impl VerificationObserver for NoVerification {
    #[inline(always)]
    fn compared(&mut self) {}
}

struct CountingVerification<'a, F> {
    completed: &'a mut u64,
    progress: &'a mut F,
}

impl<F> VerificationObserver for CountingVerification<'_, F>
where
    F: FnMut(u64),
{
    fn compared(&mut self) {
        *self.completed = (*self.completed).saturating_add(1);
        if (*self.completed).is_multiple_of(PROGRESS_CHECK_EVERY as u64) {
            (self.progress)(*self.completed);
        }
    }
}

#[derive(Default)]
struct Bijection<'a> {
    forward: HashMap<&'a str, &'a str>,
    reverse: HashMap<&'a str, &'a str>,
}

impl<'a> Bijection<'a> {
    fn accept_identifier(&mut self, left: &'a str, right: &'a str) -> bool {
        if self
            .forward
            .get(left)
            .is_some_and(|existing| *existing != right)
            || self
                .reverse
                .get(right)
                .is_some_and(|existing| *existing != left)
        {
            return false;
        }
        self.forward.entry(left).or_insert(right);
        self.reverse.entry(right).or_insert(left);
        true
    }
}

#[must_use]
pub fn detect(inputs: &[DupInput], min_tokens: usize, min_similarity: f64) -> Vec<CloneGroup> {
    let prepared = prepare(inputs, DetectionOptions::default());
    detect_prepared(inputs, &prepared, min_tokens, min_similarity)
}

pub(crate) fn detect_prepared(
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    min_tokens: usize,
    min_similarity: f64,
) -> Vec<CloneGroup> {
    detect_prepared_with_limits(
        inputs,
        prepared,
        min_tokens,
        min_similarity,
        Type2Limits::unlimited(),
    )
    .groups
}

pub(crate) fn detect_prepared_bounded(
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    min_tokens: usize,
    min_similarity: f64,
) -> Type2Detection {
    detect_prepared_with_limits(
        inputs,
        prepared,
        min_tokens,
        min_similarity,
        Type2Limits::default(),
    )
}

fn detect_prepared_with_limits(
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    min_tokens: usize,
    min_similarity: f64,
    limits: Type2Limits,
) -> Type2Detection {
    if min_tokens == 0 || prepared.is_empty() {
        return Type2Detection {
            groups: Vec::new(),
            diagnostics: Type2Diagnostics::default(),
        };
    }
    let threshold = normalize_threshold(min_similarity);
    let mut pools: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (file, source) in prepared.iter().enumerate() {
        pools.entry(&source.pool).or_default().push(file);
    }

    let mut groups = Vec::new();
    let mut seen_regions = HashSet::default();
    let mut diagnostics = Type2Diagnostics::default();
    for (pool, files) in pools {
        let outcome = detect_pool_fast(
            inputs,
            prepared,
            pool,
            &files,
            min_tokens,
            threshold,
            &mut groups,
            &mut seen_regions,
            limits,
        );
        diagnostics.record_pool(outcome);
    }
    groups.sort_by(|a, b| {
        b.tokens
            .cmp(&a.tokens)
            .then_with(|| b.similarity.total_cmp(&a.similarity))
            .then_with(|| group_sort_key(a).cmp(&group_sort_key(b)))
    });
    Type2Detection {
        groups,
        diagnostics,
    }
}

pub(crate) fn detect_prepared_bounded_with_progress(
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    min_tokens: usize,
    min_similarity: f64,
    progress: Option<&mut dyn FnMut(Type2Progress)>,
) -> Type2Detection {
    detect_prepared_with_limits_and_progress_interval(
        inputs,
        prepared,
        min_tokens,
        min_similarity,
        Type2Limits::default(),
        progress,
        TYPE2_PROGRESS_INTERVAL,
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "the detector entry point keeps immutable corpus inputs, limits, and the optional progress sink explicit while coordinating one bounded run"
)]
fn detect_prepared_with_limits_and_progress_interval(
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    min_tokens: usize,
    min_similarity: f64,
    limits: Type2Limits,
    progress: Option<&mut dyn FnMut(Type2Progress)>,
    progress_interval: Duration,
) -> Type2Detection {
    let started = Instant::now();
    let mut reporter = ProgressReporter::new(progress, progress_interval);
    let threshold = normalize_threshold(min_similarity);
    let mut pools: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (file, source) in prepared.iter().enumerate() {
        pools.entry(&source.pool).or_default().push(file);
    }
    let pools_total = pools.len();
    let eligible_files = prepared
        .iter()
        .filter(|file| min_tokens > 0 && file.tokens.len() >= min_tokens)
        .count();
    let tokens_total = prepared
        .iter()
        .map(|file| file.tokens.len())
        .fold(0usize, usize::saturating_add);
    let windows_total = prepared
        .iter()
        .map(|file| window_count(file.tokens.len(), min_tokens))
        .fold(0usize, usize::saturating_add);
    reporter.force(Type2Progress::Started {
        pools_total,
        files_total: prepared.len(),
        eligible_files,
        tokens_total,
        windows_total,
        min_tokens,
        min_similarity: threshold,
        max_seed_pairs_per_pool: limits.max_seed_pairs_per_pool,
        max_matches_per_pool: limits.max_matches_per_pool,
        max_overlap_checks_per_pool: limits.max_overlap_checks_per_pool,
    });

    if min_tokens == 0 || prepared.is_empty() {
        reporter.force(Type2Progress::Finished {
            duration_ms: duration_ms(started.elapsed()),
            groups_total: 0,
            analysis_partial: false,
            pools_truncated: 0,
            seed_pairs_skipped: 0,
            match_limit_reached: false,
            suppression_limit_reached: false,
            matches_skipped_during_suppression: 0,
        });
        return Type2Detection {
            groups: Vec::new(),
            diagnostics: Type2Diagnostics::default(),
        };
    }

    let mut groups = Vec::new();
    let mut seen_regions = HashSet::default();
    let mut diagnostics = Type2Diagnostics::default();
    for (pool_offset, (pool, files)) in pools.into_iter().enumerate() {
        let outcome = detect_pool(
            inputs,
            prepared,
            PoolScope {
                name: pool,
                index: pool_offset + 1,
                total: pools_total,
            },
            &files,
            min_tokens,
            threshold,
            &mut groups,
            &mut seen_regions,
            &mut reporter,
            limits,
        );
        diagnostics.record_pool(outcome);
    }
    let sorting_started = Instant::now();
    reporter.force(Type2Progress::SortingGroups {
        status: "started",
        groups_total: groups.len(),
        phase_elapsed_ms: 0,
    });
    groups.sort_by(|a, b| {
        b.tokens
            .cmp(&a.tokens)
            .then_with(|| b.similarity.total_cmp(&a.similarity))
            .then_with(|| group_sort_key(a).cmp(&group_sort_key(b)))
    });
    reporter.force(Type2Progress::SortingGroups {
        status: "completed",
        groups_total: groups.len(),
        phase_elapsed_ms: duration_ms(sorting_started.elapsed()),
    });
    reporter.force(Type2Progress::Finished {
        duration_ms: duration_ms(started.elapsed()),
        groups_total: groups.len(),
        analysis_partial: diagnostics.truncated,
        pools_truncated: diagnostics.pools_truncated,
        seed_pairs_skipped: diagnostics.seed_pairs_skipped,
        match_limit_reached: diagnostics.match_limit_reached,
        suppression_limit_reached: diagnostics.suppression_limit_reached,
        matches_skipped_during_suppression: diagnostics.matches_skipped_during_suppression,
    });
    Type2Detection {
        groups,
        diagnostics,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the fast detector hot path keeps its borrowed corpus, mutable result stores, and hard limits explicit to avoid an opaque state bundle"
)]
fn detect_pool_fast(
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    pool: &str,
    files: &[usize],
    min_tokens: usize,
    threshold: f64,
    groups: &mut Vec<CloneGroup>,
    seen_regions: &mut HashSet<(usize, usize, usize, usize, usize)>,
    limits: Type2Limits,
) -> PoolOutcome {
    if !files
        .iter()
        .any(|file| prepared[*file].tokens.len() >= min_tokens)
    {
        return PoolOutcome::default();
    }
    let power = rolling_power(min_tokens);
    let mut index: HashMap<(u64, u64), Vec<Occurrence>> = HashMap::default();
    let mut covered_diagonals: HashMap<DiagonalKey, MergedIntervals> = HashMap::default();
    let mut matches = Vec::new();

    for &file in files {
        let tokens = &prepared[file].tokens;
        if tokens.len() < min_tokens {
            continue;
        }
        let hashes = tokens.iter().map(Token::shape_hash).collect::<Vec<_>>();
        let alpha_hashes = alpha_window_hashes(tokens, min_tokens);
        let mut hash = window_hash(&hashes[..min_tokens]);
        for start in 0..=hashes.len() - min_tokens {
            if start > 0 {
                hash = roll_window(
                    hash,
                    hashes[start - 1],
                    hashes[start + min_tokens - 1],
                    power,
                );
            }
            index
                .entry((hash, alpha_hashes[start]))
                .or_default()
                .push(Occurrence { file, start });
        }
    }

    let plan = CandidatePlan::build(&index, limits.max_seed_pairs_per_pool, limits.rare_first);
    let plan_diagnostics = plan.diagnostics;
    let mut seed_pairs_completed = 0u64;
    let mut match_limit_reached = false;
    for bucket in plan.buckets {
        let run = detect_candidates_fast(
            prepared,
            bucket.occurrences,
            min_tokens,
            threshold,
            &mut matches,
            seen_regions,
            &mut covered_diagonals,
            bucket.selected_pairs,
            limits.max_matches_per_pool,
        );
        seed_pairs_completed = seed_pairs_completed.saturating_add(run.seed_pairs_completed);
        if run.match_limit_reached {
            match_limit_reached = true;
            break;
        }
    }

    let suppression =
        suppress_overlapping_matches_with_limit(matches, limits.max_overlap_checks_per_pool);
    for candidate in suppression.retained {
        let Some(left) = clone_instance(
            inputs,
            prepared,
            candidate.first_file,
            candidate.first_start,
            candidate.len,
        ) else {
            continue;
        };
        let Some(right) = clone_instance(
            inputs,
            prepared,
            candidate.second_file,
            candidate.second_start,
            candidate.len,
        ) else {
            continue;
        };
        groups.push(CloneGroup {
            lines: candidate.lines,
            tokens: candidate.len,
            similarity: candidate.similarity,
            format: pool.to_string(),
            fingerprint: String::new(),
            instances: vec![left, right],
        });
    }
    PoolOutcome {
        plan: plan_diagnostics,
        seed_pairs_completed,
        match_limit_reached,
        suppression_limit_reached: suppression.limit_reached,
        matches_skipped_during_suppression: suppression.matches_skipped,
    }
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the progress-aware detector is one bounded state machine whose counters, mutable stores, phase events, and limit exits must evolve together"
)]
fn detect_pool(
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    scope: PoolScope<'_>,
    files: &[usize],
    min_tokens: usize,
    threshold: f64,
    groups: &mut Vec<CloneGroup>,
    seen_regions: &mut HashSet<(usize, usize, usize, usize, usize)>,
    reporter: &mut ProgressReporter<'_>,
    limits: Type2Limits,
) -> PoolOutcome {
    let pool_started = Instant::now();
    let groups_before = groups.len();
    let eligible_files = files
        .iter()
        .filter(|file| prepared[**file].tokens.len() >= min_tokens)
        .count();
    let tokens_total = files
        .iter()
        .map(|file| prepared[*file].tokens.len())
        .fold(0usize, usize::saturating_add);
    let windows_total = files
        .iter()
        .map(|file| window_count(prepared[*file].tokens.len(), min_tokens))
        .fold(0usize, usize::saturating_add);
    reporter.force(Type2Progress::PoolStarted {
        pool: scope.name.to_string(),
        pool_index: scope.index,
        pools_total: scope.total,
        files_total: files.len(),
        eligible_files,
        tokens_total,
        windows_total,
    });
    if eligible_files == 0 {
        reporter.force(Type2Progress::PoolFinished {
            pool: scope.name.to_string(),
            pool_index: scope.index,
            pools_total: scope.total,
            duration_ms: duration_ms(pool_started.elapsed()),
            groups_created: 0,
            analysis_partial: false,
            seed_pairs_skipped: 0,
            match_limit_reached: false,
            suppression_limit_reached: false,
            matches_skipped_during_suppression: 0,
        });
        return PoolOutcome::default();
    }

    let power = rolling_power(min_tokens);
    let mut index: HashMap<(u64, u64), Vec<Occurrence>> = HashMap::default();
    let mut covered_diagonals: HashMap<DiagonalKey, MergedIntervals> = HashMap::default();
    let mut matches = Vec::new();
    let indexing_started = Instant::now();
    let mut files_completed = 0usize;
    let mut windows_indexed = 0usize;
    reporter.force(indexing_progress(
        scope,
        "starting",
        files_completed,
        files.len(),
        None,
        0,
        0,
        windows_indexed,
        windows_total,
        index.len(),
        indexing_started,
    ));

    for &file in files {
        let tokens = &prepared[file].tokens;
        if tokens.len() < min_tokens {
            files_completed += 1;
            continue;
        }
        let current_file = inputs[prepared[file].input_index]
            .path
            .to_string_lossy()
            .into_owned();
        let mut hashes = Vec::with_capacity(tokens.len());
        for (token_index, token) in tokens.iter().enumerate() {
            hashes.push(token.shape_hash());
            if (token_index + 1).is_multiple_of(PROGRESS_CHECK_EVERY) {
                reporter.periodic(|| {
                    indexing_progress(
                        scope,
                        "hashing_shapes",
                        files_completed,
                        files.len(),
                        Some(current_file.clone()),
                        token_index + 1,
                        tokens.len(),
                        windows_indexed,
                        windows_total,
                        index.len(),
                        indexing_started,
                    )
                });
            }
        }
        let indexed_before_file = windows_indexed;
        let buckets_before_file = index.len();
        let report_alpha = reporter.enabled();
        let mut alpha_progress = |operation, completed, total| {
            reporter.periodic(|| {
                indexing_progress(
                    scope,
                    operation,
                    files_completed,
                    files.len(),
                    Some(current_file.clone()),
                    completed,
                    total,
                    indexed_before_file,
                    windows_total,
                    buckets_before_file,
                    indexing_started,
                )
            });
        };
        let alpha_hashes = alpha_window_hashes_with_progress(
            tokens,
            min_tokens,
            report_alpha.then_some(&mut alpha_progress),
        );
        let mut hash = window_hash(&hashes[..min_tokens]);
        for start in 0..=hashes.len() - min_tokens {
            if start > 0 {
                hash = roll_window(
                    hash,
                    hashes[start - 1],
                    hashes[start + min_tokens - 1],
                    power,
                );
            }
            index
                .entry((hash, alpha_hashes[start]))
                .or_default()
                .push(Occurrence { file, start });
            windows_indexed = windows_indexed.saturating_add(1);
            if windows_indexed.is_multiple_of(PROGRESS_CHECK_EVERY) {
                reporter.periodic(|| {
                    indexing_progress(
                        scope,
                        "inserting_windows",
                        files_completed,
                        files.len(),
                        Some(current_file.clone()),
                        start + 1,
                        hashes.len() - min_tokens + 1,
                        windows_indexed,
                        windows_total,
                        index.len(),
                        indexing_started,
                    )
                });
            }
        }
        files_completed += 1;
        reporter.periodic(|| {
            indexing_progress(
                scope,
                "file_completed",
                files_completed,
                files.len(),
                Some(current_file),
                hashes.len() - min_tokens + 1,
                hashes.len() - min_tokens + 1,
                windows_indexed,
                windows_total,
                index.len(),
                indexing_started,
            )
        });
    }
    reporter.force(indexing_progress(
        scope,
        "completed",
        files_completed,
        files.len(),
        None,
        0,
        0,
        windows_indexed,
        windows_total,
        index.len(),
        indexing_started,
    ));

    let planning_started = Instant::now();
    reporter.force(candidate_planning_progress(
        scope,
        0,
        index.len(),
        PlanDiagnostics::default(),
        planning_started,
    ));
    let plan = CandidatePlan::build(&index, limits.max_seed_pairs_per_pool, limits.rare_first);
    let plan_diagnostics = plan.diagnostics;
    reporter.force(candidate_planning_progress(
        scope,
        index.len(),
        index.len(),
        plan_diagnostics,
        planning_started,
    ));
    let buckets_total = plan_diagnostics.candidate_buckets_selected;
    let seed_pairs_total = plan_diagnostics.seed_pairs_selected;
    let candidate_started = Instant::now();
    let mut candidate_stats = CandidateStats::default();
    let mut match_limit_reached = false;
    reporter.force(candidate_progress(
        scope,
        candidate_stats,
        buckets_total,
        0,
        seed_pairs_total,
        candidate_started,
    ));
    for bucket in plan.buckets {
        let current_bucket_occurrences = bucket.occurrences.len();
        let mut report_candidates = |stats| {
            reporter.periodic(|| {
                candidate_progress(
                    scope,
                    stats,
                    buckets_total,
                    current_bucket_occurrences,
                    seed_pairs_total,
                    candidate_started,
                )
            });
        };
        let run = detect_candidates(
            prepared,
            bucket.occurrences,
            min_tokens,
            threshold,
            &mut matches,
            seen_regions,
            &mut covered_diagonals,
            &mut candidate_stats,
            &mut report_candidates,
            bucket.selected_pairs,
            limits.max_matches_per_pool,
        );
        candidate_stats.buckets_completed += 1;
        reporter.periodic(|| {
            candidate_progress(
                scope,
                candidate_stats,
                buckets_total,
                current_bucket_occurrences,
                seed_pairs_total,
                candidate_started,
            )
        });
        if run.match_limit_reached {
            match_limit_reached = true;
            break;
        }
    }
    reporter.force(candidate_progress(
        scope,
        candidate_stats,
        buckets_total,
        0,
        seed_pairs_total,
        candidate_started,
    ));

    let sorting_started = Instant::now();
    reporter.force(Type2Progress::SortingMatches {
        pool: scope.name.to_string(),
        pool_index: scope.index,
        pools_total: scope.total,
        status: "started",
        matches_total: matches.len(),
        phase_elapsed_ms: 0,
    });
    sort_candidate_matches(&mut matches);
    reporter.force(Type2Progress::SortingMatches {
        pool: scope.name.to_string(),
        pool_index: scope.index,
        pools_total: scope.total,
        status: "completed",
        matches_total: matches.len(),
        phase_elapsed_ms: duration_ms(sorting_started.elapsed()),
    });

    let suppression_started = Instant::now();
    let matches_total = matches.len();
    let mut suppression_stats = SuppressionStats::default();
    reporter.force(suppression_progress(
        scope,
        suppression_stats,
        matches_total,
        suppression_started,
    ));
    let mut report_suppression = |stats| {
        reporter
            .periodic(|| suppression_progress(scope, stats, matches_total, suppression_started));
    };
    let suppression = suppress_sorted_overlapping_matches(
        matches,
        limits.max_overlap_checks_per_pool,
        &mut report_suppression,
    );
    let suppression_limit_reached = suppression.limit_reached;
    let matches_skipped_during_suppression = suppression.matches_skipped;
    suppression_stats = suppression.stats;
    let retained = suppression.retained;
    reporter.force(suppression_progress(
        scope,
        suppression_stats,
        matches_total,
        suppression_started,
    ));

    let materializing_started = Instant::now();
    let retained_total = retained.len();
    let mut matches_completed = 0usize;
    reporter.force(materialization_progress(
        scope,
        matches_completed,
        retained_total,
        groups.len() - groups_before,
        materializing_started,
    ));
    for candidate in retained {
        let left = clone_instance(
            inputs,
            prepared,
            candidate.first_file,
            candidate.first_start,
            candidate.len,
        );
        let right = clone_instance(
            inputs,
            prepared,
            candidate.second_file,
            candidate.second_start,
            candidate.len,
        );
        if let (Some(left), Some(right)) = (left, right) {
            groups.push(CloneGroup {
                lines: candidate.lines,
                tokens: candidate.len,
                similarity: candidate.similarity,
                format: scope.name.to_string(),
                fingerprint: String::new(),
                instances: vec![left, right],
            });
        }
        matches_completed += 1;
        if matches_completed.is_multiple_of(PROGRESS_CHECK_EVERY) {
            reporter.periodic(|| {
                materialization_progress(
                    scope,
                    matches_completed,
                    retained_total,
                    groups.len() - groups_before,
                    materializing_started,
                )
            });
        }
    }
    reporter.force(materialization_progress(
        scope,
        matches_completed,
        retained_total,
        groups.len() - groups_before,
        materializing_started,
    ));
    let seed_pairs_skipped = plan_diagnostics
        .seed_pairs_total
        .saturating_sub(candidate_stats.seed_pairs_completed);
    reporter.force(Type2Progress::PoolFinished {
        pool: scope.name.to_string(),
        pool_index: scope.index,
        pools_total: scope.total,
        duration_ms: duration_ms(pool_started.elapsed()),
        groups_created: groups.len() - groups_before,
        analysis_partial: seed_pairs_skipped > 0
            || match_limit_reached
            || suppression_limit_reached,
        seed_pairs_skipped,
        match_limit_reached,
        suppression_limit_reached,
        matches_skipped_during_suppression,
    });
    PoolOutcome {
        plan: plan_diagnostics,
        seed_pairs_completed: candidate_stats.seed_pairs_completed,
        match_limit_reached,
        suppression_limit_reached,
        matches_skipped_during_suppression,
    }
}

mod candidates;
#[cfg(test)]
use candidates::suppress_overlapping_matches;
use candidates::{
    alpha_window_hashes, alpha_window_hashes_with_progress, bounded_seed_pair_count,
    candidate_planning_progress, candidate_progress, detect_candidates, detect_candidates_fast,
    duration_ms, group_sort_key, indexing_progress, materialization_progress, normalize_threshold,
    sort_candidate_matches, suppress_overlapping_matches_with_limit,
    suppress_sorted_overlapping_matches, suppression_progress, window_count,
};

#[cfg(test)]
mod tests;
