import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  const prior = app.inspect();
  fail(app, 'documents.revise', { id: item.id, title: ' ', body: 'Should not survive.' }, 'invalid_input', 'editor');
  assert.deepEqual(app.inspect(), prior);
  const response = call(app, 'documents.get', { id: item.id }, 'reader');
  response.revision.tags.push('external mutation');
  assert.deepEqual(call(app, 'documents.get', { id: item.id }, 'reader').revision.tags, ['operations']);
  const view = app.inspect();
  view.documents[item.id].archived = true;
  assert.equal(call(app, 'documents.get', { id: item.id }, 'reader').archived, false);
  fail(app, '__proto__', {}, 'unknown_action');
}
