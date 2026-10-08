import { digest } from '../core/identity.mjs';
import { integer } from '../core/validation.mjs';
import { requireValue } from '../core/errors.mjs';
import { sourceLines, parseStructure } from './parse.mjs';
import { compareBodies, containingSection, containingBlock } from './correspondence.mjs';

// Comment APIs allow the empty final physical line, including in an empty body.
export function anchorLines(body) {
  const lines = sourceLines(body);
  if (!body.length || body.endsWith('\n')) lines.push({ start: body.length, end: body.length,
    raw: '', text: '', number: lines.length + 1 });
  return lines;
}

export function captureRange(body, startLine, endLine = startLine) {
  const lines = anchorLines(body);
  integer(startLine, 'anchor start line', 1, lines.length);
  integer(endLine, 'anchor end line', startLine, Math.min(lines.length, startLine + 199));
  const first = lines[startLine - 1];
  const last = lines[endLine - 1];
  const structure = parseStructure(body);
  const section = containingSection(structure, first.start, last.end);
  const block = containingBlock(structure, first.start);
  const quote = body.slice(first.start, last.end);
  requireValue(quote.length <= 20000, 'anchor_too_large', 'anchor quote exceeds 20000 characters');
  return { startLine, endLine, start: first.start, end: last.end, quote,
    quoteChecksum: digest(quote), sourceChecksum: digest(body),
    before: lines.slice(Math.max(0, startLine - 3), startLine - 1).map(line => line.raw).join(''),
    after: lines.slice(endLine, endLine + 2).map(line => line.raw).join(''),
    sectionId: section?.id ?? null, sectionTitle: section?.title ?? null,
    blockType: block?.type ?? 'empty', lineCount: endLine - startLine + 1 };
}

function rangeCandidate(body, lines, startIndex, count) {
  if (startIndex + count > lines.length) return null;
  const first = lines[startIndex];
  const last = lines[startIndex + count - 1];
  return { startLine: startIndex + 1, endLine: startIndex + count,
    start: first.start, end: last.end, quote: body.slice(first.start, last.end) };
}

function matchingCandidates(body, anchor, targetStructure) {
  const lines = anchorLines(body);
  const starts = new Map(lines.map((line, index) => [line.start, index]));
  const candidates = [];
  let cursor = 0;
  while (cursor <= body.length) {
    const found = body.indexOf(anchor.quote, cursor);
    if (found < 0) break;
    cursor = found + 1;
    const index = starts.get(found);
    if (index === undefined) continue;
    const candidate = rangeCandidate(body, lines, index, anchor.lineCount);
    if (!candidate || candidate.quote !== anchor.quote) continue;
    const block = containingBlock(targetStructure, found);
    const blockType = block?.type ?? 'empty';
    if (blockType !== anchor.blockType) continue;
    const before = lines.slice(Math.max(0, index - 2), index).map(line => line.raw).join('');
    const after = lines.slice(index + anchor.lineCount, index + anchor.lineCount + 2).map(line => line.raw).join('');
    candidates.push({ ...candidate, contextMatches: before === anchor.before && after === anchor.after });
  }
  return candidates;
}

function changedRegion(anchor, comparison, targetBody) {
  if (!comparison.diff.exact) return null;
  const sourceStart = anchor.startLine - 1;
  const sourceEnd = anchor.endLine;
  let delta = 0;
  for (const hunk of comparison.diff.hunks) {
    const intersects = hunk.start < sourceEnd && hunk.end > sourceStart;
    if (intersects) {
      const contains = hunk.start <= sourceStart && hunk.end >= sourceEnd;
      if (!contains) return { status: 'ambiguous', reason: 'anchor_crosses_change_boundary', candidate: null };
      if (!hunk.lines.length) return { status: 'deleted', reason: 'exact_diff_deletion', candidate: null };
      const lines = anchorLines(targetBody);
      return { status: 'edited', reason: 'exact_diff_replacement',
        candidate: rangeCandidate(targetBody, lines, hunk.start + delta, hunk.lines.length) };
    }
    delta += hunk.lines.length - (hunk.end - hunk.start);
  }
  return null;
}

export function mapAnchor(anchor, sourceBody, targetBody, maximumCells = 1000000) {
  requireValue(digest(sourceBody) === anchor.sourceChecksum, 'anchor_source_conflict', 'anchor source checksum is invalid');
  requireValue(sourceBody.slice(anchor.start, anchor.end) === anchor.quote && digest(anchor.quote) === anchor.quoteChecksum,
    'anchor_source_conflict', 'anchor quote is inconsistent with source');
  const targetChecksum = digest(targetBody);
  if (targetChecksum === anchor.sourceChecksum) return {
    status: 'unchanged', exact: true, reason: 'identical_revision_body', targetChecksum,
    target: { startLine: anchor.startLine, endLine: anchor.endLine, start: anchor.start, end: anchor.end, quote: anchor.quote },
    candidates: [],
  };
  if (!anchor.quote.length) return { status: 'ambiguous', exact: false, reason: 'empty_anchor_has_no_identity',
    targetChecksum, target: null, candidates: [] };
  const targetStructure = parseStructure(targetBody);
  const candidates = matchingCandidates(targetBody, anchor, targetStructure);
  const contextual = candidates.filter(candidate => candidate.contextMatches);
  const sourceCandidates = matchingCandidates(sourceBody, anchor, parseStructure(sourceBody));
  const sourceContext = sourceCandidates.filter(candidate => candidate.contextMatches);
  const uniqueSource = sourceCandidates.length === 1;
  const uniquelyLocatedSource = sourceContext.length === 1 && sourceContext[0].start === anchor.start;
  const selected = uniqueSource && candidates.length === 1 ? candidates[0]
    : uniquelyLocatedSource && contextual.length === 1 ? contextual[0] : null;
  if (selected) return { status: selected.start === anchor.start && selected.startLine === anchor.startLine ? 'unchanged' : 'relocated', exact: true,
    reason: candidates.length === 1 ? 'unique_quote_and_block' : 'unique_quote_context', targetChecksum,
    target: selected, candidates: [] };
  if (candidates.length) return { status: 'ambiguous', exact: false, reason: 'repeated_quote', targetChecksum,
    target: null, candidates: candidates.slice(0, 20), candidateCount: candidates.length };
  const comparison = compareBodies(sourceBody, targetBody, maximumCells);
  const changed = changedRegion(anchor, comparison, targetBody);
  if (changed) return { status: changed.status, exact: false, reason: changed.reason, targetChecksum,
    target: null, candidates: changed.candidate ? [changed.candidate] : [] };
  const section = comparison.sections.find(item => item.sourceId === anchor.sectionId);
  const reason = !comparison.diff.exact ? 'diff_budget_exhausted'
    : section?.status === 'deleted' ? 'section_and_quote_absent' : 'no_proven_line_correspondence';
  return { status: reason === 'section_and_quote_absent' ? 'deleted' : 'ambiguous', exact: false,
    reason, targetChecksum, target: null, candidates: [] };
}
