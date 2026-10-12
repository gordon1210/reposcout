import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: '# One\nOld\n## Child\nNested\n# Two\nOther\n' });
  const tree = call(app, 'structure.get', { documentId: item.id }, 'reader');
  const guard = { documentId: item.id, expectedRevisionId: item.revision.id, expectedChecksum: item.revision.checksum };
  const operations = [{ kind: 'rename', sectionId: tree.sections[0].id, title: 'Renamed' },
    { kind: 'replace', sectionId: tree.sections[2].id, body: 'New\n' }];
  const before = app.inspect();
  const preview = call(app, 'structure.preview', { ...guard, operations }, 'editor');
  assert.equal(preview.body, '# Renamed\nOld\n## Child\nNested\n# Two\nNew\n');
  assert.deepEqual(app.inspect(), before);
  fail(app, 'structure.apply', { ...guard, operations }, 'forbidden', 'reader');
  fail(app, 'structure.apply', { ...guard, expectedChecksum: 'stale', operations }, 'checksum_conflict', 'editor');
  fail(app, 'structure.apply', { ...guard, operations: [
    { kind: 'delete', sectionId: tree.sections[0].id }, { kind: 'rename', sectionId: tree.sections[1].id, title: 'Conflict' },
  ] }, 'overlapping_operations', 'editor');
  assert.deepEqual(app.inspect(), before);
  const result = call(app, 'structure.apply', { ...guard, operations }, 'editor');
  assert.equal(result.revision.parentId, item.revision.id);
  assert.equal(result.revision.body, preview.body);
  fail(app, 'structure.apply', { ...guard, operations }, 'revision_conflict', 'editor');
  const history = call(app, 'structure.get', { documentId: item.id, revisionId: item.revision.id }, 'reader');
  assert.equal(history.sections[0].title, 'One');
}
