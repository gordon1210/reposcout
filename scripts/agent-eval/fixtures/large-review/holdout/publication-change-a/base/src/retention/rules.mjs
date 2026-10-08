import { requireValue } from '../core/errors.mjs';
import { integer } from '../core/validation.mjs';
import { authorize } from '../permissions/policy.mjs';
import { entity } from '../storage/lookup.mjs';

export function setRetention(state, input, actorId) {
  authorize(state, actorId, 'manage', input.collectionId);
  entity(state, 'collections', input.collectionId);
  const keepRecent = integer(input.keepRecent, 'recent revisions to retain', 1, 100);
  const item = { collectionId: input.collectionId, keepRecent };
  state.retentionRules[input.collectionId] = item;
  return item;
}

export function protectedRevisionIds(state) {
  const ids = new Set(Object.values(state.documents).map(item => item.currentRevisionId));
  for (const approval of Object.values(state.approvals)) ids.add(approval.revisionId);
  for (const review of Object.values(state.reviews)) ids.add(review.revisionId);
  for (const release of Object.values(state.releases)) for (const entry of release.entries) ids.add(entry.revisionId);
  for (const comment of Object.values(state.comments)) ids.add(comment.revisionId);
  return ids;
}

export function retentionRule(state, collectionId) {
  const rule = state.retentionRules[collectionId];
  requireValue(rule !== undefined, 'missing_retention_rule', 'configure retention before previewing it');
  return rule;
}
