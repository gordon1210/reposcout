import { requireValue } from './errors.mjs';

export function object(value, label = 'input') {
  requireValue(value !== null && typeof value === 'object' && !Array.isArray(value),
    'invalid_input', `${label} must be an object`);
  return value;
}

export function text(value, label, { max = 10000, empty = false } = {}) {
  requireValue(typeof value === 'string', 'invalid_input', `${label} must be text`);
  requireValue((empty || value.trim().length > 0) && value.length <= max,
    'invalid_input', `${label} has an invalid length`);
  requireValue(!value.includes(String.fromCharCode(0)), 'invalid_input', `${label} contains a null character`);
  return value;
}

export function choice(value, choices, label) {
  requireValue(choices.includes(value), 'invalid_input', `invalid ${label}`, { choices });
  return value;
}

export function integer(value, label, minimum = 0, maximum = Number.MAX_SAFE_INTEGER) {
  requireValue(Number.isSafeInteger(value) && value >= minimum && value <= maximum,
    'invalid_input', `${label} must be an integer between ${minimum} and ${maximum}`);
  return value;
}

export function strings(values, label, maximum = 100) {
  requireValue(Array.isArray(values) && values.length <= maximum,
    'invalid_input', `${label} must be a bounded list`);
  return [...new Set(values.map(value => text(value, label, { max: 200 })))];
}

export function optional(value, fallback, parser) {
  return value === undefined ? fallback : parser(value);
}
