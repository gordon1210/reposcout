import { parseImport, importSummary } from './parser.mjs';
import { createDocument, reviseDocument } from '../documents/service.mjs';
import { authorize } from '../permissions/policy.mjs';
import { requireValue } from '../core/errors.mjs';
import { document } from '../storage/lookup.mjs';
import { record } from '../core/events.mjs';

export function previewImport(state, input, actorId) {
  authorize(state, actorId, 'edit', input.collectionId);
  return importSummary(parseImport(input.source));
}

export function importDocument(state, input, actorId) {
  const parsed = parseImport(input.source);
  let result;
  if (input.documentId !== undefined) {
    const item = document(state, input.documentId);
    requireValue(item.collectionId === input.collectionId, 'collection_mismatch', 'import target is in another collection');
    result = reviseDocument(state, { id: item.id, ...parsed,
      expectedRevisionId: input.expectedRevisionId }, actorId);
  } else {
    result = createDocument(state, { collectionId: input.collectionId, ...parsed }, actorId);
  }
  record(state, actorId, 'document.imported', result.id, { revisionId: result.revision.id });
  return result;
}
