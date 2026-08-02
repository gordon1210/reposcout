use serde::Serialize;
use std::time::{Duration, Instant};

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

pub(super) struct ProgressReporter<'a> {
    callback: Option<&'a mut dyn FnMut(Type2Progress)>,
    interval: Duration,
    last_emitted: Instant,
}

impl<'a> ProgressReporter<'a> {
    pub(super) fn new(
        callback: Option<&'a mut dyn FnMut(Type2Progress)>,
        interval: Duration,
    ) -> Self {
        Self {
            callback,
            interval,
            last_emitted: Instant::now(),
        }
    }

    pub(super) fn force(&mut self, progress: Type2Progress) {
        let Some(callback) = self.callback.as_deref_mut() else {
            return;
        };
        callback(progress);
        self.last_emitted = Instant::now();
    }

    pub(super) fn periodic(&mut self, progress: impl FnOnce() -> Type2Progress) {
        if self.callback.is_none() || self.last_emitted.elapsed() < self.interval {
            return;
        }
        self.force(progress());
    }

    pub(super) fn enabled(&self) -> bool {
        self.callback.is_some()
    }
}
