import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const child = call(app, 'collections.create', { name: 'Policies', parentId: collection.id });
  const item = draft(app, child, { body: 'Inherited edit access.' });
  assert.equal(call(app, 'documents.get', { id: item.id }, 'reader').revision.body, 'Inherited edit access.');
  call(app, 'members.assign', { collectionId: child.id, actorId: 'editor', role: 'reader' });
  fail(app, 'documents.revise', { id: item.id, body: 'No longer allowed' }, 'forbidden', 'editor');
  call(app, 'members.remove', { collectionId: child.id, actorId: 'editor' });
  call(app, 'documents.revise', { id: item.id, body: 'Inherited edit restored.' }, 'editor');
  assert.equal(call(app, 'documents.get', { id: item.id }, 'reader').revision.body, 'Inherited edit restored.');
}
