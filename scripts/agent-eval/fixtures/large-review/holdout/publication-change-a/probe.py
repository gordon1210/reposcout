"""Private independent public-workflow observations; never installed in review trees."""
import subprocess
import sys

SCRIPT = r'''
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { createApplication } = await import(pathToFileURL(join(process.argv[1], 'app.mjs')).href);

function call(app, action, input = {}, actor = 'admin') {
  const result = app.dispatch(action, input, actor);
  if (!result.ok) throw new Error(`${action}: ${JSON.stringify(result)}`);
  return result.data;
}
function state(app) { return JSON.stringify(app.inspect()); }
function setup({ secondId = 'ready', distinct = false } = {}) {
  const app = createApplication();
  const parent = call(app, 'collections.create', { name: 'Operating publications' });
  for (const [actorId, role] of [['editor', 'editor'], ['reviewer', 'reviewer'], ['reader', 'reader']]) {
    call(app, 'members.assign', { collectionId: parent.id, actorId, role });
  }
  const collection = call(app, 'collections.create', { name: 'Approved handbooks', parentId: parent.id });
  const configured = call(app, 'governance.policies.create', { definition: {
    name: 'Handbook checks', rules: [{ id: 'finished', message: 'Resolve all placeholders',
      require: { op: 'eq', fact: 'unresolvedMarkers', value: 0 } }],
    stages: [
      { id: 'editorial', name: 'Operational verification', checklist: [
        { id: 'ready', label: 'Verify operational instructions', evidenceRequired: true }] },
      { id: 'publication', name: 'Publication permissions', distinctReviewer: distinct, checklist: [
        { id: secondId, label: 'Verify publication rights', evidenceRequired: true }] },
    ],
  } });
  call(app, 'governance.policies.assign', { collectionId: parent.id, versionId: configured.version.id });
  const item = call(app, 'documents.create', { collectionId: collection.id, title: 'Operations handbook',
    body: '# Operations\nCheck the service panel before starting the sequence.', tags: ['operations'] }, 'editor');
  const { plan } = call(app, 'governance.plans.create', { documentId: item.id }, 'editor');
  const firstReview = call(app, 'governance.submit', { planId: plan.id, assigneeId: 'reviewer' }, 'editor').review;
  const firstEntry = call(app, 'governance.checklist.record', { planId: plan.id, stageId: 'editorial',
    checklistId: 'ready', completed: true, evidence: 'Instructions compared with the current operating record.' }, 'reviewer');
  const firstApproval = call(app, 'reviews.decide', { id: firstReview.id, decision: 'approve' }, 'reviewer').approval;
  return { app, parent, collection, item, plan, firstReview, firstEntry, firstApproval, secondId };
}
function runFlow(options = {}) {
  const { app, parent, collection, item, plan, firstEntry, firstApproval, secondId } = setup(options);
  const secondActor = options.secondActor ?? 'reviewer';
  const review = call(app, 'governance.submit', { planId: plan.id, assigneeId: secondActor }, 'editor').review;
  if (options.explicitSecond) call(app, 'governance.checklist.record', { planId: plan.id,
    stageId: 'publication', checklistId: secondId, completed: true,
    evidence: 'Publication permission separately confirmed with the content owners.' }, secondActor);
  const progress = call(app, 'governance.plans.get', { id: plan.id }, 'editor');
  const beforeDecision = state(app);
  const decision = app.dispatch('reviews.decide', { id: review.id, decision: 'approve' }, secondActor);
  const decisionAtomic = decision.ok ? null : state(app) === beforeDecision;
  const prepared = call(app, 'governance.publication.prepare', { documentIds: [item.id] });
  const selectedApproval = decision.ok ? decision.data.approval : firstApproval;
  const beforePublication = state(app);
  const published = app.dispatch('releases.publish', {
    name: 'Reviewed operational edition', entries: [{ approvalId: selectedApproval.id }],
  });
  const publicationAtomic = published.ok ? null : state(app) === beforePublication;
  const stored = app.inspect();
  return {
    inheritedPolicy: call(app, 'governance.policies.effective', { collectionId: collection.id }).policy.assignedCollectionId === parent.id,
    beforeSatisfied: progress.stages.map(stage => stage.checklistSatisfied),
    evidenceStages: progress.stages.map(stage => stage.checklist[0].entry?.stageId ?? null),
    decision: decision.ok ? 'approved' : decision.error.code,
    decisionAtomic,
    finalReviewStatus: stored.reviews[review.id].status,
    approvalCount: Object.keys(stored.approvals).length,
    ready: prepared.ready,
    publication: published.ok ? 'published' : published.error.code,
    publicationAtomic,
    releaseCount: Object.keys(stored.releases).length,
    releaseSourcePinned: published.ok ? published.data.entries[0].revisionId === item.revision.id : null,
    stage2RecordCount: Object.values(stored.governanceChecklist).filter(entry => entry.planId === plan.id && entry.stageId === 'publication').length,
    firstRecordUnchanged: JSON.stringify(stored.governanceChecklist[firstEntry.id]) === JSON.stringify(firstEntry),
  };
}
function distinctControl() {
  const { app, plan } = setup({ distinct: true });
  const before = state(app);
  const response = app.dispatch('governance.submit', { planId: plan.id, assigneeId: 'reviewer' }, 'editor');
  return { code: response.ok ? 'accepted' : response.error.code, atomic: state(app) === before };
}
function reassignmentControl() {
  const { app, plan } = setup();
  const submitted = call(app, 'governance.submit', { planId: plan.id, assigneeId: 'reviewer' }, 'editor');
  call(app, 'reviews.assign', { id: submitted.review.id, assigneeId: 'admin' });
  const before = state(app);
  const decision = app.dispatch('reviews.decide', { id: submitted.review.id, decision: 'approve' });
  return { actor: app.inspect().reviews[submitted.review.id].assigneeId,
    code: decision.ok ? 'approved' : decision.error.code, atomic: state(app) === before,
    stage2Records: Object.values(app.inspect().governanceChecklist).filter(entry => entry.stageId === 'publication').length };
}
function isolatedPlanControl() {
  const { app, collection } = setup();
  const item = call(app, 'documents.create', { collectionId: collection.id, title: 'Separate handbook',
    body: 'A distinct document with independent review requirements.' }, 'editor');
  const { plan } = call(app, 'governance.plans.create', { documentId: item.id }, 'editor');
  const review = call(app, 'governance.submit', { planId: plan.id, assigneeId: 'reviewer' }, 'editor').review;
  const before = state(app);
  const decision = app.dispatch('reviews.decide', { id: review.id, decision: 'approve' }, 'reviewer');
  return { code: decision.ok ? 'approved' : decision.error.code, atomic: state(app) === before };
}
function ungovernedPreviewControl() {
  const app = createApplication();
  const collection = call(app, 'collections.create', { name: 'Independent notes' });
  for (const [actorId, role] of [['editor', 'editor'], ['reviewer', 'reviewer'], ['reader', 'reader']])
    call(app, 'members.assign', { collectionId: collection.id, actorId, role });
  const item = call(app, 'documents.create', { collectionId: collection.id,
    title: 'Published note', body: 'Version one is approved.' }, 'editor');
  const review = call(app, 'reviews.open', { documentId: item.id, assigneeId: 'reviewer' }, 'editor');
  const approval = call(app, 'reviews.decide', { id: review.id, decision: 'approve' }, 'reviewer').approval;
  const release = call(app, 'releases.publish', { name: 'First edition', entries: [{ approvalId: approval.id }] }, 'reviewer');
  call(app, 'documents.revise', { id: item.id, title: 'Draft note', body: 'Version two remains a draft.' }, 'editor');
  const preview = call(app, 'preview.document', { documentId: item.id, includeHistory: true }, 'editor');
  const storedRelease = call(app, 'releases.get', { id: release.id }, 'reader');
  const job = call(app, 'exports.create', { releaseId: release.id, requestKey: 'private-publication-observation', format: 'text' }, 'reader');
  const artifact = call(app, 'exports.run', { id: job.id }, 'reader').artifact;
  return { ordinaryApproval: true, currentPreview: preview.body === 'Version two remains a draft.',
    previewHistory: Array.isArray(preview.history), historySummariesOnly: preview.history === undefined ? true : preview.history.every(entry => !Object.hasOwn(entry, 'body')),
    storedReleasePinned: storedRelease.entries[0].revisionId === item.revision.id,
    exportedApprovalBody: artifact.content.includes('Version one is approved.') && !artifact.content.includes('Version two remains a draft.'),
    approvalUnchanged: JSON.stringify(app.inspect().approvals[approval.id]) === JSON.stringify(approval) };
}
console.log(JSON.stringify({
  repeatedItemWithoutStageEvidence: runFlow(),
  repeatedItemWithStageEvidence: runFlow({ explicitSecond: true }),
  differentItemWithoutStageEvidence: runFlow({ secondId: 'rights' }),
  separateReviewerWithStageEvidence: runFlow({ secondActor: 'admin', explicitSecond: true, distinct: true }),
  distinctReviewerRequired: distinctControl(),
  reassignedStageNeedsCurrentActor: reassignmentControl(),
  otherPlanCannotSupplyEvidence: isolatedPlanControl(),
  workingPreviewAndStoredPublication: ungovernedPreviewControl(),
}));
'''

result = subprocess.run(["node", "--input-type=module", "-e", SCRIPT, sys.argv[1]],
                        text=True, capture_output=True, check=True, timeout=8)
sys.stdout.write(result.stdout)
