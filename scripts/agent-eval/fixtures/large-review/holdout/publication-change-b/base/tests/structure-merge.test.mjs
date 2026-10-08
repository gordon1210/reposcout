import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const initial = draft(app, collection, { body: 'Alpha\nMiddle\nOmega\n' });
  const current = call(app, 'documents.revise', { id: initial.id, body: 'Alpha edited\nMiddle\nOmega\n' }, 'editor');
  const merge = call(app, 'structure.merge.open', { documentId: initial.id, baseRevisionId: initial.revision.id,
    expectedRevisionId: current.revision.id, expectedChecksum: current.revision.checksum,
    body: 'Alpha\nMiddle\nOmega edited\n' }, 'editor');
  assert.equal(merge.plan.clean, true);
  assert.equal(merge.plan.body, 'Alpha edited\nMiddle\nOmega edited\n');
  const committed = call(app, 'structure.merge.commit', { id: merge.id, expectedVersion: merge.version }, 'editor');
  assert.equal(committed.document.revision.body, merge.plan.body);
  assert.equal(committed.document.revision.parentId, current.revision.id);
  assert.equal(committed.merge.status, 'committed');
  fail(app, 'structure.merge.commit', { id: merge.id, expectedVersion: committed.merge.version }, 'merge_closed', 'editor');

  const conflict = call(app, 'structure.merge.open', { documentId: initial.id, baseRevisionId: initial.revision.id,
    expectedRevisionId: committed.document.revision.id, expectedChecksum: committed.document.revision.checksum,
    body: 'Alpha incoming\nMiddle\nOmega\n' }, 'editor');
  assert.equal(conflict.plan.conflicts.length, 1);
  const before = app.inspect();
  fail(app, 'structure.merge.commit', { id: conflict.id, expectedVersion: 1 }, 'unresolved_conflict', 'editor');
  assert.deepEqual(app.inspect(), before);
  const resolved = call(app, 'structure.merge.resolve', { id: conflict.id, expectedVersion: 1,
    conflictId: conflict.plan.conflicts[0].id, choice: 'custom', body: 'Alpha reconciled\n' }, 'editor');
  fail(app, 'structure.merge.commit', { id: conflict.id, expectedVersion: 1 }, 'merge_version_conflict', 'editor');
  const final = call(app, 'structure.merge.commit', { id: conflict.id, expectedVersion: resolved.version }, 'editor');
  assert.equal(final.document.revision.body, 'Alpha reconciled\nMiddle\nOmega edited\n');
}
