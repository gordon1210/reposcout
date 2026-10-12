import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const child = call(app, 'collections.create', { name: 'Engineering', parentId: collection.id });
  const leaf = call(app, 'collections.create', { name: 'On call', parentId: child.id });
  const root = call(app, 'collections.list').find(item => item.id === collection.id);
  assert.equal(root.descendants, 2);
  const view = call(app, 'collections.list', {}, 'reader').find(item => item.id === leaf.id);
  assert.deepEqual(view.path.map(item => item.name), ['Operations', 'Engineering', 'On call']);
  assert.equal(view.public, false);
  assert.equal(call(app, 'collections.list', { parentId: child.id }, 'reader').length, 1);
}
