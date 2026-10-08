import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  const selected = release(app, approve(app, item));
  const share = call(app, 'shares.create', { releaseId: selected.id }, 'reviewer');
  fail(app, 'shares.revoke', { id: share.id }, 'forbidden', 'reader');
  assert.equal(call(app, 'shares.read', { token: share.token }, 'external').documents.length, 1);
  call(app, 'shares.revoke', { id: share.id }, 'reviewer');
  fail(app, 'shares.read', { token: share.token }, 'share_unavailable', 'external');
  fail(app, 'shares.create', { releaseId: selected.id }, 'forbidden', 'reader');
}
