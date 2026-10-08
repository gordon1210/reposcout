import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  const selected = release(app, approve(app, item));
  const job = call(app, 'exports.create', { releaseId: selected.id, requestKey: 'cancel-me' }, 'reader');
  fail(app, 'exports.cancel', { id: job.id }, 'forbidden', 'editor');
  call(app, 'exports.cancel', { id: job.id }, 'reader');
  fail(app, 'exports.run', { id: job.id }, 'job_unavailable', 'reader');
  const fetched = call(app, 'exports.get', { id: job.id }, 'reader');
  assert.equal(fetched.job.status, 'cancelled');
  assert.equal(fetched.artifact, null);
  assert.equal(Object.keys(app.inspect().artifacts).length, 0);
}
