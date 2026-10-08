import assert from 'node:assert/strict';
import { workspace, call, fail, draft, approve } from './support.mjs';

function policy(app, name, rules = []) {
  return call(app, 'governance.policies.create', { definition: { name, rules,
    stages: [{ id: 'editorial', name: 'Editorial', checklist: [] }] } });
}

function compareVersions() {
  const { app } = workspace();
  const first = policy(app, 'First');
  const second = call(app, 'governance.policies.revise', { policyId: first.policy.id,
    expectedVersionId: first.version.id, definition: { name: 'Second', rules: [
      { id: 'length', message: 'Needs detail', require: { op: 'gte', fact: 'words', value: 20 } },
    ], stages: [{ id: 'editorial', name: 'Editorial', checklist: [
      { id: 'accuracy', label: 'Facts checked', evidenceRequired: true },
    ] }, { id: 'legal', name: 'Legal', distinctReviewer: true, checklist: [] }] } });
  const result = call(app, 'governance.policies.compare', { beforeVersionId: first.version.id, afterVersionId: second.version.id });
  assert.equal(result.identityChanged, true);
  assert.equal(result.semanticsChanged, true);
  assert.equal(result.rules[0].change, 'added');
  assert.equal(result.stages.find(stage => stage.id === 'editorial').checklist[0].id, 'accuracy');
  assert.equal(result.stages.find(stage => stage.id === 'legal').change, 'added');
  const identical = call(app, 'governance.policies.compare', { beforeVersionId: first.version.id, afterVersionId: first.version.id });
  assert.equal(identical.identityChanged, false);
  assert.equal(identical.semanticsChanged, false);
}

function boundedImpactAndFreshApply() {
  const { app, collection } = workspace();
  const child = call(app, 'collections.create', { name: 'Independent child', parentId: collection.id });
  const original = policy(app, 'Original');
  const stricter = policy(app, 'Detailed', [{ id: 'length', message: 'Needs twenty words',
    require: { op: 'gte', fact: 'words', value: 20 } }]);
  const assignment = call(app, 'governance.policies.assign', { collectionId: collection.id, versionId: original.version.id });
  call(app, 'governance.policies.assign', { collectionId: child.id, versionId: original.version.id });
  const item = draft(app, collection);
  const protectedItem = draft(app, child);
  const { plan } = call(app, 'governance.plans.create', { documentId: item.id }, 'editor');
  const review = call(app, 'governance.submit', { planId: plan.id, assigneeId: 'reviewer' }, 'editor').review;
  const approval = call(app, 'reviews.decide', { id: review.id, decision: 'approve' }, 'reviewer').approval;
  const input = { collectionId: collection.id, versionId: stricter.version.id,
    expectedVersionId: stricter.version.id, expectedAssignmentId: assignment.id };
  const before = app.inspect();
  const preview = call(app, 'governance.assignments.preview', input);
  assert.deepEqual(app.inspect(), before, 'impact forecasting is read-only');
  assert.equal(preview.complete, true);
  assert.equal(preview.documents.find(row => row.documentId === protectedItem.id).affected, false);
  const changed = preview.documents.find(row => row.documentId === item.id);
  assert.equal(changed.affected, true);
  assert.deepEqual(changed.proposedAssessment.blockingRuleIds, ['length']);
  const oldApproval = changed.records.find(record => record.id === approval.id);
  assert.equal(oldApproval.usableBefore, true);
  assert.equal(oldApproval.usableAfter, false);
  assert.ok(oldApproval.reasonsAfter.includes('policy_changed'));
  const bounded = call(app, 'governance.assignments.preview', { ...input, documentLimit: 1, recordLimit: 1 });
  assert.equal(bounded.complete, false);
  assert.equal(bounded.omittedDocuments, 1);
  assert.equal(bounded.omittedRecords, 2);
  fail(app, 'governance.assignments.apply', { ...input, documentLimit: 1, recordLimit: 1,
    impactDigest: bounded.impactDigest }, 'incomplete_preview');
  call(app, 'documents.revise', { id: item.id, body: 'A changed working document.' }, 'editor');
  fail(app, 'governance.assignments.apply', { ...input, impactDigest: preview.impactDigest }, 'impact_conflict');
  const fresh = call(app, 'governance.assignments.preview', input);
  const applied = call(app, 'governance.assignments.apply', { ...input, impactDigest: fresh.impactDigest });
  assert.equal(applied.assignment.versionId, stricter.version.id);
  assert.equal(call(app, 'governance.plans.get', { id: plan.id }).current, false);
  assert.equal(call(app, 'governance.policies.effective', { collectionId: child.id }).policy.versionId, original.version.id);
}

function newlyConfiguredApprovals() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  const approval = approve(app, item);
  const target = policy(app, 'New governance');
  const preview = call(app, 'governance.assignments.preview', { collectionId: collection.id,
    versionId: target.version.id, expectedVersionId: target.version.id });
  const record = preview.documents[0].records.find(record => record.id === approval.id);
  assert.equal(record.usableBefore, true);
  assert.equal(record.usableAfter, false);
  assert.deepEqual(record.reasonsAfter, ['governance_plan_required']);
}

export default function run() {
  compareVersions();
  boundedImpactAndFreshApply();
  newlyConfiguredApprovals();
}
