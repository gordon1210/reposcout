import { allocate, digest } from '../core/identity.mjs';
import { record } from '../core/events.mjs';
import { requireValue } from '../core/errors.mjs';
import { entity } from '../storage/lookup.mjs';
import { getRelease } from '../releases/queries.mjs';
import { materializeSelection } from '../content/selection.mjs';
import { renderArtifact } from './render.mjs';

export function runExport(state, input, actorId) {
  const job = entity(state, 'jobs', input.id);
  requireValue(job.actorId === actorId || state.users[actorId]?.role === 'admin', 'forbidden', 'job belongs to another actor');
  if (job.status === 'complete') return { job, artifact: entity(state, 'artifacts', job.artifactId) };
  requireValue(job.status === 'pending', 'job_unavailable', 'export job cannot be run');
  const release = getRelease(state, { id: job.releaseId }, actorId);
  requireValue(!release.withdrawn, 'withdrawn_release', 'release has been withdrawn');
  const documents = materializeSelection(state, job.manifest.selection);
  const rendered = renderArtifact(release, documents, job.format);
  const id = allocate(state, 'artifact');
  const artifact = { id, jobId: job.id, releaseId: release.id, ...rendered,
    checksum: digest(rendered.content), bytes: Buffer.byteLength(rendered.content, 'utf8'),
    documents: documents.map(({ documentId, revisionId, checksum }) => ({ documentId, revisionId, checksum })) };
  state.artifacts[id] = artifact;
  job.attempts += 1;
  job.status = 'complete';
  job.artifactId = id;
  record(state, actorId, 'export.completed', release.id, { jobId: job.id, artifactId: id });
  return { job, artifact };
}
