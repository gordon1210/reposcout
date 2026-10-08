import { allocate, digest } from '../core/identity.mjs';
import { text, strings } from '../core/validation.mjs';
import { revision } from '../storage/lookup.mjs';

export function createRevision(state, documentId, input, actorId, parentId = null) {
  const title = text(input.title, 'document title', { max: 240 });
  const body = text(input.body, 'document body', { max: 200000, empty: true });
  const tags = strings(input.tags ?? [], 'tags', 24).sort();
  const id = allocate(state, 'revision');
  const item = {
    id, documentId, parentId, title, body, tags,
    language: input.language ?? 'en', authorId: actorId,
    checksum: digest(body), sequence: state.sequence + 1,
  };
  state.revisions[id] = item;
  return item;
}

export function revisionHistory(state, documentId) {
  return Object.values(state.revisions).filter(item => item.documentId === documentId)
    .sort((a, b) => b.sequence - a.sequence);
}

export function latestRevision(state, item) {
  return revision(state, item.currentRevisionId, item.id);
}

export function revisionSummary(item) {
  const { body, ...metadata } = item;
  return { ...metadata, characters: body.length, lines: body.split('\n').length };
}
