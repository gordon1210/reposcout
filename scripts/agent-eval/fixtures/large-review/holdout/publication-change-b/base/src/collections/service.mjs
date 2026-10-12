import { allocate, slug, pairKey } from '../core/identity.mjs';
import { text, choice } from '../core/validation.mjs';
import { requireValue } from '../core/errors.mjs';
import { record } from '../core/events.mjs';
import { entity } from '../storage/lookup.mjs';
import { authorize, visibleCollections } from '../permissions/policy.mjs';
import { roleNames } from '../permissions/roles.mjs';
import { descendants, validateParent, breadcrumbs } from './tree.mjs';

export function createCollection(state, input, actorId) {
  const parentId = input.parentId ?? null;
  authorize(state, actorId, 'manage', parentId);
  const name = text(input.name, 'collection name', { max: 160 });
  const code = slug(name);
  requireValue(code.length > 0, 'invalid_input', 'collection name needs a searchable slug');
  requireValue(!Object.values(state.collections).some(item => item.parentId === parentId && item.slug === code),
    'duplicate_collection', 'a collection with this name already exists here');
  const id = allocate(state, 'collection');
  state.collections[id] = { id, name, slug: code, parentId, public: input.public === true, description: input.description ?? '' };
  state.memberships[pairKey(id, actorId)] = { collectionId: id, actorId, role: 'owner' };
  record(state, actorId, 'collection.created', id, { parentId });
  return state.collections[id];
}

export function moveCollection(state, input, actorId) {
  const item = entity(state, 'collections', input.id);
  const parentId = input.parentId ?? null;
  authorize(state, actorId, 'manage', item.id);
  authorize(state, actorId, 'manage', parentId);
  validateParent(state, item.id, parentId);
  requireValue(!Object.values(state.collections).some(other => other.id !== item.id && other.parentId === parentId && other.slug === item.slug),
    'duplicate_collection', 'target already has this collection name');
  const previous = item.parentId;
  item.parentId = parentId;
  record(state, actorId, 'collection.moved', item.id, { previous, parentId });
  return item;
}

export function listCollections(state, input, actorId) {
  const visible = visibleCollections(state, actorId);
  const selected = input.parentId === undefined ? visible : visible.filter(item => item.parentId === input.parentId);
  return selected.map(item => ({ ...item, path: breadcrumbs(state, item.id), descendants: descendants(state, item.id).length }));
}

export function assignMember(state, input, actorId) {
  authorize(state, actorId, 'manage', input.collectionId);
  entity(state, 'users', input.actorId);
  const role = choice(input.role, roleNames().filter(name => name !== 'admin'), 'collection role');
  const item = { collectionId: input.collectionId, actorId: input.actorId, role };
  state.memberships[pairKey(input.collectionId, input.actorId)] = item;
  record(state, actorId, 'member.assigned', input.collectionId, { actorId: input.actorId, role });
  return item;
}

export function removeMember(state, input, actorId) {
  authorize(state, actorId, 'manage', input.collectionId);
  delete state.memberships[pairKey(input.collectionId, input.actorId)];
  record(state, actorId, 'member.removed', input.collectionId, { actorId: input.actorId });
  return { removed: true };
}
