import assert from 'node:assert/strict';
import { diffLines, lineTokens, applyLineHunks } from '../src/structure/diff.mjs';
import { mergeBodies, materializeMerge } from '../src/structure/merge.mjs';

export default function check() {
  for (const [before, after] of [['', 'a\n'], ['a\n', ''], ['a\nb\nc', 'a\nB\nc'],
    ['a\nb\na\n', 'b\na\nb\n'], ['😀\r\nlast', '😀\r\nlast\n']]) {
    const diff = diffLines(before, after);
    assert.equal(applyLineHunks(lineTokens(before), diff.hunks), after);
    assert.equal(diff.exact, true);
  }
  const bounded = diffLines('a\nb\nc\n', 'd\ne\nf\n', 1);
  assert.equal(bounded.exact, false);
  assert.equal(applyLineHunks(lineTokens('a\nb\nc\n'), bounded.hunks), 'd\ne\nf\n');
  const adjacent = mergeBodies('a\nb\n', 'A\nb\n', 'a\nB\n');
  assert.equal(adjacent.clean, true);
  assert.equal(adjacent.body, 'A\nB\n');
  const same = mergeBodies('a\n', 'A\n', 'A\n');
  assert.equal(same.body, 'A\n');
  const insertion = mergeBodies('a\n', 'x\na\n', 'y\na\n');
  assert.equal(insertion.conflicts.length, 1);
  assert.equal(materializeMerge(insertion, { [insertion.conflicts[0].id]: { choice: 'incoming' } }), 'y\na\n');
}
