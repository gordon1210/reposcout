//! Type-2 clone detection with identifier-renaming validation.
//!
//! Candidate discovery uses rolling hashes over token *shapes*. Every retained
//! pair is then verified with a pair-local two-way identifier mapping. Literal
//! categories are preserved separately and exact clones are omitted.
//!
//! ## Contract (frozen)
//! `detect(inputs, min_tokens, min_similarity) -> Vec<CloneGroup>`

mod plan;

use crate::dup::{
    DetectionOptions, DupInput, PreparedFile, Token, TokenKind, clone_instance,
    predecessor_indices, prepare, roll_window, rolling_power, window_hash,
};
use crate::model::CloneGroup;
use plan::{CandidatePlan, PlanDiagnostics};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

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

/// Detailed, bounded diagnostics for the Type-2 detector's expensive phases.
///
/// This is intentionally separate from the frozen public detector interface.
/// Normal callers retain the unobserved fast path; debug-mode callers receive
/// forced phase transitions plus at most one periodic update per second.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub(crate) enum Type2Progress {
    Started {
        pools_total: usize,
        files_total: usize,
        eligible_files: usize,
        tokens_total: usize,
        windows_total: usize,
        min_tokens: usize,
        min_similarity: f64,
        max_seed_pairs_per_pool: u64,
        max_matches_per_pool: usize,
        max_overlap_checks_per_pool: u64,
    },
    PoolStarted {
        pool: String,
        pool_index: usize,
        pools_total: usize,
        files_total: usize,
        eligible_files: usize,
        tokens_total: usize,
        windows_total: usize,
    },
    Indexing {
        pool: String,
        pool_index: usize,
        pools_total: usize,
        operation: &'static str,
        files_completed: usize,
        files_total: usize,
        current_file: Option<String>,
        current_units_completed: usize,
        current_units_total: usize,
        windows_indexed: usize,
        windows_total: usize,
        fingerprint_buckets: usize,
        phase_elapsed_ms: u64,
        windows_per_second: f64,
    },
    PlanningCandidates {
        pool: String,
        pool_index: usize,
        pools_total: usize,
        fingerprint_buckets_completed: usize,
        fingerprint_buckets_total: usize,
        candidate_buckets: usize,
        candidate_buckets_selected: usize,
        candidate_buckets_skipped: usize,
        candidate_buckets_partially_selected: usize,
        seed_pairs_total: u64,
        seed_pairs_selected: u64,
        seed_pairs_skipped: u64,
        phase_elapsed_ms: u64,
        buckets_per_second: f64,
    },
    CandidateSearch {
        pool: String,
        pool_index: usize,
        pools_total: usize,
        buckets_completed: usize,
        buckets_total: usize,
        current_bucket_occurrences: usize,
        seed_pairs_completed: u64,
        seed_pairs_total: u64,
        seeds_verified: u64,
        seeds_skipped_same_file_overlap: u64,
        seeds_skipped_covered: u64,
        covered_region_checks: u64,
        verification_tokens_compared: u64,
        qualified_matches: u64,
        duplicate_regions_rejected: u64,
        exact_matches_rejected: u64,
        matches_buffered: usize,
        phase_elapsed_ms: u64,
        seed_pairs_per_second: f64,
    },
    SortingMatches {
        pool: String,
        pool_index: usize,
        pools_total: usize,
        status: &'static str,
        matches_total: usize,
        phase_elapsed_ms: u64,
    },
    SuppressingOverlaps {
        pool: String,
        pool_index: usize,
        pools_total: usize,
        matches_completed: usize,
        matches_total: usize,
        overlap_checks: u64,
        matches_kept: usize,
        phase_elapsed_ms: u64,
        matches_per_second: f64,
    },
    MaterializingGroups {
        pool: String,
        pool_index: usize,
        pools_total: usize,
        matches_completed: usize,
        matches_total: usize,
        groups_created: usize,
        phase_elapsed_ms: u64,
        matches_per_second: f64,
    },
    PoolFinished {
        pool: String,
        pool_index: usize,
        pools_total: usize,
        duration_ms: u64,
        groups_created: usize,
        analysis_partial: bool,
        seed_pairs_skipped: u64,
        match_limit_reached: bool,
        suppression_limit_reached: bool,
        matches_skipped_during_suppression: usize,
    },
    SortingGroups {
        status: &'static str,
        groups_total: usize,
        phase_elapsed_ms: u64,
    },
    Finished {
        duration_ms: u64,
        groups_total: usize,
        analysis_partial: bool,
        pools_truncated: usize,
        seed_pairs_skipped: u64,
        match_limit_reached: bool,
        suppression_limit_reached: bool,
        matches_skipped_during_suppression: usize,
    },
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

struct ProgressReporter<'a> {
    callback: Option<&'a mut dyn FnMut(Type2Progress)>,
    interval: Duration,
    last_emitted: Instant,
}

impl<'a> ProgressReporter<'a> {
    fn new(callback: Option<&'a mut dyn FnMut(Type2Progress)>, interval: Duration) -> Self {
        Self {
            callback,
            interval,
            last_emitted: Instant::now(),
        }
    }

