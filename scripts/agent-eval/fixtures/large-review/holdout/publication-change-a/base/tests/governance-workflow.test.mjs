import assert from 'node:assert/strict';
import { workspace, call, fail, draft, approve, release } from './support.mjs';

function configure(app, collection, stages = undefined) {
  const created = call(app, 'governance.policies.create', { definition: { name: 'Publishable',
    rules: [{ id: 'clean', message: 'Resolve placeholders', require: { op: 'eq', fact: 'unresolvedMarkers', value: 0 } }],
    stages: stages ?? [{ id: 'editorial', name: 'Editorial', checklist: [
      { id: 'sources', label: 'Source facts verified', evidenceRequired: true },
    ] }, { id: 'release', name: 'Release manager', distinctReviewer: true, checklist: [] }],
  } });
  const assignment = call(app, 'governance.policies.assign', { collectionId: collection.id, versionId: created.version.id });
  return { ...created, assignment };
}

function selectedRevisionFlow() {
  const { app, collection } = workspace();
  configure(app, collection);
  const item = draft(app, collection, { title: 'Reviewed edition', body: '# Operations\nStable operating instructions.' });
  fail(app, 'reviews.open', { documentId: item.id, assigneeId: 'reviewer' }, 'governance_plan_required', 'editor');
  const { plan } = call(app, 'governance.plans.create', { documentId: item.id }, 'editor');
  const submitted = call(app, 'governance.submit', { planId: plan.id, assigneeId: 'reviewer' }, 'editor');
  fail(app, 'reviews.decide', { id: submitted.review.id, decision: 'approve' }, 'checklist_incomplete', 'reviewer');
  fail(app, 'governance.checklist.record', { planId: plan.id, stageId: 'editorial', checklistId: 'sources',
    completed: true }, 'evidence_required', 'reviewer');
  call(app, 'governance.checklist.record', { planId: plan.id, stageId: 'editorial', checklistId: 'sources',
    completed: true, evidence: 'Primary sources checked.' }, 'reviewer');
  const first = call(app, 'reviews.decide', { id: submitted.review.id, decision: 'approve' }, 'reviewer');
  fail(app, 'releases.publish', { name: 'Premature', entries: [{ approvalId: first.approval.id }] }, 'governance_blocked', 'reviewer');
  fail(app, 'governance.submit', { planId: plan.id, assigneeId: 'reviewer' }, 'distinct_reviewer', 'editor');
  const second = call(app, 'governance.submit', { planId: plan.id, assigneeId: 'admin' }, 'editor');
  call(app, 'documents.revise', { id: item.id, title: 'Working changes', body: 'TODO unfinished changes.' }, 'editor');
  const final = call(app, 'reviews.decide', { id: second.review.id, decision: 'approve' });
  const prepared = call(app, 'governance.publication.prepare', { documentIds: [item.id] }, 'reviewer');
  assert.equal(prepared.ready, true);
  assert.equal(prepared.documents[0].selectedRevisionId, item.revision.id);
  assert.notEqual(prepared.documents[0].workingRevisionId, item.revision.id);
  assert.equal(prepared.documents[0].candidates.find(candidate => candidate.approvalId === first.approval.id).eligible, false);
  const published = call(app, 'releases.publish', { name: 'Approved edition', entries: prepared.entries }, 'reviewer');
  assert.equal(published.entries[0].revisionId, item.revision.id);
  assert.equal(published.entries[0].approvalId, final.approval.id);
  const assessment = call(app, 'governance.assessments.get', { id: plan.assessmentId }, 'editor');
  assert.equal(assessment.matchesSource, true);
  assert.equal(call(app, 'governance.plans.get', { id: plan.id }, 'editor').complete, true);
}

