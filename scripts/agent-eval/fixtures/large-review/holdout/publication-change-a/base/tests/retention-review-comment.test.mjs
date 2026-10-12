import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'Commented version.' });
  call(app, 'comments.add', { documentId: item.id, body: 'Keep this context.' }, 'reader');
  const reviewed = call(app, 'documents.revise', { id: item.id, body: 'Under review.' }, 'editor');
  call(app, 'reviews.open', { documentId: item.id, assigneeId: 'reviewer' }, 'editor');
  call(app, 'documents.revise', { id: item.id, body: 'Working version.' }, 'editor');
  call(app, 'retention.set', { collectionId: collection.id, keepRecent: 1 });
  assert.deepEqual(call(app, 'retention.preview', { collectionId: collection.id }).candidates, []);
  assert.equal(call(app, 'documents.get', { id: item.id, revisionId: reviewed.revision.id }, 'reader').revision.body, 'Under review.');
}
