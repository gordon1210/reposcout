//! Structured, format-scoped code duplication detection.
//!
//! Exact Type-1 and identifier/literal-normalized Type-2 detectors share one
//! prepared token corpus. The public detector signatures in [`exact`] and
//! [`fuzzy`] remain compatibility adapters over this module.

pub mod exact;
pub mod fuzzy;
mod tokenize;

use crate::lang;
use crate::model::{
    CloneGroup, CloneInstance, DuplicateFinding, DuplicateFragment, Duplication, LineRange,
};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::{xxh3_64, xxh3_128};

const ROLLING_BASE: u64 = 0x9e37_79b1_85eb_ca87;
type GroupRegion = (PathBuf, usize, usize, usize, usize);

/// Trivia retained in the duplication token stream.
///
/// `mild` matches reposcout's historical behavior: whitespace is ignored but
/// comments participate. `weak` additionally ignores comments.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum DuplicationMode {
    Strict,
    #[default]
    Mild,
    Weak,
}

impl fmt::Display for DuplicationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Strict => "strict",
            Self::Mild => "mild",
            Self::Weak => "weak",
        })
    }
}

/// Which detected formats may share a candidate pool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum DuplicationFormatScope {
    /// Compare only files with the exact same detected language.
    #[default]
    Exact,
    /// Also combine JavaScript, TypeScript, and TSX.
    Compatible,
    /// Compare every recognized format. Useful for specialized corpora only.
    All,
}

/// One file's content presented to the duplication detectors.
pub struct DupInput {
    /// Repo-relative path used for reporting clone locations.
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Copy)]
pub struct DetectionOptions {
    pub mode: DuplicationMode,
    pub format_scope: DuplicationFormatScope,
    pub report_snippets: bool,
}

impl Default for DetectionOptions {
    fn default() -> Self {
        Self {
            mode: DuplicationMode::Mild,
            format_scope: DuplicationFormatScope::Exact,
            report_snippets: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LiteralKind {
    String,
    Character,
    Integer,
    Float,
    Boolean,
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TokenKind {
    Keyword,
    Identifier,
    Literal(LiteralKind),
    Operator,
    Punctuation,
    Comment,
    Whitespace,
    Other,
}

/// A structured lexical token with precise source coordinates.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub line: usize,
    pub end_line: usize,
    /// One-based Unicode-code-point columns; the end is exclusive.
    pub start_column: usize,
    pub end_column: usize,
    /// Zero-based half-open byte offsets.
    pub start_byte: usize,
    pub end_byte: usize,
}

impl Token {
    pub(crate) fn exact_hash(&self) -> u64 {
        token_hash(self.kind, &self.text)
    }

    pub(crate) fn shape_hash(&self) -> u64 {
        match self.kind {
            TokenKind::Identifier => token_hash(self.kind, "$IDENTIFIER"),
            TokenKind::Literal(kind) => token_hash(self.kind, literal_label(kind)),
            _ => token_hash(self.kind, &self.text),
        }
    }

    pub(crate) fn exact_eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.text == other.text
    }

    pub(crate) fn shape_eq(&self, other: &Self) -> bool {
        match (self.kind, other.kind) {
            (TokenKind::Identifier, TokenKind::Identifier) => true,
            (TokenKind::Literal(a), TokenKind::Literal(b)) => a == b,
            _ => self.exact_eq(other),
        }
    }
}

fn literal_label(kind: LiteralKind) -> &'static str {
    match kind {
        LiteralKind::String => "$STRING",
        LiteralKind::Character => "$CHARACTER",
        LiteralKind::Integer => "$INTEGER",
        LiteralKind::Float => "$FLOAT",
        LiteralKind::Boolean => "$BOOLEAN",
        LiteralKind::Null => "$NULL",
    }
}

fn token_hash(kind: TokenKind, value: &str) -> u64 {
    let mut bytes = Vec::with_capacity(value.len() + 2);
    bytes.push(kind_discriminant(kind));
    if let TokenKind::Literal(literal) = kind {
        bytes.push(literal_discriminant(literal));
    }
    bytes.extend_from_slice(value.as_bytes());
    xxh3_64(&bytes)
}

fn kind_discriminant(kind: TokenKind) -> u8 {
    match kind {
        TokenKind::Keyword => 1,
        TokenKind::Identifier => 2,
        TokenKind::Literal(_) => 3,
        TokenKind::Operator => 4,
        TokenKind::Punctuation => 5,
        TokenKind::Comment => 6,
        TokenKind::Whitespace => 7,
        TokenKind::Other => 8,
    }
}

fn literal_discriminant(kind: LiteralKind) -> u8 {
    match kind {
        LiteralKind::String => 1,
        LiteralKind::Character => 2,
        LiteralKind::Integer => 3,
        LiteralKind::Float => 4,
        LiteralKind::Boolean => 5,
        LiteralKind::Null => 6,
    }
}

#[derive(Debug)]
pub(crate) struct PreparedFile {
    pub input_index: usize,
    pub format: String,
    pub pool: String,
    pub tokens: Vec<Token>,
}

pub(crate) fn prepare(inputs: &[DupInput], options: DetectionOptions) -> Vec<PreparedFile> {
    inputs
        .iter()
        .enumerate()
        .map(|(input_index, input)| {
            let format = lang::detect(&input.path)
                .map(|info| info.name)
                .unwrap_or("Unknown")
                .to_string();
            let pool = format_pool(&format, options.format_scope);
            PreparedFile {
                input_index,
                format,
                pool,
                tokens: tokenize::tokenize_path(&input.path, &input.content, options.mode),
            }
        })
        .collect()
}

fn format_pool(format: &str, scope: DuplicationFormatScope) -> String {
    match scope {
        DuplicationFormatScope::Exact => format.to_string(),
        DuplicationFormatScope::Compatible
            if matches!(format, "JavaScript" | "TypeScript" | "TSX") =>
        {
            "ECMAScript".to_string()
        }
        DuplicationFormatScope::Compatible => format.to_string(),
        DuplicationFormatScope::All => "*".to_string(),
    }
}

/// Detection result plus the non-serialized coverage denominator needed by
/// aggregate reporting.
pub struct Detection {
    pub duplication: Duplication,
    pub coverage: DuplicateCoverage,
    pub token_counts: BTreeMap<PathBuf, usize>,
    pub formats: BTreeMap<PathBuf, String>,
    pub(crate) type2_diagnostics: fuzzy::Type2Diagnostics,
}

/// Coarse duplication phases exposed for interactive progress reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionStage {
    Tokenizing,
    ExactClones,
    Type2Clones,
    Finalizing,
}

