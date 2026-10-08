import assert from 'node:assert/strict';
import { call, workspace, draft } from './support.mjs';

export default function check() {
  const { app, collection } = workspace();
  const body = '# Start\r\nText [manual](docs/(intro)) and `[ignored](x)`\r\n## Child\r\n- [x] Done\r\n- [ ] Open\r\n```md\r\n# Not a heading\r\n[hidden](x)\r\n```\r\n# Start\r\nFinal 😀';
  const item = draft(app, collection, { body });
  const tree = call(app, 'structure.get', { documentId: item.id }, 'reader');
  assert.equal(tree.characters, body.length);
  assert.equal(tree.checksum, item.revision.checksum);
  assert.deepEqual(tree.sections.map(node => node.slug), ['start', 'child', 'start-2']);
  assert.equal(tree.sections[1].parentId, tree.sections[0].id);
  assert.equal(tree.sections[0].end, tree.sections[2].start);
  assert.equal(tree.sections[1].end, tree.sections[2].start);
  assert.equal(tree.links.length, 1);
  assert.equal(tree.links[0].destination, 'docs/(intro)');
  assert.equal(body.slice(tree.links[0].start, tree.links[0].end), '[manual](docs/(intro))');
  assert.deepEqual(tree.blocks.find(block => block.type === 'list').items.map(item => item.checked), [true, false]);
  assert.equal(tree.blocks.find(block => block.type === 'fence').closed, true);
  assert.equal(tree.blocks.map(block => body.slice(block.start, block.end)).join(''), body);
  const empty = draft(app, collection, { body: '' });
  assert.deepEqual(call(app, 'structure.get', { documentId: empty.id }, 'reader').blocks, []);
}
