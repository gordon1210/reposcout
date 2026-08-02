//! Exact (Type-1) clone detection.
//!
//! ## Contract (frozen)
//! `detect(inputs, min_tokens) -> Vec<CloneGroup>`

use crate::dup::{
    DetectionOptions, DupInput, PreparedFile, Token, clone_instance, predecessor_indices, prepare,
    roll_window, rolling_power, window_hash,
};
use crate::model::{CloneGroup, CloneInstance};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

const MAX_PREVIOUS_PER_WINDOW: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Occurrence {
    file: usize,
    start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ExactKey {
    kind: crate::dup::TokenKind,
    text: String,
}

#[derive(Default)]
struct GroupAcc {
    format: String,
    instances: BTreeMap<(PathBuf, usize, usize), CloneInstance>,
}

#[must_use]
pub fn detect(inputs: &[DupInput], min_tokens: usize) -> Vec<CloneGroup> {
    let prepared = prepare(inputs, DetectionOptions::default());
    detect_prepared(inputs, &prepared, min_tokens)
}

pub(crate) fn detect_prepared(
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    min_tokens: usize,
) -> Vec<CloneGroup> {
    if min_tokens == 0 || prepared.is_empty() {
        return Vec::new();
    }

    let mut pools: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (file, source) in prepared.iter().enumerate() {
        pools.entry(&source.pool).or_default().push(file);
    }

    let mut groups: HashMap<Vec<ExactKey>, GroupAcc> = HashMap::new();
    let mut seen_regions = HashSet::new();
    for (pool, files) in pools {
        detect_pool(
            inputs,
            prepared,
            pool,
            &files,
            min_tokens,
            &mut groups,
            &mut seen_regions,
        );
    }
    crate::dup::suppress_contained(into_sorted_groups(groups))
}

fn detect_pool(
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    pool: &str,
    files: &[usize],
    min_tokens: usize,
    groups: &mut HashMap<Vec<ExactKey>, GroupAcc>,
    seen_regions: &mut HashSet<(usize, usize, usize, usize, usize)>,
) {
    if !files
        .iter()
        .any(|file| prepared[*file].tokens.len() >= min_tokens)
    {
        return;
    }
    let power = rolling_power(min_tokens);
    let mut index: HashMap<u64, Vec<Occurrence>> = HashMap::new();

    for &file in files {
        let tokens = &prepared[file].tokens;
        if tokens.len() < min_tokens {
            continue;
        }
        let hashes = tokens.iter().map(Token::exact_hash).collect::<Vec<_>>();
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
                .entry(hash)
                .or_default()
                .push(Occurrence { file, start });
        }
    }

    for occurrences in index.values().filter(|bucket| bucket.len() >= 2) {
        for right in 1..occurrences.len() {
            let b = occurrences[right];
            for left in predecessor_indices(right, MAX_PREVIOUS_PER_WINDOW) {
                let a = occurrences[left];
                if same_file_seed_overlaps(a, b, min_tokens)
                    || !windows_equal(prepared, a, b, min_tokens)
                    || !is_left_maximal(prepared, a, b, min_tokens)
                {
                    continue;
                }

                let Some((a_start, b_start, len)) = maximal_match(prepared, a, b, min_tokens)
                else {
                    continue;
                };
                let region = canonical_region_key(a.file, a_start, b.file, b_start, len);
                if !seen_regions.insert(region) {
                    continue;
                }

                let content_key = prepared[a.file].tokens[a_start..a_start + len]
                    .iter()
                    .map(|token| ExactKey {
                        kind: token.kind,
                        text: token.text.clone(),
                    })
                    .collect::<Vec<_>>();
                let entry = groups.entry(content_key).or_default();
                if entry.format.is_empty() {
                    entry.format = pool.to_string();
                }
                add_instance(entry, inputs, prepared, a.file, a_start, len);
                add_instance(entry, inputs, prepared, b.file, b_start, len);
            }
        }
    }
}

fn windows_equal(prepared: &[PreparedFile], a: Occurrence, b: Occurrence, len: usize) -> bool {
    prepared[a.file]
        .tokens
        .get(a.start..a.start + len)
        .zip(prepared[b.file].tokens.get(b.start..b.start + len))
        .is_some_and(|(left, right)| left.iter().zip(right).all(|(a, b)| a.exact_eq(b)))
}

fn is_left_maximal(
    prepared: &[PreparedFile],
    a: Occurrence,
    b: Occurrence,
    seed_len: usize,
) -> bool {
    if a.start == 0 || b.start == 0 {
        return true;
    }
    let same_file = a.file == b.file;
    let distance = a.start.abs_diff(b.start);
    if same_file && seed_len + 1 > distance {
        return true;
    }
    !prepared[a.file].tokens[a.start - 1].exact_eq(&prepared[b.file].tokens[b.start - 1])
}

fn same_file_seed_overlaps(a: Occurrence, b: Occurrence, len: usize) -> bool {
    a.file == b.file && a.start.abs_diff(b.start) < len
}

