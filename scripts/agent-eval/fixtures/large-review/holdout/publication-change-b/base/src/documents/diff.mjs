import { getDocument } from './queries.mjs';
import { revision } from '../storage/lookup.mjs';

export function compareRevisions(state, input, actorId) {
  const item = getDocument(state, { id: input.id, revisionId: input.before }, actorId);
  const before = item.revision;
  const after = revision(state, input.after, item.id);
  const oldLines = before.body.split('\n');
  const newLines = after.body.split('\n');
  let prefix = 0;
  while (prefix < oldLines.length && prefix < newLines.length && oldLines[prefix] === newLines[prefix]) prefix += 1;
  let suffix = 0;
  while (suffix < oldLines.length - prefix && suffix < newLines.length - prefix &&
      oldLines[oldLines.length - suffix - 1] === newLines[newLines.length - suffix - 1]) suffix += 1;
  return {
    documentId: item.id, before: before.id, after: after.id,
    titleChanged: before.title !== after.title,
    tagsAdded: after.tags.filter(tag => !before.tags.includes(tag)),
    tagsRemoved: before.tags.filter(tag => !after.tags.includes(tag)),
    body: { startLine: prefix + 1, removed: oldLines.slice(prefix, oldLines.length - suffix),
      added: newLines.slice(prefix, newLines.length - suffix) },
  };
}
