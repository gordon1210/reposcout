import { digest } from '../core/identity.mjs';
import { parseStructure } from './parse.mjs';
import { diffLines } from './diff.mjs';

function sectionPath(section, byId) {
  const parts = [];
  let current = section;
  while (current) {
    parts.unshift({ level: current.level, title: current.title });
    current = byId.get(current.parentId);
  }
  return JSON.stringify(parts);
}

export function describeSections(body, structure = parseStructure(body)) {
  const byId = new Map(structure.sections.map(section => [section.id, section]));
  return structure.sections.map(section => ({ ...section,
    path: sectionPath(section, byId),
    contentChecksum: digest(body.slice(section.contentStart, section.end)),
    subtreeChecksum: digest(body.slice(section.start, section.end)),
  }));
}

function indexBy(items, key) {
  const index = new Map();
  for (const item of items) {
    const value = item[key];
    if (!index.has(value)) index.set(value, []);
    index.get(value).push(item);
  }
  return index;
}

// Correspondence is evidence, not identity: duplicate headings remain separate.
export function compareBodies(before, after, maximumCells = 1000000) {
  const from = describeSections(before);
  const to = describeSections(after);
  const sourceHashes = indexBy(from, 'subtreeChecksum');
  const targetHashes = indexBy(to, 'subtreeChecksum');
  const sourcePaths = indexBy(from, 'path');
  const targetPaths = indexBy(to, 'path');
  const used = new Set();
  const matches = new Map();
  for (const section of from) {
    const candidates = targetHashes.get(section.subtreeChecksum) ?? [];
    if (sourceHashes.get(section.subtreeChecksum).length === 1 && candidates.length === 1) {
      matches.set(section.id, { target: candidates[0], evidence: 'unique_subtree', exact: true });
      used.add(candidates[0].id);
    }
  }
  for (const section of from) {
    if (matches.has(section.id)) continue;
    const candidates = targetPaths.get(section.path) ?? [];
    if (sourcePaths.get(section.path).length === 1 && candidates.length === 1 && !used.has(candidates[0].id)) {
      matches.set(section.id, { target: candidates[0], evidence: 'unique_heading_path', exact: false });
      used.add(candidates[0].id);
    }
  }
  const sections = from.map(section => {
    const match = matches.get(section.id);
    if (match) return { sourceId: section.id, targetId: match.target.id,
      sourceStart: section.start, targetStart: match.target.start,
      status: match.exact ? (section.start === match.target.start ? 'unchanged' : 'relocated') : 'edited',
      evidence: match.evidence, exact: match.exact };
    const candidates = [...new Set([...(targetHashes.get(section.subtreeChecksum) ?? []),
      ...(targetPaths.get(section.path) ?? [])].map(item => item.id))];
    return { sourceId: section.id, targetId: null, status: candidates.length ? 'ambiguous' : 'deleted',
      evidence: candidates.length ? 'duplicate_structure' : 'no_correspondence', exact: false, candidates };
  });
  return { sourceChecksum: digest(before), targetChecksum: digest(after), sections,
    added: to.filter(section => !used.has(section.id)).map(section => section.id),
    diff: diffLines(before, after, maximumCells) };
}

export function containingSection(structure, start, end) {
  let best = null;
  for (const section of structure.sections) {
    if (section.start <= start && section.end >= end && (!best || section.level > best.level)) best = section;
  }
  return best;
}

export function containingBlock(structure, offset) {
  return structure.blocks.find(block => block.start <= offset && block.end > offset) ?? null;
}
