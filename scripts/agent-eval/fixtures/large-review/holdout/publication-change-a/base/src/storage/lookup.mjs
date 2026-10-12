import { notFound, requireValue } from '../core/errors.mjs';

export function entity(state, table, id) {
  const value = state[table]?.[id];
  if (!value) throw notFound(table, id);
  return value;
}

export function document(state, id, { archived = false } = {}) {
  const value = entity(state, 'documents', id);
  requireValue(archived || !value.archived, 'archived_document', 'document is archived', { id });
  return value;
}

export function revision(state, id, documentId) {
  const value = entity(state, 'revisions', id);
  requireValue(documentId === undefined || value.documentId === documentId,
    'revision_mismatch', 'revision belongs to another document');
  return value;
}

export function values(state, table, predicate = () => true) {
  return Object.values(state[table]).filter(predicate);
}
