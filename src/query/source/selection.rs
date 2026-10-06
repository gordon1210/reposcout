use super::{MAX_CANDIDATES, SourceQueryTarget, SourceSelector};
use crate::model::{
    DefinitionFact, DefinitionStatus, LineRange, SourceQueryDefinition, SourceQueryFile,
    SourceQueryResult, SourceQueryStatus, SourceSpan,
};
use crate::scan::{ExplicitSourceBatch, ExplicitSourceFailure};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) struct ResolvedTarget<'a> {
    pub(crate) file: Option<SourceQueryFile>,
    pub(crate) result: SourceQueryResult,
    pub(crate) source: Option<(&'a str, SourceSpan)>,
}

pub(crate) fn resolve<'a>(
    index: usize,
    target: &SourceQueryTarget,
    path: Option<&Path>,
    batch: &'a ExplicitSourceBatch,
    files: &BTreeMap<PathBuf, SourceQueryFile>,
    outline_remaining: &mut usize,
) -> ResolvedTarget<'a> {
    let file = path.and_then(|path| files.get(path)).cloned();
    let mut resolved = ResolvedTarget {
        result: empty_result(index, file.as_ref().map(|file| file.id)),
        file,
        source: None,
    };
    resolved.result.requested_range = requested_range(&target.selector);
    if resolved.result.requested_range.is_some() {
        resolved.result.selection = Some("range".to_string());
    }
    let Some(path) = path else {
        resolved.result.status = SourceQueryStatus::InvalidPath;
        return resolved;
    };
    if let Some(failure) = batch.failures.get(path) {
        resolved.result.status = failure_status(*failure);
        return resolved;
    }
    let Some(loaded) = batch.files.get(path) else {
        return resolved;
    };
    if target.expected_hash.as_deref().is_some_and(|expected| {
        !resolved
            .file
            .as_ref()
            .and_then(|file| file.sha256.as_deref())
            .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
    }) {
        resolved.result.status = SourceQueryStatus::Stale;
        return resolved;
    }
    if matches!(target.selector, SourceSelector::File) {
        resolved.result.status = SourceQueryStatus::Complete;
        resolved.result.selection = Some("file".to_string());
        resolved.source = Some((
            &loaded.content,
            file_span(
                &loaded.content,
                loaded.report.as_ref().map(|report| report.loc),
            ),
        ));
        return resolved;
    }
    if let SourceSelector::Range { start, end } = target.selector {
        if let Some(span) = line_range_span(&loaded.content, start, end) {
            resolved.result.status = SourceQueryStatus::Complete;
            resolved.source = Some((&loaded.content, span));
        } else {
            resolved.result.status = SourceQueryStatus::NotFound;
        }
        return resolved;
    }
    if matches!(
        loaded.definitions.status,
        DefinitionStatus::Unsupported | DefinitionStatus::Unavailable
    ) {
        resolved.result.status = if loaded.definitions.status == DefinitionStatus::Unsupported {
            SourceQueryStatus::Unsupported
        } else {
            SourceQueryStatus::Unavailable
        };
        return resolved;
    }
    let definitions = &loaded.definitions.definitions;
    if matches!(target.selector, SourceSelector::Outline) {
        resolved.result.status = SourceQueryStatus::Outline;
        resolved.result.selection = Some("file-outline".to_string());
        resolved.result.total_candidates = definitions.len();
        resolved.result.candidates = definitions
            .iter()
            .take(*outline_remaining)
            .map(|definition| describe(definition, true))
            .collect();
        *outline_remaining = outline_remaining.saturating_sub(resolved.result.candidates.len());
        resolved.result.omitted_candidates = definitions.len() - resolved.result.candidates.len();
        return resolved;
    }
    resolve_definition(
        &mut resolved,
        &loaded.content,
        definitions,
        loaded.definitions.status,
        &target.selector,
    );
    resolved
}

fn resolve_definition<'a>(
    resolved: &mut ResolvedTarget<'a>,
    content: &'a str,
    definitions: &[DefinitionFact],
    extraction: DefinitionStatus,
    selector: &SourceSelector,
) {
    let (matches, reason) = select(definitions, selector);
    resolved.result.selection = Some(reason.to_string());
    resolved.result.total_candidates = matches.len();
    match matches.as_slice() {
        [] => {
            resolved.result.status = if extraction == DefinitionStatus::ParseErrors {
                SourceQueryStatus::ParseError
            } else {
                SourceQueryStatus::NotFound
            }
        }
        [definition] => {
            resolved.result.definition = Some(describe(definition, false));
            if let Some(span) = definition.source_span {
                if content.get(span.start_byte..span.end_byte).is_some() {
                    resolved.result.status = SourceQueryStatus::Complete;
                    resolved.source = Some((content, span));
                } else {
                    resolved.result.status = SourceQueryStatus::Unavailable;
                }
            } else {
                resolved.result.status = SourceQueryStatus::ParseError;
            }
        }
        _ => {
            resolved.result.status = SourceQueryStatus::Ambiguous;
            resolved.result.candidates = matches
                .iter()
                .take(MAX_CANDIDATES)
                .map(|definition| describe(definition, false))
                .collect();
            resolved.result.omitted_candidates = matches.len() - resolved.result.candidates.len();
        }
    }
}

