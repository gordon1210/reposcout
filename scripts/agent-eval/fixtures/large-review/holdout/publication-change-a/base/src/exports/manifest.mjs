import { entriesForRelease } from '../releases/catalog.mjs';
import { selectionRequest } from '../content/requests.mjs';

export function createManifest(state, releaseId) {
  const entries = entriesForRelease(state, releaseId);
  return {
    releaseId,
    selection: selectionRequest(entries),
    approvals: entries.map(entry => ({ documentId: entry.documentId, approvalId: entry.approvalId })),
  };
}

export function manifestIdentity(manifest) {
  return JSON.stringify({ releaseId: manifest.releaseId, selection: manifest.selection, approvals: manifest.approvals });
}
