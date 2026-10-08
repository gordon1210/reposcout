import { visibleCollections } from '../permissions/policy.mjs';
import { text, integer } from '../core/validation.mjs';
import { queryTerms } from './tokenize.mjs';
import { score, snippet } from './ranking.mjs';

export function search(state, input, actorId) {
  const query = queryTerms(text(input.query, 'search query', { max: 300, empty: true }));
  const visible = new Set(visibleCollections(state, actorId).map(item => item.id));
  const limit = integer(input.limit ?? 20, 'search limit', 1, 100);
  const matches = state.searchEntries.filter(entry => visible.has(entry.collectionId) &&
    !state.releases[entry.releaseId].withdrawn &&
    (input.releaseId === undefined || entry.releaseId === input.releaseId) &&
    (input.language === undefined || entry.language === input.language) &&
    (input.tag === undefined || entry.tags.includes(input.tag)))
    .map(entry => ({ entry, score: score(entry, query) }))
    .filter(item => item.score > 0)
    .sort((a, b) => b.score - a.score || a.entry.id.localeCompare(b.entry.id));
  return { total: matches.length, items: matches.slice(0, limit).map(({ entry, score: relevance }) => ({
    id: entry.id, documentId: entry.documentId, revisionId: entry.revisionId,
    releaseId: entry.releaseId, title: entry.title, relevance,
    snippet: snippet(entry.body, query),
  })) };
}
