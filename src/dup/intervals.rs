use std::collections::BTreeMap;

/// Covered token ranges for one file-pair diagonal.
///
/// Ranges remain disjoint and merged, so containment needs only the nearest
/// predecessor instead of a linear scan through every earlier match.
#[derive(Default)]
pub(super) struct MergedIntervals {
    by_start: BTreeMap<usize, usize>,
}

impl MergedIntervals {
    pub(super) fn covers(&self, start: usize, end: usize) -> bool {
        self.by_start
            .range(..=start)
            .next_back()
            .is_some_and(|(_, covered_end)| *covered_end >= end)
    }

    pub(super) fn insert(&mut self, start: usize, end: usize) {
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
