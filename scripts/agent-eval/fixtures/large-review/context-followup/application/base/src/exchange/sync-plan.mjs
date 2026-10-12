import { requireValue } from '../core/errors.mjs';
import { object, text } from '../core/validation.mjs';
import { authorize } from '../permissions/policy.mjs';
import { document } from '../storage/lookup.mjs';
import { identity, parseManifest } from './manifest.mjs';
import { importRequest, executeImport } from './import-plan.mjs';
import { portableLinks, referenceGraph, referenceComponents } from './references.mjs';

function hash(value, label) {
  requireValue(typeof value === 'string' && /^[a-f0-9]{64}$/u.test(value),
    'exchange_sync_anchor', `${label} must be a SHA-256 identity`);
  return value;
}

export function synchronizationRequest(input) {
  requireValue(input.targets === undefined, 'exchange_sync_anchor', 'synchronization uses anchors instead of import targets');
  const request = importRequest(input);
  object(input.anchors ?? {}, 'synchronization anchors');
  const anchors = Object.fromEntries(Object.entries(input.anchors ?? {}).map(([key, anchor]) => {
    object(anchor, 'synchronization anchor');
    requireValue(Object.keys(anchor).every(field => ['documentId', 'revisionId', 'sourceIdentity', 'referenceIdentity'].includes(field)),
      'exchange_sync_anchor', 'unknown synchronization anchor field', { key });
    return [key, { documentId: text(anchor.documentId, 'anchor document', { max: 100 }),
      revisionId: text(anchor.revisionId, 'anchor revision', { max: 100 }),
      sourceIdentity: hash(anchor.sourceIdentity, 'source identity'),
      referenceIdentity: hash(anchor.referenceIdentity, 'reference identity') }];
  }));
  requireValue(new Set(Object.values(anchors).map(anchor => anchor.documentId)).size === Object.keys(anchors).length,
    'exchange_duplicate_target', 'synchronization anchors must name distinct destination documents');
  requireValue(input.createMissing === undefined || typeof input.createMissing === 'boolean',
    'invalid_input', 'createMissing must be a boolean');
  return { ...request, anchors, createMissing: input.createMissing === true };
}

function anchorFor(request, key) {
  return Object.hasOwn(request.anchors, key) ? request.anchors[key] : null;
}

function referenceIdentity(item, request) {
  return identity(portableLinks(item.body).map(link => ({ key: link.key,
    documentId: anchorFor(request, link.key)?.documentId ?? `new:${link.key}` })));
}

function classifyDocument(state, item, request, actorId) {
  const sourceIdentity = identity(item);
  const anchor = anchorFor(request, item.key);
  const base = { key: item.key, sourceIdentity, documentId: anchor?.documentId ?? null,
    expectedRevisionId: anchor?.revisionId ?? null };
  if (!anchor) return { ...base, status: request.createMissing ? 'create' : 'unmapped',
    reason: request.createMissing ? 'new_source' : 'destination_required' };
  try {
    const current = document(state, anchor.documentId);
    authorize(state, actorId, 'edit', current.collectionId);
    if (Object.hasOwn(request.collectionBindings, item.collectionKey) &&
      request.collectionBindings[item.collectionKey] !== current.collectionId) {
      return { ...base, status: 'conflicting', reason: 'collection_mismatch' };
    }
    if (current.currentRevisionId !== anchor.revisionId) {
      return { ...base, status: 'conflicting', reason: 'destination_changed', currentRevisionId: current.currentRevisionId };
    }
    const referenceChanged = referenceIdentity(item, request) !== anchor.referenceIdentity;
    if (sourceIdentity === anchor.sourceIdentity && !referenceChanged) {
      return { ...base, status: 'unchanged', reason: 'identities_match' };
    }
    return { ...base, status: 'revise', reason: referenceChanged ? 'reference_binding_changed' : 'source_changed' };
  } catch (error) {
    if (!error.code) throw error;
    return { ...base, status: 'conflicting', reason: error.code };
  }
}

function rewriteUnchanged(body, decisions) {
  return body.replace(/\[([^\]\n]+)\]\(exchange:([a-zA-Z0-9][a-zA-Z0-9._-]{0,79})\)/gu,
    (source, label, key) => {
      const selected = decisions.get(key);
      return selected?.status === 'unchanged' ? `[${label}](doc:${selected.documentId})` : source;
    });
}

