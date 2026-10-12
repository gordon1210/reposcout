import { digest } from '../core/identity.mjs';
import { requireValue } from '../core/errors.mjs';
import { object, text, strings } from '../core/validation.mjs';

export const MANIFEST_VERSION = 1;
export const LIMITS = Object.freeze({ documents: 200, collections: 100, characters: 4000000 });

export function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value && typeof value === 'object') {
    return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

export function identity(value) {
  return digest(canonical(value));
}

function fields(value, allowed, path) {
  object(value, path);
  const unknown = Object.keys(value).filter(key => !allowed.includes(key));
  requireValue(unknown.length === 0, 'exchange_format', 'unknown manifest fields', { path, fields: unknown });
}

export function portableKey(value, path) {
  requireValue(typeof value === 'string' && /^[a-zA-Z0-9][a-zA-Z0-9._-]{0,79}$/u.test(value),
    'exchange_format', 'portable key must contain one to eighty ASCII identifier characters', { path });
  return value;
}

function checkedText(value, path, maximum, empty = false) {
  try { return text(value, path, { max: maximum, empty }); }
  catch (error) {
    if (error.code === 'invalid_input') error.details = { ...error.details, path };
    throw error;
  }
}

function uniqueKeys(items, path) {
  const seen = new Set();
  for (const [index, item] of items.entries()) {
    requireValue(!seen.has(item.key), 'exchange_duplicate_key', 'portable keys must be unique within their kind',
      { path: `${path}[${index}].key`, key: item.key });
    seen.add(item.key);
  }
  return seen;
}

function collectionOrder(collections) {
  const byKey = new Map(collections.map(item => [item.key, item]));
  const visiting = new Set();
  const visited = new Set();
  const ordered = [];
  function visit(key, chain) {
    if (visited.has(key)) return;
    requireValue(!visiting.has(key), 'exchange_collection_cycle', 'collection ancestry is cyclic', { chain: [...chain, key] });
    const item = byKey.get(key);
    requireValue(item, 'exchange_missing_collection', 'collection parent is absent', { key, chain });
    visiting.add(key);
    if (item.parentKey !== null) visit(item.parentKey, [...chain, key]);
    visiting.delete(key);
    visited.add(key);
    ordered.push(item);
  }
  for (const key of [...byKey.keys()].sort()) visit(key, []);
  return ordered;
}

export function parseManifest(source) {
  let input = source;
  if (typeof source === 'string') {
    requireValue(source.length <= LIMITS.characters, 'exchange_limit', 'manifest is too large');
    try { input = JSON.parse(source); }
    catch { requireValue(false, 'exchange_json', 'manifest is not valid JSON'); }
  }
  fields(input, ['format', 'version', 'origin', 'collections', 'documents'], '$');
  requireValue(input.format === 'publication-exchange', 'exchange_format', 'unsupported manifest format');
  requireValue(input.version === MANIFEST_VERSION, 'exchange_version', 'unsupported manifest version',
    { supported: [MANIFEST_VERSION], received: input.version });
  const origin = checkedText(input.origin, '$.origin', 200);
  requireValue(Array.isArray(input.collections) && input.collections.length <= LIMITS.collections,
    'exchange_limit', 'manifest collections must be a bounded list');
  requireValue(Array.isArray(input.documents) && input.documents.length > 0 && input.documents.length <= LIMITS.documents,
    'exchange_limit', 'manifest needs one to two hundred documents');
  const collections = input.collections.map((item, index) => {
    const path = `$.collections[${index}]`;
    fields(item, ['key', 'name', 'parentKey', 'description'], path);
    return { key: portableKey(item.key, `${path}.key`), name: checkedText(item.name, `${path}.name`, 160),
      parentKey: item.parentKey == null ? null : portableKey(item.parentKey, `${path}.parentKey`),
      description: checkedText(item.description ?? '', `${path}.description`, 10000, true) };
  });
  const keys = uniqueKeys(collections, '$.collections');
  let characters = 0;
  const documents = input.documents.map((item, index) => {
    const path = `$.documents[${index}]`;
    fields(item, ['key', 'collectionKey', 'title', 'body', 'tags', 'language', 'source'], path);
    const body = checkedText(item.body, `${path}.body`, 200000, true);
    characters += body.length;
    requireValue(keys.has(item.collectionKey), 'exchange_missing_collection', 'document collection is absent',
      { path: `${path}.collectionKey`, key: item.collectionKey });
    const language = checkedText(item.language ?? 'en', `${path}.language`, 35);
    requireValue(/^[a-zA-Z]{2,8}(?:-[a-zA-Z0-9]{1,8})*$/u.test(language),
      'exchange_format', 'language must be a language tag', { path: `${path}.language` });
    let provenance = null;
    if (item.source != null) {
      fields(item.source, ['documentId', 'revisionId', 'checksum'], `${path}.source`);
      requireValue(typeof item.source.checksum === 'string' && /^[a-f0-9]{64}$/u.test(item.source.checksum),
        'exchange_format', 'source checksum must be SHA-256', { path: `${path}.source.checksum` });
      provenance = {
        documentId: checkedText(item.source.documentId, `${path}.source.documentId`, 100),
        revisionId: checkedText(item.source.revisionId, `${path}.source.revisionId`, 100),
        checksum: item.source.checksum,
      };
    }
    return { key: portableKey(item.key, `${path}.key`), collectionKey: item.collectionKey,
      title: checkedText(item.title, `${path}.title`, 240), body,
      tags: strings(item.tags ?? [], `${path}.tags`, 24).sort(), language, source: provenance };
  });
  requireValue(characters <= LIMITS.characters, 'exchange_limit', 'document text exceeds batch limit');
  uniqueKeys(documents, '$.documents');
  const manifest = { format: 'publication-exchange', version: MANIFEST_VERSION, origin,
    collections: collectionOrder(collections), documents: documents.sort((a, b) => a.key < b.key ? -1 : a.key > b.key ? 1 : 0) };
  requireValue(canonical(manifest).length <= LIMITS.characters, 'exchange_limit', 'normalized manifest exceeds batch limit');
  return manifest;
}