impl DetectionStage {
    pub fn message(self) -> &'static str {
        match self {
            Self::Tokenizing => "tokenizing files for duplication",
            Self::ExactClones => "finding exact duplicate blocks",
            Self::Type2Clones => "finding Type-2 duplicate blocks",
            Self::Finalizing => "finalizing duplication results",
        }
    }
}

/// Run both detectors over one prepared corpus and build detailed findings.
pub fn analyze(
    inputs: &[DupInput],
    min_tokens: usize,
    min_lines: usize,
    min_similarity: f64,
    options: DetectionOptions,
) -> Detection {
    analyze_with_progress(
        inputs,
        min_tokens,
        min_lines,
        min_similarity,
        options,
        |_| {},
    )
}

/// Run duplication analysis while reporting coarse phases to the caller.
pub fn analyze_with_progress(
    inputs: &[DupInput],
    min_tokens: usize,
    min_lines: usize,
    min_similarity: f64,
    options: DetectionOptions,
    progress: impl FnMut(DetectionStage),
) -> Detection {
    analyze_with_diagnostics(
        inputs,
        min_tokens,
        min_lines,
        min_similarity,
        options,
        progress,
        None,
    )
}

/// Internal diagnostic adapter that leaves the public progress and detector
/// interfaces stable while exposing detailed Type-2 progress to the scanner.
#[allow(clippy::too_many_arguments)]
pub(crate) fn analyze_with_diagnostics(
    inputs: &[DupInput],
    min_tokens: usize,
    min_lines: usize,
    min_similarity: f64,
    options: DetectionOptions,
    mut progress: impl FnMut(DetectionStage),
    type2_progress: Option<&mut dyn FnMut(fuzzy::Type2Progress)>,
) -> Detection {
    progress(DetectionStage::Tokenizing);
    let prepared = prepare(inputs, options);
    progress(DetectionStage::ExactClones);
    let mut exact = finalize(
        exact::detect_prepared(inputs, &prepared, min_tokens),
        min_lines,
    );
    progress(DetectionStage::Type2Clones);
    // The Type-2 detector suppresses overlapping/contained pairs while they
    // are still compact token ranges, before constructing report objects.
    let type2_detection = if let Some(type2_progress) = type2_progress {
        fuzzy::detect_prepared_bounded_with_progress(
            inputs,
            &prepared,
            min_tokens,
            min_similarity,
            Some(type2_progress),
        )
    } else {
        fuzzy::detect_prepared_bounded(inputs, &prepared, min_tokens, min_similarity)
    };
    let mut near = filter_short(type2_detection.groups, min_lines);
    progress(DetectionStage::Finalizing);
    assign_group_fingerprints(&mut exact, "exact", &prepared, inputs, options);
    assign_group_fingerprints(&mut near, "type2", &prepared, inputs, options);
    let findings = build_findings(&exact, &near, inputs, options);
    let duplication = Duplication {
        exact,
        near,
        findings,
        ..Duplication::default()
    };
    let coverage = DuplicateCoverage::from_duplication(&duplication);
    let token_counts = prepared
        .iter()
        .map(|file| (inputs[file.input_index].path.clone(), file.tokens.len()))
        .collect();
    let formats = prepared
        .iter()
        .map(|file| (inputs[file.input_index].path.clone(), file.format.clone()))
        .collect();

    Detection {
        duplication,
        coverage,
        token_counts,
        formats,
        type2_diagnostics: type2_detection.diagnostics,
    }
}

