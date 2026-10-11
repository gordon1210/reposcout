use crate::model::{
    ReviewAnalysisStatus, ReviewBrief, ReviewBriefHunk, ReviewChangeBasis, ReviewChangedSide,
    ReviewContextChange, ReviewContextRelation, ReviewContextReport, ReviewHunkRole,
    ReviewRevisionCoverage, ReviewUsageLine,
};
use crate::report::source::{body_block, source_text};
use crate::report::terminal_text;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

const SHORT_COMMIT: usize = 12;
const MAX_IMPORTERS: usize = 6;
const MAX_RENAMES: usize = 20;
const MAX_STALE_LINES: usize = 12;
const MAX_EXAMPLE_LINES: usize = 6;
const MAX_EXAMPLE_CHARS: usize = 160;
const MAX_OMITTED_FILES: usize = 40;
const PATHS_PER_COMMAND: usize = 10;
const MAX_HUNK_MOVES: usize = 3;
/// Selection level from which rewrites, moves, renames, stale spellings and omissions render as
/// counts.
const COLLAPSED: u8 = 3;

/// Rendered brief with the text of its soft evidence (importers, usage and documentation lines)
/// and of its diff evidence (examples and shown hunks), for proportional output limits.
pub(crate) struct BriefRender {
    pub text: String,
    pub soft: String,
    pub diff: String,
}

/// Compact agent projection; Markdown wraps the same text in one fenced block.
pub(crate) fn render(report: &ReviewContextReport, markdown: bool) -> String {
    render_parts(report, markdown).text
}

pub(crate) fn render_parts(report: &ReviewContextReport, markdown: bool) -> BriefRender {
    let mut parts = plain(report);
    if markdown {
        parts.text = body_block(parts.text.trim_end_matches('\n'), true)
            .trim_start()
            .to_string();
    }
    parts
}

/// Identity, sides, `(kind -> target, site)` text and source path of one reference.
type Reference<'a> = (String, BTreeSet<&'a str>, (String, String), &'a str);

/// Relations of one change that the brief can list.
#[derive(Default)]
struct Evidence<'a> {
    importers: BTreeMap<&'a str, BTreeSet<&'a str>>,
    references: Vec<Reference<'a>>,
    types: Vec<(String, BTreeSet<&'a str>)>,
}

impl Evidence<'_> {
    fn count(&self) -> usize {
        self.importers.len() + self.references.len() + self.types.len()
    }
}

fn plain(report: &ReviewContextReport) -> BriefRender {
    let default = ReviewBrief::default();
    let brief = report.brief.as_ref().unwrap_or(&default);
    let selection = &brief.selection;
    let mut used = vec![false; report.relations.len()];
    let evidence: Vec<_> = report
        .changes
        .iter()
        .map(|change| evidence(report, change, &mut used))
        .collect();
    let unlisted: usize = evidence
        .iter()
        .enumerate()
        .filter(|(index, _)| !selection.details.contains(index))
        .map(|(_, evidence)| evidence.count())
        .sum();
    let mut parts = BriefRender {
        text: header(report, brief, unlisted),
        soft: String::new(),
        diff: String::new(),
    };
    rewrites(report, brief, &mut parts);
    moved(report, brief, &mut parts.text);
    renames(report, brief, &mut parts.text);
    stale(report, brief, &mut parts.text);
    let mut arrivals: Vec<usize> = (0..brief.moves.len()).collect();
    arrivals.sort_by_key(|&movement| brief.moves[movement].to);
    for &index in &brief.entry_changes {
        if selection.entries.contains(&index) {
            entry(
                report,
                brief,
                index,
                &evidence[index],
                &arrivals,
                &mut parts,
            );
        }
    }
    let mentions: Vec<_> = brief
        .mentions
        .iter()
        .enumerate()
        .filter(|(index, _)| selection.mentions.contains(index))
        .collect();
    if !mentions.is_empty() {
        let mut text = String::from("docs naming changed definitions:\n");
        for (_, mention) in mentions {
            let _ = writeln!(
                text,
                "{}:{} {}",
                path_text(&mention.path),
                mention.line,
                terminal_text(&mention.text)
            );
        }
        parts.text.push_str(&text);
        parts.soft.push_str(&text);
    }
    omitted_hunks(report, brief, &mut parts.text);
    omitted_explained(report, brief, &mut parts.text);
    omitted_entries(report, brief, &mut parts.text);
    if brief.selection.level >= COLLAPSED {
        let _ = writeln!(parts.text, "full diff: {}", diff_command(report));
    }
    if report.changes.is_empty() {
        parts.text.push_str("no changes shown\n");
    }
    parts
}

fn short(commit: &str) -> &str {
    commit.get(..SHORT_COMMIT).unwrap_or(commit)
}

fn path_text(path: &Path) -> String {
    terminal_text(&path.to_string_lossy())
}

