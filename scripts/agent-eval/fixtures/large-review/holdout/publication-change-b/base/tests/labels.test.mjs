import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const first = draft(app, collection);
  const second = draft(app, collection);
  assert.deepEqual(call(app, 'labels.set', { documentId: first.id, labels: ['Urgent', 'urgent', 'Support'] }, 'editor').labels, ['support', 'urgent']);
  call(app, 'labels.set', { documentId: second.id, labels: ['support'] }, 'editor');
  assert.equal(call(app, 'labels.find', { label: 'SUPPORT' }, 'reader').length, 2);
  assert.deepEqual(call(app, 'labels.counts', {}, 'reader'), [{ label: 'support', documents: 2 }, { label: 'urgent', documents: 1 }]);
  call(app, 'documents.archive', { id: first.id }, 'editor');
  assert.deepEqual(call(app, 'labels.counts', {}, 'reader'), [{ label: 'support', documents: 1 }]);
  assert.equal(call(app, 'documents.history', { id: second.id }, 'reader').length, 1);
}
