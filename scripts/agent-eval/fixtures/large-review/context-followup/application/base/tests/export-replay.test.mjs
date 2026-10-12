import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  const selected = release(app, approve(app, item));
  const input = { releaseId: selected.id, requestKey: 'repeat', format: 'json' };
  const first = call(app, 'exports.create', input, 'reader');
  assert.equal(call(app, 'exports.create', input, 'reader').id, first.id);
  const completed = call(app, 'exports.run', { id: first.id }, 'reader');
  const again = call(app, 'exports.run', { id: first.id }, 'reader');
  assert.equal(completed.artifact.id, again.artifact.id);
  assert.equal(again.job.attempts, 1);
  fail(app, 'exports.create', { ...input, format: 'text' }, 'request_conflict', 'reader');
  assert.equal(call(app, 'exports.list', {}, 'reader').total, 1);
}
