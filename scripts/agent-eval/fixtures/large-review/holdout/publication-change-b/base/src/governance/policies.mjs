import { allocate, digest } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { requireValue } from '../core/errors.mjs';
import { entity } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { validateDefinition, limits } from './schema.mjs';

export function resolveVersion(state, versionId) {
  const chain = [];
  const seen = new Set();
  let cursor = versionId;
  while (cursor !== null) {
    requireValue(!seen.has(cursor) && chain.length < 16,
      'invalid_policy', 'policy inheritance is cyclic or too deep');
    seen.add(cursor);
    const version = entity(state, 'governanceVersions', cursor);
    chain.push(version);
    cursor = version.definition.extendsVersionId;
  }
  const rules = new Map();
  let stages = null;
  for (const version of chain.reverse()) {
    for (const id of version.definition.removeRules) {
      requireValue(rules.has(id), 'invalid_policy', 'cannot remove an unknown inherited rule', { ruleId: id });
      rules.delete(id);
    }
    for (const rule of version.definition.rules) rules.set(rule.id, { ...rule, originVersionId: version.id });
    if (version.definition.stages !== null) stages = structuredClone(version.definition.stages);
    requireValue(rules.size <= limits.rules, 'invalid_policy', 'resolved policy has too many rules');
  }
  requireValue(stages !== null, 'invalid_policy', 'a policy must define or inherit review stages');
  const result = { versionId, lineage: chain.map(version => version.id), rules: [...rules.values()], stages };
  return { ...result, fingerprint: digest(JSON.stringify(result)) };
}

export function createPolicy(state, input, actorId) {
  authorize(state, actorId, 'manage');
  const definition = validateDefinition(input.definition);
  const id = allocate(state, 'policy');
  const versionId = allocate(state, 'policy_version');
  const policy = { id, name: definition.name, latestVersionId: versionId, versionCount: 1, createdBy: actorId };
  state.governancePolicies[id] = policy;
  state.governanceVersions[versionId] = { id: versionId, policyId: id, number: 1, definition, createdBy: actorId };
  resolveVersion(state, versionId);
  record(state, actorId, 'governance.policy_created', id, { versionId });
  return { policy, version: state.governanceVersions[versionId] };
}

export function revisePolicy(state, input, actorId) {
  authorize(state, actorId, 'manage');
  const policy = entity(state, 'governancePolicies', input.policyId);
  requireValue(input.expectedVersionId === policy.latestVersionId,
    'policy_conflict', 'policy changed since it was read');
  const definition = validateDefinition(input.definition);
  const id = allocate(state, 'policy_version');
  const version = { id, policyId: policy.id, number: policy.versionCount + 1, definition, createdBy: actorId };
  state.governanceVersions[id] = version;
  resolveVersion(state, id);
  policy.latestVersionId = id;
  policy.versionCount = version.number;
  policy.name = definition.name;
  record(state, actorId, 'governance.policy_revised', policy.id, { versionId: id });
  return { policy, version };
}

export function getPolicy(state, input, actorId) {
  authorize(state, actorId, 'manage');
  const policy = entity(state, 'governancePolicies', input.policyId);
  const versions = Object.values(state.governanceVersions).filter(version => version.policyId === policy.id)
    .sort((a, b) => a.number - b.number);
  const selected = entity(state, 'governanceVersions', input.versionId ?? policy.latestVersionId);
  requireValue(selected.policyId === policy.id, 'policy_mismatch', 'version belongs to another policy');
  return { policy, versions, resolved: resolveVersion(state, selected.id) };
}

export function assignPolicy(state, input, actorId) {
  entity(state, 'collections', input.collectionId);
  authorize(state, actorId, 'manage', input.collectionId);
  const previous = state.governanceAssignments[input.collectionId] ?? null;
  requireValue((input.expectedAssignmentId ?? null) === (previous?.id ?? null),
    'policy_conflict', 'collection policy assignment changed');
  // Null removes only the local override; parent requirements become effective again.
  if (input.versionId === null) {
    delete state.governanceAssignments[input.collectionId];
    record(state, actorId, 'governance.policy_inherited', input.collectionId);
    return { collectionId: input.collectionId, inherited: true };
  }
  const policy = resolveVersion(state, input.versionId);
  const id = allocate(state, 'policy_assignment');
  const assignment = { id, collectionId: input.collectionId, versionId: policy.versionId, assignedBy: actorId };
  state.governanceAssignments[input.collectionId] = assignment;
  record(state, actorId, 'governance.policy_assigned', input.collectionId, { assignmentId: id, versionId: policy.versionId });
  return assignment;
}

export function effectivePolicy(state, collectionId, proposedAssignment = null) {
  const seen = new Set();
  const path = [];
  let cursor = collectionId;
  while (cursor !== null) {
    requireValue(!seen.has(cursor) && seen.size < 128, 'collection_cycle', 'invalid collection ancestry');
    seen.add(cursor);
    path.push(cursor);
    const collection = entity(state, 'collections', cursor);
    const assignment = proposedAssignment?.collectionId === cursor ?
      (proposedAssignment.versionId === null ? null : proposedAssignment) : state.governanceAssignments?.[cursor];
    if (assignment) {
      const policy = resolveVersion(state, assignment.versionId);
      return { ...policy, assignmentId: assignment.id, assignedCollectionId: cursor, collectionPath: path };
    }
    cursor = collection.parentId;
  }
  return null;
}

export function inspectPolicy(state, input, actorId) {
  authorize(state, actorId, 'read', input.collectionId);
  return { collectionId: input.collectionId, policy: effectivePolicy(state, input.collectionId) };
}
