import { revision } from '../storage/lookup.mjs';

export function materializeSelection(state, selection) {
  return selection.items.map(item => {
    const document = state.documents[item.documentId];
    if (!document) throw new Error('selection references a missing document');
    const revisionId = item.selector === undefined ? document.currentRevisionId : item.selector;
    const source = revision(state, revisionId, document.id);
    return {
      documentId: document.id, revisionId: source.id, collectionId: document.collectionId,
      title: source.title, body: source.body, tags: [...source.tags],
      language: source.language, checksum: source.checksum,
    };
  });
}

export function canonicalSelection(items) {
  return { items: items.map(item => ({ ...item })).sort((a, b) => a.documentId.localeCompare(b.documentId)) };
}
