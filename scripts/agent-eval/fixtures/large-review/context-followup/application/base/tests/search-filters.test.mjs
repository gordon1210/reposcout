import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const first = draft(app, collection, { title: 'Service guide', body: 'Guidance.', tags: ['support'], language: 'en' });
  const second = draft(app, collection, { title: 'Service reference', body: 'Reference.', tags: ['engineering'], language: 'de' });
  const selected = release(app, [approve(app, first), approve(app, second)]);
  call(app, 'search.rebuild', { releaseId: selected.id }, 'reader');
  assert.equal(call(app, 'search.query', { query: 'service', tag: 'support' }, 'reader').items[0].documentId, first.id);
  assert.equal(call(app, 'search.query', { query: 'service', language: 'de' }, 'reader').items[0].documentId, second.id);
  assert.equal(call(app, 'search.query', { query: 'service', limit: 1 }, 'reader').items.length, 1);
  call(app, 'search.rebuild', { releaseId: selected.id }, 'reader');
  assert.equal(call(app, 'search.query', { query: 'service' }, 'reader').total, 2);
}
