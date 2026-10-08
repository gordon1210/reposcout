import { allocate } from '../core/identity.mjs';
import { requireValue } from '../core/errors.mjs';
import { text } from '../core/validation.mjs';
import { record } from '../core/events.mjs';
import { authorize } from '../permissions/policy.mjs';
import { entity } from '../storage/lookup.mjs';
import { transaction } from '../storage/transaction.mjs';
import { canonical, identity } from './manifest.mjs';
import { portableLinks } from './references.mjs';
import { executeImport } from './import-plan.mjs';
import { getImportReceipt } from './import-service.mjs';
import { synchronizationRequest, synchronizationPlan, synchronizationToken } from './sync-plan.mjs';

function anchorFrom(item) {
  return { documentId: item.documentId, revisionId: item.revisionId,
    sourceIdentity: item.sourceIdentity, referenceIdentity: item.referenceIdentity };
}

export function importSynchronizationAnchors(state, input, actorId) {
  const receipt = getImportReceipt(state, input, actorId);
  return { origin: receipt.origin, manifestIdentity: receipt.manifestIdentity,
    anchors: Object.fromEntries(receipt.imported.map(item => [item.key, anchorFrom(item)])),
    collectionBindings: structuredClone(receipt.collections) };
}

function authorizeReceipt(state, receipt, actorId, action) {
  requireValue(receipt.actorId === actorId || state.users[actorId]?.role === 'admin',
    'forbidden', 'synchronization receipt belongs to another actor');
  for (const anchor of Object.values(receipt.anchors)) {
    authorize(state, actorId, action, entity(state, 'documents', anchor.documentId).collectionId);
  }
}

export function getSynchronization(state, input, actorId) {
  const receipt = entity(state, 'exchangeSyncReceipts', input.id);
  authorizeReceipt(state, receipt, actorId, 'read');
  return structuredClone(receipt);
}

export function applySynchronization(state, input, actorId) {
  const request = synchronizationRequest(input);
  const requestKey = text(input.requestKey, 'synchronization request key', { max: 200 });
  const requestIdentity = identity(request);
  const prior = Object.values(state.exchangeSyncReceipts ?? {})
    .find(receipt => receipt.actorId === actorId && receipt.requestKey === requestKey);
  if (prior) {
    requireValue(prior.requestIdentity === requestIdentity, 'exchange_request_conflict', 'synchronization key was used for another request');
    authorizeReceipt(state, prior, actorId, 'edit');
    return { ...structuredClone(prior), replayed: true };
  }
  requireValue(input.planToken === synchronizationToken(state, request, actorId),
    'exchange_stale_plan', 'preview this synchronization again before applying it');
  const plan = synchronizationPlan(state, request, actorId);
  requireValue(input.expectedSourceIdentities !== undefined &&
    canonical(input.expectedSourceIdentities) === canonical(plan.sourceIdentities),
  'exchange_source_conflict', 'confirm the complete incoming source identity map from preview');
  requireValue(!plan.blocked, 'exchange_sync_conflict', 'atomic synchronization has conflicts',
    { decisions: plan.decisions, diagnostics: plan.diagnostics });
  return transaction(state, () => {
    const imported = [];
    const rejected = [];
    const diagnostics = [...plan.diagnostics];
    const collections = {};
    for (const batch of plan.batches) {
      const result = executeImport(state, { ...batch,
        collectionBindings: { ...batch.collectionBindings, ...collections } }, actorId);
      imported.push(...result.imported);
      rejected.push(...result.rejected);
      Object.assign(collections, result.collections);
      requireValue(request.policy !== 'atomic' || result.rejected.length === 0,
        'exchange_sync_conflict', 'atomic synchronization failed during application', { diagnostics: result.diagnostics });
    }
    const unchanged = plan.decisions.filter(item => item.status === 'unchanged' && item.componentEligible);
    requireValue(imported.length > 0 || unchanged.length > 0, 'exchange_empty_result', 'no synchronization component could be applied',
      { decisions: plan.decisions, diagnostics });
    const destinations = new Map(Object.entries(request.anchors).map(([key, anchor]) => [key, anchor.documentId]));
    for (const item of imported) destinations.set(item.key, item.documentId);
    const sources = new Map(request.manifest.documents.map(item => [item.key, item]));
    for (const item of imported) {
      const original = sources.get(item.key);
      item.sourceIdentity = identity(original);
      item.referenceIdentity = identity(portableLinks(original.body)
        .map(link => ({ key: link.key, documentId: destinations.get(link.key) })));
    }
    const anchors = Object.fromEntries([
      ...Object.entries(request.anchors),
      ...unchanged.map(item => [item.key, request.anchors[item.key]]),
      ...imported.map(item => [item.key, anchorFrom(item)]),
    ]);
    const id = allocate(state, 'exchange_sync');
    const receipt = { id, actorId, requestKey, requestIdentity, origin: request.manifest.origin,
      manifestIdentity: identity(request.manifest), policy: request.policy, anchors, imported, rejected,
      unchanged: unchanged.map(item => item.key), decisions: plan.decisions, diagnostics,
      retired: plan.retired, collections, sequence: state.sequence + 1 };
    state.exchangeSyncReceipts ??= {};
    state.exchangeSyncReceipts[id] = receipt;
    record(state, actorId, 'exchange.synchronized', id, { imported: imported.map(item => item.documentId),
      unchanged: receipt.unchanged, manifestIdentity: receipt.manifestIdentity });
    return { ...structuredClone(receipt), replayed: false };
  });
}
