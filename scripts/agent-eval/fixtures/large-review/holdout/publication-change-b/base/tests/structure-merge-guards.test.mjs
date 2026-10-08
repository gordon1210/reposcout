import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'one\ntwo\nthree\n' });
  const opened = call(app, 'structure.merge.open', { documentId: item.id, baseRevisionId: item.revision.id,
    expectedRevisionId: item.revision.id, expectedChecksum: item.revision.checksum, body: 'replacement\n' }, 'editor');
  call(app, 'documents.revise', { id: item.id, body: 'concurrent\n' }, 'editor');
  const before = app.inspect();
  fail(app, 'structure.merge.commit', { id: opened.id, expectedVersion: 1 }, 'revision_conflict', 'editor');
  assert.deepEqual(app.inspect(), before);
  const abandoned = call(app, 'structure.merge.abandon', { id: opened.id, expectedVersion: 1 }, 'editor');
  assert.equal(abandoned.status, 'abandoned');
  fail(app, 'structure.merge.resolve', { id: opened.id, expectedVersion: 2 }, 'merge_closed', 'editor');
  const other = draft(app, collection);
  fail(app, 'structure.get', { documentId: item.id, revisionId: other.revision.id }, 'revision_mismatch', 'reader');
  const foreign = call(app, 'collections.create', { name: 'Restricted' });
  const secret = call(app, 'documents.create', { collectionId: foreign.id, title: 'Secret', body: 'private' });
  fail(app, 'structure.get', { documentId: secret.id }, 'forbidden', 'reader');
}
