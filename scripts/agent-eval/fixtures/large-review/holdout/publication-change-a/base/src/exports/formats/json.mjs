export function jsonArtifact(release, items) {
  return {
    mediaType: 'application/json', extension: 'json',
    content: JSON.stringify({ releaseId: release.id, name: release.name,
      documents: items.map(({ documentId, revisionId, title, body, tags, checksum, language }) =>
        ({ documentId, revisionId, title, body, tags, checksum, language })) }, null, 2) + '\n',
  };
}

export function parseJsonArtifact(artifact) {
  if (artifact.mediaType !== 'application/json') throw new Error('not a JSON artifact');
  const parsed = JSON.parse(artifact.content);
  if (!Array.isArray(parsed.documents)) throw new Error('artifact is missing its document list');
  return parsed;
}
