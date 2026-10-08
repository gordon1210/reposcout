import { authorize } from '../permissions/policy.mjs';
import { integer, choice } from '../core/validation.mjs';
import { csvCell } from '../exports/formats/csv.mjs';

export function auditEvents(state, input, actorId) {
  authorize(state, actorId, 'audit');
  const after = integer(input.afterSequence ?? 0, 'audit position');
  const limit = integer(input.limit ?? 100, 'audit limit', 1, 500);
  const events = state.events.filter(event => event.sequence > after &&
    (input.actorId === undefined || event.actor === input.actorId) &&
    (input.target === undefined || event.target === input.target) &&
    (input.action === undefined || event.action === input.action));
  return { events: events.slice(0, limit), total: events.length,
    nextSequence: events.slice(0, limit).at(-1)?.sequence ?? after };
}

export function exportAudit(state, input, actorId) {
  const result = auditEvents(state, input, actorId);
  const format = choice(input.format ?? 'json', ['json', 'csv'], 'audit format');
  if (format === 'json') return { content: JSON.stringify(result, null, 2) + '\n', mediaType: 'application/json' };
  const rows = [['sequence', 'actor', 'action', 'target'],
    ...result.events.map(event => [event.sequence, event.actor, event.action, event.target])];
  return { content: rows.map(row => row.map(csvCell).join(',')).join('\r\n') + '\r\n', mediaType: 'text/csv' };
}
