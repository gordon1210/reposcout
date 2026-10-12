import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { title: 'Unicode — instructions', body: 'Änderung\nQuote: "use carefully"', tags: ['café'] });
  const selected = release(app, approve(app, item));
  const artifact = exportRelease(app, selected);
  assert.equal(artifact.mediaType, 'application/json');
  assert.equal(artifact.extension, 'json');
  assert.equal(artifact.bytes, Buffer.byteLength(artifact.content, 'utf8'));
  const exported = JSON.parse(artifact.content);
  assert.equal(exported.documents[0].body, item.revision.body);
  assert.deepEqual(exported.documents[0].tags, ['café']);
  assert.equal(artifact.documents[0].checksum, item.revision.checksum);
}
