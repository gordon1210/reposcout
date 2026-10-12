import assert from 'node:assert/strict';
import { captureRange, mapAnchor } from '../src/structure/anchors.mjs';
import { compareBodies } from '../src/structure/correspondence.mjs';

export default function check() {
  const body = '# Repeated\nSame\n\n# Repeated\nSame\n';
  const comparison = compareBodies(body, '# Repeated\nSame\n\n# Repeated\nSame\n\n');
  assert.equal(comparison.sections.length, 2);
  assert.notEqual(comparison.sections[0].sourceId, comparison.sections[1].sourceId);
  const repeated = 'X\nRepeat\nY\nX\nRepeat\nY\n';
  const ambiguous = mapAnchor(captureRange(repeated, 2), repeated, 'Preface\n' + repeated);
  assert.equal(ambiguous.status, 'ambiguous');
  assert.equal(ambiguous.reason, 'repeated_quote');
  assert.equal(ambiguous.exact, false);
  const empty = captureRange('last\n', 2);
  assert.equal(mapAnchor(empty, 'last\n', 'last\n').status, 'unchanged');
  assert.equal(mapAnchor(empty, 'last\n', 'new\nlast\n').status, 'ambiguous');
  const unicode = '# Å\r\n- [ ] Inspect 😀\r\n';
  const selected = captureRange(unicode, 2);
  assert.equal(selected.quote, '- [ ] Inspect 😀\r\n');
  const moved = mapAnchor(selected, unicode, 'Intro\r\n' + unicode);
  assert.equal(moved.status, 'relocated');
  assert.equal(moved.target.start, 'Intro\r\n# Å\r\n'.length);
  assert.equal(moved.target.startLine, 3);
  const ordinary = '# Guide\nInspect\n';
  const fenced = mapAnchor(captureRange(ordinary, 2), ordinary, '# Guide\n```\nInspect\n```\n');
  assert.equal(fenced.exact, false);
  assert.equal(fenced.target, null);
  const crossed = mapAnchor(captureRange('a\nb\nc\n', 1, 2), 'a\nb\nc\n', 'a\nB\nc\n');
  assert.equal(crossed.reason, 'anchor_crosses_change_boundary');
  assert.equal(crossed.status, 'ambiguous');
  const budget = mapAnchor(captureRange('old\na\nb\n', 1), 'old\na\nb\n', 'new\nx\ny\n', 1);
  assert.equal(budget.reason, 'diff_budget_exhausted');
  assert.equal(budget.exact, false);
}
