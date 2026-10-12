import { requireValue } from '../core/errors.mjs';
import { text } from '../core/validation.mjs';

export function parseImport(source) {
  text(source, 'import text', { max: 200000 });
  const normalized = source.replaceAll('\r\n', '\n');
  const lines = normalized.split('\n');
  const metadata = {};
  let offset = 0;
  if (lines[0] === '---') {
    offset = lines.indexOf('---', 1);
    requireValue(offset >= 0, 'invalid_import', 'metadata header is not closed');
    for (const line of lines.slice(1, offset)) {
      const colon = line.indexOf(':');
      requireValue(colon > 0, 'invalid_import', 'metadata entries use key: value syntax');
      const key = line.slice(0, colon).trim();
      requireValue(['title', 'language', 'tags'].includes(key), 'invalid_import', 'unknown metadata field');
      requireValue(metadata[key] === undefined, 'invalid_import', 'duplicate metadata field');
      metadata[key] = line.slice(colon + 1).trim();
    }
    offset += 1;
  }
  const body = lines.slice(offset).join('\n').replace(/^\n/u, '');
  const heading = body.split('\n').find(line => line.startsWith('# '));
  const title = metadata.title || heading?.slice(2);
  requireValue(title, 'invalid_import', 'import needs a title or a top-level heading');
  return { title, body, language: metadata.language || 'en',
    tags: metadata.tags ? metadata.tags.split(',').map(tag => tag.trim()).filter(Boolean) : [] };
}

export function importSummary(item) {
  return { title: item.title, language: item.language, tags: item.tags,
    characters: item.body.length, lines: item.body.split('\n').length };
}
