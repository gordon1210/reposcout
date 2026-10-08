import { allocate, digest } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { choice, text } from '../core/validation.mjs';
import { requireValue } from '../core/errors.mjs';
import { getRelease } from '../releases/queries.mjs';
import { createManifest, manifestIdentity } from './manifest.mjs';
import { formatNames } from './render.mjs';

export function enqueueExport(state, input, actorId) {
  const release = getRelease(state, { id: input.releaseId }, actorId);
  const format = choice(input.format ?? 'json', formatNames(), 'export format');
  const requestKey = text(input.requestKey, 'request key', { max: 160 });
  const manifest = createManifest(state, release.id);
  const identity = digest(manifestIdentity(manifest) + ':' + format);
  const prior = Object.values(state.jobs).find(job => job.requestKey === requestKey && job.actorId === actorId);
  if (prior) {
    requireValue(prior.identity === identity, 'request_conflict', 'request key was used for different export input');
    return prior;
  }
  const id = allocate(state, 'job');
  state.jobs[id] = { id, releaseId: release.id, actorId, requestKey, format, manifest,
    identity, status: 'pending', attempts: 0, artifactId: null, error: null };
  record(state, actorId, 'export.queued', release.id, { jobId: id, format });
  return state.jobs[id];
}

export function cancelExport(state, input, actorId) {
  const job = state.jobs[input.id];
  requireValue(job !== undefined, 'not_found', 'export job does not exist');
  requireValue(job.actorId === actorId || state.users[actorId]?.role === 'admin', 'forbidden', 'job belongs to another actor');
  requireValue(job.status === 'pending', 'job_started', 'only pending jobs can be cancelled');
  job.status = 'cancelled';
  record(state, actorId, 'export.cancelled', job.releaseId, { jobId: job.id });
  return job;
}
