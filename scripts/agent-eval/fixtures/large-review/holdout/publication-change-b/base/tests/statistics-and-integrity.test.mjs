import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'One.' });
  call(app, 'documents.revise', { id: item.id, body: 'Two.' }, 'editor');
  release(app, approve(app, item));
  const stats = call(app, 'workspace.statistics', {}, 'reader');
  assert.equal(stats.collections, 1);
  assert.equal(stats.documents, 1);
  assert.equal(stats.revisions, 2);
  assert.equal(stats.bodyCharacters, 8);
  assert.equal(stats.publications, 1);
  assert.equal(call(app, 'workspace.integrity').healthy, true);
  fail(app, 'workspace.integrity', {}, 'forbidden', 'reader');
}
