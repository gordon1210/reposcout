export function csvCell(value) {
  let string = String(value);
  if (/^[=+@-]/u.test(string)) string = `'${string}`;
  return /[",\r\n]/u.test(string) ? `"${string.replaceAll('"', '""')}"` : string;
}

export function csvArtifact(release, items) {
  const header = ['release_id', 'document_id', 'revision_id', 'title', 'language', 'checksum'];
  const rows = items.map(item => [release.id, item.documentId, item.revisionId,
    item.title, item.language, item.checksum]);
  return { mediaType: 'text/csv; charset=utf-8', extension: 'csv',
    content: [header, ...rows].map(row => row.map(csvCell).join(',')).join('\r\n') + '\r\n' };
}
