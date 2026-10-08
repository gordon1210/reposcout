import { DomainError, requireValue } from '../core/errors.mjs';
import { document, entity, revision } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { publishRelease } from '../releases/service.mjs';
import { sourceIdentity } from './facts.mjs';
import { effectivePolicy } from './policies.mjs';
import { stageProgress } from './plans.mjs';

function roleEvidence(state, actorId, collectionId) {
  try {
    authorize(state, actorId, 'review', collectionId);
    return { actorId, authorized: true, reason: null };
  } catch (error) {
    if (!(error instanceof DomainError)) throw error;
    return { actorId, authorized: false, reason: error.code };
  }
}

export function approvalReadiness(state, approval, proposedPolicy = undefined) {
  const item = document(state, approval.documentId);
  const selected = revision(state, approval.revisionId, item.id);
  const policy = proposedPolicy === undefined ? effectivePolicy(state, item.collectionId) : proposedPolicy;
  const reasons = [];
  const review = state.reviews[approval.reviewId];
  if (!review || review.status !== 'approved' || review.revisionId !== selected.id ||
    review.documentId !== item.id || review.assigneeId !== approval.reviewerId) reasons.push('approval_review_mismatch');
  if (selected.authorId === approval.reviewerId) reasons.push('self_approval');
  const link = Object.values(state.governanceStageReviews).find(item => item.reviewId === approval.reviewId);
  let plan = null;
  let stages = [];
  const roles = [];
  if (policy !== null) {
    if (!link) reasons.push('governance_plan_required');
    else {
      plan = entity(state, 'governancePlans', link.planId);
      if (plan.source.revisionId !== selected.id || plan.source.digest !== sourceIdentity(selected).digest)
        reasons.push('source_changed');
      if (plan.collectionId !== item.collectionId) reasons.push('collection_changed');
      if (plan.policy.fingerprint !== policy.fingerprint) reasons.push('policy_changed');
      const assessment = state.governanceAssessments[plan.assessmentId];
      if (!assessment || !assessment.passed || assessment.source.digest !== plan.source.digest ||
        assessment.policyFingerprint !== plan.policy.fingerprint) reasons.push('assessment_invalid');
      stages = stageProgress(state, plan);
      if (link.stageId !== plan.policy.stages.at(-1).id) reasons.push('intermediate_approval');
      if (!stages.every(stage => stage.status === 'approved')) reasons.push('stages_incomplete');
      if (!stages.every(stage => stage.checklistSatisfied)) reasons.push('checklist_incomplete');
      for (const stage of stages) {
        if (stage.reviewerId !== null) roles.push({ stageId: stage.stageId,
          ...roleEvidence(state, stage.reviewerId, item.collectionId) });
      }
      if (roles.some(role => !role.authorized)) reasons.push('reviewer_no_longer_authorized');
      if (stages.some((stage, index) => plan.policy.stages[index].distinctReviewer &&
        stages.slice(0, index).some(previous => previous.reviewerId === stage.reviewerId)))
        reasons.push('distinct_reviewer');
    }
  }
  return {
    approvalId: approval.id, documentId: item.id, revisionId: selected.id,
    source: sourceIdentity(selected), revisionSequence: selected.sequence,
    eligible: reasons.length === 0, reasons, policyVersionId: policy?.versionId ?? null,
    policyFingerprint: policy?.fingerprint ?? null, planId: plan?.id ?? null,
    assessmentId: plan?.assessmentId ?? null, roles,
    stages: stages.map(stage => ({ stageId: stage.stageId, status: stage.status,
      reviewId: stage.reviewId, reviewerId: stage.reviewerId, checklistSatisfied: stage.checklistSatisfied,
      checklist: stage.checklist.map(item => ({ id: item.id, satisfied: item.satisfied,
        evidenceEntryId: item.entry?.id ?? null })) })),
  };
}

export function preparePublication(state, input, actorId) {
  requireValue(Array.isArray(input.documentIds) && input.documentIds.length > 0 && input.documentIds.length <= 50,
    'invalid_input', 'prepare one to fifty documents');
  requireValue(new Set(input.documentIds).size === input.documentIds.length,
    'duplicate_document', 'each document may be planned once');
  const documents = input.documentIds.map(id => {
    const item = document(state, id);
    authorize(state, actorId, 'publish', item.collectionId);
    const approvals = Object.values(state.approvals).filter(approval => approval.documentId === id);
    requireValue(approvals.length <= 1000, 'planning_limit', 'document has too many approvals for one preparation');
    const candidates = approvals.map(approval => approvalReadiness(state, approval))
      .sort((a, b) => b.revisionSequence - a.revisionSequence || a.approvalId.localeCompare(b.approvalId));
    const selected = candidates.find(candidate => candidate.eligible) ?? null;
    return { documentId: id, workingRevisionId: item.currentRevisionId,
      selectedApprovalId: selected?.approvalId ?? null, selectedRevisionId: selected?.revisionId ?? null,
      ready: selected !== null, reasons: selected ? [] : candidates.length ? ['no_eligible_approval'] : ['no_approval'],
      candidates };
  });
  return { purpose: 'new-release-preparation', ready: documents.every(item => item.ready), documents,
    entries: documents.filter(item => item.ready).map(item => ({ approvalId: item.selectedApprovalId })) };
}

export function governedPublishRelease(state, input, actorId) {
  requireValue(Array.isArray(input.entries) && input.entries.length > 0 && input.entries.length <= 50,
    'invalid_input', 'a release needs between one and fifty approved documents');
  for (const entry of input.entries) {
    const approval = entity(state, 'approvals', entry.approvalId);
    const item = document(state, approval.documentId);
    authorize(state, actorId, 'publish', item.collectionId);
    if (effectivePolicy(state, item.collectionId) === null) continue;
    const readiness = approvalReadiness(state, approval);
    requireValue(readiness.eligible, 'governance_blocked', 'approval does not satisfy publication requirements', readiness);
  }
  return publishRelease(state, input, actorId);
}
