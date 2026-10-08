import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  call(app, 'users.create', { id: 'visitor', displayName: 'Visiting reader' });
  fail(app, 'documents.get', { id: item.id }, 'forbidden', 'visitor');
  assert.equal(call(app, 'documents.list', {}, 'visitor').total, 0);
  assert.deepEqual(call(app, 'collections.list', {}, 'visitor'), []);
  const shared = call(app, 'collections.create', { name: 'Public information', public: true });
  const publicItem = call(app, 'documents.create', { collectionId: shared.id, title: 'Welcome', body: 'Public draft.' });
  assert.equal(call(app, 'documents.get', { id: publicItem.id }, 'visitor').revision.body, 'Public draft.');
  fail(app, 'documents.revise', { id: publicItem.id, body: 'Visitor edit' }, 'forbidden', 'visitor');
}
