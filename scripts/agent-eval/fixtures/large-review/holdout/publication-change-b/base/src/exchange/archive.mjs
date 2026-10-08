import { digest } from '../core/identity.mjs';
import { requireValue } from '../core/errors.mjs';
import { object } from '../core/validation.mjs';
import { canonical, identity, parseManifest, LIMITS } from './manifest.mjs';

function documentFile(item) {
  const metadata = { title: item.title, language: item.language, tags: item.tags,
    collection: item.collectionKey, source: item.source };
  const header = Object.entries(metadata).map(([key, value]) => `${key}: ${JSON.stringify(value)}`).join('\n');
  return `---\n${header}\n---\n\n${item.body}`;
}

export function renderArchive(source) {
  const manifest = parseManifest(source);
  const files = [{ path: 'manifest.json', mediaType: 'application/json', content: `${canonical(manifest)}\n` },
    ...manifest.documents.map(item => ({ path: `documents/${item.key}.md`, mediaType: 'text/markdown',
      documentKey: item.key, content: documentFile(item) }))];
  const catalog = files.map(({ content, ...entry }) => ({ ...entry,
    characters: content.length, bytes: Buffer.byteLength(content, 'utf8'), checksum: digest(content) }));
  return { format: 'publication-archive', version: 1, manifestIdentity: identity(manifest),
    archiveIdentity: identity(catalog), catalog, files: files.map(({ path, content }) => ({ path, content })) };
}

export function decodeArchive(source) {
  let archive = source;
  if (typeof source === 'string') {
    requireValue(source.length <= LIMITS.characters * 4, 'exchange_limit', 'archive is too large');
    try { archive = JSON.parse(source); }
    catch { requireValue(false, 'exchange_json', 'archive is not valid JSON'); }
  }
  object(archive, 'archive');
  requireValue(archive.format === 'publication-archive' && archive.version === 1,
    'exchange_version', 'unsupported archive contract');
  requireValue(Array.isArray(archive.catalog) && Array.isArray(archive.files) &&
    archive.files.length >= 2 && archive.files.length <= LIMITS.documents + 1 &&
    archive.catalog.length === archive.files.length,
  'exchange_archive', 'archive must have matching bounded catalog and files');
  const files = new Map();
  let characters = 0;
  for (const file of archive.files) {
    object(file, 'archive file');
    requireValue(typeof file.path === 'string' && (file.path === 'manifest.json' ||
      /^documents\/[a-zA-Z0-9][a-zA-Z0-9._-]{0,79}\.md$/u.test(file.path)),
    'exchange_archive_path', 'archive path is not a portable document path', { path: file.path });
    requireValue(!files.has(file.path), 'exchange_archive_path', 'archive paths must be unique', { path: file.path });
    requireValue(typeof file.content === 'string', 'exchange_archive', 'file content must be text', { path: file.path });
    characters += file.content.length;
    requireValue(characters <= LIMITS.characters * 3, 'exchange_limit', 'archive content exceeds limit');
    files.set(file.path, file.content);
  }
  requireValue(files.has('manifest.json'), 'exchange_archive', 'archive manifest is missing');
  const manifest = parseManifest(files.get('manifest.json'));
  const expected = renderArchive(manifest);
  requireValue(archive.manifestIdentity === expected.manifestIdentity && archive.archiveIdentity === expected.archiveIdentity,
    'exchange_archive_identity', 'archive identity does not match normalized manifest');
  requireValue(canonical(archive.catalog) === canonical(expected.catalog),
    'exchange_archive_catalog', 'archive catalog does not match manifest contents');
  requireValue(files.size === expected.files.length, 'exchange_archive', 'archive contains unlisted files');
  for (const file of expected.files) {
    requireValue(files.get(file.path) === file.content, 'exchange_archive_content',
      'archive file does not match its manifest', { path: file.path });
  }
  return manifest;
}

export function inspectArchive(_state, input) {
  const manifest = decodeArchive(input.archive);
  const archive = renderArchive(manifest);
  return { manifestIdentity: archive.manifestIdentity, archiveIdentity: archive.archiveIdentity,
    origin: manifest.origin, documents: manifest.documents.length,
    collections: manifest.collections.length, catalog: archive.catalog };
}
