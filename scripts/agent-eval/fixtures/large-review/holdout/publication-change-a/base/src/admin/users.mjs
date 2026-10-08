import { text, choice } from '../core/validation.mjs';
import { requireValue } from '../core/errors.mjs';
import { authorize } from '../permissions/policy.mjs';
import { record } from '../core/events.mjs';
import { entity } from '../storage/lookup.mjs';

export function createUser(state, input, actorId) {
  authorize(state, actorId, 'manage');
  const id = text(input.id, 'user id', { max: 100 });
  requireValue(!Object.hasOwn(state.users, id), 'duplicate_user', 'user id already exists');
  const role = choice(input.role ?? 'reader', ['admin', 'editor', 'reviewer', 'reader'], 'user role');
  state.users[id] = { id, displayName: text(input.displayName, 'display name', { max: 160 }), role, active: true };
  record(state, actorId, 'user.created', id, { role });
  return state.users[id];
}

export function setUserActive(state, input, actorId) {
  authorize(state, actorId, 'manage');
  const user = entity(state, 'users', input.id);
  const active = input.active === true;
  requireValue(active || user.id !== actorId, 'self_disable', 'administrator cannot disable the active account');
  if (!active && user.role === 'admin') requireValue(Object.values(state.users).some(item => item.id !== user.id && item.active && item.role === 'admin'),
    'last_administrator', 'workspace needs an active administrator');
  user.active = active;
  record(state, actorId, active ? 'user.enabled' : 'user.disabled', user.id);
  return user;
}

export function listUsers(state, input, actorId) {
  authorize(state, actorId, 'manage');
  return Object.values(state.users).filter(item => input.includeInactive || item.active)
    .sort((a, b) => a.displayName.localeCompare(b.displayName));
}