fn assign_group_fingerprints(
    groups: &mut [CloneGroup],
    kind: &str,
    prepared: &[PreparedFile],
    inputs: &[DupInput],
    options: DetectionOptions,
) {
    let by_path = prepared
        .iter()
        .map(|file| (inputs[file.input_index].path.as_path(), file))
        .collect::<BTreeMap<_, _>>();

    for group in groups {
        let Some(instance) = group.instances.first() else {
            continue;
        };
        let Some(file) = by_path.get(instance.path.as_path()) else {
            continue;
        };
        let start = instance.start_token.saturating_sub(1);
        let end = instance.end_token.min(file.tokens.len());
        let Some(tokens) = file.tokens.get(start..end) else {
            continue;
        };

        let mut bytes = Vec::with_capacity(tokens.len() * 8 + 64);
        append_part(&mut bytes, b"dup-family-v1");
        append_part(&mut bytes, kind.as_bytes());
        append_part(&mut bytes, group.format.as_bytes());
        append_part(&mut bytes, options.mode.to_string().as_bytes());
        if kind == "exact" {
            for token in tokens {
                bytes.extend_from_slice(&token.exact_hash().to_le_bytes());
            }
        } else {
            let mut identifiers = HashMap::new();
            let mut next_identifier = 0u64;
            for token in tokens {
                if token.kind == TokenKind::Identifier {
                    let identifier = *identifiers.entry(token.text.as_str()).or_insert_with(|| {
                        let identifier = next_identifier;
                        next_identifier += 1;
                        identifier
                    });
                    bytes.push(1);
                    bytes.extend_from_slice(&identifier.to_le_bytes());
                } else {
                    bytes.push(0);
                    bytes.extend_from_slice(&token.shape_hash().to_le_bytes());
                }
            }
        }
        group.fingerprint = format!("dup:v1:{:032x}", xxh3_128(&bytes));
    }
}

/// Compatibility wrapper using the historical mild trivia behavior and exact
/// format scoping.
pub fn detect(
    inputs: &[DupInput],
    min_tokens: usize,
    min_lines: usize,
    min_similarity: f64,
) -> Duplication {
    analyze(
        inputs,
        min_tokens,
        min_lines,
        min_similarity,
        DetectionOptions::default(),
    )
    .duplication
}

/// The union of physical source lines and duplication-token indices covered by
/// retained clone instances.
#[derive(Debug, Default)]
pub struct DuplicateCoverage {
    lines_by_path: BTreeMap<PathBuf, IntervalSet>,
    tokens_by_path: BTreeMap<PathBuf, IntervalSet>,
}

