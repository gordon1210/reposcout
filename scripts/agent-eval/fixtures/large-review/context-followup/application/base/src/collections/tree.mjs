import { requireValue } from '../core/errors.mjs';
import { collectionPath } from '../permissions/membership.mjs';

export function descendants(state, collectionId) {
  const result = [];
  const queue = [collectionId];
  while (queue.length > 0) {
    const parent = queue.shift();
    const children = Object.values(state.collections).filter(item => item.parentId === parent)
      .sort((a, b) => a.name.localeCompare(b.name));
    result.push(...children);
    queue.push(...children.map(item => item.id));
  }
  return result;
}

export function validateParent(state, collectionId, parentId) {
  if (parentId === null) return;
  const path = collectionPath(state, parentId);
  requireValue(!path.some(item => item.id === collectionId), 'collection_cycle', 'move would create a cycle');
}

export function breadcrumbs(state, collectionId) {
  return collectionPath(state, collectionId).reverse().map(({ id, name }) => ({ id, name }));
}
