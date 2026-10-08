import { digest } from '../core/identity.mjs';
import { requireValue } from '../core/errors.mjs';
import { text } from '../core/validation.mjs';
import { diffLines, lineTokens, applyLineHunks } from './diff.mjs';

function overlaps(left, right) {
  if (left.start === left.end || right.start === right.end) {
    return left.start <= right.end && right.start <= left.end;
  }
  return left.start < right.end && right.start < left.end;
}

export function mergeBodies(base, current, incoming, maximumCells = 1000000) {
  const lines = lineTokens(base);
  const ours = diffLines(base, current, maximumCells);
  const theirs = diffLines(base, incoming, maximumCells);
  const changes = [...ours.hunks.map(hunk => ({ ...hunk, side: 'current' })),
    ...theirs.hunks.map(hunk => ({ ...hunk, side: 'incoming' }))]
    .sort((a, b) => a.start - b.start || a.end - b.end || a.side.localeCompare(b.side));
  const groups = [];
  for (const change of changes) {
    const last = groups.at(-1);
    if (last && last.some(member => overlaps(member, change))) last.push(change);
    else groups.push([change]);
  }
  const parts = [];
  const conflicts = [];
  let cursor = 0;
  for (const group of groups) {
    const start = Math.min(...group.map(item => item.start));
    const end = Math.max(...group.map(item => item.end));
    if (start > cursor) parts.push({ text: lines.slice(cursor, start).join('') });
    const currentChanges = group.filter(item => item.side === 'current');
    const incomingChanges = group.filter(item => item.side === 'incoming');
    const currentText = applyLineHunks(lines, currentChanges, start, end);
    const incomingText = applyLineHunks(lines, incomingChanges, start, end);
    if (!currentChanges.length) parts.push({ text: incomingText });
    else if (!incomingChanges.length || currentText === incomingText) parts.push({ text: currentText });
    else {
      const baseText = lines.slice(start, end).join('');
      const id = `conflict:${conflicts.length + 1}:${digest(JSON.stringify([start, end, baseText, currentText, incomingText])).slice(0, 12)}`;
      conflicts.push({ id, startLine: start + 1, endLineExclusive: end + 1,
        base: baseText, current: currentText, incoming: incomingText });
      parts.push({ conflictId: id });
    }
    cursor = end;
  }
  if (cursor < lines.length) parts.push({ text: lines.slice(cursor).join('') });
  return { parts, conflicts, exact: ours.exact && theirs.exact,
    cells: ours.cells + theirs.cells, clean: conflicts.length === 0,
    body: conflicts.length ? null : parts.map(part => part.text).join('') };
}

export function materializeMerge(plan, resolutions) {
  const known = new Set(plan.conflicts.map(conflict => conflict.id));
  for (const id of Object.keys(resolutions)) requireValue(known.has(id), 'unknown_conflict', 'resolution refers to an unknown conflict');
  const selected = new Map();
  for (const conflict of plan.conflicts) {
    const resolution = resolutions[conflict.id];
    requireValue(resolution, 'unresolved_conflict', 'all conflicts must be resolved', { conflictId: conflict.id });
    requireValue(['base', 'current', 'incoming', 'custom'].includes(resolution.choice), 'invalid_input', 'invalid resolution choice');
    selected.set(conflict.id, resolution.choice === 'custom'
      ? text(resolution.body, 'resolved body', { empty: true, max: 200000 }) : conflict[resolution.choice]);
  }
  const body = plan.parts.map(part => part.conflictId ? selected.get(part.conflictId) : part.text).join('');
  text(body, 'merged body', { empty: true, max: 200000 });
  return body;
}
