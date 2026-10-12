import { getRelease } from '../releases/queries.mjs';
import { createManifest } from '../exports/manifest.mjs';
import { materializeSelection } from '../content/selection.mjs';
import { record } from '../core/events.mjs';
import { indexTerms } from './tokenize.mjs';

export function rebuildReleaseIndex(state, input, actorId) {
  const release = getRelease(state, { id: input.releaseId }, actorId);
  const manifest = createManifest(state, release.id);
  const items = materializeSelection(state, manifest.selection);
  const records = items.map(item => ({
    id: `${release.id}:${item.documentId}`, releaseId: release.id,
    documentId: item.documentId, revisionId: item.revisionId,
    collectionId: item.collectionId, title: item.title, body: item.body,
    tags: [...item.tags], language: item.language, terms: indexTerms(item),
  }));
  state.searchEntries = [...state.searchEntries.filter(item => item.releaseId !== release.id), ...records];
  record(state, actorId, 'search.indexed', release.id, { entries: records.length });
  return { releaseId: release.id, entries: records.length };
}
