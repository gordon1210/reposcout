import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: '# A\nAlpha\n## Nested\nChild\n# B\nBeta\n# C\nGamma\n' });
  const tree = call(app, 'structure.get', { documentId: item.id }, 'editor');
  const guard = { documentId: item.id, expectedRevisionId: item.revision.id, expectedChecksum: item.revision.checksum };
  const preview = call(app, 'structure.preview', { ...guard, operations: [
    { kind: 'moveAfter', sectionId: tree.sections[0].id, destinationId: tree.sections[3].id },
  ] }, 'editor');
  assert.equal(preview.body, '# B\nBeta\n# C\nGamma\n# A\nAlpha\n## Nested\nChild\n');
  fail(app, 'structure.preview', { ...guard, operations: [
    { kind: 'moveAfter', sectionId: tree.sections[0].id, destinationId: tree.sections[1].id },
  ] }, 'invalid_section_move', 'editor');
  const shifted = call(app, 'structure.preview', { ...guard, operations: [
    { kind: 'shiftLevel', sectionId: tree.sections[0].id, level: 2 },
  ] }, 'editor');
  assert.equal(shifted.body, '## A\nAlpha\n### Nested\nChild\n# B\nBeta\n# C\nGamma\n');
  fail(app, 'structure.preview', { ...guard, operations: [
    { kind: 'shiftLevel', sectionId: tree.sections[0].id, level: 6 },
  ] }, 'invalid_heading_level', 'editor');
  const inserted = call(app, 'structure.preview', { ...guard, operations: [
    { kind: 'insertAfter', sectionId: tree.sections[2].id, body: '# Added\nNew' },
  ] }, 'editor');
  assert.equal(inserted.body, '# A\nAlpha\n## Nested\nChild\n# B\nBeta\n# Added\nNew\n# C\nGamma\n');
}
