import { allocate } from '../core/identity.mjs';
import { DomainError, requireValue } from '../core/errors.mjs';
import { object, choice, text } from '../core/validation.mjs';
import { authorize } from '../permissions/policy.mjs';
import { entity, document } from '../storage/lookup.mjs';
import { transaction } from '../storage/transaction.mjs';
import { createCollection } from '../collections/service.mjs';
import { createDocument, reviseDocument } from '../documents/service.mjs';
import { identity, parseManifest } from './manifest.mjs';
import { referenceGraph, referenceComponents, bindBody, portableLinks } from './references.mjs';
import { decodeArchive } from './archive.mjs';

function bindings(value, keys, name, parse) {
  object(value, name);
  return Object.fromEntries(Object.entries(value).map(([key, entry]) => {
    requireValue(keys.has(key), 'exchange_unknown_binding', 'binding names an absent portable key', { key, name });
    return [key, parse(entry)];
  }));
}

export function importRequest(input) {
  requireValue((input.manifest !== undefined) !== (input.archive !== undefined),
    'exchange_format', 'supply exactly one manifest or archive');
  const manifest = input.archive !== undefined ? decodeArchive(input.archive) : parseManifest(input.manifest);
  const policy = choice(input.policy ?? 'atomic', ['atomic', 'partial'], 'batch policy');
  const collectionKeys = new Set(manifest.collections.map(item => item.key));
  const documentKeys = new Set(manifest.documents.map(item => item.key));
  const collectionBindings = bindings(input.collectionBindings ?? {}, collectionKeys, 'collection bindings',
    value => text(value, 'destination collection', { max: 100 }));
  const targets = bindings(input.targets ?? {}, documentKeys, 'document targets', value => {
    object(value, 'document target');
    requireValue(Object.keys(value).every(key => ['documentId', 'expectedRevisionId'].includes(key)),
      'exchange_format', 'unknown document target field');
    return { documentId: text(value.documentId, 'destination document', { max: 100 }),
      expectedRevisionId: text(value.expectedRevisionId, 'expected revision', { max: 100 }) };
  });
  requireValue(new Set(Object.values(targets).map(item => item.documentId)).size === Object.keys(targets).length,
    'exchange_duplicate_target', 'two source documents cannot update one destination');
  const parentCollectionId = input.parentCollectionId == null ? null : text(input.parentCollectionId, 'parent collection', { max: 100 });
  return { manifest, policy, collectionBindings, targets, parentCollectionId };
}

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

function applyComponent(state, request, keys, actorId, establishedCollections) {
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
  const records = [];
  for (const item of selected) {
    const collectionId = resolver.resolve(item.collectionKey);
    const target = ownBinding(request.targets, item.key);
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
  }
  return { records, collections: resolver.resolved };
}

export function executeImport(state, request, actorId) {
  const graph = referenceGraph(request.manifest);
  const components = request.policy === 'atomic'
    ? [request.manifest.documents.map(item => item.key)]
    : referenceComponents(request.manifest, graph.edges);
  const establishedCollections = new Map();
  const imported = [];
  const rejected = [];
  const diagnostics = [...graph.diagnostics];
  for (const keys of components) {
    const structural = diagnostics.filter(item => keys.includes(item.key));
    if (structural.length) {
      rejected.push(...keys.map(key => ({ key, code: 'exchange_component_invalid' })));
      continue;
    }
    try {
      const result = transaction(state, () => applyComponent(state, request, keys, actorId, establishedCollections));
      imported.push(...result.records);
      for (const [key, id] of result.collections) establishedCollections.set(key, id);
    } catch (error) {
      if (!(error instanceof DomainError)) throw error;
      diagnostics.push({ code: error.code, message: error.message, keys, details: error.details });
      rejected.push(...keys.map(key => ({ key, code: error.code })));
    }
  }
  return { imported, rejected, diagnostics, collections: Object.fromEntries(establishedCollections) };
}

export function planIdentity(state, request, actorId) {
  return identity({ actorId, sequence: state.sequence, request });
}

export function previewBatchImport(state, input, actorId) {
  const request = importRequest(input);
  const result = executeImport(structuredClone(state), request, actorId);
  return {
    version: 1, manifestIdentity: identity(request.manifest), planToken: planIdentity(state, request, actorId),
    sequence: state.sequence, policy: request.policy, applicable: result.imported.length > 0 &&
      (request.policy === 'partial' || result.rejected.length === 0), ...result,
  };
}
