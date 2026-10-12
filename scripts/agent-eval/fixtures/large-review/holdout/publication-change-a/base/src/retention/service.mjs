import { authorize } from '../permissions/policy.mjs';
import { record } from '../core/events.mjs';
import { retentionRule, protectedRevisionIds } from './rules.mjs';
import { revisionHistory } from '../documents/revisions.mjs';

export function previewRetention(state, input, actorId) {
  authorize(state, actorId, 'manage', input.collectionId);
  const rule = retentionRule(state, input.collectionId);
  const protectedIds = protectedRevisionIds(state);
  const candidates = [];
  for (const item of Object.values(state.documents)) {
    if (item.collectionId !== input.collectionId) continue;
    const history = revisionHistory(state, item.id);
    const retainedRecent = new Set(history.slice(0, rule.keepRecent).map(revision => revision.id));
    for (const revision of history) {
      if (!protectedIds.has(revision.id) && !retainedRecent.has(revision.id))
        candidates.push({ revisionId: revision.id, documentId: item.id, characters: revision.body.length });
    }
  }
  return { collectionId: input.collectionId, candidates,
    characters: candidates.reduce((sum, item) => sum + item.characters, 0) };
}

export function applyRetention(state, input, actorId) {
  const preview = previewRetention(state, input, actorId);
  for (const item of preview.candidates) delete state.revisions[item.revisionId];
  record(state, actorId, 'retention.applied', input.collectionId,
    { removed: preview.candidates.map(item => item.revisionId) });
  return { collectionId: input.collectionId, removed: preview.candidates.length, characters: preview.characters };
}
