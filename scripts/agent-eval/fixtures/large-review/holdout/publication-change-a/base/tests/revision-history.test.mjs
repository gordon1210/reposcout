import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { title: 'First title', body: 'First line.' });
  const second = call(app, 'documents.revise', { id: item.id, title: 'Second title', body: 'Two\nlines.' }, 'editor');
  const third = call(app, 'documents.revise', { id: item.id, tags: ['new'] }, 'editor');
  const history = call(app, 'documents.history', { id: item.id }, 'reader');
  assert.deepEqual(history.map(entry => entry.id), [third.revision.id, second.revision.id, item.revision.id]);
  assert.equal(history[0].title, 'Second title');
  assert.equal(history[0].lines, 2);
  assert.equal(Object.hasOwn(history[0], 'body'), false);
  assert.deepEqual(history[0].tags, ['new']);
}
