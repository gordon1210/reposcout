import assert from 'node:assert/strict';
import { createApplication } from '../app.mjs';

export function call(app, action, input = {}, actor = 'admin') {
  const response = app.dispatch(action, input, actor);
  assert.equal(response.ok, true, `${action}: ${JSON.stringify(response)}`);
  return response.data;
}

export function fail(app, action, input, code, actor = 'admin') {
  const response = app.dispatch(action, input, actor);
  assert.equal(response.ok, false, `${action} unexpectedly succeeded`);
  assert.equal(response.error.code, code);
  return response.error;
}

export function workspace(options = {}) {
  const app = createApplication();
  const collection = call(app, 'collections.create', { name: options.name ?? 'Operations', public: options.public ?? false });
  for (const [actorId, role] of [['editor', 'editor'], ['reviewer', 'reviewer'], ['reader', 'reader']]) {
    call(app, 'members.assign', { collectionId: collection.id, actorId, role });
  }
  return { app, collection };
}

export function draft(app, collection, input = {}) {
  return call(app, 'documents.create', { collectionId: collection.id,
    title: 'Operational handbook', body: 'Approved operating procedures.', tags: ['operations'], ...input }, 'editor');
}

export function approve(app, item, actorId = 'reviewer') {
  const review = call(app, 'reviews.open', { documentId: item.id, assigneeId: actorId }, 'editor');
  return call(app, 'reviews.decide', { id: review.id, decision: 'approve', note: 'Checked for publication.' }, actorId).approval;
}

export function release(app, approval, name = 'Operational release') {
  const entries = (Array.isArray(approval) ? approval : [approval]).map(item => ({ approvalId: item.id }));
  return call(app, 'releases.publish', { name, entries }, 'reviewer');
}

export function exportRelease(app, selected, format = 'json', requestKey = 'first-export') {
  const job = call(app, 'exports.create', { releaseId: selected.id, requestKey, format }, 'reader');
  return call(app, 'exports.run', { id: job.id }, 'reader').artifact;
}
