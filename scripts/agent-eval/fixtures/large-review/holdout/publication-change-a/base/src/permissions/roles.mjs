const actions = {
  admin: ['read', 'edit', 'review', 'publish', 'manage', 'audit'],
  owner: ['read', 'edit', 'review', 'publish', 'manage'],
  editor: ['read', 'edit'],
  reviewer: ['read', 'review', 'publish'],
  reader: ['read'],
};

export function canRole(role, action) {
  return actions[role]?.includes(action) ?? false;
}

export function roleNames() {
  return Object.keys(actions);
}

export function roleCapabilities(role) {
  return [...(actions[role] ?? [])];
}