fn maximal_match(
    prepared: &[PreparedFile],
    a: Occurrence,
    b: Occurrence,
    seed_len: usize,
) -> Option<(usize, usize, usize)> {
    let a_tokens = &prepared.get(a.file)?.tokens;
    let b_tokens = &prepared.get(b.file)?.tokens;
    let same_file = a.file == b.file;
    let distance = a.start.abs_diff(b.start);
    if same_file && (distance == 0 || seed_len > distance) {
        return None;
    }

    let mut left = 0usize;
    while a.start > left && b.start > left {
        if same_file && seed_len + left + 1 > distance {
            break;
        }
        if !a_tokens[a.start - left - 1].exact_eq(&b_tokens[b.start - left - 1]) {
            break;
        }
        left += 1;
    }

    let a_start = a.start - left;
    let b_start = b.start - left;
    let mut len = seed_len + left;
    while a_start + len < a_tokens.len() && b_start + len < b_tokens.len() {
        if same_file && len + 1 > distance {
            break;
        }
        if !a_tokens[a_start + len].exact_eq(&b_tokens[b_start + len]) {
            break;
        }
        len += 1;
    }
    Some((a_start, b_start, len))
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

fn add_instance(
    group: &mut GroupAcc,
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    file: usize,
    start: usize,
    len: usize,
) {
    if let Some(instance) = clone_instance(inputs, prepared, file, start, len) {
        group.instances.insert(
            (
                instance.path.clone(),
                instance.start_byte,
                instance.end_byte,
            ),
            instance,
        );
    }
}

fn into_sorted_groups(groups: HashMap<Vec<ExactKey>, GroupAcc>) -> Vec<CloneGroup> {
    let mut result = groups
        .into_iter()
        .filter_map(|(content, group)| {
            if content.is_empty() || group.instances.len() < 2 {
                return None;
            }
            let instances = group.instances.into_values().collect::<Vec<_>>();
            let lines = instances
                .iter()
                .map(crate::dup::instance_span)
                .max()
                .unwrap_or(0);
            Some(CloneGroup {
                lines,
                tokens: content.len(),
                similarity: 1.0,
                format: group.format,
                fingerprint: String::new(),
                instances,
            })
        })
        .collect::<Vec<_>>();

    result.sort_by(|a, b| {
        b.tokens
            .cmp(&a.tokens)
            .then_with(|| group_sort_key(a).cmp(&group_sort_key(b)))
    });
    result
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
    use crate::dup::{DuplicationFormatScope, DuplicationMode};

    fn input(path: &str, content: &str) -> DupInput {
        DupInput {
            path: PathBuf::from(path),
            content: content.to_string(),
        }
    }

    #[test]
    fn finds_exact_clone_across_inputs() {
        let shared = "fn shared(value: i32) { let total = value + 1; return total; }";
        let inputs = vec![
            input("a.rs", &format!("fn one() {{}}\n{shared}\n")),
            input("b.rs", &format!("fn two() {{}}\n{shared}\n")),
        ];

        let groups = detect(&inputs, 8);

        assert!(groups.iter().any(|group| {
            group.tokens >= 8
                && group
                    .instances
                    .iter()
                    .any(|instance| instance.path == *"a.rs")
                && group
                    .instances
                    .iter()
                    .any(|instance| instance.path == *"b.rs")
        }));
    }

    #[test]
    fn does_not_match_different_formats_by_default() {
        let shared = "if value > 0 { return value + 1; }";
        let inputs = vec![input("a.rs", shared), input("b.js", shared)];

        assert!(detect(&inputs, 6).is_empty());
    }

    #[test]
    fn compatible_scope_combines_javascript_and_typescript_only() {
        let shared = "const value = input + 1; return value;";
        let inputs = vec![input("a.js", shared), input("b.ts", shared)];
        let options = DetectionOptions {
            mode: DuplicationMode::Mild,
            format_scope: DuplicationFormatScope::Compatible,
            report_snippets: false,
        };
        let prepared = prepare(&inputs, options);

        assert!(!detect_prepared(&inputs, &prepared, 6).is_empty());
    }

    #[test]
    fn emits_one_maximal_region_from_overlapping_seeds() {
        let block = "let a = 1;\nlet b = a + 2;\nreturn b;";
        let inputs = vec![input("a.rs", block), input("b.rs", block)];

        let groups = detect(&inputs, 4);

        assert_eq!(groups.len(), 1);
        assert!(groups[0].tokens > 4);
    }

    #[test]
    fn finds_non_overlapping_repetition_within_one_file() {
        let block = "let value = input + 1; return value;";
        let inputs = vec![input("a.rs", &format!("{block}\nfn gap() {{}}\n{block}"))];

        let groups = detect(&inputs, 6);

        assert!(groups.iter().any(|group| group.instances.len() >= 2));
    }

    #[test]
    fn repetitive_input_is_bounded_and_deterministic() {
        let repeated = "let value = 1;\n".repeat(200);
        let inputs = vec![input("a.rs", &repeated), input("b.rs", &repeated)];

        let first = detect(&inputs, 20);
        let second = detect(&inputs, 20);
        let expected_tokens = prepare(&inputs, DetectionOptions::default())[0]
            .tokens
            .len();

        assert_eq!(first.len(), second.len());
        assert!(first.len() < 128, "{} groups", first.len());
        assert!(
            first.iter().any(|group| group.tokens == expected_tokens),
            "early bucket anchors must retain the whole-file maximal clone"
        );
        assert_eq!(
            first.iter().map(|group| group.tokens).collect::<Vec<_>>(),
            second.iter().map(|group| group.tokens).collect::<Vec<_>>()
        );
    }

    #[test]
    fn ignores_clones_below_threshold() {
        let inputs = vec![input("a.rs", "let a = 1;"), input("b.rs", "let a = 1;")];
        assert!(detect(&inputs, 20).is_empty());
    }

    #[test]
    fn maximum_threshold_returns_without_linear_power_setup() {
        let inputs = vec![input("a.rs", "let a = 1;"), input("b.rs", "let a = 1;")];

        assert!(detect(&inputs, usize::MAX).is_empty());
    }
}
