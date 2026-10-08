import assert from 'node:assert/strict';
import { call, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const initial = draft(app, collection, { body: '# Operators\nVerify 😀 pressure.\n# Equipment\nMaintain valves.\n' });
  const comment = call(app, 'comments.add', { documentId: initial.id, line: 2, body: 'Which threshold?' }, 'reviewer');
  const anchor = call(app, 'structure.anchors.capture', { documentId: initial.id, commentId: comment.id,
    expectedChecksum: initial.revision.checksum, expectedQuote: 'Verify 😀 pressure.\n' }, 'reviewer');
  const structure = call(app, 'structure.get', { documentId: initial.id }, 'reader');
  const operations = [{ kind: 'moveAfter', sectionId: structure.sections[0].id, destinationId: structure.sections[1].id }];
  const before = app.inspect();
  const proposed = call(app, 'structure.anchors.preview', { documentId: initial.id, anchorIds: [anchor.id],
    expectedRevisionId: initial.revision.id, expectedChecksum: initial.revision.checksum,
    target: { kind: 'sections', operations } }, 'editor');
  assert.equal(proposed.target.committable, false);
  assert.equal(proposed.mappings[0].status, 'relocated');
  assert.equal(proposed.mappings[0].target.startLine, 4);
  assert.deepEqual(app.inspect(), before);
  const edited = call(app, 'structure.apply', { documentId: initial.id, expectedRevisionId: initial.revision.id,
    expectedChecksum: initial.revision.checksum, operations }, 'editor');
  const compared = call(app, 'structure.compare', { documentId: initial.id, fromRevisionId: initial.revision.id }, 'reader');
  assert.equal(compared.sections[0].status, 'relocated');
  const followup = call(app, 'structure.anchors.followup', { documentId: initial.id, expectedRevisionId: edited.revision.id,
    expectedChecksum: edited.revision.checksum, entries: [{ anchorId: anchor.id, body: 'Question still applies.' }] }, 'reviewer')[0];
  assert.equal(followup.comment.line, 4);
  assert.equal(followup.link.sourceRevisionId, initial.revision.id);
  assert.equal(followup.link.targetRange.quote, 'Verify 😀 pressure.\n');
  assert.deepEqual(app.inspect().comments[comment.id], comment);
  assert.deepEqual(call(app, 'structure.anchors.followups', { anchorId: anchor.id }, 'reader'), [followup.link]);
}
