//! Deterministic admission control for Type-2 fingerprint buckets.
//!
//! Rare buckets carry more discriminating information, so they consume the
//! fixed seed-pair budget before repetitive buckets. The detector still owns
//! verification and reporting; this module owns only bounded scheduling.

use super::{Occurrence, bounded_seed_pair_count};
use rustc_hash::FxHashMap as HashMap;

pub(super) struct PlannedBucket<'a> {
    pub occurrences: &'a [Occurrence],
    pub selected_pairs: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PlanDiagnostics {
    pub candidate_buckets: usize,
    pub candidate_buckets_selected: usize,
    pub candidate_buckets_skipped: usize,
    pub candidate_buckets_partially_selected: usize,
    pub seed_pairs_total: u64,
    pub seed_pairs_selected: u64,
}

impl PlanDiagnostics {
    pub(super) fn seed_pairs_skipped(self) -> u64 {
        self.seed_pairs_total
            .saturating_sub(self.seed_pairs_selected)
    }
}

pub(super) struct CandidatePlan<'a> {
    pub buckets: Vec<PlannedBucket<'a>>,
    pub diagnostics: PlanDiagnostics,
}

impl<'a> CandidatePlan<'a> {
    pub(super) fn build(
        index: &'a HashMap<(u64, u64), Vec<Occurrence>>,
        max_seed_pairs: u64,
        rare_first: bool,
    ) -> Self {
        let mut candidates = index
            .iter()
            .filter(|(_, occurrences)| occurrences.len() >= 2)
            .map(|(fingerprint, occurrences)| {
                (
                    *fingerprint,
                    occurrences.as_slice(),
                    bounded_seed_pair_count(occurrences.len()),
                )
            })
            .collect::<Vec<_>>();
        if rare_first {
            candidates
                .sort_unstable_by(|a, b| a.1.len().cmp(&b.1.len()).then_with(|| a.0.cmp(&b.0)));
        }

        let mut remaining = max_seed_pairs;
        let mut buckets = Vec::new();
        let mut diagnostics = PlanDiagnostics {
            candidate_buckets: candidates.len(),
            ..PlanDiagnostics::default()
        };
        for (_, occurrences, seed_pairs) in candidates {
            diagnostics.seed_pairs_total = diagnostics.seed_pairs_total.saturating_add(seed_pairs);
            let selected_pairs = seed_pairs.min(remaining);
            diagnostics.seed_pairs_selected = diagnostics
                .seed_pairs_selected
                .saturating_add(selected_pairs);
            remaining = remaining.saturating_sub(selected_pairs);
            if selected_pairs == 0 {
                diagnostics.candidate_buckets_skipped += 1;
                continue;
            }
            diagnostics.candidate_buckets_selected += 1;
            if selected_pairs < seed_pairs {
                diagnostics.candidate_buckets_partially_selected += 1;
            }
            buckets.push(PlannedBucket {
                occurrences,
                selected_pairs,
            });
        }

        Self {
            buckets,
            diagnostics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rare_buckets_consume_the_budget_before_repetitive_buckets() {
        let occurrences = |count| {
            (0..count)
                .map(|start| Occurrence { file: 0, start })
                .collect::<Vec<_>>()
        };
        let mut index = HashMap::default();
        index.insert((30, 3), occurrences(4));
        index.insert((10, 1), occurrences(2));
        index.insert((20, 2), occurrences(3));

        let plan = CandidatePlan::build(&index, 3, true);

        assert_eq!(
            plan.buckets
                .iter()
                .map(|bucket| (bucket.occurrences.len(), bucket.selected_pairs))
                .collect::<Vec<_>>(),
            vec![(2, 1), (3, 2)]
        );
        assert_eq!(
            plan.diagnostics,
            PlanDiagnostics {
                candidate_buckets: 3,
                candidate_buckets_selected: 2,
                candidate_buckets_skipped: 1,
                candidate_buckets_partially_selected: 1,
                seed_pairs_total: 10,
                seed_pairs_selected: 3,
            }
        );
        assert_eq!(plan.diagnostics.seed_pairs_skipped(), 7);
    }
}
