import { requireValue } from '../core/errors.mjs';
import { documentLinks } from '../content/links.mjs';
import { getDocument } from '../documents/queries.mjs';

export function portableLinks(body) {
  const links = [...body.matchAll(/\[([^\]\n]+)\]\(exchange:([a-zA-Z0-9][a-zA-Z0-9._-]{0,79})\)/gu)]
    .map(match => ({ label: match[1], key: match[2], offset: match.index, source: match[0] }));
  const starts = [...body.matchAll(/\]\(exchange:/gu)];
  requireValue(starts.length === links.length, 'exchange_reference_syntax',
    'portable references must use [label](exchange:key) syntax');
  return links;
}

export function referenceGraph(manifest) {
  const keys = new Set(manifest.documents.map(item => item.key));
  const edges = new Map();
  const diagnostics = [];
  for (const item of manifest.documents) {
    let links = [];
    try { links = portableLinks(item.body); }
    catch (error) {
      diagnostics.push({ key: item.key, code: error.code, message: error.message, path: 'body' });
    }
    edges.set(item.key, [...new Set(links.map(link => link.key))]);
    for (const link of links) if (!keys.has(link.key)) {
      diagnostics.push({ key: item.key, code: 'exchange_missing_reference', message: 'portable reference target is absent',
        path: 'body', offset: link.offset, targetKey: link.key });
    }
  }
  return { edges, diagnostics };
}

export function referenceComponents(manifest, edges) {
  const neighbors = new Map(manifest.documents.map(item => [item.key, new Set()]));
  for (const [key, targets] of edges) for (const target of targets) {
    if (!neighbors.has(target)) continue;
    neighbors.get(key).add(target);
    neighbors.get(target).add(key);
  }
  const seen = new Set();
  const components = [];
  for (const item of manifest.documents) {
    if (seen.has(item.key)) continue;
    const pending = [item.key];
    const component = [];
    while (pending.length) {
      const key = pending.pop();
      if (seen.has(key)) continue;
      seen.add(key);
      component.push(key);
      pending.push(...neighbors.get(key));
    }
    components.push(component.sort());
  }
  return components;
}

export function bindBody(state, body, bindings, actorId) {
  for (const link of documentLinks(body)) {
    getDocument(state, { id: link.documentId,
      ...(link.revisionId ? { revisionId: link.revisionId } : {}) }, actorId);
  }
  return body.replace(/\[([^\]\n]+)\]\(exchange:([a-zA-Z0-9][a-zA-Z0-9._-]{0,79})\)/gu,
    (_source, label, key) => {
      const id = bindings.get(key);
      requireValue(id, 'exchange_missing_reference', 'reference target has no destination', { key });
      return `[${label}](doc:${id})`;
    });
}

export function portableBody(body, selected, externalPolicy) {
  const diagnostics = [];
  const links = documentLinks(body);
  let cursor = 0;
  let output = '';
  for (const link of links) {
    output += body.slice(cursor, link.offset);
    const target = selected.get(link.documentId);
    if (target && (!link.revisionId || target.revisionId === link.revisionId)) {
      output += `[${link.label}](exchange:${target.key})`;
    } else {
      diagnostics.push({ code: 'exchange_external_reference', documentId: link.documentId,
        revisionId: link.revisionId, offset: link.offset });
      requireValue(externalPolicy === 'preserve', 'exchange_external_reference',
        'export reference is outside the selected revision set', diagnostics.at(-1));
      output += link.source;
    }
    cursor = link.offset + link.source.length;
  }
  return { body: output + body.slice(cursor), diagnostics };
}
