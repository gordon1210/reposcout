import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'Heading\nCheck this instruction.' });
  const pending = call(app, 'reviews.open', { documentId: item.id, assigneeId: 'reviewer' }, 'editor');
  const comment = call(app, 'comments.add', { documentId: item.id, reviewId: pending.id,
    line: 2, body: 'Clarify the operator responsibility.' }, 'reviewer');
  assert.equal(call(app, 'reviews.get', { id: pending.id }, 'editor').openComments, 1);
  fail(app, 'reviews.decide', { id: pending.id, decision: 'approve' }, 'unresolved_comments', 'reviewer');
  call(app, 'comments.resolve', { id: comment.id }, 'editor');
  assert.equal(call(app, 'reviews.get', { id: pending.id }, 'reader').openComments, 0);
  assert.equal(call(app, 'reviews.decide', { id: pending.id, decision: 'approve' }, 'reviewer').review.status, 'approved');
}
