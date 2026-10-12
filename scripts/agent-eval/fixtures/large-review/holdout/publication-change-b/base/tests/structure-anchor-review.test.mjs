import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const initial = draft(app, collection, { body: '# Guide\nCheck valves.\n' });
  const originalReview = call(app, 'reviews.open', { documentId: initial.id, assigneeId: 'reviewer' }, 'editor');
  const comment = call(app, 'comments.add', { documentId: initial.id, reviewId: originalReview.id,
    line: 2, body: 'Confirm valve count' }, 'reviewer');
  const anchor = call(app, 'structure.anchors.capture', { documentId: initial.id, commentId: comment.id,
    expectedChecksum: initial.revision.checksum }, 'reviewer');
  call(app, 'comments.resolve', { id: comment.id }, 'editor');
  const decision = call(app, 'reviews.decide', { id: originalReview.id, decision: 'approve', note: 'Count checked' }, 'reviewer');
  const current = call(app, 'documents.revise', { id: initial.id, body: 'Preface\n# Guide\nCheck valves.\n' }, 'editor');
  const targetReview = call(app, 'reviews.open', { documentId: initial.id, assigneeId: 'reviewer' }, 'editor');
  const request = { documentId: initial.id, expectedRevisionId: current.revision.id, expectedChecksum: current.revision.checksum };
  fail(app, 'structure.anchors.followup', { ...request, entries: [{ anchorId: anchor.id, reviewId: originalReview.id,
    body: 'Recheck in revised context' }] }, 'review_mismatch', 'reviewer');
  const created = call(app, 'structure.anchors.followup', { ...request, entries: [{ anchorId: anchor.id,
    reviewId: targetReview.id, body: 'Recheck in revised context' }] }, 'reviewer')[0];
  assert.equal(created.comment.reviewId, targetReview.id);
  assert.equal(created.comment.revisionId, current.revision.id);
  assert.equal(created.link.sourceReviewId, originalReview.id);
  assert.equal(app.inspect().approvals[decision.approval.id].revisionId, initial.revision.id);
  assert.equal(app.inspect().reviews[originalReview.id].status, 'approved');
  assert.equal(call(app, 'structure.anchors.get', { id: anchor.id }, 'reader').commentSnapshot.resolved, false);
  fail(app, 'reviews.decide', { id: targetReview.id, decision: 'approve' }, 'unresolved_comments', 'reviewer');
}
