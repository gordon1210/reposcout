import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  call(app, 'watches.add', { documentId: item.id, mode: 'publication' }, 'reader');
  call(app, 'documents.revise', { id: item.id, body: 'Changed text.' }, 'editor');
  assert.deepEqual(call(app, 'digest.preview', {}, 'reader').groups, []);
  release(app, approve(app, item));
  const digest = call(app, 'digest.preview', {}, 'reader');
  assert.equal(digest.groups.length, 1);
  assert.equal(digest.groups[0].events.length, 1);
  assert.equal(digest.groups[0].events[0].action, 'release.published');
}
