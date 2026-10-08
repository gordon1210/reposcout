import { digest } from '../core/identity.mjs';

// Governance facts deliberately use source text, never the working document's release projection.
export function sourceIdentity(revision) {
  return {
    documentId: revision.documentId, revisionId: revision.id,
    digest: digest(JSON.stringify([revision.documentId, revision.id, revision.title, revision.body,
      revision.tags, revision.language, revision.authorId])),
  };
}

export function contentFacts(revision) {
  const lines = revision.body.split('\n');
  const sections = [];
  const prose = [];
  let fence = null;
  let codeBlocks = 0;
  let section = null;
  for (const [index, line] of lines.entries()) {
    const opening = /^ {0,3}(`{3,}|~{3,})(.*)$/u.exec(line);
    if (fence !== null) {
      const closing = /^ {0,3}(`+|~+)\s*$/u.exec(line);
      if (closing && closing[1][0] === fence.character && closing[1].length >= fence.length) fence = null;
      continue;
    }
    if (opening && !(opening[1][0] === '`' && opening[2].includes('`'))) {
      fence = { character: opening[1][0], length: opening[1].length };
      codeBlocks += 1;
      if (section) section.hasContent = true;
      continue;
    }
    const heading = /^ {0,3}(#{1,6})\s+(.+?)\s*#*\s*$/u.exec(line);
    if (heading) {
      section = { title: heading[2], level: heading[1].length, line: index + 1, hasContent: false };
      sections.push(section);
    } else if (line.trim()) {
      if (section) section.hasContent = true;
      prose.push(line);
    }
  }
  const ordinary = prose.join('\n');
  return {
    title: revision.title, language: revision.language, authorId: revision.authorId,
    tags: [...revision.tags], characters: revision.body.length,
    words: (ordinary.match(/[\p{L}\p{N}]+(?:['’-][\p{L}\p{N}]+)*/gu) ?? []).length,
    lines: lines.length, sectionCount: sections.length, sectionTitles: sections.map(item => item.title),
    linkCount: (ordinary.match(/\[[^\]\n]+\]\([^\s)]+(?:\s+"[^"]*")?\)/gu) ?? []).length,
    unresolvedMarkers: (ordinary.match(/\b(?:TODO|FIXME|TBD)\b/gu) ?? []).length,
    codeBlocks, emptySections: sections.filter(item => !item.hasContent).length,
    hasUnclosedFence: fence !== null,
  };
}
