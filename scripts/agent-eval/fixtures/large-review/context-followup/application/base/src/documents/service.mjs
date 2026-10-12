import { allocate } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { text } from '../core/validation.mjs';
import { requireValue } from '../core/errors.mjs';
import { document, entity } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { createRevision, latestRevision } from './revisions.mjs';

export function createDocument(state, input, actorId) {
  const collection = entity(state, 'collections', input.collectionId);
  authorize(state, actorId, 'edit', collection.id);
  const id = allocate(state, 'document');
  const first = createRevision(state, id, input, actorId);
  state.documents[id] = {
    id, collectionId: collection.id, currentRevisionId: first.id,
    archived: false, ownerId: actorId, createdSequence: state.sequence + 1,
  };
  record(state, actorId, 'document.created', id, { revisionId: first.id });
  return { ...state.documents[id], revision: first };
}

export function reviseDocument(state, input, actorId) {
  const item = document(state, input.id);
  authorize(state, actorId, 'edit', item.collectionId);
  if (input.expectedRevisionId !== undefined) {
    requireValue(item.currentRevisionId === input.expectedRevisionId,
      'revision_conflict', 'document has changed since it was read');
  }
  const previous = latestRevision(state, item);
  const next = createRevision(state, item.id, {
    title: input.title ?? previous.title,
    body: input.body ?? previous.body,
    tags: input.tags ?? previous.tags,
    language: input.language ?? previous.language,
  }, actorId, previous.id);
  item.currentRevisionId = next.id;
  record(state, actorId, 'document.revised', item.id, { previous: previous.id, revisionId: next.id });
  return { ...item, revision: next };
}

export function moveDocument(state, input, actorId) {
  const item = document(state, input.id);
  authorize(state, actorId, 'edit', item.collectionId);
  authorize(state, actorId, 'edit', input.collectionId);
  entity(state, 'collections', input.collectionId);
  const previous = item.collectionId;
  item.collectionId = input.collectionId;
  record(state, actorId, 'document.moved', item.id, { previous, collectionId: input.collectionId });
  return item;
}

export function archiveDocument(state, input, actorId) {
  const item = document(state, input.id, { archived: true });
  authorize(state, actorId, 'edit', item.collectionId);
  const archived = input.archived !== false;
  requireValue(!archived || !Object.values(state.reviews).some(review => review.documentId === item.id && review.status === 'open'),
    'open_review', 'close the review before archiving');
  item.archived = archived;
  record(state, actorId, archived ? 'document.archived' : 'document.restored', item.id);
  return item;
}

export function transferOwnership(state, input, actorId) {
  const item = document(state, input.id);
  authorize(state, actorId, 'manage', item.collectionId);
  const owner = entity(state, 'users', text(input.ownerId, 'owner id'));
  requireValue(owner.active, 'inactive_actor', 'new owner is disabled');
  item.ownerId = owner.id;
  record(state, actorId, 'document.owner_changed', item.id, { ownerId: owner.id });
  return item;
}