    fn force(&mut self, progress: Type2Progress) {
        let Some(callback) = self.callback.as_deref_mut() else {
            return;
        };
        callback(progress);
        self.last_emitted = Instant::now();
    }

    fn periodic(&mut self, progress: impl FnOnce() -> Type2Progress) {
        if self.callback.is_none() || self.last_emitted.elapsed() < self.interval {
            return;
        }
        self.force(progress());
    }

    fn enabled(&self) -> bool {
        self.callback.is_some()
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
fn indexing_progress(
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

fn candidate_planning_progress(
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

fn candidate_progress(
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

fn suppression_progress(
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

fn materialization_progress(
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

fn window_count(tokens: usize, window: usize) -> usize {
    if window == 0 {
        0
    } else {
        tokens
            .checked_sub(window)
            .map_or(0, |remaining| remaining.saturating_add(1))
    }
}

fn bounded_seed_pair_count(occurrences: usize) -> u64 {
    let rights = occurrences.saturating_sub(1) as u128;
    let maximum = MAX_PREVIOUS_PER_WINDOW as u128;
    let count = if rights <= maximum {
        rights.saturating_mul(rights + 1) / 2
    } else {
        maximum.saturating_mul(maximum + 1) / 2 + (rights - maximum).saturating_mul(maximum)
    };
    u64::try_from(count).unwrap_or(u64::MAX)
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn per_second(completed: usize, elapsed: Duration) -> f64 {
    per_second_u64(u64::try_from(completed).unwrap_or(u64::MAX), elapsed)
}

fn per_second_u64(completed: u64, elapsed: Duration) -> f64 {
    let seconds = elapsed.as_secs_f64();
    if seconds > 0.0 {
        completed as f64 / seconds
    } else {
        0.0
    }
}

#[allow(clippy::too_many_arguments)]
fn detect_candidates_fast(
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

#[allow(clippy::too_many_arguments)]
fn detect_candidates(
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

fn candidate_match(
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

fn token_line_span(prepared: &[PreparedFile], file: usize, start: usize, len: usize) -> usize {
    let tokens = &prepared[file].tokens;
    crate::dup::token_line_span(&tokens[start], &tokens[start + len - 1])
}

/// Reduce shifted variants while matches are still compact token coordinates.
/// Building full clone instances first is exceptionally costly on repetitive
/// corpora because most candidates are discarded by this same overlap rule.
#[cfg(test)]
fn suppress_overlapping_matches(mut matches: Vec<CandidateMatch>) -> Vec<CandidateMatch> {
    sort_candidate_matches(&mut matches);
    let mut ignore_progress = |_| {};
    suppress_sorted_overlapping_matches(matches, u64::MAX, &mut ignore_progress).retained
}

fn suppress_overlapping_matches_with_limit(
    mut matches: Vec<CandidateMatch>,
    max_overlap_checks: u64,
) -> SuppressionOutcome {
    sort_candidate_matches(&mut matches);
    let mut ignore_progress = |_| {};
    suppress_sorted_overlapping_matches(matches, max_overlap_checks, &mut ignore_progress)
}

fn sort_candidate_matches(matches: &mut [CandidateMatch]) {
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

fn suppress_sorted_overlapping_matches(
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

fn candidate_matches_overlap(a: CandidateMatch, b: CandidateMatch) -> bool {
    token_ranges_overlap(a.first_start, a.len, b.first_start, b.len)
        && token_ranges_overlap(a.second_start, a.len, b.second_start, b.len)
}

fn token_ranges_overlap(a_start: usize, a_len: usize, b_start: usize, b_len: usize) -> bool {
    a_start < b_start + b_len && b_start < a_start + a_len
}

/// Rolling parameterized fingerprints for every window. Identifiers are
/// represented by the distance to their previous occurrence inside the
/// window, or zero when first seen. This is rename-invariant and equivalent to
/// alpha-canonical names, but all windows are computed in linear time.
fn alpha_window_hashes(tokens: &[Token], window: usize) -> Vec<u64> {
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

fn alpha_window_hashes_with_progress(
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

fn seed_is_covered(
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

fn remember_region(
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

fn diagonal_key(
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

fn normalize_threshold(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn maximal_qualified_match(
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
    if score / (seed_len as f64) < threshold {
        return None;
    }

    let mut best = (a.start, b.start, seed_len, score / seed_len as f64);
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
        let similarity = score / len as f64;
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
        let similarity = score / len as f64;
        if similarity >= threshold {
            best = (a_start, b_start, len, similarity);
        }
    }

    Some(best)
}

/// Validate one shape-compatible pair and return its similarity credit. Equal
/// identifier spellings still reserve both sides of the bijection.
fn accept_pair<'a>(left: &'a Token, right: &'a Token, mapping: &mut Bijection<'a>) -> Option<f64> {
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

#[allow(clippy::too_many_arguments)]
fn originals_equal(
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

fn same_file_seed_overlaps(a: Occurrence, b: Occurrence, len: usize) -> bool {
    a.file == b.file && a.start.abs_diff(b.start) < len
}

fn canonical_region_key(
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

fn group_sort_key(group: &CloneGroup) -> (String, usize, usize) {
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

#[cfg(test)]
mod tests {
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
}
