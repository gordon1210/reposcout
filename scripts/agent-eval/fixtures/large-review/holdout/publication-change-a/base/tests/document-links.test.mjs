import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const target = draft(app, collection, { title: 'Target', body: 'Initial target.' });
  const source = draft(app, collection, { body: `[current](doc:${target.id}) [pinned](doc:${target.id}@${target.revision.id}) [missing](doc:document_9999)` });
  call(app, 'documents.revise', { id: target.id, title: 'Updated target' }, 'editor');
  const links = call(app, 'documents.links', { documentId: source.id }, 'reader');
  assert.equal(links.length, 3);
  assert.equal(links[0].title, 'Updated target');
  assert.equal(links[1].title, 'Target');
  assert.equal(links[2].status, 'unavailable');
  assert.equal(links[2].title, null);
}
