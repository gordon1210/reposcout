import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const original = draft(app, collection, { body: '# Start\nInspect valve\n# End\nFinish\n' });
  const anchor = call(app, 'structure.anchors.capture', { documentId: original.id, startLine: 2,
    expectedChecksum: original.revision.checksum }, 'reader');
  const current = call(app, 'documents.revise', { id: original.id, body: 'Preface\n# Start\nInspect valve\n# End\nFinish\n' }, 'editor');
  const merge = call(app, 'structure.merge.open', { documentId: original.id, baseRevisionId: original.revision.id,
    expectedRevisionId: current.revision.id, expectedChecksum: current.revision.checksum,
    body: '# Start\nInspect valve\n# End\nFinish safely\n' }, 'editor');
  const request = { documentId: original.id, anchorIds: [anchor.id], expectedRevisionId: current.revision.id,
    expectedChecksum: current.revision.checksum, target: { kind: 'merge', id: merge.id, expectedVersion: merge.version } };
  const preview = call(app, 'structure.anchors.preview', request, 'editor');
  assert.equal(preview.target.committable, false);
  assert.equal(preview.mappings[0].target.startLine, 3);
  assert.equal(preview.mappings[0].status, 'relocated');
  fail(app, 'structure.anchors.preview', { ...request, target: { ...request.target, expectedVersion: 99 } },
    'merge_version_conflict', 'editor');
  const committed = call(app, 'structure.merge.commit', { id: merge.id, expectedVersion: merge.version }, 'editor');
  const followup = call(app, 'structure.anchors.followup', { documentId: original.id,
    expectedRevisionId: committed.document.revision.id, expectedChecksum: committed.document.revision.checksum,
    entries: [{ anchorId: anchor.id, body: 'Check in the merged context' }] }, 'reader')[0];
  assert.equal(followup.comment.line, 3);
  assert.equal(followup.link.targetRevisionId, committed.document.revision.id);
}
