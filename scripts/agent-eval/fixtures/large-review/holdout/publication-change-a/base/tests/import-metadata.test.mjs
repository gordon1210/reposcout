import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const source = '---\ntitle: Escalation guide\nlanguage: de\ntags: support, operations\n---\n\n# Contact\nCall the team.\n';
  const preview = call(app, 'import.preview', { collectionId: collection.id, source }, 'editor');
  assert.equal(preview.title, 'Escalation guide');
  assert.equal(preview.language, 'de');
  assert.deepEqual(preview.tags, ['support', 'operations']);
  assert.equal(call(app, 'documents.list').total, 0);
  const item = call(app, 'import.apply', { collectionId: collection.id, source }, 'editor');
  assert.equal(item.revision.body, '# Contact\nCall the team.\n');
  assert.equal(item.revision.title, 'Escalation guide');
  assert.equal(call(app, 'documents.list').total, 1);
}
