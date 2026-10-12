import { choice } from '../core/validation.mjs';
import { jsonArtifact } from './formats/json.mjs';
import { textArtifact } from './formats/text.mjs';
import { csvArtifact } from './formats/csv.mjs';

const renderers = { json: jsonArtifact, text: textArtifact, csv: csvArtifact };

export function renderArtifact(release, items, format) {
  choice(format, Object.keys(renderers), 'export format');
  return renderers[format](release, items);
}

export function formatNames() {
  return Object.keys(renderers);
}
