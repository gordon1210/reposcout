import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const child = call(app, 'collections.create', { name: 'Delivery', parentId: collection.id });
  const other = call(app, 'collections.create', { name: 'Reference' });
  fail(app, 'collections.move', { id: collection.id, parentId: child.id }, 'collection_cycle');
  assert.equal(call(app, 'collections.list').find(item => item.id === collection.id).parentId, null);
  call(app, 'collections.move', { id: child.id, parentId: other.id });
  assert.equal(call(app, 'collections.list').find(item => item.id === child.id).parentId, other.id);
  fail(app, 'collections.create', { name: 'Delivery', parentId: other.id }, 'duplicate_collection');
}
