//! Added methods that only dynamic dispatch reaches: new implementations of a member that base
//! files already declare, with no resolved reference in head.

use crate::metrics::testcov::is_test_file;
use crate::model::{
    CallReferenceSyntax, ReviewContextChange, ReviewContextRelation, ReviewDispatchGap,
};
use crate::scan::ReviewCapture;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Call sites kept per name; `sites_total` stays exact.
const MAX_SITES: usize = 3;
const METHOD: &str = "method";

/// Added method count and declaring head files per member name.
type Added<'a> = BTreeMap<&'a str, (usize, BTreeSet<&'a Path>)>;

fn test_path(path: &Path) -> bool {
    is_test_file(&path.to_string_lossy())
}

/// The member name of a method qualified by its container, such as `Type.name` or `Type::name`.
fn member(name: &str) -> &str {
    name.rsplit(['.', ':']).next().unwrap_or(name)
}

/// Declaration lines of methods per member name in one revision's file.
fn methods<'a>(
    capture: &'a ReviewCapture,
    index: usize,
    path: &Path,
) -> BTreeMap<&'a str, Vec<usize>> {
    let mut names = BTreeMap::<&str, Vec<usize>>::new();
    if let Some(file) = capture.revisions[index].files().get(path) {
        for definition in &file.definitions.definitions {
            if definition.symbol.kind == METHOD {
                names
                    .entry(member(&definition.symbol.name))
                    .or_default()
                    .push(definition.declaration_span.start_line);
            }
        }
    }
    names
}

/// Dispatch gaps per method name, names with call sites first, then by added count and name, and
/// the number of distinct files declaring their added methods.
pub(super) fn dispatch(
    capture: &ReviewCapture,
    changes: &[ReviewContextChange],
    relations: &[ReviewContextRelation],
) -> (Vec<ReviewDispatchGap>, usize) {
    let added = added(capture, changes, relations);
    if added.is_empty() {
        return (Vec::new(), 0);
    }
    let files = added
        .values()
        .flat_map(|(_, paths)| paths)
        .collect::<BTreeSet<_>>()
        .len();
    let mut sites = receiver_sites(capture, &added);
    let mut gaps: Vec<_> = added
        .into_iter()
        .map(|(name, (definitions, _))| {
            let found = sites.remove(name).unwrap_or_default();
            ReviewDispatchGap {
                name: name.to_string(),
                definitions,
                sites_total: found.len(),
                sites: found.into_iter().take(MAX_SITES).collect(),
            }
        })
        .collect();
    gaps.sort_by(|left, right| {
        (right.sites_total > 0)
            .cmp(&(left.sites_total > 0))
            .then(right.definitions.cmp(&left.definitions))
            .then(left.name.cmp(&right.name))
    });
    (gaps, files)
}

/// Per non-test changed file and member name that non-test base files declare as a method: head
/// methods without a resolved reference from a non-test file, beyond the file's base count.
fn added<'a>(
    capture: &'a ReviewCapture,
    changes: &'a [ReviewContextChange],
    relations: &'a [ReviewContextRelation],
) -> Added<'a> {
    let mut existing = BTreeSet::<&str>::new();
    for (path, file) in capture.revisions[0].files() {
        if !test_path(path) {
            existing.extend(
                file.definitions
                    .definitions
                    .iter()
                    .filter(|definition| definition.symbol.kind == METHOD)
                    .map(|definition| member(&definition.symbol.name)),
            );
        }
    }
    let resolved: BTreeSet<(&str, usize)> = relations
        .iter()
        .filter(|relation| relation.side == "head" && relation.kind == "symbol-reference")
        .filter_map(|relation| relation.symbol.as_ref())
        .filter(|symbol| !test_path(Path::new(&symbol.source.path)))
        .map(|symbol| {
            (
                symbol.target.path.as_str(),
                symbol.target.declaration_span.start_line,
            )
        })
        .collect();
    let mut added = Added::new();
    for change in changes {
        let Some(head) = change.head.as_ref().filter(|head| !test_path(&head.path)) else {
            continue;
        };
        let base = change
            .base
            .as_ref()
            .map(|base| methods(capture, 0, &base.path))
            .unwrap_or_default();
        let head_path = head.path.to_string_lossy();
        for (name, lines) in methods(capture, 1, &head.path) {
            if !existing.contains(name) {
                continue;
            }
            let unresolved = lines
                .iter()
                .filter(|line| !resolved.contains(&(head_path.as_ref(), **line)))
                .count();
            let count = unresolved.saturating_sub(base.get(name).map_or(0, Vec::len));
            if count > 0 {
                let entry = added.entry(name).or_default();
                entry.0 += count;
                entry.1.insert(head.path.as_path());
            }
        }
    }
    added
}

/// Head receiver call or reference sites per added member name, outside test files and the files
/// declaring that name's added methods.
fn receiver_sites<'a>(
    capture: &ReviewCapture,
    added: &Added<'a>,
) -> BTreeMap<&'a str, BTreeSet<(PathBuf, usize)>> {
    let mut sites = BTreeMap::<&str, BTreeSet<(PathBuf, usize)>>::new();
    for (path, file) in capture.revisions[1].files() {
        if test_path(path) {
            continue;
        }
        let Some(facts) = file
            .graph_facts
            .as_ref()
            .and_then(|facts| facts.call_references.as_ref())
        else {
            continue;
        };
        for relation in &facts.relations {
            if !matches!(
                relation.syntax,
                CallReferenceSyntax::Receiver | CallReferenceSyntax::Dynamic
            ) {
                continue;
            }
            let Some(name) = relation.candidate.member.as_deref() else {
                continue;
            };
            if let Some((&key, (_, declaring))) = added.get_key_value(name)
                && !declaring.contains(path.as_path())
            {
                sites
                    .entry(key)
                    .or_default()
                    .insert((path.clone(), relation.site.start_line));
            }
        }
    }
    sites
}
