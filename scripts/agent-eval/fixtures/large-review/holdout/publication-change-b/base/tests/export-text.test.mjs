import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { title: 'Handbook', body: 'Start here.\nThen continue.' });
  const selected = release(app, approve(app, item), 'Operator manual');
  const artifact = exportRelease(app, selected, 'text');
  assert.equal(artifact.mediaType, 'text/plain; charset=utf-8');
  assert.ok(artifact.content.startsWith('Operator manual\n\nHandbook\n========'));
  assert.ok(artifact.content.includes('Start here.\nThen continue.'));
  assert.ok(artifact.content.includes(`Revision: ${item.revision.id}`));
  assert.ok(artifact.content.endsWith('\n'));
}
