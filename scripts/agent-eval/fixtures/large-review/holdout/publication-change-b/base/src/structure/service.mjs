import { document, revision, entity } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { reviseDocument } from '../documents/service.mjs';
import { allocate, digest } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { requireValue } from '../core/errors.mjs';
import { text, integer, choice } from '../core/validation.mjs';
import { parseStructure } from './parse.mjs';
import { planSectionOperations } from './operations.mjs';
import { diffLines } from './diff.mjs';
import { mergeBodies, materializeMerge } from './merge.mjs';

function selectedDocument(state, input, actorId, action = 'read') {
  const item = document(state, input.documentId);
  authorize(state, actorId, action, item.collectionId);
  return item;
}

function guardedRevision(state, item, input) {
  requireValue(input.expectedRevisionId === item.currentRevisionId, 'revision_conflict', 'current revision changed');
  const current = revision(state, item.currentRevisionId, item.id);
  requireValue(input.expectedChecksum === current.checksum, 'checksum_conflict', 'current checksum changed');
  return current;
}

export function getStructure(state, input, actorId) {
  const item = selectedDocument(state, input, actorId);
  const selected = revision(state, input.revisionId ?? item.currentRevisionId, item.id);
  return { documentId: item.id, revisionId: selected.id, ...parseStructure(selected.body) };
}

export function compareStructure(state, input, actorId) {
  const item = selectedDocument(state, input, actorId);
  const before = revision(state, input.fromRevisionId, item.id);
  const after = revision(state, input.toRevisionId ?? item.currentRevisionId, item.id);
  return { documentId: item.id, fromRevisionId: before.id, toRevisionId: after.id,
    ...diffLines(before.body, after.body, input.maximumCells ?? 1000000) };
}

export function previewSections(state, input, actorId) {
  const item = selectedDocument(state, input, actorId, 'edit');
  const current = guardedRevision(state, item, input);
  return { revisionId: current.id, ...planSectionOperations(current.body, input.expectedChecksum, input.operations) };
}

export function applySections(state, input, actorId) {
  const item = selectedDocument(state, input, actorId, 'edit');
  const current = guardedRevision(state, item, input);
  const plan = planSectionOperations(current.body, input.expectedChecksum, input.operations);
  requireValue(plan.changed, 'unchanged_document', 'section operations do not change the document');
  const result = reviseDocument(state, { id: item.id, expectedRevisionId: current.id, body: plan.body }, actorId);
  record(state, actorId, 'structure.applied', item.id, { revisionId: result.revision.id, operations: input.operations.length });
  return result;
}

function assertAncestor(state, item, baseId, currentId) {
  const visited = new Set();
  let cursor = currentId;
  while (cursor !== null) {
    requireValue(!visited.has(cursor), 'invalid_revision_chain', 'revision ancestry contains a cycle');
    if (cursor === baseId) return;
    visited.add(cursor);
    cursor = revision(state, cursor, item.id).parentId;
  }
  requireValue(false, 'unrelated_revision', 'merge base is not an ancestor of the current revision');
}

export function openMerge(state, input, actorId) {
  const item = selectedDocument(state, input, actorId, 'edit');
  const current = guardedRevision(state, item, input);
  const base = revision(state, input.baseRevisionId, item.id);
  assertAncestor(state, item, base.id, current.id);
  const incomingBody = text(input.body, 'incoming body', { empty: true, max: 200000 });
  const maximumCells = integer(input.maximumCells ?? 1000000, 'diff cell budget', 1, 2000000);
  const plan = mergeBodies(base.body, current.body, incomingBody, maximumCells);
  const id = allocate(state, 'structureMerge');
  const session = { id, documentId: item.id, baseRevisionId: base.id, currentRevisionId: current.id,
    currentChecksum: current.checksum, incomingChecksum: digest(incomingBody), actorId,
    status: 'open', version: 1, plan, resolutions: {}, resultRevisionId: null };
  state.structureMerges[id] = session;
  record(state, actorId, 'structure.merge_opened', item.id, { mergeId: id, conflicts: plan.conflicts.length });
  return session;
}

function sessionFor(state, input, actorId, write = false) {
  const session = entity(state, 'structureMerges', input.id);
  const item = document(state, session.documentId);
  authorize(state, actorId, write ? 'edit' : 'read', item.collectionId);
  if (write) {
    requireValue(session.actorId === actorId || state.users[actorId].role === 'admin', 'forbidden', 'merge is owned by another editor');
    requireValue(session.status === 'open', 'merge_closed', 'merge session is already closed');
    requireValue(input.expectedVersion === session.version, 'merge_version_conflict', 'merge session changed');
  }
  return { session, item };
}

export function getMerge(state, input, actorId) {
  return sessionFor(state, input, actorId).session;
}

export function resolveMerge(state, input, actorId) {
  const { session } = sessionFor(state, input, actorId, true);
  const conflict = session.plan.conflicts.find(item => item.id === input.conflictId);
  requireValue(conflict, 'unknown_conflict', 'conflict does not exist');
  const selected = choice(input.choice, ['base', 'current', 'incoming', 'custom'], 'resolution');
  const resolution = { choice: selected };
  if (selected === 'custom') resolution.body = text(input.body, 'resolved body', { empty: true, max: 200000 });
  session.resolutions[conflict.id] = resolution;
  session.version += 1;
  record(state, actorId, 'structure.merge_resolved', session.documentId, { mergeId: session.id, conflictId: conflict.id });
  return session;
}

export function commitMerge(state, input, actorId) {
  const { session, item } = sessionFor(state, input, actorId, true);
  const current = guardedRevision(state, item, { expectedRevisionId: session.currentRevisionId, expectedChecksum: session.currentChecksum });
  const body = materializeMerge(session.plan, session.resolutions);
  const result = reviseDocument(state, { id: item.id, expectedRevisionId: current.id, body }, actorId);
  session.status = 'committed';
  session.resultRevisionId = result.revision.id;
  session.version += 1;
  record(state, actorId, 'structure.merge_committed', item.id, { mergeId: session.id, revisionId: result.revision.id });
  return { merge: session, document: result };
}

export function abandonMerge(state, input, actorId) {
  const { session } = sessionFor(state, input, actorId, true);
  session.status = 'abandoned';
  session.version += 1;
  record(state, actorId, 'structure.merge_abandoned', session.documentId, { mergeId: session.id });
  return session;
}
