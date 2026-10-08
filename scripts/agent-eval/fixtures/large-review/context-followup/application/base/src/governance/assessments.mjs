import { allocate } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { requireValue } from '../core/errors.mjs';
import { document, revision, entity } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { sourceIdentity, contentFacts } from './facts.mjs';
import { effectivePolicy } from './policies.mjs';
import { evaluateRules } from './evaluate.mjs';

export function assessSource(state, item, selected, policy) {
  const source = sourceIdentity(selected);
  const facts = contentFacts(selected);
  const evaluation = policy === null ? { passed: true, blockingRuleIds: [], warningCount: 0, results: [] } :
    evaluateRules(policy, facts);
  return { source, collectionId: item.collectionId, policyVersionId: policy?.versionId ?? null,
    policyFingerprint: policy?.fingerprint ?? null, facts, ...evaluation };
}

export function previewAssessment(state, input, actorId) {
  const item = document(state, input.documentId);
  authorize(state, actorId, 'edit', item.collectionId);
  const selected = revision(state, input.revisionId ?? item.currentRevisionId, item.id);
  return assessSource(state, item, selected, effectivePolicy(state, item.collectionId));
}

export function createAssessment(state, input, actorId) {
  const result = previewAssessment(state, input, actorId);
  const existing = Object.values(state.governanceAssessments).find(item =>
    item.source.digest === result.source.digest && item.policyFingerprint === result.policyFingerprint &&
    item.collectionId === result.collectionId);
  if (existing) return existing;
  const id = allocate(state, 'assessment');
  const assessment = { id, ...result, createdBy: actorId };
  state.governanceAssessments[id] = assessment;
  record(state, actorId, 'governance.assessed', result.source.documentId,
    { assessmentId: id, revisionId: result.source.revisionId, passed: result.passed });
  return assessment;
}

export function getAssessment(state, input, actorId) {
  const assessment = entity(state, 'governanceAssessments', input.id);
  const item = document(state, assessment.source.documentId, { archived: true });
  authorize(state, actorId, 'read', item.collectionId);
  const selected = state.revisions[assessment.source.revisionId];
  const policy = effectivePolicy(state, item.collectionId);
  return { assessment, sourceAvailable: selected !== undefined,
    matchesSource: selected !== undefined && sourceIdentity(selected).digest === assessment.source.digest,
    matchesPolicy: (policy?.fingerprint ?? null) === assessment.policyFingerprint,
    matchesCollection: item.collectionId === assessment.collectionId };
}

export function requireAssessment(state, assessment, item, selected, policy) {
  requireValue(assessment.source.documentId === item.id && assessment.source.revisionId === selected.id &&
    assessment.source.digest === sourceIdentity(selected).digest,
  'stale_assessment', 'assessment does not describe the selected revision');
  requireValue(assessment.collectionId === item.collectionId &&
    assessment.policyFingerprint === (policy?.fingerprint ?? null),
  'stale_assessment', 'assessment policy or collection has changed');
  requireValue(assessment.passed, 'governance_blocked', 'document fails required policy checks',
    { assessmentId: assessment.id, ruleIds: assessment.blockingRuleIds });
}
