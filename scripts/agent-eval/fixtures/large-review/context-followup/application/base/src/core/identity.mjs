import { createHash } from 'node:crypto';
import { text } from './validation.mjs';

export function allocate(state, kind) {
  const next = (state.counters[kind] ?? 0) + 1;
  state.counters[kind] = next;
  return `${kind}_${String(next).padStart(4, '0')}`;
}

export function slug(value) {
  return text(value, 'slug source', { max: 200 }).normalize('NFKC').toLowerCase()
    .replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
}

export function digest(value) {
  return createHash('sha256').update(value, 'utf8').digest('hex');
}

export function pairKey(left, right) {
  return JSON.stringify([left, right]);
}