#[derive(Debug, Default)]
struct IntervalSet {
    intervals: Vec<Range<usize>>,
    covered: usize,
}

impl IntervalSet {
    fn insert(&mut self, mut range: Range<usize>) {
        if range.is_empty() {
            return;
        }

        let first = self
            .intervals
            .partition_point(|existing| existing.end < range.start);
        let mut last = first;
        while last < self.intervals.len() && self.intervals[last].start <= range.end {
            let existing = &self.intervals[last];
            range.start = range.start.min(existing.start);
            range.end = range.end.max(existing.end);
            self.covered = self.covered.saturating_sub(existing.end - existing.start);
            last += 1;
        }

        self.covered = self.covered.saturating_add(range.end - range.start);
        self.intervals.splice(first..last, [range]);
    }

    fn len(&self) -> usize {
        self.covered
    }
}

impl DuplicateCoverage {
    pub fn from_duplication(duplication: &Duplication) -> Self {
        let mut coverage = Self::default();

        for group in duplication.exact.iter().chain(&duplication.near) {
            for instance in &group.instances {
                let end_line = occupied_end_line(instance);
                if instance.start_line > 0 && instance.start_line <= end_line {
                    coverage
                        .lines_by_path
                        .entry(instance.path.clone())
                        .or_default()
                        .insert(instance.start_line - 1..end_line);
                }
                if instance.start_token > 0 && instance.start_token <= instance.end_token {
                    coverage
                        .tokens_by_path
                        .entry(instance.path.clone())
                        .or_default()
                        .insert(instance.start_token - 1..instance.end_token);
                }
            }
        }

        coverage
    }

    pub fn total_lines(&self) -> usize {
        self.lines_by_path.values().map(IntervalSet::len).sum()
    }

    pub fn total_tokens(&self) -> usize {
        self.tokens_by_path.values().map(IntervalSet::len).sum()
    }

    pub fn covered_lines(&self, path: &Path) -> usize {
        self.lines_by_path.get(path).map_or(0, IntervalSet::len)
    }

    pub(crate) fn covered_lines_excluding(&self, path: &Path, excluded: &[LineRange]) -> usize {
        let Some(covered) = self.lines_by_path.get(path) else {
            return 0;
        };
        let mut excluded_set = IntervalSet::default();
        for range in excluded {
            if range.start > 0 && range.start <= range.end {
                excluded_set.insert(range.start - 1..range.end);
            }
        }

        let mut overlap = 0usize;
        let mut covered_index = 0usize;
        let mut excluded_index = 0usize;
        while covered_index < covered.intervals.len()
            && excluded_index < excluded_set.intervals.len()
        {
            let covered_range = &covered.intervals[covered_index];
            let excluded_range = &excluded_set.intervals[excluded_index];
            let start = covered_range.start.max(excluded_range.start);
            let end = covered_range.end.min(excluded_range.end);
            if start < end {
                overlap = overlap.saturating_add(end - start);
            }
            if covered_range.end <= excluded_range.end {
                covered_index += 1;
            } else {
                excluded_index += 1;
            }
        }
        covered.len().saturating_sub(overlap)
    }

    pub fn covered_tokens(&self, path: &Path) -> usize {
        self.tokens_by_path.get(path).map_or(0, IntervalSet::len)
    }
}

pub(crate) fn window_hash(hashes: &[u64]) -> u64 {
    hashes.iter().fold(0u64, |hash, token| {
        hash.wrapping_mul(ROLLING_BASE).wrapping_add(*token)
    })
}

pub(crate) fn rolling_power(window: usize) -> u64 {
    let mut base = ROLLING_BASE;
    let mut exponent = window.saturating_sub(1);
    let mut power = 1u64;
    while exponent > 0 {
        if exponent & 1 == 1 {
            power = power.wrapping_mul(base);
        }
        base = base.wrapping_mul(base);
        exponent >>= 1;
    }
    power
}

pub(crate) fn roll_window(hash: u64, outgoing: u64, incoming: u64, power: u64) -> u64 {
    hash.wrapping_sub(outgoing.wrapping_mul(power))
        .wrapping_mul(ROLLING_BASE)
        .wrapping_add(incoming)
}

