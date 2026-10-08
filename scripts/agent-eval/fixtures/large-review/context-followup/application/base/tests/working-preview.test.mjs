import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'Initial text.' });
  const approved = approve(app, item);
  release(app, approved);
  const next = call(app, 'documents.revise', { id: item.id, body: 'Latest working text.' }, 'editor');
  assert.equal(call(app, 'preview.document', { documentId: item.id }, 'reader').body, 'Latest working text.');
  const bundle = call(app, 'preview.bundle', { documentIds: [item.id], format: 'json' }, 'reader');
  assert.equal(bundle.preview, true);
  assert.equal(bundle.documents[0].revisionId, next.revision.id);
  assert.equal(JSON.parse(bundle.content).documents[0].body, 'Latest working text.');
  assert.equal(Object.keys(app.inspect().artifacts).length, 0);
}
