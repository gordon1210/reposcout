import { allocate, digest } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { requireValue } from '../core/errors.mjs';
import { integer, text, object } from '../core/validation.mjs';
import { document, revision, entity } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { addComment } from '../comments/service.mjs';
import { compareBodies } from './correspondence.mjs';
import { captureRange, mapAnchor } from './anchors.mjs';
import { planSectionOperations } from './operations.mjs';
import { materializeMerge } from './merge.mjs';

function visibleDocument(state, documentId, actorId) {
  const item = document(state, documentId);
  authorize(state, actorId, 'read', item.collectionId);
  return item;
}

function boundedIds(ids) {
  requireValue(Array.isArray(ids) && ids.length > 0 && ids.length <= 100, 'invalid_input', 'anchor batch must contain one to 100 identities');
  ids.forEach(id => text(id, 'anchor identity', { max: 100 }));
  requireValue(new Set(ids).size === ids.length, 'duplicate_anchor', 'anchor identities must be unique');
  return ids;
}

function sourceForAnchor(state, item, anchor) {
  requireValue(anchor.documentId === item.id, 'anchor_document_mismatch', 'anchor belongs to another document');
  const source = revision(state, anchor.revisionId, item.id);
  requireValue(source.checksum === anchor.range.sourceChecksum, 'anchor_source_conflict', 'anchor source revision checksum changed');
  return source;
}

export function captureDiscussionAnchor(state, input, actorId) {
  const item = visibleDocument(state, input.documentId, actorId);
  const comment = input.commentId === undefined ? null : entity(state, 'comments', input.commentId);
  requireValue(!comment || comment.documentId === item.id, 'anchor_document_mismatch', 'comment belongs to another document');
  const reviewId = comment ? comment.reviewId : input.reviewId ?? null;
  const review = reviewId === null ? null : entity(state, 'reviews', reviewId);
  requireValue(!review || review.documentId === item.id, 'review_mismatch', 'review belongs to another document');
  const revisionId = comment?.revisionId ?? review?.revisionId ?? input.revisionId ?? item.currentRevisionId;
  requireValue(input.revisionId === undefined || input.revisionId === revisionId, 'revision_mismatch', 'explicit source revision disagrees with discussion');
  requireValue(input.reviewId === undefined || input.reviewId === reviewId, 'review_mismatch', 'explicit review disagrees with comment');
  const source = revision(state, revisionId, item.id);
  requireValue(!review || review.revisionId === source.id, 'review_mismatch', 'review and anchor source revisions disagree');
  requireValue(input.expectedChecksum === source.checksum, 'checksum_conflict', 'source revision checksum does not match');
  const startLine = comment?.line ?? input.startLine ?? 1;
  requireValue(!comment || input.startLine === undefined || input.startLine === comment.line,
    'anchor_line_mismatch', 'comment anchor starts at the original comment line');
  const range = captureRange(source.body, startLine, input.endLine ?? startLine);
  if (input.expectedQuote !== undefined) requireValue(input.expectedQuote === range.quote, 'anchor_quote_conflict', 'quoted source does not match');
  const id = allocate(state, 'structureAnchor');
  const anchor = { id, documentId: item.id, revisionId, commentId: comment?.id ?? null, reviewId,
    actorId, range, commentSnapshot: comment ? { body: comment.body, actorId: comment.actorId,
      resolved: comment.resolved, line: comment.line } : null };
  state.structureAnchors[id] = anchor;
  record(state, actorId, 'structure.anchor_captured', item.id, { anchorId: id, revisionId, commentId: anchor.commentId });
  return anchor;
}

export function getDiscussionAnchor(state, input, actorId) {
  const anchor = entity(state, 'structureAnchors', input.id);
  visibleDocument(state, anchor.documentId, actorId);
  return anchor;
}

export function compareRevisionStructure(state, input, actorId) {
  const item = visibleDocument(state, input.documentId, actorId);
  const from = revision(state, input.fromRevisionId, item.id);
  const to = revision(state, input.toRevisionId ?? item.currentRevisionId, item.id);
  return { documentId: item.id, fromRevisionId: from.id, toRevisionId: to.id,
    ...compareBodies(from.body, to.body, input.maximumCells ?? 1000000) };
}

function previewTarget(state, item, input, actorId) {
  const kind = input.target?.kind ?? 'revision';
  if (kind === 'revision') {
    const selected = revision(state, input.target?.revisionId ?? item.currentRevisionId, item.id);
    return { kind, revisionId: selected.id, body: selected.body, checksum: selected.checksum, committable: true };
  }
  authorize(state, actorId, 'edit', item.collectionId);
  const current = revision(state, item.currentRevisionId, item.id);
  requireValue(input.expectedRevisionId === current.id && input.expectedChecksum === current.checksum,
    'revision_conflict', 'proposed mapping requires current revision and checksum');
  if (kind === 'sections') {
    const plan = planSectionOperations(current.body, current.checksum, input.target.operations);
    return { kind, revisionId: null, body: plan.body, checksum: plan.checksum, committable: false };
  }
  requireValue(kind === 'merge', 'invalid_input', 'unsupported mapping target kind');
  const session = entity(state, 'structureMerges', input.target.id);
  requireValue(session.documentId === item.id && session.currentRevisionId === current.id
    && session.currentChecksum === current.checksum, 'revision_conflict', 'merge target no longer matches document');
  requireValue(session.status === 'open', 'merge_closed', 'merge preview requires an open session');
  requireValue(input.target.expectedVersion === session.version, 'merge_version_conflict', 'merge session changed');
  const body = materializeMerge(session.plan, session.resolutions);
  return { kind, revisionId: null, body, checksum: digest(body), committable: false,
    mergeId: session.id, mergeVersion: session.version };
}