/// Bound pathological hash buckets while retaining both stable early anchors
/// and recent occurrences. Early anchors keep large repeated files connected;
/// recent occurrences preserve local/non-overlapping repetitions.
pub(crate) fn predecessor_indices(right: usize, maximum: usize) -> impl Iterator<Item = usize> {
    const EARLY_ANCHORS: usize = 8;
    let early = EARLY_ANCHORS.min(maximum).min(right);
    let recent_capacity = maximum.saturating_sub(early);
    let recent_start = if right > maximum {
        right - recent_capacity
    } else {
        early
    };
    (0..early).chain(recent_start..right)
}

pub(crate) fn clone_instance(
    inputs: &[DupInput],
    prepared: &[PreparedFile],
    file: usize,
    start: usize,
    len: usize,
) -> Option<CloneInstance> {
    let source = prepared.get(file)?;
    let first = source.tokens.get(start)?;
    let last = source.tokens.get(start + len - 1)?;
    let input = inputs.get(source.input_index)?;
    Some(CloneInstance {
        path: input.path.clone(),
        start_line: first.line,
        end_line: last.end_line,
        start_column: first.start_column,
        end_column: last.end_column,
        start_byte: first.start_byte,
        end_byte: last.end_byte,
        start_token: start + 1,
        end_token: start + len,
    })
}

fn finalize(groups: Vec<CloneGroup>, min_lines: usize) -> Vec<CloneGroup> {
    suppress_contained(filter_short(
        dedup_groups(prune_overlaps(groups)),
        min_lines,
    ))
}

fn prune_overlaps(groups: Vec<CloneGroup>) -> Vec<CloneGroup> {
    groups.into_iter().filter_map(prune_group).collect()
}

fn prune_group(mut group: CloneGroup) -> Option<CloneGroup> {
    // Within one group, token span and similarity are group-constant. Prefer
    // the widest physical range, which fixes the same-start shorter-wins bug.
    group.instances.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| instance_span(b).cmp(&instance_span(a)))
            .then(a.start_line.cmp(&b.start_line))
            .then(a.start_byte.cmp(&b.start_byte))
    });

    let mut kept: Vec<CloneInstance> = Vec::with_capacity(group.instances.len());
    for instance in group.instances.drain(..) {
        if kept.iter().any(|other| instances_overlap(other, &instance)) {
            continue;
        }
        kept.push(instance);
    }
    if kept.len() < 2 {
        return None;
    }
    kept.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.start_byte.cmp(&b.start_byte))
            .then(a.start_line.cmp(&b.start_line))
    });
    group.lines = kept.iter().map(instance_span).max().unwrap_or(0);
    group.instances = kept;
    Some(group)
}

fn instances_overlap(a: &CloneInstance, b: &CloneInstance) -> bool {
    if a.path != b.path {
        return false;
    }
    if a.end_byte > a.start_byte && b.end_byte > b.start_byte {
        a.start_byte < b.end_byte && b.start_byte < a.end_byte
    } else {
        a.start_line <= occupied_end_line(b) && b.start_line <= occupied_end_line(a)
    }
}

fn filter_short(groups: Vec<CloneGroup>, min_lines: usize) -> Vec<CloneGroup> {
    if min_lines <= 1 {
        return groups;
    }
    groups
        .into_iter()
        .filter(|group| max_span(group) >= min_lines)
        .collect()
}

fn max_span(group: &CloneGroup) -> usize {
    group.instances.iter().map(instance_span).max().unwrap_or(0)
}

pub(crate) fn instance_span(instance: &CloneInstance) -> usize {
    endpoint_occupied_end_line(
        instance.start_line,
        instance.end_line,
        instance.end_column,
        instance.start_byte,
        instance.end_byte,
    )
    .saturating_sub(instance.start_line)
    .saturating_add(1)
}

fn occupied_end_line(instance: &CloneInstance) -> usize {
    endpoint_occupied_end_line(
        instance.start_line,
        instance.end_line,
        instance.end_column,
        instance.start_byte,
        instance.end_byte,
    )
}

pub(crate) fn token_line_span(first: &Token, last: &Token) -> usize {
    endpoint_occupied_end_line(
        first.line,
        last.end_line,
        last.end_column,
        first.start_byte,
        last.end_byte,
    )
    .saturating_sub(first.line)
    .saturating_add(1)
}

