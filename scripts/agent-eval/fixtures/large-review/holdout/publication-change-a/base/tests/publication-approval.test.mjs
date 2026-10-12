import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const first = draft(app, collection, { title: 'First' });
  const second = draft(app, collection, { title: 'Second' });
  const approved = approve(app, first);
  fail(app, 'releases.publish', { name: 'Incorrect identity', entries: [{ approvalId: approved.id, documentId: second.id }] }, 'approval_mismatch', 'reviewer');
  fail(app, 'releases.publish', { name: 'Duplicate', entries: [{ approvalId: approved.id }, { approvalId: approved.id }] }, 'duplicate_document', 'reviewer');
  const result = release(app, approved);
  assert.equal(result.entries[0].revisionId, first.revision.id);
  assert.equal(result.entries[0].checksum, first.revision.checksum);
  assert.equal(call(app, 'releases.list', {}, 'reader').total, 1);
  assert.equal(call(app, 'releases.get', { id: result.id }, 'reader').name, 'Operational release');
}
