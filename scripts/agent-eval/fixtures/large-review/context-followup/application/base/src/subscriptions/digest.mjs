import { allocate } from '../core/identity.mjs';
import { actor, visibleCollections } from '../permissions/policy.mjs';
import { eventSummary } from '../core/events.mjs';

function applies(event, watch) {
  const direct = event.target === watch.documentId;
  const publication = event.action === 'release.published' && event.details.documents.includes(watch.documentId);
  return (direct || publication) && (watch.mode === 'all' || publication);
}

export function previewDigest(state, input, actorId) {
  actor(state, actorId);
  const visible = new Set(visibleCollections(state, actorId).map(item => item.id));
  const groups = Object.values(state.watches).filter(watch => watch.actorId === actorId &&
    visible.has(state.documents[watch.documentId].collectionId)).map(watch => {
    const item = state.documents[watch.documentId];
    const current = state.revisions[item.currentRevisionId];
    return { documentId: item.id, title: current.title,
      events: state.events.filter(event => event.sequence > watch.afterSequence && applies(event, watch)).map(eventSummary) };
  }).filter(group => group.events.length > 0);
  return { actorId, throughSequence: state.sequence, groups };
}

export function deliverDigest(state, input, actorId) {
  const digest = previewDigest(state, input, actorId);
  const id = allocate(state, 'delivery');
  state.deliveries[id] = { id, ...digest };
  for (const watch of Object.values(state.watches)) {
    if (watch.actorId === actorId) watch.afterSequence = digest.throughSequence;
  }
  return state.deliveries[id];
}
