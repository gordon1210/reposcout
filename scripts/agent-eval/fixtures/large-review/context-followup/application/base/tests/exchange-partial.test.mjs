import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const old = draft(app, collection);
  const manifest = { format: 'publication-exchange', version: 1, origin: 'batch:partial',
    collections: [{ key: 'target', name: 'Target' }], documents: [
      { key: 'a', collectionKey: 'target', title: 'A', body: '[B](exchange:b)' },
      { key: 'b', collectionKey: 'target', title: 'B', body: 'Needs current target revision' },
      { key: 'c', collectionKey: 'target', title: 'C', body: 'Independent content' },
    ] };
  const request = { manifest, collectionBindings: { target: collection.id },
    targets: { b: { documentId: old.id, expectedRevisionId: 'revision_9999' } } };
  const before = app.inspect();
  const atomic = call(app, 'exchange.import.preview', request, 'editor');
  assert.equal(atomic.applicable, false);
  assert.equal(atomic.imported.length, 0);
  assert.equal(atomic.rejected.length, 3);
  fail(app, 'exchange.import.apply', { ...request, planToken: atomic.planToken, requestKey: 'atomic' },
    'exchange_atomic_rejected', 'editor');
  assert.deepEqual(app.inspect(), before);
  const partialRequest = { ...request, policy: 'partial' };
  const partial = call(app, 'exchange.import.preview', partialRequest, 'editor');
  assert.equal(partial.applicable, true);
  assert.deepEqual(partial.imported.map(item => item.key), ['c']);
  assert.deepEqual(partial.rejected.map(item => item.key), ['a', 'b']);
  const applied = call(app, 'exchange.import.apply', { ...partialRequest, planToken: partial.planToken,
    requestKey: 'partial' }, 'editor');
  assert.equal(applied.imported.length, 1);
  assert.equal(call(app, 'documents.list', {}, 'editor').total, 2);
  assert.equal(call(app, 'documents.history', { id: old.id }, 'editor').length, 1);
  assert.equal(app.inspect().events.some(event => event.action === 'document.created' &&
    ![old.id, applied.imported[0].documentId].includes(event.target)), false);
}
