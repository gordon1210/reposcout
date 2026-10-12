import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const first = draft(app, collection, { title: 'First document', body: 'First approved content.' });
  const second = draft(app, collection, { title: 'Second document', body: 'Second approved content.' });
  const approved = [approve(app, first), approve(app, second)];
  const selected = release(app, approved);
  assert.equal(selected.entries.length, 2);
  const exported = JSON.parse(exportRelease(app, selected).content);
  assert.deepEqual(exported.documents.map(item => item.documentId), [first.id, second.id]);
  assert.deepEqual(exported.documents.map(item => item.body), ['First approved content.', 'Second approved content.']);
  assert.equal(exported.releaseId, selected.id);
}
