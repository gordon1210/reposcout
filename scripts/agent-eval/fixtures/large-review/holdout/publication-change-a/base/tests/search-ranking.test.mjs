import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const first = draft(app, collection, { title: 'Incident handbook', body: 'Recovery steps.' });
  const second = draft(app, collection, { title: 'Reference', body: 'This text mentions incident.', tags: [] });
  const selected = release(app, [approve(app, first), approve(app, second)]);
  call(app, 'search.rebuild', { releaseId: selected.id }, 'reader');
  const result = call(app, 'search.query', { query: 'incident' }, 'reader');
  assert.equal(result.total, 2);
  assert.deepEqual(result.items.map(item => item.documentId), [first.id, second.id]);
  assert.ok(result.items[0].relevance > result.items[1].relevance);
  assert.equal(call(app, 'search.query', { query: 'incident missing' }, 'reader').total, 0);
  assert.equal(call(app, 'search.query', { query: 'the and' }, 'reader').total, 0);
}
