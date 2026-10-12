import { getDocument } from '../documents/queries.mjs';
import { slug } from '../core/identity.mjs';

export function headings(body) {
  const counts = new Map();
  const result = [];
  let fenced = false;
  for (const [index, line] of body.split('\n').entries()) {
    if (line.trimStart().startsWith('```')) { fenced = !fenced; continue; }
    const match = fenced ? null : /^(#{1,6}) +(.+)$/u.exec(line);
    if (!match) continue;
    const title = match[2].trim();
    const base = slug(title) || 'section';
    const count = counts.get(base) ?? 0;
    counts.set(base, count + 1);
    result.push({ level: match[1].length, title, line: index + 1, anchor: count ? `${base}-${count}` : base });
  }
  return result;
}

export function documentOutline(state, input, actorId) {
  const item = getDocument(state, { id: input.documentId, revisionId: input.revisionId }, actorId);
  return { documentId: item.id, revisionId: item.revision.id, headings: headings(item.revision.body) };
}
