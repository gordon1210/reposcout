import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { body: '# Start\nText\n## Next\n```text\n# Not a heading\n```\n## Next' });
  const outline = call(app, 'documents.outline', { documentId: item.id }, 'reader');
  assert.deepEqual(outline.headings.map(item => item.title), ['Start', 'Next', 'Next']);
  assert.deepEqual(outline.headings.map(item => item.anchor), ['start', 'next', 'next-1']);
  assert.deepEqual(outline.headings.map(item => item.line), [1, 3, 7]);
  assert.equal(outline.revisionId, item.revision.id);
}
