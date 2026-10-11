//! Token edits repeated across hunks, the hunks they fully explain, deviating hunks and examples.

use super::hunks::{Edit, Hunk, aligned, line_tokens, substitute};
use super::lexicon::{Rename, Site};
use crate::model::{ReviewBriefHunk, ReviewHunkRole, ReviewRewrite};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

/// An edit becomes a rewrite once it occurs in this many distinct hunks.
pub(super) const MIN_REWRITE_HUNKS: usize = 3;
const MAX_REWRITES: usize = 16;
const MAX_REWRITE_CHARS: usize = 120;

/// Rewrites from hunks of changes present in both revisions, most frequent first. Renames and
/// other edits with the same tokens are separate rewrites.
pub(super) fn rewrites(hunks: &[Hunk<'_>]) -> Vec<ReviewRewrite> {
    let mut counts = BTreeMap::<(&str, &str, bool), (usize, bool)>::new();
    for hunk in hunks {
        let mut seen = BTreeSet::new();
        for edit in hunk.edits.iter().flatten().flatten() {
            let entry = counts.entry(edit.class_key()).or_default();
            if seen.insert(edit.class_key()) {
                entry.0 += 1;
            }
            entry.1 |= edit.inside;
        }
    }
    let mut rewrites: Vec<_> = counts
        .into_iter()
        .filter(|((old, new, _), (hunks, _))| {
            *hunks >= MIN_REWRITE_HUNKS
                && old.chars().count() <= MAX_REWRITE_CHARS
                && new.chars().count() <= MAX_REWRITE_CHARS
        })
        .map(|((old, new, rename), (hunks, inside))| ReviewRewrite {
            old: old.to_string(),
            new: new.to_string(),
            inside,
            hunks,
            rename,
        })
        .collect();
    rewrites.sort_by(|left, right| {
        right
            .hunks
            .cmp(&left.hunks)
            .then_with(|| right.rename.cmp(&left.rename))
            .then_with(|| left.old.cmp(&right.old))
            .then_with(|| left.new.cmp(&right.new))
    });
    rewrites.truncate(MAX_REWRITES);
    rewrites
}

/// How listed rewrites explain one hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Explained {
    /// Some line is not its counterpart after listed rewrites.
    No,
    /// Every edit is a listed rename proven pure, and every line equals its counterpart after
    /// them byte for byte: the hunk folds into the rewrite-only sites.
    Renames,
    /// Every edit is a listed rewrite, but some edit is no proven pure rename or some line also
    /// changes its indentation: the hunk shows its changed lines only.
    Rewrites,
}

pub(super) struct Rules<'a> {
    pub list: &'a [ReviewRewrite],
    index: BTreeMap<(&'a str, &'a str, bool), usize>,
}

impl<'a> Rules<'a> {
    pub(super) fn new(list: &'a [ReviewRewrite]) -> Self {
        let index = list
            .iter()
            .enumerate()
            .map(|(index, rule)| ((rule.old.as_str(), rule.new.as_str(), rule.rename), index))
            .collect();
        Self { list, index }
    }

    /// The rewrite with the edit's tokens and class.
    fn rule_of(&self, edit: &Edit) -> Option<usize> {
        self.index.get(&edit.class_key()).copied()
    }

