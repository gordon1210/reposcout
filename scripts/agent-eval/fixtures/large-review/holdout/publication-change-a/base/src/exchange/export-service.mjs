import { requireValue } from '../core/errors.mjs';
import { digest } from '../core/identity.mjs';
import { choice, strings } from '../core/validation.mjs';
import { authorize } from '../permissions/policy.mjs';
import { entity } from '../storage/lookup.mjs';
import { getDocument } from '../documents/queries.mjs';
import { entriesForRelease } from '../releases/catalog.mjs';
import { identity, parseManifest } from './manifest.mjs';
import { portableBody } from './references.mjs';
import { renderArchive } from './archive.mjs';

function collectCollections(state, items, actorId) {
  const collected = new Map();
  const visiting = new Set();
  function visit(id) {
    if (collected.has(id)) return;
    requireValue(!visiting.has(id), 'exchange_collection_cycle', 'workspace collection ancestry is cyclic');
    visiting.add(id);
    const collection = entity(state, 'collections', id);
    authorize(state, actorId, 'read', id);
    if (collection.parentId !== null) visit(collection.parentId);
    collected.set(id, { key: id, name: collection.name,
      parentKey: collection.parentId, description: collection.description ?? '' });
    visiting.delete(id);
  }
  for (const item of items) visit(item.collectionId);
  return [...collected.values()];
}

export function buildExchangeManifest(state, selected, origin, actorId, externalPolicy = 'reject') {
  choice(externalPolicy, ['reject', 'preserve'], 'external reference policy');
  const documents = selected.map(entry => getDocument(state, {
    id: entry.documentId, revisionId: entry.revisionId,
  }, actorId));
  requireValue(new Set(documents.map(item => item.id)).size === documents.length,
    'duplicate_document', 'interchange selects each document once');
  const bindings = new Map(documents.map(item => [item.id, { key: item.id, revisionId: item.revision.id }]));
  const diagnostics = [];
  const entries = documents.map(item => {
    const portable = portableBody(item.revision.body, bindings, externalPolicy);
    diagnostics.push(...portable.diagnostics.map(detail => ({ ...detail, sourceDocumentId: item.id })));
    return { key: item.id, collectionKey: item.collectionId, title: item.revision.title,
      body: portable.body, tags: item.revision.tags, language: item.revision.language,
      source: { documentId: item.id, revisionId: item.revision.id, checksum: item.revision.checksum } };
  });
  const manifest = parseManifest({ format: 'publication-exchange', version: 1, origin,
    collections: collectCollections(state, documents, actorId), documents: entries });
  return { manifest, diagnostics };
}

function resultFor(state, selected, origin, actorId, input, mode) {
  const built = buildExchangeManifest(state, selected, origin, actorId, input.externalPolicy ?? 'reject');
  const archive = renderArchive(built.manifest);
  return { mode, manifest: built.manifest, diagnostics: built.diagnostics, archive,
    identity: archive.manifestIdentity, sources: selected.map(item => ({ ...item })) };
}

export function exportReleaseArchive(state, input, actorId) {
  const release = entity(state, 'releases', input.releaseId);
  const entries = entriesForRelease(state, release.id);
  for (const entry of entries) {
    const item = getDocument(state, { id: entry.documentId, revisionId: entry.revisionId }, actorId);
    requireValue(item.revision.checksum === entry.checksum && digest(item.revision.body) === entry.checksum,
      'exchange_source_integrity', 'release source checksum does not match retained revision',
      { documentId: entry.documentId, revisionId: entry.revisionId });
  }
  return resultFor(state, entries, `release:${release.id}`, actorId, input, 'release');
}

export function exportWorkingArchive(state, input, actorId) {
  const ids = strings(input.documentIds, 'working document IDs', 200);
  requireValue(ids.length > 0, 'invalid_input', 'working archive requires explicit document selection');
  const selected = ids.sort().map(id => {
    const item = getDocument(state, { id }, actorId);
    return { documentId: item.id, revisionId: item.revision.id, checksum: item.revision.checksum };
  });
  return resultFor(state, selected, `working:${identity(selected)}`, actorId, input, 'working');
}
