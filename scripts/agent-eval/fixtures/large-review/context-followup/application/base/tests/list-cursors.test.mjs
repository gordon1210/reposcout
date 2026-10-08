import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const first = draft(app, collection, { title: 'First' });
  const second = draft(app, collection, { title: 'Second' });
  const third = draft(app, collection, { title: 'Third' });
  const firstPage = call(app, 'documents.list', { limit: 2 }, 'reader');
  assert.deepEqual(firstPage.items.map(item => item.id), [first.id, second.id]);
  assert.equal(firstPage.next, second.id);
  const secondPage = call(app, 'documents.list', { limit: 2, after: firstPage.next }, 'reader');
  assert.deepEqual(secondPage.items.map(item => item.id), [third.id]);
  assert.equal(secondPage.next, null);
  fail(app, 'documents.list', { after: 'absent' }, 'invalid_cursor', 'reader');
  fail(app, 'documents.list', { limit: 0 }, 'invalid_input', 'reader');
}
