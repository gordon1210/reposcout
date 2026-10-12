import { pairKey } from '../core/identity.mjs';
import { entity } from '../storage/lookup.mjs';

export function collectionPath(state, collectionId) {
  const path = [];
  const seen = new Set();
  let current = collectionId;
  while (current !== null) {
    if (seen.has(current)) throw new Error('collection hierarchy contains a cycle');
    seen.add(current);
    const item = entity(state, 'collections', current);
    path.push(item);
    current = item.parentId;
  }
  return path;
}

export function membershipRole(state, actorId, collectionId) {
  for (const collection of collectionPath(state, collectionId)) {
    const membership = state.memberships[pairKey(collection.id, actorId)];
    if (membership) return membership.role;
  }
  return null;
}

export function collectionVisible(state, actor, collection) {
  return actor.role === 'admin' || collection.public || membershipRole(state, actor.id, collection.id) !== null;
}
