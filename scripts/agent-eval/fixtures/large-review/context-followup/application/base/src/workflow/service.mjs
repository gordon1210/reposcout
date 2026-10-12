import { allocate } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { choice, text } from '../core/validation.mjs';
import { requireValue } from '../core/errors.mjs';
import { document, entity, revision } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { canDecide, unresolvedComments } from './rules.mjs';

export function openReview(state, input, actorId) {
  const item = document(state, input.documentId);
  authorize(state, actorId, 'edit', item.collectionId);
  const selected = revision(state, input.revisionId ?? item.currentRevisionId, item.id);
  const assigneeId = text(input.assigneeId, 'reviewer id');
  authorize(state, assigneeId, 'review', item.collectionId);
  requireValue(assigneeId !== selected.authorId, 'self_approval', 'choose an independent reviewer');
  requireValue(!Object.values(state.reviews).some(review => review.documentId === item.id && review.revisionId === selected.id && review.status === 'open'),
    'duplicate_review', 'this revision already has an open review');
  const id = allocate(state, 'review');
  state.reviews[id] = { id, documentId: item.id, revisionId: selected.id, authorId: selected.authorId,
    assigneeId, status: 'open', note: input.note ?? '', decisionNote: null };
  record(state, actorId, 'review.opened', item.id, { reviewId: id, revisionId: selected.id });
  return state.reviews[id];
}

export function assignReview(state, input, actorId) {
  const review = entity(state, 'reviews', input.id);
  const item = document(state, review.documentId);
  authorize(state, actorId, 'manage', item.collectionId);
  authorize(state, input.assigneeId, 'review', item.collectionId);
  requireValue(review.status === 'open', 'closed_review', 'closed reviews cannot be reassigned');
  requireValue(review.authorId !== input.assigneeId, 'self_approval', 'review author cannot become reviewer');
  review.assigneeId = input.assigneeId;
  record(state, actorId, 'review.assigned', item.id, { reviewId: review.id, assigneeId: review.assigneeId });
  return review;
}

export function decideReview(state, input, actorId) {
  const review = entity(state, 'reviews', input.id);
  const item = document(state, review.documentId);
  authorize(state, actorId, 'review', item.collectionId);
  canDecide(review, actorId);
  const decision = choice(input.decision, ['approve', 'reject'], 'review decision');
  requireValue(decision !== 'approve' || unresolvedComments(state, review.id).length === 0,
    'unresolved_comments', 'resolve review comments before approval');
  review.status = decision === 'approve' ? 'approved' : 'rejected';
  review.decisionNote = text(input.note ?? '', 'decision note', { empty: true });
  let approval = null;
  if (decision === 'approve') {
    const id = allocate(state, 'approval');
    approval = { id, documentId: item.id, revisionId: review.revisionId, reviewId: review.id, reviewerId: actorId };
    state.approvals[id] = approval;
  }
  record(state, actorId, `review.${review.status}`, item.id, { reviewId: review.id, approvalId: approval?.id ?? null });
  return { review, approval };
}
