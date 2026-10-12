import { authorize, visibleCollections } from '../permissions/policy.mjs';
import { protectedRevisionIds } from '../retention/rules.mjs';

export function workspaceStatistics(state, input, actorId) {
  const visible = new Set(visibleCollections(state, actorId).map(item => item.id));
  const documents = Object.values(state.documents).filter(item => visible.has(item.collectionId));
  const ids = new Set(documents.map(item => item.id));
  const revisions = Object.values(state.revisions).filter(item => ids.has(item.documentId));
  return {
    collections: visible.size, documents: documents.length,
    archivedDocuments: documents.filter(item => item.archived).length,
    revisions: revisions.length, bodyCharacters: revisions.reduce((sum, item) => sum + item.body.length, 0),
    openReviews: Object.values(state.reviews).filter(item => ids.has(item.documentId) && item.status === 'open').length,
    publications: Object.values(state.releases).filter(item => !item.withdrawn && item.entries.every(entry => ids.has(entry.documentId))).length,
  };
}

export function integrityReport(state, input, actorId) {
  authorize(state, actorId, 'audit');
  const missingRevisions = [];
  for (const id of protectedRevisionIds(state)) if (!state.revisions[id]) missingRevisions.push(id);
  const orphanRevisions = Object.values(state.revisions).filter(item => !state.documents[item.documentId]).map(item => item.id);
  const missingArtifacts = Object.values(state.jobs).filter(item => item.artifactId && !state.artifacts[item.artifactId]).map(item => item.id);
  return { healthy: missingRevisions.length === 0 && orphanRevisions.length === 0 && missingArtifacts.length === 0,
    missingRevisions, orphanRevisions, missingArtifacts };
}
