import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  call(app, 'watches.add', { documentId: item.id }, 'reader');
  call(app, 'documents.revise', { id: item.id, title: 'Current handbook', body: 'Changed working copy.' }, 'editor');
  const preview = call(app, 'digest.preview', {}, 'reader');
  assert.equal(preview.groups.length, 1);
  assert.equal(preview.groups[0].title, 'Current handbook');
  assert.equal(preview.groups[0].events[0].action, 'document.revised');
  const sent = call(app, 'digest.deliver', {}, 'reader');
  assert.deepEqual(sent.groups, preview.groups);
  assert.deepEqual(call(app, 'digest.preview', {}, 'reader').groups, []);
  call(app, 'watches.remove', { documentId: item.id }, 'reader');
  assert.deepEqual(call(app, 'watches.list', {}, 'reader'), []);
}
