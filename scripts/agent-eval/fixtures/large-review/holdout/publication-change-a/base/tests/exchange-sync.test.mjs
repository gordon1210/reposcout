import assert from 'node:assert/strict';
import { call, fail, workspace } from './support.mjs';

function applyPlan(app, request, requestKey) {
  const plan = call(app, 'exchange.sync.preview', request, 'editor');
  return call(app, 'exchange.sync.apply', { ...request, requestKey, planToken: plan.planToken,
    expectedSourceIdentities: plan.sourceIdentities }, 'editor');
}

export default function check() {
  const { app, collection } = workspace();
  const manifest = { format: 'publication-exchange', version: 1, origin: 'author:sync',
    collections: [{ key: 'manual', name: 'Manual' }], documents: [
      { key: 'a', collectionKey: 'manual', title: 'A', body: 'See [B](exchange:b).' },
      { key: 'b', collectionKey: 'manual', title: 'B', body: 'First body' },
      { key: 'c', collectionKey: 'manual', title: 'C', body: 'Independent' },
    ] };
  const batch = { manifest, collectionBindings: { manual: collection.id } };
  const preview = call(app, 'exchange.import.preview', batch, 'editor');
  const imported = call(app, 'exchange.import.apply', { ...batch, planToken: preview.planToken,
    requestKey: 'seed-sync' }, 'editor');
  const baseline = call(app, 'exchange.import.anchors', { id: imported.id }, 'editor');
  const request = { ...batch, anchors: baseline.anchors };
  const initial = call(app, 'exchange.sync.preview', request, 'editor');
  assert.equal(initial.counts.unchanged, 3);
  assert.equal(initial.predicted.length, 0);
  const before = app.inspect();
  const repeated = call(app, 'exchange.sync.preview', request, 'editor');
  assert.deepEqual(repeated, initial);
  assert.deepEqual(app.inspect(), before);
  const noop = applyPlan(app, request, 'noop');
  assert.equal(noop.imported.length, 0);
  assert.equal(noop.unchanged.length, 3);
  assert.equal(Object.keys(app.inspect().revisions).length, Object.keys(before.revisions).length);
  const changed = structuredClone(manifest);
  changed.documents[1].body = 'Second body';
  const nextRequest = { ...request, manifest: changed };
  const next = call(app, 'exchange.sync.preview', nextRequest, 'editor');
  assert.equal(next.counts.revise, 1);
  assert.equal(next.counts.unchanged, 2);
  fail(app, 'exchange.sync.apply', { ...nextRequest, requestKey: 'bad-confirmation', planToken: next.planToken,
    expectedSourceIdentities: {} }, 'exchange_source_conflict', 'editor');
  const receipt = applyPlan(app, nextRequest, 'incremental');
  assert.equal(receipt.imported.length, 1);
  assert.equal(receipt.imported[0].key, 'b');
  assert.equal(call(app, 'documents.history', { id: baseline.anchors.a.documentId }, 'editor').length, 1);
  assert.equal(call(app, 'documents.history', { id: baseline.anchors.b.documentId }, 'editor').length, 2);
  assert.equal(call(app, 'exchange.sync.preview', { ...nextRequest, anchors: receipt.anchors }, 'editor').counts.unchanged, 3);
  call(app, 'documents.revise', { id: receipt.anchors.b.documentId, body: 'Local edit' }, 'editor');
  const newer = structuredClone(changed);
  newer.documents[0].title = 'Updated A';
  newer.documents[2].title = 'Updated C';
  const conflictRequest = { ...nextRequest, manifest: newer, anchors: receipt.anchors };
  const atomic = call(app, 'exchange.sync.preview', conflictRequest, 'editor');
  assert.equal(atomic.blocked, true);
  assert.equal(atomic.decisions.find(item => item.key === 'b').reason, 'destination_changed');
  const partialRequest = { ...conflictRequest, policy: 'partial' };
  const partial = call(app, 'exchange.sync.preview', partialRequest, 'editor');
  assert.deepEqual(partial.predicted.map(item => item.key), ['c']);
  const applied = applyPlan(app, partialRequest, 'partial-sync');
  assert.deepEqual(applied.imported.map(item => item.key), ['c']);
  assert.equal(call(app, 'documents.get', { id: baseline.anchors.a.documentId }, 'editor').revision.title, 'A');
  assert.equal(call(app, 'documents.get', { id: baseline.anchors.b.documentId }, 'editor').revision.body, 'Local edit');
}
