import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const template = call(app, 'templates.create', { collectionId: collection.id, name: 'Literal', titlePattern: 'Literal',
    bodyPattern: '{{value}} -- {{value}}', required: ['value'] }, 'editor');
  const item = call(app, 'templates.instantiate', { id: template.id, values: { value: '$& {{other}}' } }, 'editor');
  assert.equal(item.revision.body, '$& {{other}} -- $& {{other}}');
  fail(app, 'templates.instantiate', { id: template.id, values: { value: 'x', unexpected: 'y' } }, 'extra_parameter', 'editor');
  const empty = call(app, 'templates.instantiate', { id: template.id, values: { value: '' } }, 'editor');
  assert.equal(empty.revision.body, ' -- ');
}
