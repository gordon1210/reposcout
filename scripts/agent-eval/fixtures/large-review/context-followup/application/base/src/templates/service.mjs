import { allocate } from '../core/identity.mjs';
import { text } from '../core/validation.mjs';
import { record } from '../core/events.mjs';
import { authorize } from '../permissions/policy.mjs';
import { entity } from '../storage/lookup.mjs';
import { createDocument } from '../documents/service.mjs';
import { validateParameters, substitute } from './parameters.mjs';

export function createTemplate(state, input, actorId) {
  authorize(state, actorId, 'edit', input.collectionId);
  const titlePattern = text(input.titlePattern, 'title pattern', { max: 240 });
  const bodyPattern = text(input.bodyPattern, 'body pattern', { max: 50000, empty: true });
  const required = validateParameters(titlePattern + '\n' + bodyPattern, input.required ?? []);
  const id = allocate(state, 'template');
  state.templates[id] = { id, collectionId: input.collectionId, name: text(input.name, 'template name', { max: 160 }),
    titlePattern, bodyPattern, required, actorId };
  record(state, actorId, 'template.created', id, { collectionId: input.collectionId });
  return state.templates[id];
}

export function instantiateTemplate(state, input, actorId) {
  const template = entity(state, 'templates', input.id);
  authorize(state, actorId, 'read', template.collectionId);
  const title = substitute(template.titlePattern, input.values, template.required);
  const body = substitute(template.bodyPattern, input.values, template.required);
  const item = createDocument(state, { collectionId: input.collectionId ?? template.collectionId,
    title, body, tags: input.tags ?? [] }, actorId);
  record(state, actorId, 'template.instantiated', item.id, { templateId: template.id });
  return item;
}

export function listTemplates(state, input, actorId) {
  authorize(state, actorId, 'read', input.collectionId);
  return Object.values(state.templates).filter(item => item.collectionId === input.collectionId)
    .map(({ bodyPattern, ...item }) => ({ ...item, characters: bodyPattern.length }));
}
