import { allocate, digest } from '../core/identity.mjs';
import { integer } from '../core/validation.mjs';
import { record } from '../core/events.mjs';
import { requireValue } from '../core/errors.mjs';
import { entity } from '../storage/lookup.mjs';
import { authorize } from '../permissions/policy.mjs';
import { getRelease } from '../releases/queries.mjs';
import { createManifest } from '../exports/manifest.mjs';
import { materializeSelection } from '../content/selection.mjs';

export function createShare(state, input, actorId) {
  const release = getRelease(state, { id: input.releaseId }, actorId);
  requireValue(!release.withdrawn, 'withdrawn_release', 'cannot share a withdrawn release');
  for (const entry of release.entries) authorize(state, actorId, 'publish', state.documents[entry.documentId].collectionId);
  const id = allocate(state, 'share');
  const item = { id, releaseId: release.id, actorId, revoked: false, views: 0,
    maxViews: integer(input.maxViews ?? 100, 'maximum views', 1, 10000),
    token: digest(`synthetic-share:${id}:${release.id}`) };
  state.shares[id] = item;
  record(state, actorId, 'share.created', release.id, { shareId: id });
  return item;
}

export function readShare(state, input) {
  const share = Object.values(state.shares).find(item => item.token === input.token);
  requireValue(share !== undefined, 'not_found', 'share does not exist');
  requireValue(!share.revoked && share.views < share.maxViews, 'share_unavailable', 'share is no longer available');
  const release = entity(state, 'releases', share.releaseId);
  requireValue(!release.withdrawn, 'withdrawn_release', 'release has been withdrawn');
  const items = materializeSelection(state, createManifest(state, release.id).selection);
  share.views += 1;
  record(state, 'external', 'share.viewed', release.id, { shareId: share.id });
  return { releaseId: release.id, name: release.name, documents: items };
}

export function revokeShare(state, input, actorId) {
  const share = entity(state, 'shares', input.id);
  requireValue(share.actorId === actorId || state.users[actorId]?.role === 'admin', 'forbidden', 'share belongs to another actor');
  share.revoked = true;
  record(state, actorId, 'share.revoked', share.releaseId, { shareId: share.id });
  return { id: share.id, revoked: true };
}
