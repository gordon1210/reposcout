import { entity, document } from '../storage/lookup.mjs';
import { authorize, visibleCollections } from '../permissions/policy.mjs';
import { page } from '../core/pagination.mjs';
import { unresolvedComments } from './rules.mjs';

export function getReview(state, input, actorId) {
  const review = entity(state, 'reviews', input.id);
  const item = document(state, review.documentId, { archived: true });
  authorize(state, actorId, 'read', item.collectionId);
  return { ...review, openComments: unresolvedComments(state, review.id).length };
}

export function reviewQueue(state, input, actorId) {
  const visible = new Set(visibleCollections(state, actorId).map(item => item.id));
  const entries = Object.values(state.reviews).filter(item =>
    visible.has(state.documents[item.documentId].collectionId) &&
    (input.assigneeId === undefined || item.assigneeId === input.assigneeId) &&
    (input.status === undefined || item.status === input.status));
  return page(entries, input);
}
