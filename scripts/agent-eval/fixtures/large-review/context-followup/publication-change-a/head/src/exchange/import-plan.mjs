import { DomainError, requireValue } from '../core/errors.mjs';
import { object, choice, text } from '../core/validation.mjs';
import { transaction } from '../storage/transaction.mjs';
import { identity, parseManifest } from './manifest.mjs';
import { referenceGraph, referenceComponents } from './references.mjs';
import { decodeArchive } from './archive.mjs';
import { componentPreparation, applyPreparedComponent } from './import-component.mjs';

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
      const prepared = componentPreparation(state, request, keys, actorId, establishedCollections);
      const first = prepared.rows.next();
      const result = transaction(state, () => applyPreparedComponent(state, prepared, first, actorId));
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
