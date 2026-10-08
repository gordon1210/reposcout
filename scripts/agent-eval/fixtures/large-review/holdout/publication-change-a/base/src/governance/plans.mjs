import { allocate } from '../core/identity.mjs';
import { text, choice } from '../core/validation.mjs';
import { record } from '../core/events.mjs';
import { requireValue } from '../core/errors.mjs';
import { document, entity, revision } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { openReview, decideReview } from '../workflow/service.mjs';
import { effectivePolicy } from './policies.mjs';
import { sourceIdentity } from './facts.mjs';
import { createAssessment, requireAssessment } from './assessments.mjs';

function planContext(state, planId, actorId, action) {
  const plan = entity(state, 'governancePlans', planId);
  const item = document(state, plan.source.documentId);
  authorize(state, actorId, action, item.collectionId);
  const selected = revision(state, plan.source.revisionId, item.id);
  requireValue(sourceIdentity(selected).digest === plan.source.digest,
    'stale_plan', 'selected revision differs from the plan source');
  return { plan, item, selected };
}

function requireCurrentPolicy(state, plan, item) {
  const current = effectivePolicy(state, item.collectionId);
  requireValue(item.collectionId === plan.collectionId && current?.fingerprint === plan.policy.fingerprint,
    'stale_plan', 'policy or collection changed; create a new plan');
}

export function stageProgress(state, plan) {
  return plan.policy.stages.map((stage, index) => {
    const link = Object.values(state.governanceStageReviews).find(item => item.planId === plan.id && item.stageId === stage.id);
    const review = link ? state.reviews[link.reviewId] : null;
    const records = Object.values(state.governanceChecklist).filter(item => item.planId === plan.id && item.stageId === stage.id)
      .sort((a, b) => a.ordinal - b.ordinal);
    const latest = new Map(records.map(item => [item.checklistId, item]));
    const checklist = stage.checklist.map(item => {
      const entry = latest.get(item.id) ?? null;
      return { ...item, entry, satisfied: entry !== null && entry.completed &&
        entry.actorId === review?.assigneeId && (!item.evidenceRequired || entry.evidence.trim().length > 0) };
    });
    return { index, stageId: stage.id, name: stage.name, reviewId: review?.id ?? null,
      reviewerId: review?.assigneeId ?? null, status: review?.status ?? 'pending',
      checklist, checklistSatisfied: checklist.every(item => item.satisfied) };
  });
}

export function createPlan(state, input, actorId) {
  const item = document(state, input.documentId);
  authorize(state, actorId, 'edit', item.collectionId);
  const selected = revision(state, input.revisionId ?? item.currentRevisionId, item.id);
  const policy = effectivePolicy(state, item.collectionId);
  requireValue(policy !== null, 'no_policy', 'collection has no review policy');
  const assessment = input.assessmentId === undefined ?
    createAssessment(state, { documentId: item.id, revisionId: selected.id }, actorId) :
    entity(state, 'governanceAssessments', input.assessmentId);
  requireAssessment(state, assessment, item, selected, policy);
  const id = allocate(state, 'governance_plan');
  const plan = { id, collectionId: item.collectionId, source: sourceIdentity(selected),
    assessmentId: assessment.id, policy: structuredClone(policy), createdBy: actorId };
  state.governancePlans[id] = plan;
  record(state, actorId, 'governance.planned', item.id, { planId: id, revisionId: selected.id });
  return { plan, stages: stageProgress(state, plan) };
}

export function getPlan(state, input, actorId) {
  const plan = entity(state, 'governancePlans', input.id);
  const item = document(state, plan.source.documentId, { archived: true });
  authorize(state, actorId, 'read', item.collectionId);
  const current = effectivePolicy(state, item.collectionId);
  const stages = stageProgress(state, plan);
  const selected = state.revisions[plan.source.revisionId];
  return { plan, stages, complete: stages.every(stage => stage.status === 'approved' && stage.checklistSatisfied),
    current: current?.fingerprint === plan.policy.fingerprint && item.collectionId === plan.collectionId &&
      selected !== undefined && sourceIdentity(selected).digest === plan.source.digest };
}

