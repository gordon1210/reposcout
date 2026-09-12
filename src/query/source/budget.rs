use super::SourceQueryOptions;
use super::selection::ResolvedTarget;
use crate::metrics::tokens::TokenCounter;
use crate::model::{
    SourceQueryChunk, SourceQueryFile, SourceQueryReport, SourceQueryResult, SourceQueryStatus,
    SourceSpan,
};
use anyhow::{Result, anyhow};

pub(crate) fn fits(
    report: &SourceQueryReport,
    options: &SourceQueryOptions,
    counter: &TokenCounter,
) -> Result<bool> {
    let rendered = crate::report::source::render(report, options.format, options.pretty_json)?;
    Ok(rendered.len() <= options.byte_budget && counter.count(&rendered) <= options.token_budget)
}

pub(crate) fn admit(
    report: &mut SourceQueryReport,
    resolved: ResolvedTarget<'_>,
    options: &SourceQueryOptions,
    counter: &TokenCounter,
) -> Result<()> {
    admit_with_fit(report, resolved, options, |candidate| {
        fits(candidate, options, counter)
    })
}

pub(crate) fn admit_with_fit(
    report: &mut SourceQueryReport,
    resolved: ResolvedTarget<'_>,
    options: &SourceQueryOptions,
    fits: impl Fn(&SourceQueryReport) -> Result<bool>,
) -> Result<()> {
    let mut result = resolved.result;
    if let Some((content, span)) = resolved.source {
        if span.end_byte.saturating_sub(span.start_byte) <= options.byte_budget {
            let mut candidate = appended(report, resolved.file.as_ref(), result.clone());
            let file = result
                .file
                .ok_or_else(|| anyhow!("resolved source has no file identity"))?;
            let source_id = merge_source(&mut candidate, file, content, span)?;
            if let Some(last) = candidate.results.last_mut() {
                last.source = Some(source_id);
            }
            if fit_candidate_with(&mut candidate, &fits)? {
                *report = candidate;
                return Ok(());
            }
        }
        result.status = SourceQueryStatus::BudgetOmitted;
        result.source = None;
    }
    loop {
        let mut candidate = appended(report, resolved.file.as_ref(), result.clone());
        if fit_candidate_with(&mut candidate, &fits)? {
            *report = candidate;
            return Ok(());
        }
        if result.candidates.pop().is_none() {
            break;
        }
        result.omitted_candidates = result
            .total_candidates
            .saturating_sub(result.candidates.len());
    }
    result.status = SourceQueryStatus::BudgetOmitted;
    result.file = None;
    result.selection = None;
    result.definition = None;
    result.source = None;
    result.change = None;
    let mut candidate = appended(report, None, result);
    if fit_candidate_with(&mut candidate, &fits)? {
        *report = candidate;
    }
    Ok(())
}

fn appended(
    report: &SourceQueryReport,
    file: Option<&SourceQueryFile>,
    result: SourceQueryResult,
) -> SourceQueryReport {
    let mut candidate = report.clone();
    if let Some(file) = file
        && !candidate.files.iter().any(|present| present.id == file.id)
    {
        candidate.files.push(file.clone());
        candidate.files.sort_by_key(|file| file.id);
    }
    candidate.results.push(result);
    candidate.omitted_targets = candidate
        .requested_targets
        .saturating_sub(candidate.results.len());
    candidate
}

fn fit_candidate_with(
    candidate: &mut SourceQueryReport,
    fits: &impl Fn(&SourceQueryReport) -> Result<bool>,
) -> Result<bool> {
    if fits(candidate)? {
        return Ok(true);
    }
    if candidate.root.take().is_some() {
        candidate.root_omitted = true;
        return fits(candidate);
    }
    Ok(false)
}

fn merge_source(
    report: &mut SourceQueryReport,
    file: usize,
    content: &str,
    span: SourceSpan,
) -> Result<usize> {
    let overlaps = report
        .sources
        .iter()
        .filter(|source| {
            source.file == file
                && source.span.start_byte < span.end_byte
                && source.span.end_byte > span.start_byte
        })
        .map(|source| source.id)
        .collect::<Vec<_>>();
    let id = overlaps.iter().copied().min().unwrap_or_else(|| {
        report
            .sources
            .iter()
            .map(|source| source.id)
            .max()
            .unwrap_or(0)
            + 1
    });
    let mut combined = span;
    for source in report
        .sources
        .iter()
        .filter(|source| overlaps.contains(&source.id))
    {
        combined.start_byte = combined.start_byte.min(source.span.start_byte);
        combined.end_byte = combined.end_byte.max(source.span.end_byte);
        combined.start_line = combined.start_line.min(source.span.start_line);
        combined.end_line = combined.end_line.max(source.span.end_line);
    }
    let selected = content
        .get(combined.start_byte..combined.end_byte)
        .ok_or_else(|| anyhow!("source span does not match the loaded content"))?;
    for result in &mut report.results {
        if result
            .source
            .is_some_and(|source| overlaps.contains(&source))
        {
            result.source = Some(id);
        }
    }
    report
        .sources
        .retain(|source| !overlaps.contains(&source.id));
    report.sources.push(SourceQueryChunk {
        id,
        file,
        span: combined,
        content: selected.to_string(),
    });
    report.sources.sort_by_key(|source| source.id);
    Ok(id)
}
