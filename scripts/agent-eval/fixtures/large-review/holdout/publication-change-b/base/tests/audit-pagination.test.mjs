import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  call(app, 'documents.revise', { id: item.id, body: 'Edited.' }, 'editor');
  const first = call(app, 'audit.list', { target: item.id, limit: 1 });
  assert.equal(first.total, 2);
  assert.equal(first.events[0].action, 'document.created');
  const second = call(app, 'audit.list', { target: item.id, limit: 1, afterSequence: first.nextSequence });
  assert.equal(second.events[0].action, 'document.revised');
  assert.ok(second.nextSequence > first.nextSequence);
  fail(app, 'audit.list', {}, 'forbidden', 'reader');
  const csv = call(app, 'audit.export', { target: item.id, format: 'csv' });
  assert.ok(csv.content.includes('document.created'));
  assert.ok(csv.content.includes('document.revised'));
}