fn endpoint_occupied_end_line(
    start_line: usize,
    end_line: usize,
    end_column: usize,
    start_byte: usize,
    end_byte: usize,
) -> usize {
    if end_column == 1 && end_line > start_line && end_byte > start_byte {
        end_line - 1
    } else {
        end_line
    }
}

fn dedup_groups(groups: Vec<CloneGroup>) -> Vec<CloneGroup> {
    let mut seen: HashSet<Vec<GroupRegion>> = HashSet::new();
    let mut out = Vec::with_capacity(groups.len());
    for group in groups {
        let mut signature = group
            .instances
            .iter()
            .map(|instance| {
                (
                    instance.path.clone(),
                    instance.start_byte,
                    instance.end_byte,
                    instance.start_line,
                    instance.end_line,
                )
            })
            .collect::<Vec<_>>();
        signature.sort();
        if seen.insert(signature) {
            out.push(group);
        }
    }
    out
}

/// Drop a shorter group only when it has the same copy cardinality and every
/// precise instance is contained by a distinct instance of an earlier,
/// larger group. A shorter block with an additional copy remains actionable.
pub(crate) fn suppress_contained(groups: Vec<CloneGroup>) -> Vec<CloneGroup> {
    let mut kept: Vec<CloneGroup> = Vec::with_capacity(groups.len());
    let mut by_copy_set: HashMap<Vec<PathBuf>, Vec<usize>> = HashMap::new();
    for group in groups {
        let copy_set = group_copy_set(&group);
        let contained = by_copy_set.get(&copy_set).is_some_and(|candidates| {
            candidates
                .iter()
                .any(|index| group_contains(&kept[*index], &group))
        });
        if !contained {
            let index = kept.len();
            kept.push(group);
            by_copy_set.entry(copy_set).or_default().push(index);
        }
    }
    kept
}

