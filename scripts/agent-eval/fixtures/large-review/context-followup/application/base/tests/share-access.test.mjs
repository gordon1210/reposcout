import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  const selected = release(app, approve(app, item));
  const share = call(app, 'shares.create', { releaseId: selected.id, maxViews: 2 }, 'reviewer');
  const response = call(app, 'shares.read', { token: share.token }, 'external');
  assert.equal(response.documents[0].body, item.revision.body);
  assert.equal(response.releaseId, selected.id);
  call(app, 'shares.read', { token: share.token }, 'external');
  fail(app, 'shares.read', { token: share.token }, 'share_unavailable', 'external');
  fail(app, 'shares.read', { token: 'incorrect-token' }, 'not_found', 'external');
}
