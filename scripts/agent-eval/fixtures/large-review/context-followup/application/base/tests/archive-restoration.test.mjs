import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  call(app, 'documents.archive', { id: item.id }, 'editor');
  assert.equal(call(app, 'documents.list', {}, 'reader').total, 0);
  assert.equal(call(app, 'documents.list', { includeArchived: true }, 'reader').total, 1);
  fail(app, 'documents.get', { id: item.id }, 'archived_document', 'reader');
  assert.equal(call(app, 'documents.get', { id: item.id, includeArchived: true }, 'reader').archived, true);
  call(app, 'documents.archive', { id: item.id, archived: false }, 'editor');
  assert.equal(call(app, 'documents.get', { id: item.id }, 'reader').archived, false);
  call(app, 'reviews.open', { documentId: item.id, assigneeId: 'reviewer' }, 'editor');
  fail(app, 'documents.archive', { id: item.id }, 'open_review', 'editor');
}
