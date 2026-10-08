import { canonicalSelection } from './selection.mjs';

export function selectionRequest(entries) {
  const items = entries.map(entry => ({ documentId: entry.documentId, selector: entry.revisionId }));
  return canonicalSelection(items);
}

export function latestRequest(documentIds) {
  return canonicalSelection(documentIds.map(documentId => ({ documentId })));
}