fn label(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Display path of a change: the head path, or the base path of a deletion.
fn change_path(change: &ReviewContextChange) -> &Path {
    change
        .head
        .as_ref()
        .or(change.base.as_ref())
        .map_or(Path::new(""), |side| side.path.as_path())
}

/// Both paths of a rename or copy, and a mode change.
fn change_label(change: &ReviewContextChange) -> String {
    let mut label = match (&change.base, &change.head) {
        (Some(base), Some(head)) if base.path != head.path => {
            format!("{} -> {}", path_text(&base.path), path_text(&head.path))
        }
        (_, Some(side)) | (Some(side), None) => path_text(&side.path),
        (None, None) => "unknown path".into(),
    };
    if let (Some(base), Some(head)) = (&change.base, &change.head)
        && base.mode != head.mode
    {
        let _ = write!(label, " (mode {:o} -> {:o})", base.mode, head.mode);
    }
    label
}

/// `git diff` between the resolved base (the merge base in merge-base mode) and head commits,
/// with the hunk boundaries of the brief's own diff.
fn diff_command(report: &ReviewContextReport) -> String {
    format!(
        "git diff --no-indent-heuristic {} {}",
        short(&report.comparison.base_commit),
        short(&report.comparison.head_commit)
    )
}

/// Literal repository-root pathspec, independent of the working directory and of characters
/// that Git reads as pathspec magic or wildcards.
fn pathspec(path: &Path) -> String {
    shell(&format!(":(top,literal){}", path.to_string_lossy()))
}

fn header(report: &ReviewContextReport, brief: &ReviewBrief, unlisted: usize) -> String {
    let c = &report.comparison;
    let t = &report.totals;
    let mut output = format!(
        "review-context brief ({}): base {} head {}",
        c.mode,
        short(&c.base_commit),
        short(&c.head_commit)
    );
    if c.requested_base_commit != c.base_commit {
        let _ = write!(
            output,
            " (requested base {})",
            short(&c.requested_base_commit)
        );
    }
    let _ = writeln!(
        output,
        "; changes {}, changed definitions (base+head) {}, diff tokens {}",
        t.changes, t.definitions, t.diff_tokens
    );
    output.push_str(
        "note: static potential impact only; run the repository checks before concluding; [test] marks filename/syntax hints; read files with git show <commit>:<path>\n",
    );
    let gaps = gaps(report, brief);
    if !gaps.is_empty() {
        let _ = writeln!(output, "gaps: {}", gaps.join("; "));
    }
    if let Some(line) = dispatch(brief) {
        output.push_str(&line);
    }
    let omitted = omitted(report, brief);
    if !omitted.is_empty() {
        let _ = writeln!(output, "omitted from output: {}", omitted.join(", "));
    }
    let mut counted = Vec::new();
    counted.extend(pair(
        "transitive dependents",
        brief.transitive_dependents[0],
        brief.transitive_dependents[1],
    ));
    counted.extend(pair(
        "resolver-config scope files",
        brief.config_scope_files[0],
        brief.config_scope_files[1],
    ));
    if unlisted > 0 {
        counted.push(format!(
            "relations of changes without listed details {unlisted}"
        ));
    }
    if !counted.is_empty() {
        let _ = writeln!(output, "counted, not listed: {}", counted.join("; "));
    }
    output
}

/// Names listed on the unresolved-dispatch line; the remainder is counted.
const MAX_DISPATCH_NAMES: usize = 3;

/// Added methods reusing method names of existing files without resolved callers. Every level
/// lists the same names; the collapsed level replaces head call sites with their count.
fn dispatch(brief: &ReviewBrief) -> Option<String> {
    if brief.dispatch.is_empty() {
        return None;
    }
    let definitions: usize = brief.dispatch.iter().map(|gap| gap.definitions).sum();
    let collapsed = brief.selection.level >= COLLAPSED;
    let mut names = brief
        .dispatch
        .iter()
        .take(MAX_DISPATCH_NAMES)
        .map(|gap| {
            let calls = if gap.sites_total == 0 {
                "no receiver call found".to_string()
            } else if collapsed {
                format!(
                    "{} receiver call {}",
                    gap.sites_total,
                    if gap.sites_total == 1 {
                        "site"
                    } else {
                        "sites"
                    }
                )
            } else {
                let mut sites: Vec<_> = gap
                    .sites
                    .iter()
                    .map(|(path, line)| format!("{}:{line}", path_text(path)))
                    .collect();
                if gap.sites_total > gap.sites.len() {
                    sites.push(format!("+{} more", gap.sites_total - gap.sites.len()));
                }
                format!("called at {}", sites.join(", "))
            };
            format!("{} ({}) {calls}", terminal_text(&gap.name), gap.definitions)
        })
        .collect::<Vec<_>>();
    if brief.dispatch.len() > MAX_DISPATCH_NAMES {
        let more = brief.dispatch.len() - MAX_DISPATCH_NAMES;
        names.push(format!(
            "{more} more {}",
            if more == 1 { "name" } else { "names" }
        ));
    }
    Some(format!(
        "unresolved dispatch: added methods {definitions} in files {} reuse existing method names without a resolved caller; trace the dispatcher and the code that validates, stores or orders their results: {}\n",
        brief.dispatch_files,
        names.join("; ")
    ))
}

/// `label N` when both revisions agree, `label B/H (base/head)` otherwise; nothing when both are zero.
fn pair(label: &str, base: usize, head: usize) -> Option<String> {
    match (base, head) {
        (0, 0) => None,
        _ if base == head => Some(format!("{label} {head}")),
        _ => Some(format!("{label} {base}/{head} (base/head)")),
    }
}

type Metric = fn(&ReviewRevisionCoverage) -> usize;

/// Gaps that limit the evidence about this change; repository-wide counters stay in JSON.
fn gaps(report: &ReviewContextReport, brief: &ReviewBrief) -> Vec<String> {
    let t = &report.totals;
    let mut gaps = Vec::new();
    if t.changes_not_analyzed > 0 {
        gaps.push(format!("{} changes not analyzed", t.changes_not_analyzed));
    }
    if t.changes_without_hunks > 0 {
        gaps.push(format!("{} changes without hunks", t.changes_without_hunks));
    }
    if !report.comparison.rename_detection_complete {
        gaps.push("rename detection incomplete".into());
    }
    let default = ReviewRevisionCoverage::default();
    let base = report.coverage.first().unwrap_or(&default);
    let head = report.coverage.get(1).unwrap_or(&default);
    let metrics: [(&str, Metric); 4] = [
        ("inventory truncated", |side| {
            usize::from(side.inventory_truncated)
        }),
        ("unavailable inputs", |side| {
            side.unavailable_files.values().sum()
        }),
        ("changed files without graph", |side| {
            side.changed_files_without_graph
        }),
        ("resolver config errors", |side| side.config_errors),
    ];
    gaps.extend(
        metrics
            .into_iter()
            .filter_map(|(label, metric)| pair(label, metric(base), metric(head))),
    );
    let calls = brief.call_gaps;
    gaps.extend(pair(
        "changed files without call references",
        calls[0][0],
        calls[1][0],
    ));
    gaps.extend(pair(
        "changed files with incomplete call references",
        calls[0][1],
        calls[1][1],
    ));
    gaps
}

/// Shown hunks, rule-explained or not as `explained` says, that the output does not include, in
/// change and hunk order. Collapsed output counts rule-explained hunks with all others.
fn omitted_hunk_list(brief: &ReviewBrief, explained: bool) -> Vec<(usize, &ReviewBriefHunk)> {
    brief
        .hunks
        .iter()
        .enumerate()
        .filter(|(index, hunk)| {
            hunk.role == ReviewHunkRole::Shown
                && (hunk.explained && brief.selection.level < COLLAPSED) == explained
                && !(brief.selection.hunks.contains(index)
                    && brief.selection.entries.contains(&hunk.change))
        })
        .collect()
}

fn omitted(report: &ReviewContextReport, brief: &ReviewBrief) -> Vec<String> {
    let selection = &brief.selection;
    let hunks = omitted_hunk_list(brief, false).len();
    let explained = omitted_hunk_list(brief, true).len();
    let entries = brief
        .entry_changes
        .iter()
        .filter(|index| !selection.entries.contains(index))
        .count();
    let details = brief
        .entry_changes
        .iter()
        .filter(|index| selection.entries.contains(index) && !selection.details.contains(index))
        .count();
    let usage = brief.usage_total.saturating_sub(
        report
            .changes
            .iter()
            .map(|change| usage_lines(brief, change).len())
            .sum(),
    );
    [
        (
            hunks,
            if selection.level < 2 {
                "hunks (listed at the end)"
            } else {
                "hunks (counted at the end)"
            },
        ),
        (
            explained,
            if selection.explained_sites {
                "rule-explained hunks (grouped at the end)"
            } else {
                "rule-explained hunks (counted at the end)"
            },
        ),
        (entries, "change entries"),
        (details, "change details"),
        (
            brief
                .mentions_total
                .saturating_sub(selection.mentions.len()),
            "doc lines",
        ),
        (usage, "usage lines"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, name)| format!("{count} {name}"))
    .collect()
}

fn code(text: &str) -> String {
    format!("`{}`", terminal_text(text))
}

fn rule_text(brief: &ReviewBrief, index: usize) -> String {
    let rule = &brief.rewrites[index];
    let edit = match (rule.old.is_empty(), rule.new.is_empty()) {
        (true, _) => format!("insert {}", code(&rule.new)),
        (_, true) => format!("delete {}", code(&rule.old)),
        _ => format!("{} → {}", code(&rule.old), code(&rule.new)),
    };
    format!(
        "R{} {edit}{} ({} hunks)",
        index + 1,
        match (rule.inside, rule.rename) {
            (false, _) => "",
            (true, true) => ", also inside names",
            (true, false) => ", also inside literals",
        },
        rule.hunks
    )
}

fn rule_names(rules: &[usize]) -> String {
    rules
        .iter()
        .map(|rule| format!("R{}", rule + 1))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rewrite example hunks with their changed lines, clipped.
fn examples(report: &ReviewContextReport, brief: &ReviewBrief, parts: &mut BriefRender) {
    for &index in &brief.examples {
        let hunk = &brief.hunks[index];
        let change = &report.changes[hunk.change];
        let mut text = format!(
            "  e.g. {}:{} [{}]\n",
            path_text(change_path(change)),
            hunk.site_line,
            rule_names(&hunk.rewrites)
        );
        for line in hunk.changed.iter().take(MAX_EXAMPLE_LINES) {
            let clipped: String = if line.chars().count() > MAX_EXAMPLE_CHARS {
                let mut clipped: String = line.chars().take(MAX_EXAMPLE_CHARS - 1).collect();
                clipped.push('…');
                clipped
            } else {
                line.clone()
            };
            let _ = writeln!(text, "    {}", source_text(&clipped));
        }
        if hunk.changed.len() > MAX_EXAMPLE_LINES {
            let _ = writeln!(
                text,
                "    … +{} lines",
                hunk.changed.len() - MAX_EXAMPLE_LINES
            );
        }
        parts.text.push_str(&text);
        parts.diff.push_str(&text);
    }
}

/// Hunks of one change with their indices; hunks are stored in change order.
fn change_hunks(
    brief: &ReviewBrief,
    change: usize,
) -> impl Iterator<Item = (usize, &ReviewBriefHunk)> + Clone {
    let start = brief.hunks.partition_point(|hunk| hunk.change < change);
    brief.hunks[start..]
        .iter()
        .take_while(move |hunk| hunk.change == change)
        .enumerate()
        .map(move |(offset, hunk)| (start + offset, hunk))
}

/// Hunks with `role` and the files they belong to.
fn role_counts(brief: &ReviewBrief, role: ReviewHunkRole) -> (usize, usize) {
    let hunks: Vec<_> = brief
        .hunks
        .iter()
        .filter(|hunk| hunk.role == role)
        .collect();
    let files: BTreeSet<_> = hunks.iter().map(|hunk| hunk.change).collect();
    (hunks.len(), files.len())
}

fn rewrites(report: &ReviewContextReport, brief: &ReviewBrief, parts: &mut BriefRender) {
    if brief.rewrites.is_empty() {
        return;
    }
    let output = &mut parts.text;
    if brief.selection.level >= COLLAPSED {
        let (hunks, files) = role_counts(brief, ReviewHunkRole::Rewrite);
        let moved: usize = brief.hunks.iter().map(|hunk| moved_lines(hunk).len()).sum();
        let _ = writeln!(
            output,
            "rewrites: {} rules, {hunks} rewrite-only hunks in {files} files{} (not listed)",
            brief.rewrites.len(),
            if moved > 0 {
                format!(", {moved} moved lines with rewrites")
            } else {
                String::new()
            }
        );
        return;
    }
    output
        .push_str("rewrites (token edits; rewrite-only hunks contain only renames listed here):\n");
    for index in 0..brief.rewrites.len() {
        let _ = writeln!(output, "  {}", rule_text(brief, index));
    }
    if brief.selection.examples {
        examples(report, brief, parts);
    }
    let output = &mut parts.text;
    let mut groups = BTreeMap::<&[usize], BTreeMap<&Path, Vec<usize>>>::new();
    let mut moved = BTreeMap::<Vec<usize>, BTreeMap<&Path, Vec<usize>>>::new();
    for hunk in &brief.hunks {
        let path = change_path(&report.changes[hunk.change]);
        if hunk.role == ReviewHunkRole::Rewrite {
            groups
                .entry(&hunk.rewrites)
                .or_default()
                .entry(path)
                .or_default()
                .push(hunk.site_line);
        }
        let mut lines = BTreeMap::<usize, Vec<usize>>::new();
        for &(rule, line) in &hunk.moved_sites {
            lines.entry(line).or_default().push(rule);
        }
        for (line, rules) in lines {
            moved
                .entry(rules)
                .or_default()
                .entry(path)
                .or_default()
                .push(line);
        }
    }
    let site_lines = brief.selection.site_lines;
    let list = |output: &mut String, rules: &[usize], files: BTreeMap<&Path, Vec<usize>>| {
        let sites: Vec<_> = files
            .into_iter()
            .map(|(path, lines)| (path, site_suffix(&lines, site_lines)))
            .collect();
        let _ = writeln!(output, "    {}: {}", rule_names(rules), compress(&sites));
    };
    if !groups.is_empty() {
        let _ = writeln!(
            output,
            "  rewrite-only hunks{}:",
            if site_lines {
                ""
            } else {
                " (×N hunks per file)"
            }
        );
        for (rules, files) in groups {
            list(output, rules, files);
        }
    }
    if !moved.is_empty() {
        let _ = writeln!(
            output,
            "  moved lines with rewrites (head lines){}:",
            if site_lines {
                ""
            } else {
                " (×N lines per file)"
            }
        );
        for (rules, files) in moved {
            list(output, &rules, files);
        }
    }
}

/// `:a,b` with site lines, otherwise `×N` for more than one site.
fn site_suffix(lines: &[usize], site_lines: bool) -> String {
    if site_lines {
        format!(
            ":{}",
            lines
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )
    } else if lines.len() > 1 {
        format!("×{}", lines.len())
    } else {
        String::new()
    }
}

/// Distinct head lines of a hunk's moved rewrite sites.
fn moved_lines(hunk: &ReviewBriefHunk) -> BTreeSet<usize> {
    hunk.moved_sites.iter().map(|(_, line)| *line).collect()
}

/// `dir/{a,b}.ext:suffix` for files sharing directory, extension and suffix, in path order.
/// Paths with whitespace or control characters stand alone, quoted like command arguments.
fn compress(sites: &[(&Path, String)]) -> String {
    type Key<'a> = (String, String, &'a str);
    // A group's key, or `None` with the finished text of a quoted site.
    let mut groups: Vec<(Option<Key<'_>>, Vec<String>)> = Vec::new();
    let mut positions = BTreeMap::<Key<'_>, usize>::new();
    for (path, suffix) in sites {
        let text = path.to_string_lossy();
        if text.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
            groups.push((None, vec![format!("{}{suffix}", shell(&text))]));
            continue;
        }
        let (dir, name) = text
            .rsplit_once('/')
            .map_or(("", text.as_ref()), |(dir, name)| (dir, name));
        let (stem, extension) = match name.rsplit_once('.') {
            Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
            _ => (name, String::new()),
        };
        let key = (dir.to_string(), extension, suffix.as_str());
        if let Some(&position) = positions.get(&key) {
            groups[position].1.push(stem.to_string());
        } else {
            positions.insert(key.clone(), groups.len());
            groups.push((Some(key), vec![stem.to_string()]));
        }
    }
    groups
        .into_iter()
        .map(|(key, mut stems)| {
            let Some((dir, extension, suffix)) = key else {
                return stems.remove(0);
            };
            let prefix = if dir.is_empty() {
                String::new()
            } else {
                format!("{dir}/")
            };
            let name = if stems.len() == 1 {
                stems.remove(0)
            } else {
                format!("{{{}}}", stems.join(","))
            };
            terminal_text(&format!("{prefix}{name}{extension}{suffix}"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `path:a-b`, or `:a-b` when `path` is `relative_to`.
fn place(path: &Path, lines: [usize; 2], relative_to: Option<&Path>) -> String {
    let range = if lines[0] == lines[1] {
        lines[0].to_string()
    } else {
        format!("{}-{}", lines[0], lines[1])
    };
    if relative_to == Some(path) {
        format!(":{range}")
    } else {
        format!("{}:{range}", path_text(path))
    }
}

/// Base path of a move's source hunk and head path of its destination hunk.
fn move_paths<'a>(
    report: &'a ReviewContextReport,
    brief: &ReviewBrief,
    from: usize,
    to: usize,
) -> (&'a Path, &'a Path) {
    let side = |hunk: usize, base: bool| {
        let change = &report.changes[brief.hunks[hunk].change];
        let side = if base {
            change.base.as_ref().or(change.head.as_ref())
        } else {
            change.head.as_ref().or(change.base.as_ref())
        };
        side.map_or(Path::new(""), |side| side.path.as_path())
    };
    (side(from, true), side(to, false))
}

/// Every move touching a moved hunk, as base lines of its source and head lines of its
/// destination.
fn moved(report: &ReviewContextReport, brief: &ReviewBrief, output: &mut String) {
    let is_moved = |hunk: usize| brief.hunks[hunk].role == ReviewHunkRole::Moved;
    if !brief
        .hunks
        .iter()
        .any(|hunk| hunk.role == ReviewHunkRole::Moved)
    {
        return;
    }
    if brief.selection.level >= COLLAPSED {
        let (hunks, files) = role_counts(brief, ReviewHunkRole::Moved);
        let _ = writeln!(
            output,
            "moved hunks: {hunks} in {files} files (destinations not listed)"
        );
        return;
    }
    output.push_str(
        "moved hunks (every changed line recurs in another hunk after rewrites; base lines → head lines):\n",
    );
    for movement in brief
        .moves
        .iter()
        .filter(|movement| is_moved(movement.from) || is_moved(movement.to))
    {
        let (from, to) = move_paths(report, brief, movement.from, movement.to);
        let _ = writeln!(
            output,
            "  {} → {}",
            place(from, movement.from_lines, None),
            place(to, movement.to_lines, Some(from))
        );
    }
}

/// Renamed, copied or mode-changed files without an entry, with their folded hunks.
fn renames(report: &ReviewContextReport, brief: &ReviewBrief, output: &mut String) {
    if brief.renames.is_empty() {
        return;
    }
    if brief.selection.level >= COLLAPSED {
        let _ = writeln!(
            output,
            "renamed or mode changed without a shown hunk: {} files (not listed)",
            brief.renames.len()
        );
        return;
    }
    let mut listed: Vec<_> = brief
        .renames
        .iter()
        .take(MAX_RENAMES)
        .map(|&index| {
            let hunks = change_hunks(brief, index);
            let count = |role| hunks.clone().filter(|(_, hunk)| hunk.role == role).count();
            let folded: Vec<_> = [
                (count(ReviewHunkRole::Rewrite), "rewrite-only"),
                (count(ReviewHunkRole::Moved), "moved"),
            ]
            .into_iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, what)| format!("{count} {what}"))
            .collect();
            let label = change_label(&report.changes[index]);
            if folded.is_empty() {
                label
            } else {
                format!("{label} ({})", folded.join(", "))
            }
        })
        .collect();
    if brief.renames.len() > MAX_RENAMES {
        listed.push(format!(
            "+{} more: git diff -M --summary {} {}",
            brief.renames.len() - MAX_RENAMES,
            short(&report.comparison.base_commit),
            short(&report.comparison.head_commit)
        ));
    }
    let _ = writeln!(
        output,
        "renamed or mode changed without a shown hunk: {}",
        listed.join(", ")
    );
}

/// POSIX shell word for a ref or path, quoted only when needed.
fn shell(word: &str) -> String {
    if word.chars().any(char::is_control) {
        let mut quoted = String::from("$'");
        for ch in word.chars() {
            match ch {
                '\n' => quoted.push_str("\\n"),
                '\t' => quoted.push_str("\\t"),
                '\r' => quoted.push_str("\\r"),
                '\\' | '\'' => {
                    quoted.push('\\');
                    quoted.push(ch);
                }
                ch if ch.is_control() => {
                    let _ = write!(quoted, "\\U{:08x}", u32::from(ch));
                }
                ch => quoted.push(ch),
            }
        }
        quoted.push('\'');
        return quoted;
    }
    if !word.is_empty()
        && word
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_./@^~:+=,-".contains(ch))
    {
        terminal_text(word)
    } else {
        terminal_text(&format!("'{}'", word.replace('\'', "'\\''")))
    }
}

fn stale(report: &ReviewContextReport, brief: &ReviewBrief, output: &mut String) {
    if brief.stale.is_empty() {
        return;
    }
    let grep = |names: &mut dyn Iterator<Item = &str>| {
        let mut command = String::from("git grep -n -w -F");
        for name in names {
            let _ = write!(command, " -e {}", shell(name));
        }
        let _ = write!(command, " {} -- :/", short(&report.comparison.head_commit));
        command
    };
    let total: usize = brief.stale.iter().map(|name| name.head_lines).sum();
    if brief.selection.level >= COLLAPSED {
        let _ = writeln!(
            output,
            "old spellings still in head (verify intent): {} names in {total} lines (not listed)",
            brief.stale.len()
        );
        return;
    }
    if brief.selection.level >= 1 {
        let names: Vec<_> = brief
            .stale
            .iter()
            .map(|name| {
                format!(
                    "{} → {} {} lines",
                    code(&name.old),
                    code(&name.new),
                    name.head_lines
                )
            })
            .collect();
        let _ = writeln!(
            output,
            "old spellings still in head (verify intent): {}: {}",
            names.join(", "),
            grep(&mut brief.stale.iter().map(|name| name.old.as_str()))
        );
        return;
    }
    let mut shown = vec![0; brief.stale.len()];
    let mut left = MAX_STALE_LINES;
    while left > 0 {
        let before = left;
        for (index, name) in brief.stale.iter().enumerate() {
            if left > 0 && shown[index] < name.hits.len() {
                shown[index] += 1;
                left -= 1;
            }
        }
        if left == before {
            break;
        }
    }
    output.push_str("old spellings still in head (verify intent):\n");
    for (name, shown) in brief.stale.iter().zip(&shown) {
        let _ = writeln!(
            output,
            "  {} → {}: {} head lines (base {})",
            code(&name.old),
            code(&name.new),
            name.head_lines,
            name.base_lines
        );
        for hit in name.hits.iter().take(*shown) {
            let _ = writeln!(
                output,
                "    {}:{} {}",
                path_text(&hit.path),
                hit.line,
                terminal_text(&hit.text)
            );
        }
    }
    let listed: usize = shown.iter().sum();
    if total > listed {
        let _ = writeln!(
            output,
            "  … +{} more lines: {}",
            total - listed,
            grep(&mut brief.stale.iter().map(|name| name.old.as_str()))
        );
    }
}

fn entry(
    report: &ReviewContextReport,
    brief: &ReviewBrief,
    index: usize,
    evidence: &Evidence<'_>,
    arrivals: &[usize],
    parts: &mut BriefRender,
) {
    let change = &report.changes[index];
    let selection = &brief.selection;
    let paths = change_label(change);
    let hunks: Vec<_> = change_hunks(brief, index).collect();
    let count = |role| hunks.iter().filter(|(_, hunk)| hunk.role == role).count();
    let shown: Vec<_> = hunks
        .iter()
        .filter(|(position, hunk)| {
            hunk.role == ReviewHunkRole::Shown && selection.hunks.contains(position)
        })
        .collect();
    let mut summary = vec![match (change.diff.as_ref(), hunks.len()) {
        (None, _) => "hunks unknown".to_string(),
        (_, 1) => "1 hunk".into(),
        (_, count) => format!("{count} hunks"),
    }];
    if change.hunk_status == ReviewAnalysisStatus::Partial {
        summary.push("partial".into());
    }
    for (count, what) in [
        (count(ReviewHunkRole::Rewrite), "rewrite-only"),
        (count(ReviewHunkRole::Moved), "moved"),
        (count(ReviewHunkRole::Shown) - shown.len(), "omitted"),
    ] {
        if count > 0 {
            summary.push(format!("{count} {what}"));
        }
    }
    let output = &mut parts.text;
    let _ = writeln!(output, "{} {paths} ({})", change.status, summary.join("; "));
    let analysis = analysis(change);
    if !analysis.is_empty() {
        let _ = writeln!(output, "  analysis: {}", analysis.join(", "));
    }
    let details = selection.details.contains(&index);
    if details {
        definitions(change, output);
    }
    for (position, _) in &shown {
        let text = annotated(report, brief, *position, arrivals);
        let text = source_text(&text);
        output.push_str(&text);
        parts.diff.push_str(&text);
    }
    if count(ReviewHunkRole::Shown) == 0 {
        for (position, hunk) in hunks
            .iter()
            .filter(|(_, hunk)| hunk.role == ReviewHunkRole::Moved)
        {
            let _ = writeln!(
                output,
                "  @@ {} {} @@ [{} lines hidden: moved {}]",
                span('-', hunk.old_start, hunk.old_lines),
                span('+', hunk.new_start, hunk.new_lines),
                hunk.moved_lines,
                move_places(report, brief, *position, arrivals)
            );
        }
    }
    if change.diff.is_none() {
        output.push_str("  diff unavailable\n");
    }
    if details {
        relations(brief, change, evidence, output);
    }
    if selection.importers.contains(&index) && !evidence.importers.is_empty() {
        let text = importers(report, evidence);
        output.push_str(&text);
        parts.soft.push_str(&text);
    }
    let usage = usage_lines(brief, change);
    if !usage.is_empty() {
        let mut text =
            String::from("  usage lines (identifier text match, no resolved reference):\n");
        for line in usage {
            let _ = writeln!(
                text,
                "    {}:{}{}  {}",
                path_text(&line.path),
                line.line,
                if line.side == "base" { " (base)" } else { "" },
                terminal_text(&line.text)
            );
        }
        output.push_str(&text);
        parts.soft.push_str(&text);
    }
}

/// Grouped reference sites, type relations and entry chains of a change with details.
fn relations(
    brief: &ReviewBrief,
    change: &ReviewContextChange,
    evidence: &Evidence<'_>,
    output: &mut String,
) {
    let mut groups: Vec<(&str, &str, Vec<&str>)> = Vec::new();
    for (_, sides, (target, site), _) in &evidence.references {
        let marker = sides_marker(sides);
        match groups
            .iter_mut()
            .find(|group| group.0 == target && group.1 == marker)
        {
            Some(group) => group.2.push(site),
            None => groups.push((target, marker, vec![site])),
        }
    }
    for (target, marker, sites) in groups {
        let _ = writeln!(output, "  {target}{marker}: {}", sites.join(", "));
    }
    for (key, sides) in &evidence.types {
        let _ = writeln!(output, "  type {key}{}", sides_marker(sides));
    }
    if let Some(head) = &change.head {
        for chain in &brief.entry_chains {
            if chain.last() == Some(&head.path) && chain.len() > 2 {
                let names: Vec<_> = chain[..chain.len() - 1]
                    .iter()
                    .map(|path| path_text(path))
                    .collect();
                let _ = writeln!(output, "  entry: {}", names.join(" <- "));
            }
        }
    }
}

/// `imported by:` with importers lacking a listed reference first, capped with an exact rest.
fn importers(report: &ReviewContextReport, evidence: &Evidence<'_>) -> String {
    let callers: BTreeSet<_> = evidence
        .references
        .iter()
        .map(|reference| reference.3)
        .collect();
    let mut importers: Vec<_> = evidence.importers.iter().collect();
    importers.sort_by_key(|(path, _)| (callers.contains(*path), **path));
    let mut listed: Vec<_> = importers
        .iter()
        .take(MAX_IMPORTERS)
        .map(|(path, sides)| {
            format!(
                "{}{}{}",
                terminal_text(path),
                if is_test(report, path) { " [test]" } else { "" },
                sides_marker(sides)
            )
        })
        .collect();
    if importers.len() > MAX_IMPORTERS {
        listed.push(format!("+{} more", importers.len() - MAX_IMPORTERS));
    }
    format!("  imported by: {}\n", listed.join(", "))
}

/// Hunk text with its rewrites, deviation and hidden lines on the header line; moved lines
/// name where they went to or came from. `arrivals` orders move indices by destination hunk.
fn annotated(
    report: &ReviewContextReport,
    brief: &ReviewBrief,
    index: usize,
    arrivals: &[usize],
) -> String {
    let hunk = &brief.hunks[index];
    let mut notes = Vec::new();
    if !hunk.rewrites.is_empty() {
        notes.push(rule_names(&hunk.rewrites));
    }
    if let Some(rule) = hunk.variant_of {
        notes.push(format!("deviates from R{}", rule + 1));
    }
    if hunk.unchanged_lines > 0 {
        notes.push(format!(
            "{} lines hidden: unchanged after rewrites",
            hunk.unchanged_lines
        ));
    }
    if hunk.moved_lines > 0 {
        notes.push(format!(
            "{} lines hidden: moved {}",
            hunk.moved_lines,
            move_places(report, brief, index, arrivals)
        ));
    }
    if notes.is_empty() {
        return hunk.text.clone();
    }
    let (first, rest) = hunk
        .text
        .split_once('\n')
        .unwrap_or((hunk.text.as_str(), ""));
    format!("{first} [{}]\n{rest}", notes.join("; "))
}

/// Destinations and sources of one hunk's moved lines, relative to its own path.
fn move_places(
    report: &ReviewContextReport,
    brief: &ReviewBrief,
    index: usize,
    arrivals: &[usize],
) -> String {
    let hunk = &brief.hunks[index];
    let own = report.changes[hunk.change]
        .head
        .as_ref()
        .or(report.changes[hunk.change].base.as_ref())
        .map(|side| side.path.as_path());
    let mut places: Vec<String> = Vec::new();
    let start = brief
        .moves
        .partition_point(|movement| movement.from < index);
    for movement in brief.moves[start..]
        .iter()
        .take_while(|movement| movement.from == index)
    {
        let (_, to) = move_paths(report, brief, movement.from, movement.to);
        places.push(format!("to {}", place(to, movement.to_lines, own)));
    }
    let start = arrivals.partition_point(|&movement| brief.moves[movement].to < index);
    for movement in arrivals[start..]
        .iter()
        .map(|&movement| &brief.moves[movement])
        .take_while(|movement| movement.to == index)
    {
        let (from, _) = move_paths(report, brief, movement.from, movement.to);
        places.push(format!("from {}", place(from, movement.from_lines, own)));
    }
    let total = places.len();
    places.truncate(MAX_HUNK_MOVES);
    if total > MAX_HUNK_MOVES {
        places.push(format!("+{} more", total - MAX_HUNK_MOVES));
    }
    places.join(", ")
}

/// Selected usage lines of one change in path and line order.
fn usage_lines<'a>(
    brief: &'a ReviewBrief,
    change: &ReviewContextChange,
) -> Vec<&'a ReviewUsageLine> {
    let identity = [&change.base, &change.head].map(|side| side.as_ref().map(|side| &side.path));
    let mut lines: Vec<_> = brief
        .usage
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            brief.selection.usage.contains(index)
                && line.change.each_ref().map(Option::as_ref) == identity
        })
        .map(|(_, line)| line)
        .collect();
    lines.sort_by(|left, right| {
        (&left.path, &left.side, left.line).cmp(&(&right.path, &right.side, right.line))
    });
    lines
}

