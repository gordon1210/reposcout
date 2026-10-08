import { entity } from '../storage/lookup.mjs';
import { requireValue } from '../core/errors.mjs';
import { getRelease } from '../releases/queries.mjs';
import { page } from '../core/pagination.mjs';

export function getExport(state, input, actorId) {
  const job = entity(state, 'jobs', input.id);
  getRelease(state, { id: job.releaseId }, actorId);
  requireValue(job.actorId === actorId || state.users[actorId]?.role === 'admin', 'forbidden', 'job belongs to another actor');
  return { job, artifact: job.artifactId ? entity(state, 'artifacts', job.artifactId) : null };
}

export function listExports(state, input, actorId) {
  return page(Object.values(state.jobs).filter(job => job.actorId === actorId &&
    (input.status === undefined || job.status === input.status)), input);
}
