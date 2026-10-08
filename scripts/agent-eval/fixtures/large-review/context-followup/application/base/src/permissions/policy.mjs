import { requireValue } from '../core/errors.mjs';
import { entity } from '../storage/lookup.mjs';
import { canRole } from './roles.mjs';
import { collectionVisible, membershipRole } from './membership.mjs';

export function actor(state, actorId) {
  const value = entity(state, 'users', actorId);
  requireValue(value.active, 'inactive_actor', 'user is disabled');
  return value;
}

export function authorize(state, actorId, action, collectionId = null) {
  const user = actor(state, actorId);
  if (user.role === 'admin') return user;
  const effective = collectionId === null ? user.role : membershipRole(state, actorId, collectionId);
  const collection = collectionId === null ? null : entity(state, 'collections', collectionId);
  if (action === 'read' && collection?.public && collectionVisible(state, user, collection)) return user;
  requireValue(canRole(effective, action), 'forbidden', `actor cannot ${action}`, { collectionId });
  return user;
}

export function visibleCollections(state, actorId) {
  const user = actor(state, actorId);
  return Object.values(state.collections).filter(collection => collectionVisible(state, user, collection));
}
