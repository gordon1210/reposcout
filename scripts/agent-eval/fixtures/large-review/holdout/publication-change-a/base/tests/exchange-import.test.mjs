import assert from 'node:assert/strict';
import { call, fail, workspace } from './support.mjs';

function source() {
  return { format: 'publication-exchange', version: 1, origin: 'authoring:handbook',
    collections: [{ key: 'manual', name: 'Manual' }], documents: [
      { key: 'intro', collectionKey: 'manual', title: 'Introduction', body: 'Read [details](exchange:details).' },
      { key: 'details', collectionKey: 'manual', title: 'Details', body: 'Return to [introduction](exchange:intro).' },
    ] };
}

export default function check() {
  const { app, collection } = workspace();
  const request = { manifest: source(), collectionBindings: { manual: collection.id } };
  const before = app.inspect();
  const preview = call(app, 'exchange.import.preview', request, 'editor');
  assert.equal(preview.applicable, true);
  assert.equal(preview.imported.length, 2);
  assert.deepEqual(app.inspect(), before);
  const imported = call(app, 'exchange.import.apply', { ...request, planToken: preview.planToken, requestKey: 'first' }, 'editor');
  const ids = Object.fromEntries(imported.imported.map(item => [item.key, item.documentId]));
  assert.equal(call(app, 'documents.get', { id: ids.intro }, 'editor').revision.body, `Read [details](doc:${ids.details}).`);
  assert.equal(call(app, 'documents.get', { id: ids.details }, 'editor').revision.body, `Return to [introduction](doc:${ids.intro}).`);
  assert.equal(call(app, 'documents.history', { id: ids.intro }, 'editor').length, 1);
  assert.equal(call(app, 'documents.links', { documentId: ids.intro }, 'editor')[0].status, 'resolved');
  const committed = app.inspect();
  const replay = call(app, 'exchange.import.apply', { ...request, planToken: preview.planToken, requestKey: 'first' }, 'editor');
  assert.equal(replay.replayed, true);
  assert.deepEqual(app.inspect(), committed);
  assert.deepEqual(call(app, 'exchange.import.get', { id: imported.id }, 'editor').imported, imported.imported);
  fail(app, 'exchange.import.get', { id: imported.id }, 'forbidden', 'reader');
  const changed = source();
  changed.documents[0].title = 'Altered';
  fail(app, 'exchange.import.apply', { ...request, manifest: changed, requestKey: 'first' }, 'exchange_request_conflict', 'editor');
  const fresh = call(app, 'exchange.import.preview', request, 'editor');
  call(app, 'documents.revise', { id: ids.intro, title: 'Concurrent edit' }, 'editor');
  fail(app, 'exchange.import.apply', { ...request, requestKey: 'second', planToken: fresh.planToken }, 'exchange_stale_plan', 'editor');
}
