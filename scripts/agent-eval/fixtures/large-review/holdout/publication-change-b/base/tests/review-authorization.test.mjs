import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  fail(app, 'reviews.open', { documentId: item.id, assigneeId: 'reader' }, 'forbidden', 'editor');
  call(app, 'members.assign', { collectionId: collection.id, actorId: 'editor', role: 'owner' });
  fail(app, 'reviews.open', { documentId: item.id, assigneeId: 'editor' }, 'self_approval', 'editor');
  const pending = call(app, 'reviews.open', { documentId: item.id, assigneeId: 'reviewer' }, 'editor');
  fail(app, 'reviews.open', { documentId: item.id, assigneeId: 'reviewer' }, 'duplicate_review', 'editor');
  fail(app, 'reviews.decide', { id: pending.id, decision: 'approve' }, 'wrong_reviewer');
  assert.equal(call(app, 'reviews.get', { id: pending.id }, 'reader').status, 'open');
}