export function previewAnchorMap(state, input, actorId) {
  const item = visibleDocument(state, input.documentId, actorId);
  const ids = boundedIds(input.anchorIds);
  const maximumCells = integer(input.maximumCells ?? 1000000, 'mapping cell budget', 1, 2000000);
  const target = previewTarget(state, item, input, actorId);
  const mappings = ids.map(id => {
    const anchor = entity(state, 'structureAnchors', id);
    const source = sourceForAnchor(state, item, anchor);
    return { anchorId: id, sourceRevisionId: source.id, sourceQuote: anchor.range.quote,
      sourceReviewId: anchor.reviewId, sourceCommentId: anchor.commentId,
      ...mapAnchor(anchor.range, source.body, target.body, maximumCells) };
  });
  const counts = { unchanged: 0, relocated: 0, edited: 0, deleted: 0, ambiguous: 0 };
  for (const mapping of mappings) counts[mapping.status] += 1;
  const { body, ...identity } = target;
  return { documentId: item.id, target: identity, counts, mappings,
    allExact: mappings.every(mapping => mapping.exact),
    diagnostics: mappings.filter(mapping => !mapping.exact).map(mapping => ({ anchorId: mapping.anchorId,
      status: mapping.status, reason: mapping.reason, requiresSelection: true })) };
}

function validatedReview(state, reviewId, item, current) {
  if (reviewId === null) return;
  const review = entity(state, 'reviews', reviewId);
  requireValue(review.documentId === item.id && review.revisionId === current.id, 'review_mismatch', 'follow-up review must describe the current revision');
  requireValue(review.status === 'open', 'closed_review', 'follow-up review must be open');
}

function prepareFollowup(state, item, current, entry, maximumCells) {
  object(entry, 'follow-up entry');
  const anchor = entity(state, 'structureAnchors', entry.anchorId);
  const source = sourceForAnchor(state, item, anchor);
  const mapping = mapAnchor(anchor.range, source.body, current.body, maximumCells);
  let selected;
  let selectionMode;
  if (entry.selection !== undefined) {
    object(entry.selection, 'explicit anchor selection');
    requireValue(entry.acknowledgedStatus === mapping.status, 'anchor_status_conflict', 'acknowledge the current mapping status');
    selected = captureRange(current.body, entry.selection.startLine, entry.selection.endLine ?? entry.selection.startLine);
    requireValue(typeof entry.selection.quote === 'string' && selected.quote === entry.selection.quote,
      'anchor_quote_conflict', 'explicit target quote does not match current source');
    selectionMode = 'explicit';
  } else {
    requireValue(mapping.exact && mapping.target, 'anchor_not_exact', 'non-exact anchor requires explicit target selection',
      { anchorId: anchor.id, status: mapping.status, reason: mapping.reason });
    selected = mapping.target;
    selectionMode = 'mapped';
  }
  const reviewId = entry.reviewId ?? null;
  validatedReview(state, reviewId, item, current);
  const body = text(entry.body, 'follow-up comment body', { max: 4000 });
  requireValue(!Object.values(state.structureFollowups).some(link => link.anchorId === anchor.id
    && link.targetRevisionId === current.id && link.reviewId === reviewId),
  'duplicate_followup', 'anchor already has a follow-up for this revision and review');
  return { anchor, mapping, selected, selectionMode, reviewId, body };
}

export function createAnchorFollowups(state, input, actorId) {
  const item = visibleDocument(state, input.documentId, actorId);
  const current = revision(state, item.currentRevisionId, item.id);
  requireValue(input.expectedRevisionId === current.id, 'revision_conflict', 'follow-up target is no longer current');
  requireValue(input.expectedChecksum === current.checksum, 'checksum_conflict', 'follow-up target checksum changed');
  requireValue(Array.isArray(input.entries), 'invalid_input', 'follow-up entries must be a list');
  boundedIds(input.entries.map(entry => object(entry, 'follow-up entry').anchorId));
  const maximumCells = integer(input.maximumCells ?? 1000000, 'mapping cell budget', 1, 2000000);
  // Validate the entire batch before creating any comment; the dispatcher transaction
  // also protects event/counter rollback if an existing comment service rejects it.
  const prepared = input.entries.map(entry => prepareFollowup(state, item, current, entry, maximumCells));
  return prepared.map(({ anchor, mapping, selected, selectionMode, reviewId, body }) => {
    const comment = addComment(state, { documentId: item.id, revisionId: current.id,
      reviewId, line: selected.startLine, body }, actorId);
    const id = allocate(state, 'structureFollowup');
    const link = { id, anchorId: anchor.id, sourceRevisionId: anchor.revisionId,
      sourceCommentId: anchor.commentId, sourceReviewId: anchor.reviewId,
      targetRevisionId: current.id, targetChecksum: current.checksum, commentId: comment.id, reviewId,
      statusAtCreation: mapping.status, selectionMode, targetRange: captureRange(current.body, selected.startLine, selected.endLine), actorId };
    state.structureFollowups[id] = link;
    record(state, actorId, 'structure.followup_created', item.id, { followupId: id, anchorId: anchor.id, commentId: comment.id });
    return { comment, link };
  });
}

export function listAnchorFollowups(state, input, actorId) {
  const anchor = entity(state, 'structureAnchors', input.anchorId);
  visibleDocument(state, anchor.documentId, actorId);
  return Object.values(state.structureFollowups).filter(link => link.anchorId === anchor.id);
}
