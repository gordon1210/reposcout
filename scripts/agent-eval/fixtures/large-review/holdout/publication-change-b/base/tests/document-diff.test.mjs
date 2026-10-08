import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { title: 'Before', body: 'Heading\nOld text\nEnd', tags: ['old', 'stable'] });
  const next = call(app, 'documents.revise', { id: item.id, title: 'After', body: 'Heading\nNew text\nEnd', tags: ['new', 'stable'] }, 'editor');
  const diff = call(app, 'documents.diff', { id: item.id, before: item.revision.id, after: next.revision.id }, 'reader');
  assert.equal(diff.titleChanged, true);
  assert.deepEqual(diff.body, { startLine: 2, removed: ['Old text'], added: ['New text'] });
  assert.deepEqual(diff.tagsAdded, ['new']);
  assert.deepEqual(diff.tagsRemoved, ['old']);
  const other = draft(app, collection);
  fail(app, 'documents.diff', { id: item.id, before: item.revision.id, after: other.revision.id }, 'revision_mismatch', 'reader');
}
