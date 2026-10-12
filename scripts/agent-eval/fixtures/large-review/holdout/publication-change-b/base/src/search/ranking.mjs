export function score(entry, query) {
  if (query.length === 0) return 0;
  let result = 0;
  for (const term of query) {
    if (entry.terms.title.includes(term)) result += 5;
    else if (entry.terms.tags.includes(term)) result += 3;
    else if (entry.terms.body.includes(term)) result += 1;
    else return 0;
  }
  return result;
}

export function snippet(body, query, maximum = 160) {
  const lower = body.toLocaleLowerCase('en');
  const positions = query.map(term => lower.indexOf(term)).filter(index => index >= 0);
  const first = positions.length ? Math.min(...positions) : 0;
  const start = Math.max(0, first - 40);
  const excerpt = body.slice(start, start + maximum);
  return `${start > 0 ? '…' : ''}${excerpt}${start + maximum < body.length ? '…' : ''}`;
}
