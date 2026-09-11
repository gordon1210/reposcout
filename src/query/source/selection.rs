use super::{MAX_CANDIDATES, SourceQueryTarget, SourceSelector};
use crate::model::{
    DefinitionFact, DefinitionStatus, SourceQueryDefinition, SourceQueryFile, SourceQueryResult,
    SourceQueryStatus, SourceSpan,
};
use crate::scan::{ExplicitSourceBatch, ExplicitSourceFailure};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) struct ResolvedTarget<'a> {
    pub(crate) file: Option<SourceQueryFile>,
    pub(crate) result: SourceQueryResult,
    pub(crate) source: Option<(&'a str, SourceSpan)>,
}

pub(super) fn resolve<'a>(
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
    let (matches, reason) = select(definitions, &target.selector);
    resolved.result.selection = Some(reason.to_string());
    resolved.result.total_candidates = matches.len();
    match matches.as_slice() {
        [] => {
            resolved.result.status = if loaded.definitions.status == DefinitionStatus::ParseErrors {
                SourceQueryStatus::ParseError
            } else {
                SourceQueryStatus::NotFound
            }
        }
        [definition] => {
            resolved.result.definition = Some(describe(definition, false));
            if let Some(span) = definition.source_span {
                if loaded.content.get(span.start_byte..span.end_byte).is_some() {
                    resolved.result.status = SourceQueryStatus::Complete;
                    resolved.source = Some((&loaded.content, span));
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
    resolved
}

pub(crate) fn empty_result(target: usize, file: Option<usize>) -> SourceQueryResult {
    SourceQueryResult {
        target,
        file,
        status: SourceQueryStatus::Unavailable,
        change: None,
        selection: None,
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
