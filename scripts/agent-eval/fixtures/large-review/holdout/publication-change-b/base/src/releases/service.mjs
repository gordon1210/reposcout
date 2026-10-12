import { allocate } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { text } from '../core/validation.mjs';
import { requireValue } from '../core/errors.mjs';
import { document, entity } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { releaseEntry } from './catalog.mjs';

export function publishRelease(state, input, actorId) {
  requireValue(Array.isArray(input.entries) && input.entries.length > 0 && input.entries.length <= 50,
    'invalid_input', 'a release needs between one and fifty approved documents');
  const entries = input.entries.map(item => releaseEntry(state, item));
  requireValue(new Set(entries.map(item => item.documentId)).size === entries.length,
    'duplicate_document', 'release may contain each document once');
  for (const entry of entries) {
    const item = document(state, entry.documentId);
    authorize(state, actorId, 'publish', item.collectionId);
  }
  const id = allocate(state, 'release');
  const item = { id, name: text(input.name, 'release name', { max: 200 }), entries,
    publisherId: actorId, withdrawn: false, sequence: state.sequence + 1 };
  state.releases[id] = item;
  record(state, actorId, 'release.published', id, { documents: entries.map(entry => entry.documentId) });
  return item;
}

export function withdrawRelease(state, input, actorId) {
  const item = entity(state, 'releases', input.id);
  for (const entry of item.entries) authorize(state, actorId, 'publish', state.documents[entry.documentId].collectionId);
  item.withdrawn = true;
  for (const share of Object.values(state.shares)) if (share.releaseId === item.id) share.revoked = true;
  state.searchEntries = state.searchEntries.filter(entry => entry.releaseId !== item.id);
  record(state, actorId, 'release.withdrawn', item.id, { reason: input.reason ?? '' });
  return { id: item.id, withdrawn: true };
}