function staleAssessmentsAndPolicyChanges() {
  const { app, collection } = workspace();
  const policy = configure(app, collection);
  const item = draft(app, collection);
  const assessment = call(app, 'governance.assessments.create', { documentId: item.id }, 'editor');
  const newer = call(app, 'documents.revise', { id: item.id, body: 'New complete prose.' }, 'editor');
  fail(app, 'governance.plans.create', { documentId: item.id, assessmentId: assessment.id }, 'stale_assessment', 'editor');
  const { plan } = call(app, 'governance.plans.create', { documentId: item.id, revisionId: item.revision.id,
    assessmentId: assessment.id }, 'editor');
  assert.equal(plan.source.revisionId, item.revision.id);
  assert.notEqual(plan.source.revisionId, newer.revision.id);
  const submitted = call(app, 'governance.submit', { planId: plan.id, assigneeId: 'reviewer' }, 'editor');
  const changed = call(app, 'governance.policies.revise', { policyId: policy.policy.id,
    expectedVersionId: policy.version.id, definition: { name: 'Changed', rules: [], stages: [
      { id: 'single', name: 'One stage', checklist: [] },
    ] } });
  assert.equal(call(app, 'governance.plans.get', { id: plan.id }).current, true, 'new versions do not silently change assignments');
  call(app, 'governance.policies.assign', { collectionId: collection.id, versionId: changed.version.id,
    expectedAssignmentId: policy.assignment.id });
  fail(app, 'reviews.decide', { id: submitted.review.id, decision: 'approve' }, 'stale_plan', 'reviewer');
  call(app, 'reviews.decide', { id: submitted.review.id, decision: 'reject' }, 'reviewer');
  assert.equal(call(app, 'governance.assessments.get', { id: assessment.id }).matchesPolicy, false);
}

function appendOnlyEvidenceAndReassignment() {
  const { app, collection } = workspace();
  configure(app, collection);
  const item = draft(app, collection);
  const { plan } = call(app, 'governance.plans.create', { documentId: item.id }, 'editor');
  const submitted = call(app, 'governance.submit', { planId: plan.id, assigneeId: 'reviewer' }, 'editor');
  const first = call(app, 'governance.checklist.record', { planId: plan.id, stageId: 'editorial', checklistId: 'sources',
    completed: false, evidence: 'One source remains unchecked.' }, 'reviewer');
  fail(app, 'governance.checklist.record', { planId: plan.id, stageId: 'editorial', checklistId: 'sources',
    completed: true, evidence: 'Now checked.' }, 'checklist_conflict', 'reviewer');
  const second = call(app, 'governance.checklist.record', { planId: plan.id, stageId: 'editorial', checklistId: 'sources',
    expectedEntryId: first.id, completed: true, evidence: 'Now checked.' }, 'reviewer');
  assert.deepEqual(app.inspect().governanceChecklist[first.id], first);
  assert.equal(second.supersedesId, first.id);
  call(app, 'reviews.assign', { id: submitted.review.id, assigneeId: 'admin' });
  fail(app, 'reviews.decide', { id: submitted.review.id, decision: 'approve' }, 'checklist_incomplete');
  call(app, 'governance.checklist.record', { planId: plan.id, stageId: 'editorial', checklistId: 'sources',
    expectedEntryId: second.id, completed: true, evidence: 'Independently verified after reassignment.' });
  call(app, 'reviews.decide', { id: submitted.review.id, decision: 'approve' });
}

function noPolicyCompatibility() {
  const { app, collection } = workspace();
  const item = draft(app, collection);
  const first = approve(app, item);
  const published = release(app, first);
  const newer = call(app, 'documents.revise', { id: item.id, body: 'Updated approved text.' }, 'editor');
  const second = approve(app, newer);
  const preparation = call(app, 'governance.publication.prepare', { documentIds: [item.id] }, 'reviewer');
  assert.equal(preparation.documents[0].selectedApprovalId, second.id);
  assert.equal(call(app, 'releases.get', { id: published.id }, 'reader').entries[0].revisionId, item.revision.id);
}

function reviewerRevocationAffectsOnlyNewPublication() {
  const { app, collection } = workspace();
  configure(app, collection, [{ id: 'single', name: 'Independent review', checklist: [] }]);
  const item = draft(app, collection);
  const { plan } = call(app, 'governance.plans.create', { documentId: item.id }, 'editor');
  const review = call(app, 'governance.submit', { planId: plan.id, assigneeId: 'reviewer' }, 'editor').review;
  const approval = call(app, 'reviews.decide', { id: review.id, decision: 'approve' }, 'reviewer').approval;
  const published = release(app, approval);
  call(app, 'members.remove', { collectionId: collection.id, actorId: 'reviewer' });
  const preview = call(app, 'governance.publication.prepare', { documentIds: [item.id] });
  assert.equal(preview.ready, false);
  assert.ok(preview.documents[0].candidates[0].reasons.includes('reviewer_no_longer_authorized'));
  fail(app, 'releases.publish', { name: 'Another release', entries: [{ approvalId: approval.id }] }, 'governance_blocked');
  assert.equal(call(app, 'releases.get', { id: published.id }, 'reader').entries[0].revisionId, item.revision.id);
}

export default function run() {
  selectedRevisionFlow();
  staleAssessmentsAndPolicyChanges();
  appendOnlyEvidenceAndReassignment();
  noPolicyCompatibility();
  reviewerRevocationAffectsOnlyNewPublication();
}