fn file_span(content: &str, line_count: Option<usize>) -> SourceSpan {
    SourceSpan {
        start_byte: 0,
        end_byte: content.len(),
        start_line: 1,
        end_line: line_count.unwrap_or_else(|| content.lines().count()).max(1),
    }
}

pub(crate) fn requested_range(selector: &SourceSelector) -> Option<LineRange> {
    match selector {
        SourceSelector::Range { start, end } => Some(LineRange {
            start: *start,
            end: *end,
        }),
        _ => None,
    }
}

fn line_range_span(content: &str, start: usize, end: usize) -> Option<SourceSpan> {
    if content.is_empty() {
        return None;
    }
    let mut offset = 0;
    let mut start_byte = None;
    for (index, line) in content.split_inclusive('\n').enumerate() {
        let line_number = index + 1;
        if line_number == start {
            start_byte = Some(offset);
        }
        offset += line.len();
        if line_number == end {
            return Some(SourceSpan {
                start_byte: start_byte?,
                end_byte: offset,
                start_line: start,
                end_line: end,
            });
        }
    }
    None
}

pub(crate) fn empty_result(target: usize, file: Option<usize>) -> SourceQueryResult {
    SourceQueryResult {
        target,
        file,
        status: SourceQueryStatus::Unavailable,
        change: None,
        selection: None,
        requested_range: None,
        definition: None,
        source: None,
        candidates: Vec::new(),
        total_candidates: 0,
        omitted_candidates: 0,
    }
}

fn select<'a>(
    definitions: &'a [DefinitionFact],
    selector: &SourceSelector,
) -> (Vec<&'a DefinitionFact>, &'static str) {
    match selector {
        SourceSelector::Symbol(name) => {
            let exact = definitions
                .iter()
                .filter(|definition| definition.symbol.name == *name)
                .collect::<Vec<_>>();
            if !exact.is_empty() {
                return (exact, "exact-qualified-name");
            }
            (
                definitions
                    .iter()
                    .filter(|definition| {
                        definition
                            .symbol
                            .name
                            .rsplit(['.', ':', '\\'])
                            .find(|part| !part.is_empty())
                            == Some(name.as_str())
                    })
                    .collect(),
                "exact-simple-name",
            )
        }
        SourceSelector::Line(line) => {
            let matches = definitions
                .iter()
                .filter(|definition| {
                    definition.declaration_span.start_line <= *line
                        && definition.declaration_span.end_line >= *line
                })
                .collect::<Vec<_>>();
            (innermost(matches), "innermost-line")
        }
        SourceSelector::Outline => (Vec::new(), "file-outline"),
        SourceSelector::File => (Vec::new(), "file"),
        SourceSelector::Range { .. } => (Vec::new(), "range"),
    }
}

fn innermost(matches: Vec<&DefinitionFact>) -> Vec<&DefinitionFact> {
    let mut ordered = matches.into_iter().enumerate().collect::<Vec<_>>();
    ordered.sort_by_key(|(_, definition)| {
        let span = definition.declaration_span;
        (std::cmp::Reverse(span.start_byte), span.end_byte)
    });
    let mut retained = Vec::new();
    let mut min_end: Option<usize> = None;
    for group in ordered.chunk_by(|(_, left), (_, right)| {
        let left = left.declaration_span;
        let right = right.declaration_span;
        left.start_byte == right.start_byte && left.end_byte == right.end_byte
    }) {
        let end = group[0].1.declaration_span.end_byte;
        if min_end.is_none_or(|previous| previous > end) {
            retained.extend_from_slice(group);
        }
        min_end = Some(min_end.map_or(end, |previous| previous.min(end)));
    }
    retained.sort_by_key(|(index, _)| *index);
    retained
        .into_iter()
        .map(|(_, definition)| definition)
        .collect()
}

pub(crate) fn describe(definition: &DefinitionFact, outline: bool) -> SourceQueryDefinition {
    SourceQueryDefinition {
        name: definition.symbol.name.clone(),
        kind: definition.symbol.kind.clone(),
        declaration_span: definition.declaration_span,
        source_span: definition.source_span,
        signature: outline.then(|| definition.symbol.signature.clone()),
    }
}

pub(crate) fn failure_status(failure: ExplicitSourceFailure) -> SourceQueryStatus {
    match failure {
        ExplicitSourceFailure::InvalidPath => SourceQueryStatus::InvalidPath,
        ExplicitSourceFailure::Excluded => SourceQueryStatus::Excluded,
        ExplicitSourceFailure::IgnoreError => SourceQueryStatus::IgnoreError,
        ExplicitSourceFailure::Unsupported => SourceQueryStatus::Unsupported,
        ExplicitSourceFailure::Unreadable => SourceQueryStatus::Unreadable,
        ExplicitSourceFailure::NotRegularFile => SourceQueryStatus::NotRegularFile,
        ExplicitSourceFailure::Oversized => SourceQueryStatus::Oversized,
        ExplicitSourceFailure::BudgetExceeded => SourceQueryStatus::InputBudgetExceeded,
        ExplicitSourceFailure::DeadlineExceeded => SourceQueryStatus::DeadlineExceeded,
        ExplicitSourceFailure::Missing => SourceQueryStatus::NotFound,
        ExplicitSourceFailure::Binary => SourceQueryStatus::Binary,
        ExplicitSourceFailure::Conflict => SourceQueryStatus::Conflict,
    }
}
