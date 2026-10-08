import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const initial = draft(app, collection, { body: '# Guide\nOld procedure\nStable ending\n' });
  const anchor = call(app, 'structure.anchors.capture', { documentId: initial.id, startLine: 2,
    expectedChecksum: initial.revision.checksum }, 'reader');
  const edited = call(app, 'documents.revise', { id: initial.id, body: '# Guide\nNew procedure\nStable ending\n' }, 'editor');
  const preview = call(app, 'structure.anchors.preview', { documentId: initial.id, anchorIds: [anchor.id] }, 'reader');
  assert.equal(preview.mappings[0].status, 'edited');
  assert.equal(preview.mappings[0].exact, false);
  assert.equal(preview.mappings[0].candidates[0].startLine, 2);
  const request = { documentId: initial.id, expectedRevisionId: edited.revision.id, expectedChecksum: edited.revision.checksum };
  fail(app, 'structure.anchors.followup', { ...request, entries: [{ anchorId: anchor.id, body: 'Still unclear' }] }, 'anchor_not_exact', 'reader');
  fail(app, 'structure.anchors.followup', { ...request, entries: [{ anchorId: anchor.id, body: 'Still unclear',
    acknowledgedStatus: 'edited', selection: { startLine: 2, quote: 'Old procedure\n' } }] }, 'anchor_quote_conflict', 'reader');
  const followup = call(app, 'structure.anchors.followup', { ...request, entries: [{ anchorId: anchor.id,
    body: 'Please explain the replacement', acknowledgedStatus: 'edited', selection: { startLine: 2, quote: 'New procedure\n' } }] }, 'reader')[0];
  assert.equal(followup.link.selectionMode, 'explicit');
  assert.equal(followup.comment.line, 2);
  fail(app, 'structure.anchors.followup', { ...request, entries: [{ anchorId: anchor.id,
    body: 'Duplicate', acknowledgedStatus: 'edited', selection: { startLine: 2, quote: 'New procedure\n' } }] }, 'duplicate_followup', 'reader');
  call(app, 'documents.revise', { id: initial.id, body: '# Guide\nStable ending\n' }, 'editor');
  const deleted = call(app, 'structure.anchors.preview', { documentId: initial.id, anchorIds: [anchor.id] }, 'reader');
  assert.equal(deleted.mappings[0].status, 'deleted');
}
