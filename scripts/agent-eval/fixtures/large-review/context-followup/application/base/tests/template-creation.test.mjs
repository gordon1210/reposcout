import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  fail(app, 'templates.create', { collectionId: collection.id, name: 'Incorrect', titlePattern: '{{name}}', bodyPattern: '', required: [] }, 'parameter_mismatch', 'editor');
  const template = call(app, 'templates.create', { collectionId: collection.id, name: 'Runbook', titlePattern: '{{service}} runbook',
    bodyPattern: '# {{service}}\nContact {{owner}} for escalation.', required: ['service', 'owner'] }, 'editor');
  const item = call(app, 'templates.instantiate', { id: template.id, values: { service: 'Payments', owner: 'Support' } }, 'editor');
  assert.equal(item.revision.title, 'Payments runbook');
  assert.equal(item.revision.body, '# Payments\nContact Support for escalation.');
  assert.equal(call(app, 'templates.list', { collectionId: collection.id }, 'reader').length, 1);
  fail(app, 'templates.instantiate', { id: template.id, values: { service: 'Missing owner' } }, 'invalid_input', 'editor');
}
