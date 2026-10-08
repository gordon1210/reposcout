import { requireValue } from '../core/errors.mjs';
import { object, text, integer, choice } from '../core/validation.mjs';
import { digest } from '../core/identity.mjs';
import { parseStructure } from './parse.mjs';

function sectionById(structure, id) {
  const section = structure.sections.find(item => item.id === id);
  requireValue(section, 'section_not_found', 'section identity is no longer present', { id });
  return section;
}

function replacement(value) {
  return text(value, 'replacement body', { empty: true, max: 200000 });
}

function terminated(value, newline) {
  return value.length && !value.endsWith('\n') ? value + newline : value;
}

export function planSectionOperations(body, expectedChecksum, operations) {
  const structure = parseStructure(body);
  requireValue(structure.checksum === expectedChecksum, 'checksum_conflict', 'document checksum does not match');
  requireValue(Array.isArray(operations) && operations.length > 0 && operations.length <= 100,
    'invalid_input', 'operations must contain between one and 100 items');
  const newline = body.includes('\r\n') ? '\r\n' : '\n';
  const edits = [];
  for (const raw of operations) {
    const operation = object(raw, 'section operation');
    choice(operation.kind, ['replace', 'rename', 'delete', 'insertAfter', 'moveAfter', 'shiftLevel'], 'section operation');
    const section = sectionById(structure, operation.sectionId);
    if (operation.kind === 'replace') {
      edits.push({ start: section.contentStart, end: section.end, text: terminated(replacement(operation.body), newline) });
    } else if (operation.kind === 'rename') {
      const title = text(operation.title, 'heading title', { max: 240 });
      requireValue(!/[\r\n]/.test(title), 'invalid_input', 'heading title must fit on one line');
      edits.push({ start: section.start, end: section.contentStart, text: `${'#'.repeat(section.level)} ${title}${newline}` });
    } else if (operation.kind === 'delete') {
      edits.push({ start: section.start, end: section.end, text: '' });
    } else if (operation.kind === 'insertAfter') {
      const inserted = replacement(operation.body);
      const prefix = section.end > 0 && body[section.end - 1] !== '\n' ? newline : '';
      edits.push({ start: section.end, end: section.end, text: prefix + terminated(inserted, newline) });
    } else if (operation.kind === 'moveAfter') {
      const destination = sectionById(structure, operation.destinationId);
      requireValue(destination.id !== section.id && !(destination.start >= section.start && destination.start < section.end)
        && !(section.start >= destination.start && section.start < destination.end),
      'invalid_section_move', 'source and destination must not contain one another');
      const prefix = destination.end > 0 && body[destination.end - 1] !== '\n' ? newline : '';
      edits.push({ start: section.start, end: section.end, text: '' });
      edits.push({ start: destination.end, end: destination.end,
        text: prefix + terminated(body.slice(section.start, section.end), newline) });
    } else {
      const level = integer(operation.level, 'heading level', 1, 6);
      const delta = level - section.level;
      for (const nested of structure.sections.filter(item => item.start >= section.start && item.start < section.end)) {
        const next = nested.level + delta;
        requireValue(next >= 1 && next <= 6, 'invalid_heading_level', 'descendant heading would leave levels one through six');
        edits.push({ start: nested.start, end: nested.contentStart, text: `${'#'.repeat(next)} ${nested.title}${newline}` });
      }
    }
  }
  edits.sort((a, b) => a.start - b.start || a.end - b.end);
  for (let index = 1; index < edits.length; index += 1) {
    const previous = edits[index - 1];
    const current = edits[index];
    requireValue(current.start >= previous.end && current.start !== previous.start,
      'overlapping_operations', 'section operations overlap in the original document');
  }
  let cursor = 0;
  let result = '';
  for (const edit of edits) {
    result += body.slice(cursor, edit.start) + edit.text;
    cursor = edit.end;
  }
  result += body.slice(cursor);
  replacement(result);
  return { body: result, checksum: digest(result), edits, changed: result !== body };
}
