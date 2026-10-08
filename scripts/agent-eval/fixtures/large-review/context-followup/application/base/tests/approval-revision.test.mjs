import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'Reviewed body.' });
  const review = call(app, 'reviews.open', { documentId: item.id, assigneeId: 'reviewer' }, 'editor');
  call(app, 'documents.revise', { id: item.id, body: 'Later working body.' }, 'editor');
  const decision = call(app, 'reviews.decide', { id: review.id, decision: 'approve' }, 'reviewer');
  assert.equal(decision.approval.revisionId, item.revision.id);
  assert.equal(decision.review.status, 'approved');
  fail(app, 'reviews.decide', { id: review.id, decision: 'reject' }, 'closed_review', 'reviewer');
  assert.equal(call(app, 'reviews.queue', { status: 'open' }, 'reviewer').total, 0);
  assert.equal(call(app, 'reviews.queue', { status: 'approved' }, 'reviewer').total, 1);
}
