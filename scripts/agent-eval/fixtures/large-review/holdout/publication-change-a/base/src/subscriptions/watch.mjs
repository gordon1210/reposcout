import { pairKey } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { document } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { choice } from '../core/validation.mjs';

export function watchDocument(state, input, actorId) {
  const item = document(state, input.documentId);
  authorize(state, actorId, 'read', item.collectionId);
  const mode = choice(input.mode ?? 'all', ['all', 'publication'], 'watch mode');
  const key = pairKey(actorId, item.id);
  const prior = state.watches[key];
  state.watches[key] = { actorId, documentId: item.id, mode, afterSequence: prior?.afterSequence ?? state.sequence };
  return state.watches[key];
}

export function unwatchDocument(state, input, actorId) {
  delete state.watches[pairKey(actorId, input.documentId)];
  return { watching: false };
}

export function listWatches(state, input, actorId) {
  return Object.values(state.watches).filter(item => item.actorId === actorId)
    .map(item => ({ ...item, archived: state.documents[item.documentId].archived }));
}
