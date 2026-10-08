import { requireValue } from '../core/errors.mjs';
import { object, text, strings } from '../core/validation.mjs';

export function placeholders(source) {
  return [...new Set([...source.matchAll(/\{\{\s*([a-zA-Z][a-zA-Z0-9_]*)\s*\}\}/gu)].map(match => match[1]))].sort();
}

export function validateParameters(source, required) {
  const declared = strings(required, 'required parameters', 30).sort();
  const actual = placeholders(source);
  requireValue(actual.every(name => declared.includes(name)) && declared.every(name => actual.includes(name)),
    'parameter_mismatch', 'template placeholders and required parameters differ');
  return declared;
}

export function substitute(source, supplied, required) {
  object(supplied, 'template values');
  requireValue(Object.keys(supplied).every(name => required.includes(name)),
    'extra_parameter', 'template values contain an undeclared parameter');
  for (const name of required) text(supplied[name], `parameter ${name}`, { max: 10000, empty: true });
  return source.replace(/\{\{\s*([a-zA-Z][a-zA-Z0-9_]*)\s*\}\}/gu, (_, name) => supplied[name]);
}
