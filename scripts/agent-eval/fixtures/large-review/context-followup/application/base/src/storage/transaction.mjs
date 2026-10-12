import { snapshot, restore } from './state.mjs';

export function transaction(state, operation) {
  const before = snapshot(state);
  try {
    return operation();
  } catch (error) {
    restore(state, before);
    throw error;
  }
}

export function readOnly(state, operation) {
  const before = snapshot(state);
  const result = operation();
  if (JSON.stringify(before) !== JSON.stringify(state)) {
    restore(state, before);
    throw new Error('read-only operation mutated application state');
  }
  return result;
}