fn analysis(change: &ReviewContextChange) -> Vec<String> {
    let mut items = Vec::new();
    for (name, side) in [("base", &change.base), ("head", &change.head)] {
        let Some(side) = side else {
            continue;
        };
        if side.status != "captured" {
            items.push(format!("{name} {}", side.status));
        }
        let unsupported = side.mapping_status == ReviewAnalysisStatus::Unavailable
            && side.extraction == crate::model::DefinitionStatus::Unsupported;
        if side.mapping_status != ReviewAnalysisStatus::Available && !unsupported {
            items.push(format!(
                "{name} mapping {} (extraction {})",
                label(&side.mapping_status),
                label(&side.extraction)
            ));
        }
        for (count, what) in [
            (side.ambiguous_definitions, "ambiguous definitions"),
            (side.unprocessed_ranges, "unprocessed ranges"),
            (side.ambiguous_counterparts, "ambiguous counterparts"),
            (side.unprocessed_counterparts, "unprocessed counterparts"),
        ] {
            if count > 0 {
                items.push(format!("{name} {count} {what}"));
            }
        }
        if side.counterpart_seed_mapping_incomplete {
            items.push(format!("{name} counterpart seeds incomplete"));
        }
    }
    items
}

fn definitions(change: &ReviewContextChange, output: &mut String) {
    let mut order = Vec::new();
    let mut spans = BTreeMap::<(&str, &str), [Option<(usize, usize)>; 2]>::new();
    for (index, side) in [&change.base, &change.head].into_iter().enumerate() {
        for definition in side.iter().flat_map(|side| &side.definitions) {
            let key = (
                definition.symbol.kind.as_str(),
                definition.symbol.name.as_str(),
            );
            let entry = spans.entry(key).or_insert_with(|| {
                order.push(key);
                [None, None]
            });
            entry[index].get_or_insert((
                definition.declaration_span.start_line,
                definition.declaration_span.end_line,
            ));
        }
    }
    let both = change.base.is_some() && change.head.is_some();
    for key in order {
        let range = |(start, end): (usize, usize)| format!("{start}-{end}");
        let location = match spans[&key] {
            [Some(base), Some(head)] if base == head => range(head),
            [Some(base), Some(head)] => format!("base {} head {}", range(base), range(head)),
            [Some(base), None] if both => format!("{} (base only)", range(base)),
            [None, Some(head)] if both => format!("{} (head only)", range(head)),
            [Some(span), None] | [None, Some(span)] => range(span),
            [None, None] => continue,
        };
        let _ = writeln!(
            output,
            "  {} {} {location}",
            terminal_text(key.0),
            terminal_text(key.1)
        );
    }
    for (name, side) in [("base", &change.base), ("head", &change.head)] {
        if let Some(side) = side.as_ref().filter(|side| side.definitions_omitted > 0) {
            let _ = writeln!(
                output,
                "  {name}: {} more definitions omitted",
                side.definitions_omitted
            );
        }
    }
}

