use crate::model::{
    ReviewAnalysisStatus, ReviewBrief, ReviewChangeBasis, ReviewChangedSide, ReviewContextChange,
    ReviewContextRelation, ReviewContextReport, ReviewRevisionCoverage, ReviewUsageLine,
};
use crate::report::source::{body_block, source_text};
use crate::report::terminal_text;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

const SHORT_COMMIT: usize = 12;

/// Compact agent projection; Markdown wraps the same text in one fenced block.
pub(crate) fn render(report: &ReviewContextReport, markdown: bool) -> String {
    let text = plain(report);
    if markdown {
        body_block(text.trim_end_matches('\n'), true)
            .trim_start()
            .to_string()
    } else {
        text
    }
}

fn plain(report: &ReviewContextReport) -> String {
    let default = ReviewBrief::default();
    let brief = report.brief.as_ref().unwrap_or(&default);
    let mut output = header(report, brief);
    let mut used = vec![false; report.relations.len()];
    if report.changes.is_empty() {
        output.push_str("no changes shown\n");
    }
    for change in &report.changes {
        render_change(report, brief, change, &mut used, &mut output);
    }
    let leftovers: Vec<_> = report
        .relations
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|(relation, _)| relation)
        .collect();
    if !leftovers.is_empty() {
        output.push_str("other relations:\n");
        for relation in leftovers {
            let _ = writeln!(
                output,
                "  {} {} {} -> {}",
                relation.side,
                relation.kind,
                terminal_text(&relation.edge.source),
                terminal_text(&relation.edge.target)
            );
        }
    }
    if !brief.mentions.is_empty() {
        output.push_str("docs naming changed identifiers:\n");
        for mention in &brief.mentions {
            let _ = writeln!(
                output,
                "{}:{} {}",
                path_text(&mention.path),
                mention.line,
                terminal_text(&mention.text)
            );
        }
    }
    output
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

fn header(report: &ReviewContextReport, brief: &ReviewBrief) -> String {
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
        "note: static potential impact; [test] marks filename/syntax hints; read files with git show <commit>:<path>\n",
    );
    let gaps = gaps(report);
    if !gaps.is_empty() {
        let _ = writeln!(output, "gaps: {}", gaps.join("; "));
    }
    let omitted = omitted(report, brief);
    if !omitted.is_empty() {
        let _ = writeln!(
            output,
            "omitted from output: {}; full diff: git diff {} {} -- <path>",
            omitted.join(", "),
            short(&c.base_commit),
            short(&c.head_commit)
        );
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
    if !counted.is_empty() {
        let _ = writeln!(output, "counted, not listed: {}", counted.join("; "));
    }
    output
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

fn gaps(report: &ReviewContextReport) -> Vec<String> {
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
    let metrics: [(&str, Metric); 13] = [
        ("inventory truncated", |side| {
            usize::from(side.inventory_truncated)
        }),
        ("unavailable inputs", |side| {
            side.unavailable_files.values().sum()
        }),
        ("unrecognized files", |side| {
            side.unsupported_inventory_files
        }),
        ("changed files without graph", |side| {
            side.changed_files_without_graph
        }),
        ("unsupported graph paths", |side| {
            side.unsupported_graph_paths
        }),
        ("unresolved imports", |side| side.unresolved_imports),
        ("unresolved type relations", |side| {
            side.unresolved_type_relations
        }),
        ("parse errors", |side| side.parse_errors),
        ("resolver config errors", |side| side.config_errors),
        ("files without call-reference support", |side| {
            side.unsupported_call_files
        }),
        ("incomplete call-reference files", |side| {
            side.incomplete_call_files
        }),
        ("unresolved references", |side| {
            side.call_resolution.unresolved
        }),
        ("omitted references", |side| side.call_resolution.omitted),
    ];
    gaps.extend(
        metrics
            .into_iter()
            .filter_map(|(label, metric)| pair(label, metric(base), metric(head))),
    );
    gaps
}

fn omitted(report: &ReviewContextReport, brief: &ReviewBrief) -> Vec<String> {
    let t = &report.totals;
    let diffs = report
        .changes
        .iter()
        .filter(|change| change.diff.is_none() && change.diff_tokens.is_some())
        .count();
    let hunks: usize = report
        .changes
        .iter()
        .map(|change| change.diff_hunks_omitted)
        .sum();
    [
        (t.changes_omitted, "changes"),
        (t.definitions_omitted, "definitions"),
        (diffs, "diffs"),
        (hunks, "diff hunks"),
        (
            brief
                .direct_relations
                .saturating_sub(report.relations.len()),
            "direct relations",
        ),
        (
            brief.mentions_total.saturating_sub(brief.mentions.len()),
            "doc lines",
        ),
        (
            brief.usage_total.saturating_sub(
                report
                    .changes
                    .iter()
                    .map(|change| usage_lines(brief, change).len())
                    .sum(),
            ),
            "usage lines",
        ),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, name)| format!("{count} {name}"))
    .collect()
}

