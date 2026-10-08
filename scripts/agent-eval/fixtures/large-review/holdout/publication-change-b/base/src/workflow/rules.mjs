import { requireValue } from '../core/errors.mjs';

export function canDecide(review, actorId) {
  requireValue(review.status === 'open', 'closed_review', 'review is already closed');
  requireValue(review.assigneeId === actorId, 'wrong_reviewer', 'only the assigned reviewer can decide');
  requireValue(review.authorId !== actorId, 'self_approval', 'authors cannot approve their own revision');
}

export function approvalFor(state, documentId, revisionId) {
  return Object.values(state.approvals).find(item => item.documentId === documentId && item.revisionId === revisionId) ?? null;
}

export function unresolvedComments(state, reviewId) {
  return Object.values(state.comments).filter(item => item.reviewId === reviewId && !item.resolved);
}
