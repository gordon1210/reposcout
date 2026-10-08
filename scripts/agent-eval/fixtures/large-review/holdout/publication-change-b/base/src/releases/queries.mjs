import { entity } from '../storage/lookup.mjs';
import { authorize, visibleCollections } from '../permissions/policy.mjs';
import { page } from '../core/pagination.mjs';

export function getRelease(state, input, actorId) {
  const release = entity(state, 'releases', input.id);
  for (const entry of release.entries) authorize(state, actorId, 'read', state.documents[entry.documentId].collectionId);
  return release;
}

export function listReleases(state, input, actorId) {
  const visible = new Set(visibleCollections(state, actorId).map(item => item.id));
  const candidates = Object.values(state.releases).filter(release =>
    (input.includeWithdrawn || !release.withdrawn) &&
    release.entries.every(entry => visible.has(state.documents[entry.documentId].collectionId)));
  return page(candidates, input);
}
