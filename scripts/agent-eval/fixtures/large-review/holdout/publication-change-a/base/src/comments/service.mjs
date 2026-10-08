import { allocate } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { text, integer } from '../core/validation.mjs';
import { requireValue } from '../core/errors.mjs';
import { document, entity, revision } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';

export function addComment(state, input, actorId) {
  const item = document(state, input.documentId);
  authorize(state, actorId, 'read', item.collectionId);
  const selected = revision(state, input.revisionId ?? item.currentRevisionId, item.id);
  const line = integer(input.line ?? 1, 'comment line', 1, selected.body.split('\n').length);
  const reviewId = input.reviewId ?? null;
  if (reviewId !== null) {
    const review = entity(state, 'reviews', reviewId);
    requireValue(review.documentId === item.id && review.revisionId === selected.id,
      'review_mismatch', 'comment does not describe the reviewed revision');
    requireValue(review.status === 'open', 'closed_review', 'cannot comment on a closed review');
  }
  const id = allocate(state, 'comment');
  state.comments[id] = { id, documentId: item.id, revisionId: selected.id, reviewId,
    actorId, line, body: text(input.body, 'comment body', { max: 4000 }), resolved: false };
  record(state, actorId, 'comment.added', item.id, { commentId: id });
  return state.comments[id];
}

export function resolveComment(state, input, actorId) {
  const comment = entity(state, 'comments', input.id);
  const item = document(state, comment.documentId);
  authorize(state, actorId, 'edit', item.collectionId);
  comment.resolved = input.resolved !== false;
  record(state, actorId, comment.resolved ? 'comment.resolved' : 'comment.reopened', item.id, { commentId: comment.id });
  return comment;
}

export function listComments(state, input, actorId) {
  const item = document(state, input.documentId, { archived: true });
  authorize(state, actorId, 'read', item.collectionId);
  return Object.values(state.comments).filter(comment => comment.documentId === item.id &&
    (input.revisionId === undefined || comment.revisionId === input.revisionId) &&
    (input.includeResolved || !comment.resolved));
}
