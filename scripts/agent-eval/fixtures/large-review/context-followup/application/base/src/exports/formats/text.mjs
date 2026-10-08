export function textArtifact(release, items) {
  const sections = items.map(item => [item.title, '='.repeat(item.title.length), item.body,
    `Revision: ${item.revisionId}`, `Checksum: ${item.checksum}`].join('\n'));
  return { mediaType: 'text/plain; charset=utf-8', extension: 'txt',
    content: `${release.name}\n\n${sections.join('\n\n---\n\n')}\n` };
}

export function textSummary(items) {
  return items.map(item => ({ documentId: item.documentId, title: item.title,
    words: item.body.trim() === '' ? 0 : item.body.trim().split(/\s+/u).length }));
}
