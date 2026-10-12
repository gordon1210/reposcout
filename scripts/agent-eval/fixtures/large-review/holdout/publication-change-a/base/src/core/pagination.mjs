import { integer } from './validation.mjs';
import { requireValue } from './errors.mjs';

export function page(items, input = {}, key = item => item.id) {
  const limit = integer(input.limit ?? 20, 'limit', 1, 100);
  const ordered = [...items].sort((a, b) => String(key(a)).localeCompare(String(key(b))));
  let offset = 0;
  if (input.after !== undefined) {
    const found = ordered.findIndex(item => key(item) === input.after);
    requireValue(found >= 0, 'invalid_cursor', 'cursor is not present in this result');
    offset = found + 1;
  }
  const selected = ordered.slice(offset, offset + limit);
  return {
    items: selected,
    total: ordered.length,
    next: offset + selected.length < ordered.length ? key(selected.at(-1)) : null,
  };
}

export function newest(items) {
  return [...items].sort((a, b) => b.sequence - a.sequence || a.id.localeCompare(b.id));
}
