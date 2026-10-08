import assert from 'node:assert/strict';
import { call, fail, workspace, draft, approve, release, exportRelease } from './support.mjs';
import { csvCell } from '../src/exports/formats/csv.mjs';

export default function check() {
  const { app, collection } = workspace();
  const item = draft(app, collection, { title: '=DANGEROUS(), "quoted"', body: 'Safe body.' });
  const selected = release(app, approve(app, item));
  const artifact = exportRelease(app, selected, 'csv');
  assert.equal(artifact.mediaType, 'text/csv; charset=utf-8');
  assert.ok(artifact.content.startsWith('release_id,document_id,revision_id,title,language,checksum\r\n'));
  assert.ok(artifact.content.includes(`"'=DANGEROUS(), ""quoted"""`));
  assert.equal(artifact.content.split('\r\n').length, 3);
  assert.equal(csvCell('normal'), 'normal');
  assert.equal(csvCell('line\nbreak'), '"line\nbreak"');
}
