import { initialState, snapshot } from '../storage/state.mjs';
import { transaction } from '../storage/transaction.mjs';
import { object, text } from '../core/validation.mjs';
import { requireValue, responseFor } from '../core/errors.mjs';
import { actor } from '../permissions/policy.mjs';
import { routes } from './routes.mjs';

export function createApplication() {
  const state = initialState();
  return {
    dispatch(action, input = {}, actorId = 'admin') {
      try {
        text(action, 'action', { max: 100 });
        object(input);
        const handler = Object.hasOwn(routes, action) ? routes[action] : null;
        requireValue(handler, 'unknown_action', 'action is not registered', { action });
        if (action !== 'shares.read') actor(state, actorId);
        const data = transaction(state, () => handler(state, structuredClone(input), actorId));
        return { ok: true, data: structuredClone(data) };
      } catch (error) {
        return responseFor(error);
      }
    },
    inspect() { return snapshot(state); },
    actions() { return Object.keys(routes).sort(); },
  };
}
