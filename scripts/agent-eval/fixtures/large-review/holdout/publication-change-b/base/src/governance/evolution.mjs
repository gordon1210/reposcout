import { digest } from '../core/identity.mjs';
import { integer } from '../core/validation.mjs';
import { requireValue } from '../core/errors.mjs';
import { entity } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { sourceIdentity } from './facts.mjs';
import { resolveVersion, effectivePolicy, assignPolicy } from './policies.mjs';
import { assessSource } from './assessments.mjs';
import { approvalReadiness } from './publication.mjs';
import { stageProgress } from './plans.mjs';

function equal(left, right) { return JSON.stringify(left) === JSON.stringify(right); }

function changedFields(before, after, fields) {
  return fields.filter(field => !equal(before[field], after[field]))
    .map(field => ({ field, before: before[field], after: after[field] }));
}

function compareEntries(before, after, fields) {
  const old = new Map(before.map((item, index) => [item.id, { item, index }]));
  const next = new Map(after.map((item, index) => [item.id, { item, index }]));
  const result = [];
  for (const id of new Set([...old.keys(), ...next.keys()])) {
    const left = old.get(id);
    const right = next.get(id);
    if (!left) result.push({ id, change: 'added', after: right.item, toIndex: right.index });
    else if (!right) result.push({ id, change: 'removed', before: left.item, fromIndex: left.index });
    else {
      const fieldsChanged = changedFields(left.item, right.item, fields);
      if (fieldsChanged.length || left.index !== right.index) result.push({ id, change: 'changed',
        fields: fieldsChanged, fromIndex: left.index, toIndex: right.index });
    }
  }
  return result;
}

export function compareResolvedPolicies(before, after) {
  const rules = compareEntries(before?.rules ?? [], after?.rules ?? [], ['message', 'severity', 'when', 'require']);
  const stages = compareEntries(before?.stages ?? [], after?.stages ?? [], ['name', 'distinctReviewer']);
  for (const stage of after?.stages ?? []) {
    const old = before?.stages.find(item => item.id === stage.id);
    if (!old) continue;
    const checklist = compareEntries(old.checklist, stage.checklist, ['label', 'evidenceRequired']);
    if (checklist.length) {
      let change = stages.find(item => item.id === stage.id);
      if (!change) {
        change = { id: stage.id, change: 'changed', fields: [],
          fromIndex: before.stages.indexOf(old), toIndex: after.stages.indexOf(stage) };
        stages.push(change);
      }
      change.checklist = checklist;
    }
  }
  return {
    beforeVersionId: before?.versionId ?? null, afterVersionId: after?.versionId ?? null,
    identityChanged: (before?.fingerprint ?? null) !== (after?.fingerprint ?? null),
    semanticsChanged: rules.length > 0 || stages.length > 0,
    rules, stages,
  };
}

export function comparePolicyVersions(state, input, actorId) {
  authorize(state, actorId, 'manage');
  return compareResolvedPolicies(resolveVersion(state, input.beforeVersionId), resolveVersion(state, input.afterVersionId));
}

function descendantCollections(state, rootId) {
  const children = new Map();
  for (const item of Object.values(state.collections)) {
    if (!children.has(item.parentId)) children.set(item.parentId, []);
    children.get(item.parentId).push(item.id);
  }
  const seen = new Set();
  const queue = [rootId];
  for (let index = 0; index < queue.length; index += 1) {
    const id = queue[index];
    requireValue(!seen.has(id) && seen.size < 1000, 'planning_limit', 'assignment subtree is cyclic or exceeds 1000 collections');
    seen.add(id);
    queue.push(...(children.get(id) ?? []).sort());
  }
  return seen;
}

function recordGroups(state) {
  const result = new Map();
  function add(kind, record, documentId) {
    if (!result.has(documentId)) result.set(documentId, []);
    result.get(documentId).push({ kind, record });
  }
  for (const record of Object.values(state.governanceAssessments)) add('assessment', record, record.source.documentId);
  for (const record of Object.values(state.governancePlans)) add('plan', record, record.source.documentId);
  for (const record of Object.values(state.approvals)) add('approval', record, record.documentId);
  for (const records of result.values()) records.sort((a, b) => a.kind.localeCompare(b.kind) || a.record.id.localeCompare(b.record.id));
  return result;
}

