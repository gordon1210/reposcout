import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  fail(app, 'documents.create', { collectionId: collection.id, title: ' ', body: 'body' }, 'invalid_input', 'editor');
  assert.equal(call(app, 'documents.list').total, 0);
  const item = draft(app, collection, { title: 'Valid empty body', body: '', tags: ['reference', 'reference'] });
  assert.equal(item.revision.body, '');
  assert.deepEqual(item.revision.tags, ['reference']);
  assert.equal(item.id, 'document_0001');
  item.revision.body = 'Caller mutation';
  assert.equal(call(app, 'documents.get', { id: item.id }, 'reader').revision.body, '');
  fail(app, 'documents.create', { collectionId: 'missing', title: 'Title', body: '' }, 'not_found');
}
