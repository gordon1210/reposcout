import { entity, revision } from '../storage/lookup.mjs';
import { requireValue } from '../core/errors.mjs';

export function releaseEntry(state, input) {
  const approval = entity(state, 'approvals', input.approvalId);
  const selected = revision(state, approval.revisionId, approval.documentId);
  requireValue(input.documentId === undefined || input.documentId === approval.documentId,
    'approval_mismatch', 'approval belongs to another document');
  return {
    documentId: approval.documentId,
    revisionId: selected.id,
    approvalId: approval.id,
    checksum: selected.checksum,
  };
}

export function entriesForRelease(state, releaseId) {
  const release = entity(state, 'releases', releaseId);
  requireValue(!release.withdrawn, 'withdrawn_release', 'release has been withdrawn');
  return release.entries;
}