function recordImpact(state, item, kind, record, before, after) {
  if (kind === 'approval') {
    const old = approvalReadiness(state, record, before);
    const next = approvalReadiness(state, record, after);
    return { kind, id: record.id, revisionId: record.revisionId, usableBefore: old.eligible,
      usableAfter: next.eligible, reasonsBefore: old.reasons, reasonsAfter: next.reasons };
  }
  const selected = state.revisions[record.source.revisionId];
  const sourceMatches = selected !== undefined && sourceIdentity(selected).digest === record.source.digest;
  const recordedFingerprint = kind === 'assessment' ? record.policyFingerprint : record.policy.fingerprint;
  const oldMatches = recordedFingerprint === (before?.fingerprint ?? null);
  const nextMatches = recordedFingerprint === (after?.fingerprint ?? null);
  const reasons = [];
  if (!sourceMatches) reasons.push(selected ? 'source_changed' : 'source_unavailable');
  if (record.collectionId !== item.collectionId) reasons.push('collection_changed');
  if (!nextMatches) reasons.push('policy_changed');
  if (kind === 'assessment' && !record.passed) reasons.push('failed_assessment');
  const rejected = kind === 'plan' && stageProgress(state, record).some(stage => stage.status === 'rejected');
  if (rejected) reasons.push('rejected_plan');
  return { kind, id: record.id, revisionId: record.source.revisionId,
    usableBefore: sourceMatches && oldMatches && record.collectionId === item.collectionId &&
      (kind !== 'assessment' || record.passed) && !rejected,
    usableAfter: reasons.length === 0, reasonsAfter: reasons };
}

export function previewAssignment(state, input, actorId) {
  const collection = entity(state, 'collections', input.collectionId);
  authorize(state, actorId, 'manage', collection.id);
  const assignment = state.governanceAssignments[collection.id] ?? null;
  requireValue((input.expectedAssignmentId ?? null) === (assignment?.id ?? null),
    'policy_conflict', 'collection policy assignment changed');
  let targetPolicy = null;
  if (input.versionId !== null) {
    const version = entity(state, 'governanceVersions', input.versionId);
    targetPolicy = entity(state, 'governancePolicies', version.policyId);
    requireValue(input.expectedVersionId === targetPolicy.latestVersionId,
      'policy_conflict', 'target policy gained another version');
    resolveVersion(state, version.id);
  }
  const documentLimit = integer(input.documentLimit ?? 100, 'document limit', 1, 500);
  const recordLimit = integer(input.recordLimit ?? 500, 'record limit', 1, 2000);
  const subtree = descendantCollections(state, collection.id);
  const all = Object.values(state.documents).filter(item => !item.archived && subtree.has(item.collectionId))
    .sort((a, b) => a.id.localeCompare(b.id));
  const selected = all.slice(0, documentLimit);
  const groups = recordGroups(state);
  const proposed = { id: 'proposed', collectionId: collection.id, versionId: input.versionId };
  let remaining = recordLimit;
  let omittedRecords = 0;
  const documents = selected.map(item => {
    // A descendant-local policy assignment can shield a document from this change.
    authorize(state, actorId, 'read', item.collectionId);
    const before = effectivePolicy(state, item.collectionId);
    const after = effectivePolicy(state, item.collectionId, proposed);
    const difference = compareResolvedPolicies(before, after);
    const working = entity(state, 'revisions', item.currentRevisionId);
    const assessment = assessSource(state, item, working, after);
    const records = groups.get(item.id) ?? [];
    const admitted = records.slice(0, remaining);
    remaining -= admitted.length;
    omittedRecords += records.length - admitted.length;
    return { documentId: item.id, collectionId: item.collectionId, workingRevisionId: working.id,
      source: sourceIdentity(working), affected: difference.identityChanged, difference,
      proposedAssessment: { passed: assessment.passed, blockingRuleIds: assessment.blockingRuleIds,
        warningCount: assessment.warningCount },
      records: admitted.map(({ kind, record }) => recordImpact(state, item, kind, record, before, after)),
      omittedRecords: records.length - admitted.length };
  });
  const result = {
    collectionId: collection.id, expectedAssignmentId: assignment?.id ?? null,
    proposedVersionId: input.versionId, targetLatestVersionId: targetPolicy?.latestVersionId ?? null,
    documents, omittedDocuments: all.length - selected.length, omittedRecords,
    complete: all.length === selected.length && omittedRecords === 0,
    totals: { documents: all.length, inspectedDocuments: selected.length,
      affectedDocuments: documents.filter(item => item.affected).length,
      blockingDocuments: documents.filter(item => !item.proposedAssessment.passed).length,
      newlyUnusableRecords: documents.flatMap(item => item.records).filter(item => item.usableBefore && !item.usableAfter).length },
  };
  return { ...result, impactDigest: digest(JSON.stringify(result)) };
}

export function applyAssignmentPreview(state, input, actorId) {
  const preview = previewAssignment(state, input, actorId);
  requireValue(preview.complete, 'incomplete_preview', 'increase preview limits before applying the assignment');
  requireValue(input.impactDigest === preview.impactDigest,
    'impact_conflict', 'assignment impact changed; preview it again');
  const assignment = assignPolicy(state, { collectionId: input.collectionId, versionId: input.versionId,
    expectedAssignmentId: input.expectedAssignmentId }, actorId);
  return { assignment, impactDigest: preview.impactDigest, totals: preview.totals };
}
