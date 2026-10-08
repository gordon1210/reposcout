import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  const pending = call(app, 'reviews.open', { documentId: item.id, assigneeId: 'reviewer' }, 'editor');
  const result = call(app, 'reviews.decide', { id: pending.id, decision: 'reject', note: 'Missing contact details.' }, 'reviewer');
  assert.equal(result.approval, null);
  assert.equal(result.review.status, 'rejected');
  assert.equal(result.review.decisionNote, 'Missing contact details.');
  assert.deepEqual(Object.values(app.inspect().approvals), []);
  call(app, 'documents.revise', { id: item.id, body: 'Contact details included.' }, 'editor');
  assert.equal(approve(app, item).documentId, item.id);
}
