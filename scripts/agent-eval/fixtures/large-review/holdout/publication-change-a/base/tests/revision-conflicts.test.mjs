import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'First.' });
  const second = call(app, 'documents.revise', { id: item.id, body: 'Second.', expectedRevisionId: item.currentRevisionId }, 'editor');
  const before = app.inspect();
  fail(app, 'documents.revise', { id: item.id, body: 'Stale edit.', expectedRevisionId: item.currentRevisionId }, 'revision_conflict', 'editor');
  assert.deepEqual(app.inspect(), before);
  assert.equal(call(app, 'documents.get', { id: item.id }, 'reader').revision.body, 'Second.');
  assert.equal(second.revision.parentId, item.currentRevisionId);
  assert.equal(call(app, 'documents.get', { id: item.id, revisionId: item.currentRevisionId }, 'reader').revision.body, 'First.');
}
