import { document, revision } from '../storage/lookup.mjs';
import { authorize, visibleCollections } from '../permissions/policy.mjs';
import { page } from '../core/pagination.mjs';
import { latestRevision, revisionHistory, revisionSummary } from './revisions.mjs';

export function getDocument(state, input, actorId) {
  const item = document(state, input.id, { archived: input.includeArchived === true });
  authorize(state, actorId, 'read', item.collectionId);
  const selected = input.revisionId ? revision(state, input.revisionId, item.id) : latestRevision(state, item);
  return { ...item, revision: selected };
}

export function listDocuments(state, input, actorId) {
  const visible = new Set(visibleCollections(state, actorId).map(item => item.id));
  let candidates = Object.values(state.documents).filter(item => visible.has(item.collectionId));
  if (input.collectionId) candidates = candidates.filter(item => item.collectionId === input.collectionId);
  if (!input.includeArchived) candidates = candidates.filter(item => !item.archived);
  if (input.ownerId) candidates = candidates.filter(item => item.ownerId === input.ownerId);
  const result = page(candidates, input);
  return { ...result, items: result.items.map(item => ({ ...item, revision: revisionSummary(latestRevision(state, item)) })) };
}

export function history(state, input, actorId) {
  const item = document(state, input.id, { archived: true });
  authorize(state, actorId, 'read', item.collectionId);
  return revisionHistory(state, item.id).map(revisionSummary);
}
