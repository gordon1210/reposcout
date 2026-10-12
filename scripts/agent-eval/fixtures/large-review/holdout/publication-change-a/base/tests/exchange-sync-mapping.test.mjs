import assert from 'node:assert/strict';
import { call, fail, workspace } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const manifest = { format: 'publication-exchange', version: 1, origin: 'author:new',
    collections: [{ key: 'manual', name: 'Manual' }], documents: [
      { key: 'a', collectionKey: 'manual', title: 'A', body: '[B](exchange:b)' },
      { key: 'b', collectionKey: 'manual', title: 'B', body: 'Body B' },
    ] };
  const request = { manifest, collectionBindings: { manual: collection.id }, anchors: {} };
  const unmapped = call(app, 'exchange.sync.preview', request, 'editor');
  assert.equal(unmapped.counts.unmapped, 2);
  assert.equal(unmapped.applicable, false);
  const createRequest = { ...request, createMissing: true };
  const preview = call(app, 'exchange.sync.preview', createRequest, 'editor');
  assert.equal(preview.counts.create, 2);
  const receipt = call(app, 'exchange.sync.apply', { ...createRequest, requestKey: 'create', planToken: preview.planToken,
    expectedSourceIdentities: preview.sourceIdentities }, 'editor');
  const snapshot = app.inspect();
  const replay = call(app, 'exchange.sync.apply', { ...createRequest, requestKey: 'create' }, 'editor');
  assert.equal(replay.replayed, true);
  assert.deepEqual(app.inspect(), snapshot);
  const removed = structuredClone(manifest);
  removed.documents = [removed.documents[1]];
  const plan = call(app, 'exchange.sync.preview', { ...request, manifest: removed, anchors: receipt.anchors }, 'editor');
  assert.deepEqual(plan.retired, ['a']);
  assert.equal(call(app, 'documents.list', {}, 'editor').total, 2);
  const collision = { ...receipt.anchors, b: receipt.anchors.a };
  fail(app, 'exchange.sync.preview', { ...request, anchors: collision }, 'exchange_duplicate_target', 'editor');
  const otherCollection = call(app, 'collections.create', { name: 'Other' });
  const movedMapping = call(app, 'exchange.sync.preview', { ...request, anchors: receipt.anchors,
    collectionBindings: { manual: otherCollection.id } }, 'editor');
  assert.equal(movedMapping.counts.conflicting, 2);
  assert.equal(movedMapping.decisions[0].reason, 'collection_mismatch');
}
