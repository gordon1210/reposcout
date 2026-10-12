const stop = new Set(['the', 'a', 'an', 'and', 'or', 'is', 'of', 'to', 'in', 'for']);

export function terms(value) {
  return [...new Set(value.normalize('NFKC').toLocaleLowerCase('en')
    .split(/[^\p{L}\p{N}]+/u).filter(word => word.length > 1 && !stop.has(word)))];
}

export function queryTerms(value) {
  return terms(value).slice(0, 12);
}

export function indexTerms(item) {
  return { title: terms(item.title), tags: item.tags.flatMap(terms), body: terms(item.body) };
}