fn side_of<'a>(change: &'a ReviewContextChange, side: &str) -> Option<&'a ReviewChangedSide> {
    if side == "base" {
        change.base.as_ref()
    } else {
        change.head.as_ref()
    }
}

fn is_test(report: &ReviewContextReport, path: &str) -> bool {
    report
        .context
        .iter()
        .any(|file| file.path == Path::new(path) && !file.test_evidence.is_empty())
}

/// Revision markers for evidence present on only one side; shared evidence is rendered once.
fn sides_marker(sides: &BTreeSet<&str>) -> &'static str {
    match (sides.contains("base"), sides.contains("head")) {
        (true, false) => " (base only)",
        (false, true) => " (head only)",
        _ => "",
    }
}

/// A call or reference site inside the changed lines of a changed file is visible in its diff.
fn site_in_changed_lines(
    report: &ReviewContextReport,
    side: &str,
    path: &str,
    line: usize,
) -> bool {
    report.changes.iter().any(|change| {
        side_of(change, side).is_some_and(|changed| {
            changed.path == Path::new(path)
                && changed
                    .ranges
                    .iter()
                    .any(|range| range.start <= line && line <= range.end)
        })
    })
}

/// Incoming references, type relations and importers of one change. Relations attributed to an
/// earlier change, outgoing and same-file references, references from changed lines, and the
/// change's own imports are not listed.
fn evidence<'a>(
    report: &'a ReviewContextReport,
    change: &ReviewContextChange,
    used: &mut [bool],
) -> Evidence<'a> {
    let mut evidence = Evidence::default();
    for (relation, used) in report.relations.iter().zip(used.iter_mut()) {
        let Some(path) = side_of(change, &relation.side).map(|side| side.path.as_path()) else {
            continue;
        };
        let target = Path::new(&relation.edge.target) == path;
        if *used || !(target || Path::new(&relation.edge.source) == path) {
            continue;
        }
        *used = true;
        let side = relation.side.as_str();
        match (&relation.symbol, &relation.type_relation) {
            (Some(symbol), _) => {
                if Path::new(&symbol.target.path) != path
                    || Path::new(&symbol.source.path) == path
                    || site_in_changed_lines(
                        report,
                        side,
                        &symbol.source.path,
                        symbol.site.start_line,
                    )
                {
                    continue;
                }
                let (key, line) = reference(report, change, relation);
                match evidence.references.iter_mut().find(|entry| entry.0 == key) {
                    Some(entry) => {
                        entry.1.insert(side);
                        if side == "head" {
                            entry.2 = line;
                        }
                    }
                    None => evidence.references.push((
                        key,
                        BTreeSet::from([side]),
                        line,
                        symbol.source.path.as_str(),
                    )),
                }
            }
            (None, Some(edge)) => {
                let key = format!(
                    "{} {} {}",
                    terminal_text(&edge.source),
                    terminal_text(&edge.relation),
                    terminal_text(&edge.target)
                );
                match evidence.types.iter_mut().find(|entry| entry.0 == key) {
                    Some(entry) => {
                        entry.1.insert(side);
                    }
                    None => evidence.types.push((key, BTreeSet::from([side]))),
                }
            }
            (None, None) if target => {
                evidence
                    .importers
                    .entry(relation.edge.source.as_str())
                    .or_default()
                    .insert(side);
            }
            (None, None) => {}
        }
    }
    evidence
}

