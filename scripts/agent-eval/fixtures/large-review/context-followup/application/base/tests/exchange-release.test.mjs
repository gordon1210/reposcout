import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { title: 'Released title', body: 'Retained publication text.' });
  const published = release(app, approve(app, item));
  call(app, 'documents.revise', { id: item.id, title: 'Working title', body: 'Unpublished working text.' }, 'editor');
  const released = call(app, 'exchange.export.release', { releaseId: published.id }, 'reader');
  assert.equal(released.mode, 'release');
  assert.equal(released.manifest.documents[0].body, 'Retained publication text.');
  assert.equal(released.manifest.documents[0].title, 'Released title');
  assert.equal(released.manifest.documents[0].source.revisionId, item.revision.id);
  const working = call(app, 'exchange.export.working', { documentIds: [item.id] }, 'reader');
  assert.equal(working.mode, 'working');
  assert.equal(working.manifest.documents[0].body, 'Unpublished working text.');
  assert.notEqual(released.identity, working.identity);
  assert.deepEqual(call(app, 'exchange.export.release', { releaseId: published.id }, 'reader'), released);
  const target = call(app, 'collections.create', { name: 'Imported publication' });
  const request = { archive: released.archive, collectionBindings: { [collection.id]: target.id } };
  const preview = call(app, 'exchange.import.preview', request);
  const receipt = call(app, 'exchange.import.apply', { ...request, planToken: preview.planToken, requestKey: 'release-copy' });
  const copied = call(app, 'documents.get', { id: receipt.imported[0].documentId });
  assert.equal(copied.revision.body, 'Retained publication text.');
  assert.equal(Object.values(app.inspect().approvals).some(approval => approval.documentId === copied.id), false);
  call(app, 'releases.withdraw', { id: published.id });
  fail(app, 'exchange.export.release', { releaseId: published.id }, 'withdrawn_release', 'reader');
}
