import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'Discoverable handbook.' });
  const selected = release(app, approve(app, item));
  const share = call(app, 'shares.create', { releaseId: selected.id }, 'reviewer');
  const queued = call(app, 'exports.create', { releaseId: selected.id, requestKey: 'queued' }, 'reader');
  call(app, 'search.rebuild', { releaseId: selected.id }, 'reader');
  call(app, 'releases.withdraw', { id: selected.id, reason: 'Superseded by a new policy.' }, 'reviewer');
  assert.equal(call(app, 'releases.list', {}, 'reader').total, 0);
  assert.equal(call(app, 'releases.list', { includeWithdrawn: true }, 'reader').total, 1);
  fail(app, 'shares.read', { token: share.token }, 'share_unavailable');
  fail(app, 'exports.run', { id: queued.id }, 'withdrawn_release', 'reader');
  assert.equal(call(app, 'search.query', { query: 'handbook' }, 'reader').total, 0);
}