/// Identity without site lines (so shifted base/head sites compare equal), the shared
/// `kind -> target` text and the rendered site.
fn reference(
    report: &ReviewContextReport,
    change: &ReviewContextChange,
    relation: &ReviewContextRelation,
) -> (String, (String, String)) {
    let Some(symbol) = &relation.symbol else {
        return (String::new(), (String::new(), String::new()));
    };
    let changed_target = [&change.base, &change.head]
        .into_iter()
        .flatten()
        .any(|side| side.path == Path::new(&symbol.target.path));
    let target = if changed_target {
        terminal_text(&symbol.target.name)
    } else {
        format!(
            "{} @ {}",
            terminal_text(&symbol.target.name),
            terminal_text(&symbol.target.path)
        )
    };
    let key = format!(
        "{} {} {} {} {} {:?}",
        label(&symbol.kind),
        symbol.source.path,
        symbol.source.name,
        symbol.target.path,
        symbol.target.name,
        relation.change_basis
    );
    let group = format!(
        "{} -> {target}{}",
        label(&symbol.kind),
        if relation.change_basis == Some(ReviewChangeBasis::ChangedFile) {
            " (file-level)"
        } else {
            ""
        }
    );
    let site = format!(
        "{}:{} {}{}",
        terminal_text(&symbol.source.path),
        symbol.site.start_line,
        terminal_text(&symbol.source.name),
        if is_test(report, &symbol.source.path) {
            " [test]"
        } else {
            ""
        }
    );
    (key, (group, site))
}

