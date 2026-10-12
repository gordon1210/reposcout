import { getDocument } from '../documents/queries.mjs';
import { latestRequest } from '../content/requests.mjs';
import { materializeSelection } from '../content/selection.mjs';
import { renderArtifact } from '../exports/render.mjs';
import { strings } from '../core/validation.mjs';

export function previewDocument(state, input, actorId) {
  const item = getDocument(state, { id: input.documentId }, actorId);
  return { documentId: item.id, revisionId: item.revision.id,
    title: item.revision.title, body: item.revision.body, preview: true };
}

export function previewBundle(state, input, actorId) {
  const ids = strings(input.documentIds, 'document ids', 50);
  for (const id of ids) getDocument(state, { id }, actorId);
  const items = materializeSelection(state, latestRequest(ids));
  return { ...renderArtifact({ id: 'preview', name: input.name ?? 'Working copy preview' }, items, input.format ?? 'json'),
    preview: true, documents: items.map(({ documentId, revisionId }) => ({ documentId, revisionId })) };
}
