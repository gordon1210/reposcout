import { allocate } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { requireValue } from '../core/errors.mjs';
import { text } from '../core/validation.mjs';
import { authorize } from '../permissions/policy.mjs';
import { entity } from '../storage/lookup.mjs';
import { transaction } from '../storage/transaction.mjs';
import { identity } from './manifest.mjs';
import { importRequest, executeImport, planIdentity } from './import-plan.mjs';

export function applyBatchImport(state, input, actorId) {
  const request = importRequest(input);
  const requestKey = text(input.requestKey, 'batch request key', { max: 200 });
  const requestIdentity = identity(request);
  const receipts = state.exchangeReceipts ?? {};
  const prior = Object.values(receipts).find(item => item.actorId === actorId && item.requestKey === requestKey);
  if (prior) {
    requireValue(prior.requestIdentity === requestIdentity, 'exchange_request_conflict', 'request key was used for another batch');
    for (const item of prior.imported) {
      const current = entity(state, 'documents', item.documentId);
      authorize(state, actorId, 'edit', current.collectionId);
    }
    return { ...structuredClone(prior), replayed: true };
  }
  requireValue(input.planToken === planIdentity(state, request, actorId), 'exchange_stale_plan',
    'preview this exact batch again before applying it');
  return transaction(state, () => {
    const result = executeImport(state, request, actorId);
    requireValue(request.policy !== 'atomic' || result.rejected.length === 0,
      'exchange_atomic_rejected', 'atomic import rejected every document', { diagnostics: result.diagnostics });
    requireValue(result.imported.length > 0, 'exchange_empty_result', 'no import component could be applied',
      { diagnostics: result.diagnostics });
    const id = allocate(state, 'exchange');
    const receipt = { id, actorId, requestKey, requestIdentity,
      manifestIdentity: identity(request.manifest), origin: request.manifest.origin,
      policy: request.policy, ...result, sequence: state.sequence + 1 };
    state.exchangeReceipts ??= {};
    state.exchangeReceipts[id] = receipt;
    record(state, actorId, 'exchange.imported', id, {
      manifestIdentity: receipt.manifestIdentity,
      documents: result.imported.map(item => item.documentId), rejected: result.rejected.map(item => item.key),
    });
    return { ...structuredClone(receipt), replayed: false };
  });
}

export function getImportReceipt(state, input, actorId) {
  const item = entity(state, 'exchangeReceipts', input.id);
  requireValue(item.actorId === actorId || state.users[actorId]?.role === 'admin',
    'forbidden', 'import receipt belongs to another actor');
  for (const entry of item.imported) {
    authorize(state, actorId, 'read', entity(state, 'documents', entry.documentId).collectionId);
  }
  return structuredClone(item);
}
