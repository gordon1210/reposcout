import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'One line.' });
  fail(app, 'comments.add', { documentId: item.id, line: 2, body: 'Outside body.' }, 'invalid_input', 'reader');
  const comment = call(app, 'comments.add', { documentId: item.id, body: 'Reader question.' }, 'reader');
  assert.equal(call(app, 'comments.list', { documentId: item.id }, 'reader').length, 1);
  fail(app, 'comments.resolve', { id: comment.id }, 'forbidden', 'reader');
  call(app, 'comments.resolve', { id: comment.id }, 'editor');
  assert.equal(call(app, 'comments.list', { documentId: item.id }, 'reader').length, 0);
  assert.equal(call(app, 'comments.list', { documentId: item.id, includeResolved: true }, 'reader').length, 1);
  call(app, 'comments.resolve', { id: comment.id, resolved: false }, 'editor');
  assert.equal(call(app, 'comments.list', { documentId: item.id }, 'reader').length, 1);
}