export function synchronizationPlan(state, request, actorId) {
  const graph = referenceGraph(request.manifest);
  const decisions = request.manifest.documents.map(item => classifyDocument(state, item, request, actorId));
  const byKey = new Map(decisions.map(item => [item.key, item]));
  for (const diagnostic of graph.diagnostics) {
    Object.assign(byKey.get(diagnostic.key), { status: 'conflicting', reason: diagnostic.code });
  }
  const components = referenceComponents(request.manifest, graph.edges).map(keys => {
    const blockedBy = keys.filter(key => ['conflicting', 'unmapped'].includes(byKey.get(key).status));
    const changes = keys.filter(key => ['create', 'revise'].includes(byKey.get(key).status));
    return { keys, blockedBy, changes, eligible: blockedBy.length === 0 };
  });
  const anyBlocked = components.some(component => !component.eligible);
  for (const component of components) {
    component.willApply = component.eligible && !(request.policy === 'atomic' && anyBlocked);
    for (const key of component.keys) {
      const decision = byKey.get(key);
      decision.componentEligible = component.willApply;
      decision.blockedBy = component.blockedBy;
    }
  }
  const batches = [];
  const simulation = { imported: [], rejected: [], diagnostics: [], collections: {} };
  const simulatedState = structuredClone(state);
  for (const component of components.filter(item => item.willApply && item.changes.length > 0)) {
    const actionable = new Set(component.changes);
    const documents = request.manifest.documents.filter(item => actionable.has(item.key))
      .map(item => ({ ...item, body: rewriteUnchanged(item.body, byKey) }));
    const manifest = parseManifest({ ...request.manifest, documents });
    const targets = Object.fromEntries(decisions.filter(item => actionable.has(item.key) && item.status === 'revise')
      .map(item => [item.key, { documentId: item.documentId, expectedRevisionId: item.expectedRevisionId }]));
    const batch = { manifest, policy: 'atomic', collectionBindings: request.collectionBindings,
      parentCollectionId: request.parentCollectionId, targets };
    batches.push(batch);
    const result = executeImport(simulatedState, { ...batch,
      collectionBindings: { ...batch.collectionBindings, ...simulation.collections } }, actorId);
    if (result.rejected.length > 0) {
      component.willApply = false;
      component.blockedBy = result.rejected.map(item => item.key);
      for (const key of component.keys) {
        byKey.get(key).componentEligible = false;
        byKey.get(key).blockedBy = component.blockedBy;
      }
    }
    simulation.imported.push(...result.imported);
    simulation.rejected.push(...result.rejected);
    simulation.diagnostics.push(...result.diagnostics);
    Object.assign(simulation.collections, result.collections);
  }
  const retired = Object.keys(request.anchors).filter(key => !byKey.has(key)).sort();
  const counts = Object.fromEntries(['unchanged', 'create', 'revise', 'conflicting', 'unmapped']
    .map(status => [status, decisions.filter(item => item.status === status).length]));
  return { decisions, components, counts, retired, batches, simulation,
    sourceIdentities: Object.fromEntries(decisions.map(item => [item.key, item.sourceIdentity])),
    blocked: request.policy === 'atomic' && (anyBlocked || simulation.rejected.length > 0),
    diagnostics: [...graph.diagnostics, ...simulation.diagnostics] };
}

export function synchronizationToken(state, request, actorId) {
  return identity({ actorId, sequence: state.sequence, synchronization: request });
}

export function previewSynchronization(state, input, actorId) {
  const request = synchronizationRequest(input);
  const plan = synchronizationPlan(state, request, actorId);
  const { batches: _internal, simulation, ...report } = plan;
  return { ...report, planToken: synchronizationToken(state, request, actorId), sequence: state.sequence,
    manifestIdentity: identity(request.manifest), policy: request.policy,
    applicable: !plan.blocked && (simulation.imported.length > 0 || plan.decisions.some(item => item.status === 'unchanged' && item.componentEligible)),
    predicted: simulation.imported, rejected: simulation.rejected };
}