/// `-a,b` / `+c,d` with Git's single-line shorthand.
fn span(sign: char, start: usize, lines: usize) -> String {
    if lines == 1 {
        format!("{sign}{start}")
    } else {
        format!("{sign}{start},{lines}")
    }
}

/// `git diff` limited to every base and head path of `changes`.
fn fetch_command(report: &ReviewContextReport, changes: &[usize]) -> String {
    let mut paths = BTreeSet::new();
    for &index in changes {
        let change = &report.changes[index];
        for side in [&change.base, &change.head].into_iter().flatten() {
            paths.insert(side.path.as_path());
        }
    }
    let paths: Vec<_> = paths.into_iter().map(pathspec).collect();
    format!("{} -- {}", diff_command(report), paths.join(" "))
}

/// Omitted rule-explained hunks grouped by rewrite combination: sites per group and one command
/// per ten files, or per group counts and one command; one count line with that command from
/// level 2.
fn omitted_explained(report: &ReviewContextReport, brief: &ReviewBrief, output: &mut String) {
    let hunks = omitted_hunk_list(brief, true);
    if hunks.is_empty() {
        return;
    }
    let mut groups = BTreeMap::<&[usize], BTreeMap<usize, Vec<usize>>>::new();
    for (_, hunk) in &hunks {
        groups
            .entry(&hunk.rewrites)
            .or_default()
            .entry(hunk.change)
            .or_default()
            .push(hunk.site_line);
    }
    let changes: BTreeSet<usize> = hunks.iter().map(|(_, hunk)| hunk.change).collect();
    let changes: Vec<_> = changes.into_iter().collect();
    let command = if changes.len() <= PATHS_PER_COMMAND {
        fetch_command(report, &changes)
    } else {
        diff_command(report)
    };
    if brief.selection.level >= 2 {
        let _ = writeln!(
            output,
            "omitted rule-explained hunks: {} in {} {}: {command}",
            hunks.len(),
            changes.len(),
            if changes.len() == 1 { "file" } else { "files" }
        );
        return;
    }
    let _ = writeln!(
        output,
        "omitted rule-explained hunks ({} in {} {}; every edit is a listed rewrite):",
        hunks.len(),
        changes.len(),
        if changes.len() == 1 { "file" } else { "files" }
    );
    for (rules, files) in &groups {
        let count: usize = files.values().map(Vec::len).sum();
        if brief.selection.explained_sites {
            let sites: Vec<_> = files
                .iter()
                .map(|(&change, lines)| {
                    let lines: Vec<_> = lines.iter().map(usize::to_string).collect();
                    (
                        change_path(&report.changes[change]),
                        format!(":{}", lines.join(",")),
                    )
                })
                .collect();
            let _ = writeln!(
                output,
                "  {} ({count} {}): {}",
                rule_names(rules),
                if count == 1 { "hunk" } else { "hunks" },
                compress(&sites)
            );
        } else {
            let _ = writeln!(
                output,
                "  {}: {count} {} in {} {}",
                rule_names(rules),
                if count == 1 { "hunk" } else { "hunks" },
                files.len(),
                if files.len() == 1 { "file" } else { "files" }
            );
        }
    }
    if brief.selection.explained_sites {
        for chunk in changes.chunks(PATHS_PER_COMMAND) {
            let _ = writeln!(output, "  {}", fetch_command(report, chunk));
        }
    } else {
        let _ = writeln!(output, "  {command}");
    }
}

