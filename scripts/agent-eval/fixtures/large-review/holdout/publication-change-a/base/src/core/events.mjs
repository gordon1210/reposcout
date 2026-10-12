import { allocate } from './identity.mjs';

export function record(state, actor, action, target, details = {}) {
  state.sequence += 1;
  const event = {
    id: allocate(state, 'event'), sequence: state.sequence,
    actor, action, target, details: structuredClone(details),
  };
  state.events.push(event);
  return event;
}

export function related(state, target, after = 0) {
  return state.events.filter(event => event.target === target && event.sequence > after);
}

export function eventSummary(event) {
  return {
    id: event.id, sequence: event.sequence, action: event.action,
    target: event.target, actor: event.actor,
  };
}
