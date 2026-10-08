import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app } = workspace();
  fail(app, 'users.create', { id: 'outsider', displayName: 'Outsider' }, 'forbidden', 'reader');
  call(app, 'users.create', { id: 'temporary', displayName: 'Temporary reader' });
  fail(app, 'users.create', { id: 'temporary', displayName: 'Duplicate' }, 'duplicate_user');
  call(app, 'users.active', { id: 'temporary', active: false });
  fail(app, 'collections.list', {}, 'inactive_actor', 'temporary');
  assert.equal(call(app, 'users.list').some(item => item.id === 'temporary'), false);
  assert.equal(call(app, 'users.list', { includeInactive: true }).some(item => item.id === 'temporary'), true);
  fail(app, 'users.active', { id: 'admin', active: false }, 'self_disable');
  call(app, 'users.active', { id: 'temporary', active: true });
  assert.deepEqual(call(app, 'collections.list', {}, 'temporary'), []);
}
