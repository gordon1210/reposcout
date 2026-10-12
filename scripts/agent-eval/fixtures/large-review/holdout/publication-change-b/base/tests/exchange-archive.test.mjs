import assert from 'node:assert/strict';
import { call, fail, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const second = draft(app, collection, { title: 'Target', body: 'Unicode content: ä, 文.' });
  const first = draft(app, collection, { title: 'Source', body: `See [target](doc:${second.id}).` });
  fail(app, 'exchange.export.working', { documentIds: [first.id] }, 'exchange_external_reference', 'reader');
  const external = call(app, 'exchange.export.working', { documentIds: [first.id], externalPolicy: 'preserve' }, 'reader');
  assert.equal(external.diagnostics.length, 1);
  const exported = call(app, 'exchange.export.working', { documentIds: [first.id, second.id] }, 'reader');
  assert.equal(exported.manifest.documents.find(item => item.key === first.id).body, `See [target](exchange:${second.id}).`);
  const inspected = call(app, 'exchange.archive.inspect', { archive: exported.archive });
  assert.equal(inspected.documents, 2);
  assert.equal(inspected.catalog.length, 3);
  assert.ok(inspected.catalog.some(file => file.bytes > file.characters));
  const tampered = structuredClone(exported.archive);
  tampered.files[1].content += 'changed';
  fail(app, 'exchange.archive.inspect', { archive: tampered }, 'exchange_archive_content');
  const escaped = structuredClone(exported.archive);
  escaped.files[1].path = '../secrets';
  fail(app, 'exchange.archive.inspect', { archive: escaped }, 'exchange_archive_path');
  const repeated = structuredClone(exported.archive);
  repeated.files[1].path = 'manifest.json';
  fail(app, 'exchange.archive.inspect', { archive: repeated }, 'exchange_archive_path');
  const catalog = structuredClone(exported.archive);
  catalog.catalog[1].checksum = '0'.repeat(64);
  fail(app, 'exchange.archive.inspect', { archive: catalog }, 'exchange_archive_catalog');
}
