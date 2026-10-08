import { strings } from '../core/validation.mjs';
import { record } from '../core/events.mjs';
import { document } from '../storage/lookup.mjs';
import { authorize, visibleCollections } from '../permissions/policy.mjs';

export function labelDocument(state, input, actorId) {
  const item = document(state, input.documentId);
  authorize(state, actorId, 'edit', item.collectionId);
  const labels = strings(input.labels, 'labels', 24).map(label => label.toLocaleLowerCase('en')).sort();
  state.labels[item.id] = [...new Set(labels)];
  record(state, actorId, 'document.labelled', item.id, { labels: state.labels[item.id] });
  return { documentId: item.id, labels: state.labels[item.id] };
}

export function findLabel(state, input, actorId) {
  const visible = new Set(visibleCollections(state, actorId).map(item => item.id));
  const wanted = String(input.label).toLocaleLowerCase('en');
  return Object.entries(state.labels).filter(([id, labels]) => labels.includes(wanted) &&
    visible.has(state.documents[id].collectionId) && !state.documents[id].archived)
    .map(([documentId, labels]) => ({ documentId, labels }));
}

export function labelCounts(state, input, actorId) {
  const visible = new Set(visibleCollections(state, actorId).map(item => item.id));
  const counts = new Map();
  for (const [id, labels] of Object.entries(state.labels)) {
    if (!visible.has(state.documents[id].collectionId) || state.documents[id].archived) continue;
    for (const label of labels) counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return [...counts].sort(([a], [b]) => a.localeCompare(b)).map(([label, documents]) => ({ label, documents }));
}
