export class DomainError extends Error {
  constructor(code, message, details = {}) {
    super(message);
    this.name = 'DomainError';
    this.code = code;
    this.details = details;
  }
}

export function requireValue(condition, code, message, details = {}) {
  if (!condition) throw new DomainError(code, message, details);
}

export function notFound(kind, id) {
  return new DomainError('not_found', `${kind} ${id} does not exist`, { kind, id });
}

export function responseFor(error) {
  if (!(error instanceof DomainError)) throw error;
  return { ok: false, error: { code: error.code, message: error.message, details: error.details } };
}
