import { allocate } from '../core/identity.mjs';
import { text } from '../core/validation.mjs';
import { record } from '../core/events.mjs';
import { actor } from '../permissions/policy.mjs';
import { entity } from '../storage/lookup.mjs';
import { requireValue } from '../core/errors.mjs';
import { search } from './query.mjs';

export function saveSearch(state, input, actorId) {
  actor(state, actorId);
  const id = allocate(state, 'search');
  const query = { query: text(input.query, 'query', { max: 300 }),
    ...(input.language ? { language: input.language } : {}),
    ...(input.tag ? { tag: input.tag } : {}) };
  state.savedSearches[id] = { id, actorId, name: text(input.name, 'search name', { max: 160 }), query };
  record(state, actorId, 'search.saved', id);
  return state.savedSearches[id];
}

export function runSavedSearch(state, input, actorId) {
  const saved = entity(state, 'savedSearches', input.id);
  requireValue(saved.actorId === actorId, 'forbidden', 'saved search belongs to another actor');
  return search(state, saved.query, actorId);
}

export function deleteSavedSearch(state, input, actorId) {
  const saved = entity(state, 'savedSearches', input.id);
  requireValue(saved.actorId === actorId, 'forbidden', 'saved search belongs to another actor');
  delete state.savedSearches[input.id];
  return { deleted: true };
}
