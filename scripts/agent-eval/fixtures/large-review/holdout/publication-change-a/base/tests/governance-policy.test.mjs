const checks = [];
function test(name, check) { checks.push({ name, check }); }
import assert from 'node:assert/strict';
import { workspace, call, fail, draft } from './support.mjs';
import { contentFacts } from '../src/governance/facts.mjs';

function definition(overrides = {}) {
  return { name: 'Editorial', rules: [{ id: 'clean', message: 'Resolve editorial markers',
    require: { op: 'eq', fact: 'unresolvedMarkers', value: 0 } }],
  stages: [{ id: 'editorial', name: 'Editorial review', checklist: [] }], ...overrides };
}

test('policy validation rejects unknown facts and excessive expression depth atomically', () => {
  const { app } = workspace();
  const before = app.inspect();
  fail(app, 'governance.policies.create', { definition: definition({ rules: [{ id: 'bad', message: 'Bad',
    require: { op: 'eq', fact: 'workingRevisionId', value: 'x' } }] }) }, 'invalid_input');
  assert.deepEqual(app.inspect(), before);
  let expression = { op: 'eq', fact: 'words', value: 0 };
  for (let index = 0; index < 10; index += 1) expression = { op: 'not', arg: expression };
  fail(app, 'governance.policies.create', { definition: definition({ rules: [{ id: 'bad', message: 'Bad', require: expression }] }) }, 'invalid_policy');
  assert.deepEqual(app.inspect(), before);
});

test('immutable policy inheritance replaces rules by id and explicitly removes inherited rules', () => {
  const { app, collection } = workspace();
  const parent = call(app, 'governance.policies.create', { definition: definition() });
  const child = call(app, 'governance.policies.create', { definition: {
    name: 'Short exception', extendsVersionId: parent.version.id, removeRules: ['clean'],
    rules: [{ id: 'size', message: 'At least two words', require: { op: 'gte', fact: 'words', value: 2 } }],
  } });
  call(app, 'governance.policies.assign', { collectionId: collection.id, versionId: child.version.id });
  const item = draft(app, collection, { body: 'TODO later' });
  const result = call(app, 'governance.assessments.preview', { documentId: item.id }, 'editor');
  assert.equal(result.passed, true);
  assert.deepEqual(result.results.map(rule => rule.ruleId), ['size']);
  assert.equal(result.results[0].originVersionId, child.version.id);
  assert.equal(call(app, 'governance.policies.get', { policyId: parent.policy.id }).versions.length, 1);
});

test('collection policy inherits and removing a local override restores parent policy', () => {
  const { app, collection } = workspace();
  const child = call(app, 'collections.create', { name: 'Child', parentId: collection.id });
  const first = call(app, 'governance.policies.create', { definition: definition() });
  const second = call(app, 'governance.policies.create', { definition: definition({ name: 'Local', rules: [] }) });
  call(app, 'governance.policies.assign', { collectionId: collection.id, versionId: first.version.id });
  assert.equal(call(app, 'governance.policies.effective', { collectionId: child.id }).policy.versionId, first.version.id);
  const override = call(app, 'governance.policies.assign', { collectionId: child.id, versionId: second.version.id });
  fail(app, 'governance.policies.assign', { collectionId: child.id, versionId: null }, 'policy_conflict');
  call(app, 'governance.policies.assign', { collectionId: child.id, versionId: null, expectedAssignmentId: override.id });
  assert.equal(call(app, 'governance.policies.effective', { collectionId: child.id }).policy.versionId, first.version.id);
});

test('assessment evidence is deterministic, includes skipped rules, and distinguishes warnings', () => {
  const { app, collection } = workspace();
  const policy = call(app, 'governance.policies.create', { definition: definition({ rules: [
    { id: 'conditional', message: 'French needs links', when: { op: 'eq', fact: 'language', value: 'fr' },
      require: { op: 'gt', fact: 'linkCount', value: 0 } },
    { id: 'warning', message: 'Prefer three sections', severity: 'warning', require: { op: 'gte', fact: 'sectionCount', value: 3 } },
  ] }) });
  call(app, 'governance.policies.assign', { collectionId: collection.id, versionId: policy.version.id });
  const item = draft(app, collection);
  const first = call(app, 'governance.assessments.create', { documentId: item.id }, 'editor');
  const second = call(app, 'governance.assessments.create', { documentId: item.id }, 'editor');
  assert.equal(first.id, second.id);
  assert.equal(first.passed, true);
  assert.equal(first.warningCount, 1);
  assert.equal(first.results[0].status, 'skipped');
  assert.equal(first.results[1].evidence.actual, 0);
});

test('content facts exclude code markers and distinguish fenced headings from actual sections', () => {
  const facts = contentFacts({ title: 'Guide', language: 'en', authorId: 'editor', tags: [],
    body: '# Start\nReady now.\n~~~js\n# Not a heading\nTODO FIXME\n~~~\n## Empty\n## Next\nTBD\n' });
  assert.equal(facts.sectionCount, 3);
  assert.equal(facts.codeBlocks, 1);
  assert.equal(facts.unresolvedMarkers, 1);
  assert.equal(facts.emptySections, 1);
  assert.equal(facts.hasUnclosedFence, false);
});

export default function run() {
  for (const { name, check } of checks) {
    try { check(); } catch (error) { error.message = `${name}: ${error.message}`; throw error; }
  }
}
