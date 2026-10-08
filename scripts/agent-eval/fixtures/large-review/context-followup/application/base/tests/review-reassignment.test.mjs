import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  const pending = call(app, 'reviews.open', { documentId: item.id, assigneeId: 'reviewer' }, 'editor');
  call(app, 'users.create', { id: 'second-reviewer', displayName: 'Second reviewer', role: 'reviewer' });
  call(app, 'members.assign', { collectionId: collection.id, actorId: 'second-reviewer', role: 'reviewer' });
  call(app, 'reviews.assign', { id: pending.id, assigneeId: 'second-reviewer' });
  fail(app, 'reviews.decide', { id: pending.id, decision: 'approve' }, 'wrong_reviewer', 'reviewer');
  const result = call(app, 'reviews.decide', { id: pending.id, decision: 'approve' }, 'second-reviewer');
  assert.equal(result.approval.reviewerId, 'second-reviewer');
  fail(app, 'reviews.assign', { id: pending.id, assigneeId: 'reviewer' }, 'closed_review');
}
