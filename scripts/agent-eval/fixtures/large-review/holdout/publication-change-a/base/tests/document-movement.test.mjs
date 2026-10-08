import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  const target = call(app, 'collections.create', { name: 'Restricted' });
  fail(app, 'documents.move', { id: item.id, collectionId: target.id }, 'forbidden', 'editor');
  assert.equal(call(app, 'documents.get', { id: item.id }).collectionId, collection.id);
  call(app, 'documents.move', { id: item.id, collectionId: target.id });
  fail(app, 'documents.get', { id: item.id }, 'forbidden', 'reader');
  assert.equal(call(app, 'documents.get', { id: item.id }).collectionId, target.id);
  call(app, 'documents.owner', { id: item.id, ownerId: 'reviewer' });
  assert.equal(call(app, 'documents.get', { id: item.id }).ownerId, 'reviewer');
}
