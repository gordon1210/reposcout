import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: 'Protected approved text.' });
  release(app, approve(app, item));
  const discard = call(app, 'documents.revise', { id: item.id, body: 'Transient draft.' }, 'editor');
  const current = call(app, 'documents.revise', { id: item.id, body: 'Current draft.' }, 'editor');
  call(app, 'retention.set', { collectionId: collection.id, keepRecent: 1 });
  const preview = call(app, 'retention.preview', { collectionId: collection.id });
  assert.deepEqual(preview.candidates.map(entry => entry.revisionId), [discard.revision.id]);
  assert.equal(call(app, 'retention.apply', { collectionId: collection.id }).removed, 1);
  assert.deepEqual(call(app, 'documents.history', { id: item.id }, 'reader').map(entry => entry.id), [current.revision.id, item.revision.id]);
  assert.equal(call(app, 'workspace.integrity').healthy, true);
}
