import assert from 'node:assert/strict';
import { call, workspace, draft } from './support.mjs';
export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'First working copy.' });
  const updated = call(app, 'documents.revise', { id: item.id, body: 'Second working copy.' }, 'editor');
  const ordinary = call(app, 'preview.document', { documentId: item.id }, 'reader');
  assert.equal(Object.hasOwn(ordinary, 'history'), false);
  const expanded = call(app, 'preview.document', { documentId: item.id, includeHistory: true }, 'reader');
  assert.equal(expanded.body, 'Second working copy.');
  assert.deepEqual(expanded.history.map(entry => entry.id), [updated.revision.id, item.revision.id]);
  assert.equal(Object.hasOwn(expanded.history[0], 'body'), false);
}