/// Every omitted shown hunk that is not rule-explained by file, with exact `git diff` commands of
/// at most ten files each.
fn omitted_hunks(report: &ReviewContextReport, brief: &ReviewBrief, output: &mut String) {
    let hunks = omitted_hunk_list(brief, false);
    if hunks.is_empty() {
        return;
    }
    let mut files = BTreeMap::<usize, Vec<String>>::new();
    for (_, hunk) in &hunks {
        let change = &report.changes[hunk.change];
        files
            .entry(hunk.change)
            .or_default()
            .push(if change.head.is_some() {
                span('+', hunk.new_start, hunk.new_lines)
            } else {
                span('-', hunk.old_start, hunk.old_lines)
            });
    }
    let diff = diff_command(report);
    let command = |changes: &[usize]| fetch_command(report, changes);
    let changes: Vec<_> = files.keys().copied().collect();
    if brief.selection.level >= COLLAPSED {
        let _ = writeln!(
            output,
            "omitted hunks: {} in {} files",
            hunks.len(),
            changes.len()
        );
        return;
    }
    if brief.selection.level >= 2 {
        let _ = writeln!(
            output,
            "omitted hunks: {} in {} files: {}",
            hunks.len(),
            changes.len(),
            if changes.len() <= PATHS_PER_COMMAND {
                command(&changes)
            } else {
                diff.clone()
            }
        );
        return;
    }
    let _ = writeln!(
        output,
        "omitted hunks ({} in {} files):",
        hunks.len(),
        changes.len()
    );
    for chunk in changes
        .iter()
        .take(MAX_OMITTED_FILES)
        .copied()
        .collect::<Vec<_>>()
        .chunks(PATHS_PER_COMMAND)
    {
        for index in chunk {
            let _ = writeln!(
                output,
                "  {} @@ {}",
                change_label(&report.changes[*index]),
                files[index].join(" ")
            );
        }
        let _ = writeln!(output, "  {}", command(chunk));
    }
    if changes.len() > MAX_OMITTED_FILES {
        let rest: usize = changes[MAX_OMITTED_FILES..]
            .iter()
            .map(|index| files[index].len())
            .sum();
        let _ = writeln!(
            output,
            "  … +{} more files with {rest} hunks: {diff}",
            changes.len() - MAX_OMITTED_FILES
        );
    }
}