    /// Every block has as many removed as added lines, and each removed line becomes its added
    /// counterpart byte for byte when its token edits, all listed rewrites, are applied.
    /// Outside `indentation_sensitive` files a line may also change its leading whitespace,
    /// which makes the hunk `Rewrites` at most; `pure` decides whether a rename is provable.
    pub(super) fn explains(
        &self,
        hunk: &Hunk<'_>,
        indentation_sensitive: bool,
        pure: &dyn Fn(&Rename<'_>) -> bool,
    ) -> Result<Explained> {
        if !self.ruled(hunk) || !hunk.compact() {
            return Ok(Explained::No);
        }
        let mut renames = true;
        let mut edited = false;
        for block in &hunk.blocks {
            for (left, right) in block.removed.clone().zip(block.added.clone()) {
                let (old, new) = (&hunk.lines[left], &hunk.lines[right]);
                if old.text == new.text {
                    continue;
                }
                let (old_tokens, old_spans) = line_tokens(old.text);
                let (new_tokens, new_spans) = line_tokens(new.text);
                let edits = if old_tokens == new_tokens {
                    Vec::new()
                } else {
                    let Some(edits) = aligned(&old_tokens, &new_tokens)? else {
                        return Ok(Explained::No);
                    };
                    edits
                };
                for (edit, removed, added) in &edits {
                    if self.rule_of(edit).is_none() {
                        return Ok(Explained::No);
                    }
                    edited = true;
                    let proven = edit.rename
                        && edit.whole.as_ref().is_some_and(|(old_name, new_name)| {
                            pure(&Rename {
                                base: Site {
                                    change: hunk.change,
                                    line: old.number,
                                    byte: old_spans[removed.start].start,
                                },
                                head: Site {
                                    change: hunk.change,
                                    line: new.number,
                                    byte: new_spans[added.start].start,
                                },
                                old: old_name,
                                new: new_name,
                            })
                        });
                    renames &= proven;
                }
                let rewritten = substitute((old.text, &old_spans), (new.text, &new_spans), &edits);
                if rewritten != new.text {
                    if indentation_sensitive || rewritten.trim_start() != new.text.trim_start() {
                        return Ok(Explained::No);
                    }
                    renames = false;
                }
            }
        }
        Ok(match (edited, renames) {
            (false, _) => Explained::No,
            (true, true) => Explained::Renames,
            (true, false) => Explained::Rewrites,
        })
    }

    /// Every block has edits and every edit is a listed rewrite of its class.
    pub(super) fn ruled(&self, hunk: &Hunk<'_>) -> bool {
        !hunk.edits.is_empty()
            && hunk.edits.iter().all(|edits| {
                edits.as_ref().is_some_and(|edits| {
                    !edits.is_empty() && edits.iter().all(|edit| self.rule_of(edit).is_some())
                })
            })
    }

    /// Rewrites applied in the hunk, in rewrite order.
    pub(super) fn used(&self, hunk: &Hunk<'_>) -> Vec<usize> {
        let used: BTreeSet<_> = hunk
            .edits
            .iter()
            .flatten()
            .flatten()
            .filter_map(|edit| self.rule_of(edit))
            .collect();
        used.into_iter().collect()
    }

    /// The first rewrite that an edit outside the rewrites deviates from: one with the same
    /// tokens in the other class, one sharing a side, or one differing in a single token.
    pub(super) fn variant_of(&self, hunk: &Hunk<'_>) -> Option<usize> {
        hunk.edits
            .iter()
            .flatten()
            .flatten()
            .filter(|edit| self.rule_of(edit).is_none())
            .find_map(|edit| {
                self.index
                    .get(&(edit.old.as_str(), edit.new.as_str(), !edit.rename))
                    .copied()
                    .or_else(|| self.list.iter().position(|rule| deviates(edit, rule)))
            })
    }
}

/// Shares a non-empty side with the rewrite, or has its token shape with exactly one token changed.
fn deviates(edit: &Edit, rule: &ReviewRewrite) -> bool {
    if (!edit.old.is_empty() && edit.old == rule.old)
        || (!edit.new.is_empty() && edit.new == rule.new)
    {
        return true;
    }
    let split = |text: &str| -> Vec<String> {
        text.split(' ')
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (old, new, rule_old, rule_new) = (
        split(&edit.old),
        split(&edit.new),
        split(&rule.old),
        split(&rule.new),
    );
    if old.len() != rule_old.len() || new.len() != rule_new.len() || old.len() + new.len() < 2 {
        return false;
    }
    old.iter()
        .zip(&rule_old)
        .chain(new.iter().zip(&rule_new))
        .filter(|(left, right)| left != right)
        .count()
        == 1
}

/// Rewrite-only hunks covering every rewrite they can, most rewrites per example first.
pub(super) fn examples(hunks: &[ReviewBriefHunk], rewrites: usize) -> Vec<usize> {
    let mut uncovered: BTreeSet<usize> = (0..rewrites).collect();
    let mut chosen = Vec::new();
    while !uncovered.is_empty() {
        let best = hunks
            .iter()
            .enumerate()
            .filter(|(_, hunk)| hunk.role == ReviewHunkRole::Rewrite)
            .map(|(index, hunk)| {
                let covered = hunk
                    .rewrites
                    .iter()
                    .filter(|rule| uncovered.contains(rule))
                    .count();
                (
                    covered,
                    std::cmp::Reverse(hunk.changed.len()),
                    std::cmp::Reverse(index),
                )
            })
            .filter(|(covered, ..)| *covered > 0)
            .max();
        let Some((_, _, std::cmp::Reverse(index))) = best else {
            break;
        };
        for rule in &hunks[index].rewrites {
            uncovered.remove(rule);
        }
        chosen.push(index);
    }
    chosen.sort_unstable();
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(old: &str, new: &str) -> ReviewRewrite {
        ReviewRewrite {
            old: old.into(),
            new: new.into(),
            inside: false,
            hunks: 3,
            rename: false,
        }
    }

    fn edit(old: &str, new: &str) -> Edit {
        Edit {
            old: old.into(),
            new: new.into(),
            inside: false,
            whole: None,
            rename: false,
        }
    }

    #[test]
    fn deviation_shares_a_side_or_differs_in_one_token() {
        let nearest = rule("false", "mode : 'nearest' }");
        assert!(deviates(&edit("false", "mode : 'down' }"), &nearest));
        assert!(deviates(
            &edit("f ( 1 )", "g ( 1 )"),
            &rule("f ( 2 )", "g ( 1 )")
        ));
        assert!(!deviates(&edit("true", "mode : 'up' }"), &nearest));
        // Empty sides of insertions and deletions never count as shared.
        assert!(!deviates(&edit("", "}"), &rule("", "{ step :")));
    }
}
