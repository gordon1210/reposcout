import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const initial = draft(app, collection, { body: 'First\nSecond\nThird\n' });
  const anchors = [1, 2].map(startLine => call(app, 'structure.anchors.capture', {
    documentId: initial.id, startLine, expectedChecksum: initial.revision.checksum }, 'reader'));
  const current = call(app, 'documents.revise', { id: initial.id, body: 'First\nChanged\nThird\n' }, 'editor');
  const guard = { documentId: initial.id, expectedRevisionId: current.revision.id, expectedChecksum: current.revision.checksum };
  const before = app.inspect();
  fail(app, 'structure.anchors.followup', { ...guard, entries: anchors.map(anchor => ({ anchorId: anchor.id, body: 'Investigate' })) },
    'anchor_not_exact', 'reader');
  assert.deepEqual(app.inspect(), before);
  fail(app, 'structure.anchors.followup', { ...guard, entries: [
    { anchorId: anchors[0].id, body: 'One' }, { anchorId: anchors[0].id, body: 'Two' },
  ] }, 'duplicate_anchor', 'reader');
  const results = call(app, 'structure.anchors.followup', { ...guard, entries: [
    { anchorId: anchors[0].id, body: 'Investigate first' },
    { anchorId: anchors[1].id, body: 'Investigate change', acknowledgedStatus: 'edited', selection: { startLine: 2, quote: 'Changed\n' } },
  ] }, 'reader');
  assert.equal(results.length, 2);
  call(app, 'documents.revise', { id: initial.id, body: 'First\nChanged again\nThird\n' }, 'editor');
  fail(app, 'structure.anchors.followup', { ...guard, entries: [{ anchorId: anchors[0].id, body: 'Stale' }] }, 'revision_conflict', 'reader');
}