export function submitStage(state, input, actorId) {
  const { plan, item } = planContext(state, input.planId, actorId, 'edit');
  requireCurrentPolicy(state, plan, item);
  const progress = stageProgress(state, plan);
  const next = progress.find(stage => stage.status !== 'approved');
  requireValue(next !== undefined, 'completed_plan', 'all review stages are approved');
  requireValue(next.status === 'pending', 'stage_unavailable', 'next stage already has a review', { status: next.status });
  requireValue(progress.slice(0, next.index).every(stage => stage.checklistSatisfied),
    'checklist_incomplete', 'previous review evidence is incomplete');
  const stage = plan.policy.stages[next.index];
  const assigneeId = text(input.assigneeId, 'reviewer id', { max: 100 });
  requireValue(!stage.distinctReviewer || !progress.slice(0, next.index).some(item => item.reviewerId === assigneeId),
    'distinct_reviewer', 'stage requires a reviewer different from previous stages');
  const review = openReview(state, { documentId: item.id, revisionId: plan.source.revisionId,
    assigneeId, note: input.note ?? '' }, actorId);
  const id = allocate(state, 'stage_review');
  state.governanceStageReviews[id] = { id, planId: plan.id, stageId: stage.id, reviewId: review.id };
  record(state, actorId, 'governance.stage_submitted', item.id, { planId: plan.id, stageId: stage.id, reviewId: review.id });
  return { planId: plan.id, stageId: stage.id, review };
}

export function recordChecklist(state, input, actorId) {
  const { plan, item } = planContext(state, input.planId, actorId, 'review');
  requireCurrentPolicy(state, plan, item);
  const stage = plan.policy.stages.find(item => item.id === input.stageId);
  requireValue(stage !== undefined, 'not_found', 'review stage does not exist');
  const checklist = stage.checklist.find(item => item.id === input.checklistId);
  requireValue(checklist !== undefined, 'not_found', 'checklist item does not exist');
  const status = stageProgress(state, plan).find(item => item.stageId === stage.id);
  requireValue(status.status === 'open' && status.reviewerId === actorId,
    'wrong_reviewer', 'only the active stage reviewer can record checklist evidence');
  requireValue(typeof input.completed === 'boolean', 'invalid_input', 'completed must be boolean');
  const evidence = text(input.evidence ?? '', 'checklist evidence', { max: 4000, empty: true });
  requireValue(!input.completed || !checklist.evidenceRequired || evidence.trim().length > 0,
    'evidence_required', 'this checklist item needs written evidence');
  const entries = Object.values(state.governanceChecklist).filter(entry => entry.planId === plan.id &&
    entry.stageId === stage.id && entry.checklistId === checklist.id);
  const previous = entries.sort((a, b) => b.ordinal - a.ordinal)[0] ?? null;
  requireValue((input.expectedEntryId ?? null) === (previous?.id ?? null),
    'checklist_conflict', 'checklist evidence changed since it was read');
  const id = allocate(state, 'checklist_entry');
  const entry = { id, planId: plan.id, stageId: stage.id, checklistId: checklist.id,
    source: structuredClone(plan.source), policyFingerprint: plan.policy.fingerprint,
    ordinal: (previous?.ordinal ?? 0) + 1, supersedesId: previous?.id ?? null,
    completed: input.completed, evidence, actorId };
  state.governanceChecklist[id] = entry;
  record(state, actorId, 'governance.checklist_recorded', item.id, { entryId: id, planId: plan.id });
  return entry;
}

export function governedOpenReview(state, input, actorId) {
  const item = document(state, input.documentId);
  authorize(state, actorId, 'edit', item.collectionId);
  if (effectivePolicy(state, item.collectionId) === null) return openReview(state, input, actorId);
  requireValue(input.planId !== undefined, 'governance_plan_required', 'create and submit a governance plan');
  const plan = entity(state, 'governancePlans', input.planId);
  requireValue(plan.source.documentId === item.id && (input.revisionId === undefined ||
    plan.source.revisionId === input.revisionId), 'revision_mismatch', 'plan does not select the requested source');
  return submitStage(state, input, actorId).review;
}

export function governedDecideReview(state, input, actorId) {
  const review = entity(state, 'reviews', input.id);
  const item = document(state, review.documentId);
  authorize(state, actorId, 'review', item.collectionId);
  const decision = choice(input.decision, ['approve', 'reject'], 'review decision');
  const link = Object.values(state.governanceStageReviews).find(item => item.reviewId === review.id);
  // Rejections remain available when policies change so stale reviews can be closed.
  if (decision === 'approve' && link) {
    const { plan } = planContext(state, link.planId, actorId, 'review');
    requireCurrentPolicy(state, plan, item);
    const progress = stageProgress(state, plan);
    const stage = progress.find(stage => stage.stageId === link.stageId);
    requireValue(progress.slice(0, stage.index).every(item => item.status === 'approved' && item.checklistSatisfied),
      'stage_order', 'previous review stages must be complete');
    requireValue(stage.checklistSatisfied, 'checklist_incomplete', 'complete all required checklist evidence');
    requireValue(!plan.policy.stages[stage.index].distinctReviewer ||
      !progress.slice(0, stage.index).some(item => item.reviewerId === actorId),
    'distinct_reviewer', 'stage requires a different reviewer');
  } else if (decision === 'approve') {
    requireValue(effectivePolicy(state, item.collectionId) === null,
      'governance_plan_required', 'configured documents require a governance plan');
  }
  return decideReview(state, input, actorId);
}