/// Change entries left out of the output without a shown hunk to list, by path and status.
fn omitted_entries(report: &ReviewContextReport, brief: &ReviewBrief, output: &mut String) {
    if brief.selection.level >= COLLAPSED {
        return;
    }
    let entries: Vec<_> = brief
        .entry_changes
        .iter()
        .filter(|&&index| {
            !brief.selection.entries.contains(&index)
                && !change_hunks(brief, index).any(|(_, hunk)| hunk.role == ReviewHunkRole::Shown)
        })
        .collect();
    if entries.is_empty() {
        return;
    }
    let mut listed: Vec<_> = entries
        .iter()
        .take(MAX_OMITTED_FILES)
        .map(|&&index| {
            let change = &report.changes[index];
            format!("{} ({})", change_label(change), change.status)
        })
        .collect();
    if entries.len() > MAX_OMITTED_FILES {
        listed.push(format!("+{} more", entries.len() - MAX_OMITTED_FILES));
    }
    let _ = writeln!(
        output,
        "omitted change entries without hunks: {}",
        listed.join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_compress_by_directory_extension_and_suffix() {
        let sites = [
            (
                Path::new("src/billing/tariffs/capital.ts"),
                ":6".to_string(),
            ),
            (
                Path::new("src/billing/tariffs/coastal.ts"),
                ":6".to_string(),
            ),
            (Path::new("src/billing/tariffs/index.ts"), ":9".to_string()),
            (Path::new("README"), String::new()),
        ];
        assert_eq!(
            compress(&sites),
            "src/billing/tariffs/{capital,coastal}.ts:6 src/billing/tariffs/index.ts:9 README"
        );
    }

    #[test]
    fn shell_words_quote_only_when_needed() {
        assert_eq!(shell("origin/main~2"), "origin/main~2");
        assert_eq!(shell("my branch"), "'my branch'");
        assert_eq!(shell("it's"), "'it'\\''s'");
        assert_eq!(shell("a\nb\t'c\\\u{1b}"), "$'a\\nb\\t\\'c\\\\\\U0000001b'");
    }
}
