import { allocate } from '../core/identity.mjs';
import { requireValue } from '../core/errors.mjs';
import { authorize } from '../permissions/policy.mjs';
import { entity, document } from '../storage/lookup.mjs';
import { createCollection } from '../collections/service.mjs';
import { createDocument, reviseDocument } from '../documents/service.mjs';
import { identity } from './manifest.mjs';
import { bindBody, portableLinks } from './references.mjs';

function ownBinding(bindings, key) {
  return Object.hasOwn(bindings, key) ? bindings[key] : undefined;
}

function collectionResolver(state, request, actorId) {
  const items = new Map(request.manifest.collections.map(item => [item.key, item]));
  const resolved = new Map();
  function resolve(key) {
    if (resolved.has(key)) return resolved.get(key);
    const bound = ownBinding(request.collectionBindings, key);
    if (bound !== undefined) {
      const target = entity(state, 'collections', bound);
      authorize(state, actorId, 'edit', target.id);
      resolved.set(key, target.id);
      return target.id;
    }
    const item = items.get(key);
    const parentId = item.parentKey === null ? request.parentCollectionId : resolve(item.parentKey);
    const created = createCollection(state, { name: item.name, description: item.description, parentId, public: false }, actorId);
    resolved.set(key, created.id);
    return created.id;
  }
  return { resolve, resolved };
}

export function componentPreparation(state, request, keys, actorId, establishedCollections) {
  const selected = request.manifest.documents.filter(item => keys.includes(item.key));
  const resolver = collectionResolver(state, {
    ...request, collectionBindings: { ...request.collectionBindings, ...Object.fromEntries(establishedCollections) },
  }, actorId);
  const plannedIds = new Map();
  const allocator = { counters: { ...state.counters } };
  for (const item of selected) {
    const target = ownBinding(request.targets, item.key);
    plannedIds.set(item.key, target ? target.documentId : allocate(allocator, 'document'));
  }
  function* rows() {
    for (const item of selected) {
      yield { item, collectionId: resolver.resolve(item.collectionKey),
        target: ownBinding(request.targets, item.key), plannedIds };
    }
  }
  return { rows: rows(), collections: resolver.resolved };
}

export function applyPreparedComponent(state, prepared, first, actorId) {
  const records = [];
  let cursor = first;
  while (!cursor.done) {
    const { item, collectionId, target, plannedIds } = cursor.value;
    if (target) {
      const current = document(state, target.documentId);
      authorize(state, actorId, 'edit', current.collectionId);
      requireValue(current.collectionId === collectionId, 'collection_mismatch', 'import update cannot move a document', { key: item.key });
      requireValue(current.currentRevisionId === target.expectedRevisionId,
        'revision_conflict', 'import target changed since selection', { key: item.key });
    }
    const body = bindBody(state, item.body, plannedIds, actorId);
    const content = { title: item.title, body, tags: item.tags, language: item.language };
    const result = target
      ? reviseDocument(state, { id: target.documentId, expectedRevisionId: target.expectedRevisionId, ...content }, actorId)
      : createDocument(state, { collectionId, ...content }, actorId);
    requireValue(result.id === plannedIds.get(item.key), 'exchange_identity_conflict', 'allocated document identity changed during import');
    records.push({ key: item.key, documentId: result.id, revisionId: result.revision.id,
      collectionId, checksum: result.revision.checksum, operation: target ? 'update' : 'create', source: item.source,
      sourceIdentity: identity(item), referenceIdentity: identity(portableLinks(item.body).map(link =>
        ({ key: link.key, documentId: plannedIds.get(link.key) }))) });
    cursor = prepared.rows.next();
  }
  return { records, collections: prepared.collections };
}