fn render_change(
    report: &ReviewContextReport,
    brief: &ReviewBrief,
    change: &ReviewContextChange,
    used: &mut [bool],
    output: &mut String,
) {
    let paths = match (&change.base, &change.head) {
        (Some(base), Some(head)) if base.path != head.path => {
            format!("{} -> {}", path_text(&base.path), path_text(&head.path))
        }
        (_, Some(side)) | (Some(side), None) => path_text(&side.path),
        (None, None) => "unknown path".into(),
    };
    let hunks = match (change.hunk_status, change.hunks) {
        (ReviewAnalysisStatus::Unavailable, _) | (_, None) => "hunks unknown".to_string(),
        (ReviewAnalysisStatus::Partial, Some(count)) => format!("{count} hunks, partial"),
        (_, Some(1)) => "1 hunk".into(),
        (_, Some(count)) => format!("{count} hunks"),
    };
    let _ = writeln!(output, "{} {paths} ({hunks})", change.status);
    let analysis = analysis(change);
    if !analysis.is_empty() {
        let _ = writeln!(output, "  analysis: {}", analysis.join(", "));
    }
    definitions(change, output);
    match (&change.diff, change.diff_tokens) {
        (Some(diff), _) => {
            let start = if diff.starts_with("@@") {
                0
            } else {
                diff.find("\n@@").map_or(diff.len(), |index| index + 1)
            };
            let hunks = &diff[start..];
            if !hunks.is_empty() {
                output.push_str(&source_text(hunks));
                if !hunks.ends_with('\n') {
                    output.push('\n');
                }
            }
            if change.diff_hunks_omitted > 0 {
                let _ = writeln!(
                    output,
                    "  … {} more hunks omitted",
                    change.diff_hunks_omitted
                );
            }
        }
        (None, Some(tokens)) => {
            let _ = writeln!(output, "  diff omitted ({tokens} tokens)");
        }
        (None, None) => output.push_str("  diff unavailable\n"),
    }
    relations(report, change, used, output);
    let usage = usage_lines(brief, change);
    if !usage.is_empty() {
        output.push_str("  usage lines (identifier text match, no resolved reference):\n");
        for line in usage {
            let _ = writeln!(
                output,
                "    {}:{}{}  {}",
                path_text(&line.path),
                line.line,
                if line.side == "base" { " (base)" } else { "" },
                terminal_text(&line.text)
            );
        }
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

/// Retained usage lines of one change in path and line order.
fn usage_lines<'a>(
    brief: &'a ReviewBrief,
    change: &ReviewContextChange,
) -> Vec<&'a ReviewUsageLine> {
    let identity = [&change.base, &change.head].map(|side| side.as_ref().map(|side| &side.path));
    let mut lines: Vec<_> = brief
        .usage
        .iter()
        .filter(|line| line.change.each_ref().map(Option::as_ref) == identity)
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
        if side.mapping_status != ReviewAnalysisStatus::Available {
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

fn side_path<'a>(change: &'a ReviewContextChange, side: &str) -> Option<&'a Path> {
    let changed: &Option<ReviewChangedSide> = if side == "base" {
        &change.base
    } else {
        &change.head
    };
    changed.as_ref().map(|changed| changed.path.as_path())
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

fn relations(
    report: &ReviewContextReport,
    change: &ReviewContextChange,
    used: &mut [bool],
    output: &mut String,
) {
    let mut importers = BTreeMap::<&str, BTreeSet<&str>>::new();
    let mut imports = BTreeMap::<&str, BTreeSet<&str>>::new();
    let mut references = Vec::<(String, BTreeSet<&str>, String)>::new();
    let mut types = Vec::<(String, BTreeSet<&str>)>::new();
    for (relation, used) in report.relations.iter().zip(used.iter_mut()) {
        let Some(path) = side_path(change, &relation.side) else {
            continue;
        };
        let target = Path::new(&relation.edge.target) == path;
        if *used || !(target || Path::new(&relation.edge.source) == path) {
            continue;
        }
        *used = true;
        let side = relation.side.as_str();
        match (
            relation.kind.as_str(),
            &relation.symbol,
            &relation.type_relation,
        ) {
            (_, Some(_), _) => {
                let (key, line) = reference(report, change, relation);
                match references.iter_mut().find(|entry| entry.0 == key) {
                    Some(entry) => {
                        entry.1.insert(side);
                        if side == "head" {
                            entry.2 = line;
                        }
                    }
                    None => references.push((key, BTreeSet::from([side]), line)),
                }
            }
            (_, None, Some(edge)) => {
                let key = format!(
                    "{} {} {}",
                    terminal_text(&edge.source),
                    terminal_text(&edge.relation),
                    terminal_text(&edge.target)
                );
                match types.iter_mut().find(|entry| entry.0 == key) {
                    Some(entry) => {
                        entry.1.insert(side);
                    }
                    None => types.push((key, BTreeSet::from([side]))),
                }
            }
            _ if target => {
                importers
                    .entry(relation.edge.source.as_str())
                    .or_default()
                    .insert(side);
            }
            _ => {
                imports
                    .entry(relation.edge.target.as_str())
                    .or_default()
                    .insert(side);
            }
        }
    }
    for (name, files) in [("imported by", importers), ("imports", imports)] {
        if files.is_empty() {
            continue;
        }
        let listed: Vec<_> = files
            .iter()
            .map(|(path, sides)| {
                format!(
                    "{}{}{}",
                    terminal_text(path),
                    if is_test(report, path) { " [test]" } else { "" },
                    sides_marker(sides)
                )
            })
            .collect();
        let _ = writeln!(output, "  {name}: {}", listed.join(", "));
    }
    for (_, sides, line) in references {
        let _ = writeln!(output, "  {line}{}", sides_marker(&sides));
    }
    for (key, sides) in types {
        let _ = writeln!(output, "  type {key}{}", sides_marker(&sides));
    }
}

/// Identity without site lines (so shifted base/head sites compare equal) and the rendered line.
fn reference(
    report: &ReviewContextReport,
    change: &ReviewContextChange,
    relation: &ReviewContextRelation,
) -> (String, String) {
    let Some(symbol) = &relation.symbol else {
        return (String::new(), String::new());
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
    let line = format!(
        "{} {}:{} {} -> {target}{}{}",
        label(&symbol.kind),
        terminal_text(&symbol.source.path),
        symbol.site.start_line,
        terminal_text(&symbol.source.name),
        if is_test(report, &symbol.source.path) {
            " [test]"
        } else {
            ""
        },
        if relation.change_basis == Some(ReviewChangeBasis::ChangedFile) {
            " (file-level)"
        } else {
            ""
        }
    );
    (key, line)
}
