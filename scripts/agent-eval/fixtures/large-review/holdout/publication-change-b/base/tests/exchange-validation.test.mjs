import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const manifest = { format: 'publication-exchange', version: 1, origin: 'validation',
    collections: [{ key: 'child', name: 'Child', parentKey: 'parent' }, { key: 'parent', name: 'Parent' }],
    documents: [{ key: 'guide', collectionKey: 'child', title: 'Guide', body: '' }] };
  const preview = call(app, 'exchange.import.preview', { manifest, parentCollectionId: collection.id });
  assert.equal(preview.applicable, true);
  const receipt = call(app, 'exchange.import.apply', { manifest, parentCollectionId: collection.id,
    planToken: preview.planToken, requestKey: 'nested' });
  const state = app.inspect();
  assert.equal(state.collections[receipt.collections.child].parentId, receipt.collections.parent);
  assert.equal(state.collections[receipt.collections.parent].parentId, collection.id);
  assert.equal(state.collections[receipt.collections.child].public, false);
  fail(app, 'exchange.import.preview', { manifest: { ...manifest, version: 2 } }, 'exchange_version');
  fail(app, 'exchange.import.preview', { manifest: { ...manifest, extra: true } }, 'exchange_format');
  const cycle = structuredClone(manifest);
  cycle.collections[1].parentKey = 'child';
  fail(app, 'exchange.import.preview', { manifest: cycle }, 'exchange_collection_cycle');
  const missing = structuredClone(manifest);
  missing.documents[0].body = '[Missing](exchange:absent)';
  const broken = call(app, 'exchange.import.preview', { manifest: missing });
  assert.equal(broken.applicable, false);
  assert.equal(broken.diagnostics[0].code, 'exchange_missing_reference');
  const item = draft(app, collection);
  const update = { ...manifest, collections: [{ key: 'child', name: 'Child' }],
    documents: [{ key: 'guide', collectionKey: 'child', title: 'Updated', body: 'New body' }] };
  const request = { manifest: update, collectionBindings: { child: collection.id },
    targets: { guide: { documentId: item.id, expectedRevisionId: item.revision.id } } };
  const plan = call(app, 'exchange.import.preview', request, 'editor');
  const imported = call(app, 'exchange.import.apply', { ...request, planToken: plan.planToken, requestKey: 'update' }, 'editor');
  assert.equal(imported.imported[0].documentId, item.id);
  assert.equal(imported.imported[0].operation, 'update');
  assert.equal(call(app, 'documents.history', { id: item.id }, 'editor').length, 2);
  assert.equal(call(app, 'documents.get', { id: item.id, revisionId: item.revision.id }, 'editor').revision.body, item.revision.body);
}
