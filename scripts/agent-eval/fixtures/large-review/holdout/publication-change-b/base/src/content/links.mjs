import { getDocument } from '../documents/queries.mjs';

export function documentLinks(body) {
  return [...body.matchAll(/\[([^\]]+)\]\(doc:([a-z]+_[0-9]+)(?:@([a-z]+_[0-9]+))?\)/gu)]
    .map(match => ({ label: match[1], documentId: match[2], revisionId: match[3] ?? null,
      offset: match.index, source: match[0] }));
}

export function checkLinks(state, input, actorId) {
  const selected = getDocument(state, { id: input.documentId, revisionId: input.revisionId }, actorId);
  return documentLinks(selected.revision.body).map(link => {
    try {
      const target = getDocument(state, { id: link.documentId,
        ...(link.revisionId ? { revisionId: link.revisionId } : {}) }, actorId);
      return { ...link, status: 'resolved', title: target.revision.title, resolvedRevisionId: target.revision.id };
    } catch (error) {
      if (!['not_found', 'forbidden', 'archived_document', 'revision_mismatch'].includes(error.code)) throw error;
      return { ...link, status: 'unavailable', title: null, resolvedRevisionId: null };
    }
  });
}
