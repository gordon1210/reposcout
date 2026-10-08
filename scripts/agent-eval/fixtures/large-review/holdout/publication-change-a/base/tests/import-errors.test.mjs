import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  for (const source of ['No heading or title', '---\ntitle: Unclosed', '---\nunknown: x\n---\n# Heading', '---\ntitle: A\ntitle: B\n---\nBody']) {
    fail(app, 'import.apply', { collectionId: collection.id, source }, 'invalid_import', 'editor');
  }
  assert.equal(call(app, 'documents.list').total, 0);
  const item = call(app, 'import.apply', { collectionId: collection.id, source: '# Heading\r\nFirst body.' }, 'editor');
  const revised = call(app, 'import.apply', { collectionId: collection.id, documentId: item.id,
    source: '# Revised\nSecond body.', expectedRevisionId: item.revision.id }, 'editor');
  assert.equal(revised.revision.title, 'Revised');
  assert.equal(revised.revision.parentId, item.revision.id);
}
