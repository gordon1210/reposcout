import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { title: 'Operations handbook', body: 'Searchable procedures.' });
  const selected = release(app, approve(app, item));
  call(app, 'search.rebuild', { releaseId: selected.id }, 'reader');
  const saved = call(app, 'search.save', { name: 'My handbooks', query: 'handbook' }, 'reader');
  assert.equal(call(app, 'search.saved', { id: saved.id }, 'reader').total, 1);
  fail(app, 'search.saved', { id: saved.id }, 'forbidden', 'editor');
  fail(app, 'search.delete', { id: saved.id }, 'forbidden', 'editor');
  call(app, 'search.delete', { id: saved.id }, 'reader');
  fail(app, 'search.saved', { id: saved.id }, 'not_found', 'reader');
}