fn group_copy_set(group: &CloneGroup) -> Vec<PathBuf> {
    let mut paths = group
        .instances
        .iter()
        .map(|instance| instance.path.clone())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn group_contains(larger: &CloneGroup, smaller: &CloneGroup) -> bool {
    if larger.tokens < smaller.tokens || larger.instances.len() != smaller.instances.len() {
        return false;
    }
    let mut used = vec![false; larger.instances.len()];
    for small in &smaller.instances {
        let Some(index) = larger
            .instances
            .iter()
            .enumerate()
            .position(|(index, big)| !used[index] && instance_contains(big, small))
        else {
            return false;
        };
        used[index] = true;
    }
    true
}

fn instance_contains(larger: &CloneInstance, smaller: &CloneInstance) -> bool {
    if larger.path != smaller.path {
        return false;
    }
    if larger.end_byte > larger.start_byte && smaller.end_byte > smaller.start_byte {
        larger.start_byte <= smaller.start_byte && larger.end_byte >= smaller.end_byte
    } else {
        larger.start_line <= smaller.start_line
            && occupied_end_line(larger) >= occupied_end_line(smaller)
    }
}

fn build_findings(
    exact: &[CloneGroup],
    near: &[CloneGroup],
    inputs: &[DupInput],
    options: DetectionOptions,
) -> Vec<DuplicateFinding> {
    let content = inputs
        .iter()
        .map(|input| (input.path.clone(), input.content.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut exact_pairs = HashSet::new();
    let mut findings = Vec::new();

    for group in exact {
        if let Some(anchor) = group.instances.first() {
            let family_id = family_id("exact", group);
            for other in group.instances.iter().skip(1) {
                exact_pairs.insert(pair_signature(anchor, other));
                findings.push(make_finding(
                    "exact", &family_id, group, anchor, other, &content, options,
                ));
            }
        }
    }

    for group in near {
        if let Some(anchor) = group.instances.first() {
            let family_id = family_id("type2", group);
            for other in group.instances.iter().skip(1) {
                if exact_pairs.contains(&pair_signature(anchor, other)) {
                    continue;
                }
                findings.push(make_finding(
                    "type2", &family_id, group, anchor, other, &content, options,
                ));
            }
        }
    }

    findings.sort_by(|a, b| {
        b.tokens
            .cmp(&a.tokens)
            .then_with(|| b.similarity.total_cmp(&a.similarity))
            .then(a.id.cmp(&b.id))
    });
    findings
}

fn make_finding(
    kind: &str,
    family_id: &str,
    group: &CloneGroup,
    a: &CloneInstance,
    b: &CloneInstance,
    content: &BTreeMap<PathBuf, &str>,
    options: DetectionOptions,
) -> DuplicateFinding {
    let id = finding_id(kind, &group.format, a, b);
    DuplicateFinding {
        id,
        family_id: family_id.to_string(),
        kind: kind.to_string(),
        format: group.format.clone(),
        tokens: group.tokens,
        lines_a: instance_span(a),
        lines_b: instance_span(b),
        similarity: group.similarity,
        confidence: if kind == "exact" || group.similarity >= 0.90 {
            "high".to_string()
        } else {
            "medium".to_string()
        },
        normalization: options.mode.to_string(),
        fragment_a: fragment(a, content, options.report_snippets),
        fragment_b: fragment(b, content, options.report_snippets),
        removable_lines: instance_span(a).min(instance_span(b)),
    }
}

fn fragment(
    instance: &CloneInstance,
    content: &BTreeMap<PathBuf, &str>,
    include_snippet: bool,
) -> DuplicateFragment {
    let snippet = include_snippet
        .then(|| {
            content
                .get(&instance.path)
                .and_then(|source| source.get(instance.start_byte..instance.end_byte))
                .map(|source| source.chars().take(2_000).collect::<String>())
        })
        .flatten();
    DuplicateFragment {
        path: instance.path.clone(),
        start_line: instance.start_line,
        end_line: instance.end_line,
        start_column: instance.start_column,
        end_column: instance.end_column,
        start_byte: instance.start_byte,
        end_byte: instance.end_byte,
        start_token: instance.start_token,
        end_token: instance.end_token,
        snippet,
    }
}

fn family_id(kind: &str, group: &CloneGroup) -> String {
    if !group.fingerprint.is_empty() {
        return group
            .fingerprint
            .strip_prefix("dup:v1:")
            .unwrap_or(&group.fingerprint)
            .to_string();
    }
    let mut bytes = Vec::new();
    append_part(&mut bytes, kind.as_bytes());
    append_part(&mut bytes, group.format.as_bytes());
    let mut instances = group.instances.iter().collect::<Vec<_>>();
    instances.sort_by_key(|instance| (&instance.path, instance.start_byte, instance.end_byte));
    for instance in instances {
        append_instance(&mut bytes, instance);
    }
    format!("{:032x}", xxh3_128(&bytes))
}

fn finding_id(kind: &str, format: &str, a: &CloneInstance, b: &CloneInstance) -> String {
    let mut bytes = Vec::new();
    append_part(&mut bytes, kind.as_bytes());
    append_part(&mut bytes, format.as_bytes());
    let mut pair = [a, b];
    pair.sort_by_key(|instance| (&instance.path, instance.start_byte, instance.end_byte));
    append_instance(&mut bytes, pair[0]);
    append_instance(&mut bytes, pair[1]);
    format!("{:032x}", xxh3_128(&bytes))
}

fn append_instance(bytes: &mut Vec<u8>, instance: &CloneInstance) {
    append_part(bytes, instance.path.to_string_lossy().as_bytes());
    bytes.extend_from_slice(&instance.start_byte.to_le_bytes());
    bytes.extend_from_slice(&instance.end_byte.to_le_bytes());
}

fn append_part(bytes: &mut Vec<u8>, part: &[u8]) {
    bytes.extend_from_slice(&part.len().to_le_bytes());
    bytes.extend_from_slice(part);
}

fn pair_signature(
    a: &CloneInstance,
    b: &CloneInstance,
) -> ((PathBuf, usize, usize), (PathBuf, usize, usize)) {
    let a = (a.path.clone(), a.start_byte, a.end_byte);
    let b = (b.path.clone(), b.start_byte, b.end_byte);
    if a <= b { (a, b) } else { (b, a) }
}

/// Compatibility lexer: structured tokens, mild trivia behavior, no format.
pub fn tokenize(content: &str) -> Vec<Token> {
    tokenize::tokenize_unscoped(content)
}

pub fn is_word(token: &str) -> bool {
    token
        .chars()
        .next()
        .is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '$'))
}

#[cfg(test)]
mod tests {
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
            stages.push(stage)
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
}
