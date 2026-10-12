import { digest } from '../core/identity.mjs';
import { text } from '../core/validation.mjs';

// Offsets are UTF-16 code units, matching JavaScript slice and editor selections.
export function sourceLines(source) {
  const lines = [];
  let start = 0;
  while (start < source.length) {
    const newline = source.indexOf('\n', start);
    const end = newline < 0 ? source.length : newline + 1;
    const raw = source.slice(start, end);
    lines.push({ start, end, raw, text: raw.replace(/\r?\n$/, ''), number: lines.length + 1 });
    start = end;
  }
  return lines;
}

function heading(line) {
  const match = /^( {0,3})(#{1,6})[ \t]+(.+?)\s*$/.exec(line);
  if (!match) return null;
  return { level: match[2].length, title: match[3].replace(/[ \t]+#+[ \t]*$/, '') };
}

function fence(line) {
  const match = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(line);
  if (!match || (match[1][0] === '`' && match[2].includes('`'))) return null;
  return { marker: match[1][0], size: match[1].length, info: match[2].trim() };
}

function listItem(line) {
  const match = /^( *)([-+*]|\d{1,9}[.)])[ \t]+(.*)$/.exec(line);
  if (!match) return null;
  const task = /^\[([ xX])\][ \t]+(.*)$/.exec(match[3]);
  return { indent: match[1].length, ordered: /^\d/.test(match[2]), marker: match[2],
    content: task ? task[2] : match[3], checked: task ? task[1] !== ' ' : null };
}

function inlineLinks(line, offset, lineNumber) {
  const links = [];
  let codeTicks = 0;
  for (let i = 0; i < line.length; i += 1) {
    if (line[i] === '\\') { i += 1; continue; }
    if (line[i] === '`') {
      let end = i + 1;
      while (line[end] === '`') end += 1;
      const count = end - i;
      if (codeTicks === 0) codeTicks = count;
      else if (codeTicks === count) codeTicks = 0;
      i = end - 1;
      continue;
    }
    if (codeTicks || line[i] !== '[') continue;
    let close = i + 1;
    while (close < line.length && line[close] !== ']') {
      close += line[close] === '\\' ? 2 : 1;
    }
    if (line[close + 1] !== '(') continue;
    let end = close + 2;
    let depth = 1;
    while (end < line.length && depth) {
      if (line[end] === '\\') { end += 2; continue; }
      if (line[end] === '(') depth += 1;
      if (line[end] === ')') depth -= 1;
      end += 1;
    }
    if (depth) continue;
    links.push({ label: line.slice(i + 1, close), destination: line.slice(close + 2, end - 1),
      image: i > 0 && line[i - 1] === '!', start: offset + i, end: offset + end, line: lineNumber });
    i = end - 1;
  }
  return links;
}

export function parseStructure(source) {
  text(source, 'document body', { empty: true, max: 200000 });
  const lines = sourceLines(source);
  const blocks = [];
  const headings = [];
  const links = [];
  let index = 0;
  while (index < lines.length) {
    const first = lines[index];
    const startIndex = index;
    const opening = fence(first.text);
    const title = heading(first.text);
    const item = listItem(first.text);
    let type;
    let details = {};
    if (opening) {
      type = 'fence';
      index += 1;
      let closed = false;
      while (index < lines.length) {
        const candidate = lines[index].text.trim();
        index += 1;
        if (candidate.length >= opening.size && [...candidate].every(char => char === opening.marker)) {
          closed = true;
          break;
        }
      }
      details = { ...opening, closed };
    } else if (title) {
      type = 'heading';
      index += 1;
      details = title;
    } else if (!first.text.trim()) {
      type = 'blank';
      while (index < lines.length && !lines[index].text.trim()) index += 1;
    } else if (item) {
      type = 'list';
      const items = [];
      while (index < lines.length) {
        const current = listItem(lines[index].text);
        if (!current) break;
        items.push({ ...current, start: lines[index].start, end: lines[index].end, line: lines[index].number });
        index += 1;
      }
      details = { items };
    } else {
      type = 'paragraph';
      index += 1;
      while (index < lines.length && lines[index].text.trim() && !heading(lines[index].text)
        && !fence(lines[index].text) && !listItem(lines[index].text)) index += 1;
    }
    const end = lines[index - 1].end;
    const block = { id: `block:${first.start}:${digest(source.slice(first.start, end)).slice(0, 12)}`,
      type, start: first.start, end, startLine: first.number, endLine: lines[index - 1].number, ...details };
    blocks.push(block);
    if (type === 'heading') headings.push(block);
    if (type !== 'fence') {
      for (let n = startIndex; n < index; n += 1) links.push(...inlineLinks(lines[n].text, lines[n].start, lines[n].number));
    }
  }
  const stack = [];
  const sections = [];
  const slugs = new Map();
  for (const node of headings) {
    while (stack.length && stack.at(-1).level >= node.level) stack.pop().end = node.start;
    const base = node.title.normalize('NFKC').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-|-$/g, '') || 'section';
    const occurrence = (slugs.get(base) ?? 0) + 1;
    slugs.set(base, occurrence);
    const section = { id: node.id, slug: occurrence === 1 ? base : `${base}-${occurrence}`,
      title: node.title, level: node.level, start: node.start, contentStart: node.end,
      end: source.length, parentId: stack.at(-1)?.id ?? null, startLine: node.startLine };
    sections.push(section);
    stack.push(section);
  }
  return { checksum: digest(source), characters: source.length, lineCount: lines.length, blocks, sections, links };
}
