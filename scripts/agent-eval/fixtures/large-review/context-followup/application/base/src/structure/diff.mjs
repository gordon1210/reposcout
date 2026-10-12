import { integer, text } from '../core/validation.mjs';
import { sourceLines } from './parse.mjs';

export function lineTokens(value) {
  text(value, 'diff body', { max: 200000, empty: true });
  return sourceLines(value).map(line => line.raw);
}

// Deterministic LCS with prefix/suffix reduction and a strict allocation ceiling.
// Exhaustion returns a conservative whole-middle replacement, never an incomplete diff.
export function diffLines(before, after, maximumCells = 1000000) {
  integer(maximumCells, 'diff cell budget', 1, 2000000);
  const left = lineTokens(before);
  const right = lineTokens(after);
  let prefix = 0;
  while (prefix < left.length && prefix < right.length && left[prefix] === right[prefix]) prefix += 1;
  let suffix = 0;
  while (suffix < left.length - prefix && suffix < right.length - prefix
    && left[left.length - 1 - suffix] === right[right.length - 1 - suffix]) suffix += 1;
  const a = left.slice(prefix, left.length - suffix);
  const b = right.slice(prefix, right.length - suffix);
  if (!a.length && !b.length) return { hunks: [], exact: true, cells: 0 };
  if (!a.length || !b.length) return {
    hunks: [{ start: prefix, end: prefix + a.length, lines: b }], exact: true, cells: 0,
  };
  const cells = (a.length + 1) * (b.length + 1);
  if (cells > maximumCells) return {
    hunks: [{ start: prefix, end: prefix + a.length, lines: b }], exact: false, cells: 0,
  };
  const width = b.length + 1;
  const table = new Uint32Array(cells);
  for (let row = a.length - 1; row >= 0; row -= 1) {
    for (let column = b.length - 1; column >= 0; column -= 1) {
      table[row * width + column] = a[row] === b[column]
        ? 1 + table[(row + 1) * width + column + 1]
        : Math.max(table[(row + 1) * width + column], table[row * width + column + 1]);
    }
  }
  const hunks = [];
  let row = 0;
  let column = 0;
  let pending = null;
  function flush() {
    if (pending) hunks.push(pending);
    pending = null;
  }
  while (row < a.length || column < b.length) {
    if (row < a.length && column < b.length && a[row] === b[column]) {
      flush(); row += 1; column += 1;
      continue;
    }
    pending ??= { start: prefix + row, end: prefix + row, lines: [] };
    if (row < a.length && (column === b.length
      || table[(row + 1) * width + column] >= table[row * width + column + 1])) {
      row += 1;
      pending.end = prefix + row;
    } else {
      pending.lines.push(b[column]);
      column += 1;
    }
  }
  flush();
  return { hunks, exact: true, cells };
}

export function applyLineHunks(lines, hunks, start = 0, end = lines.length) {
  const output = [];
  let cursor = start;
  for (const hunk of hunks) {
    output.push(lines.slice(cursor, hunk.start).join(''), hunk.lines.join(''));
    cursor = hunk.end;
  }
  output.push(lines.slice(cursor, end).join(''));
  return output.join('');
}
