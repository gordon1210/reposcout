export function initialState() {
  return {
    counters: {}, sequence: 0,
    structureMerges: {},
    structureAnchors: {},
    structureFollowups: {},
    governancePolicies: {},
    governanceVersions: {},
    governanceAssignments: {},
    governanceAssessments: {},
    governancePlans: {},
    governanceStageReviews: {},
    governanceChecklist: {},
    exchangeReceipts: {},
    exchangeSyncReceipts: {},
    users: {
      admin: { id: 'admin', displayName: 'Workspace administrator', role: 'admin', active: true },
      editor: { id: 'editor', displayName: 'Document editor', role: 'editor', active: true },
      reviewer: { id: 'reviewer', displayName: 'Independent reviewer', role: 'reviewer', active: true },
      reader: { id: 'reader', displayName: 'Workspace reader', role: 'reader', active: true },
    },
    collections: {}, memberships: {}, documents: {}, revisions: {},
    reviews: {}, approvals: {}, releases: {}, jobs: {}, artifacts: {},
    shares: {}, comments: {}, watches: {}, deliveries: {}, labels: {},
    templates: {}, savedSearches: {}, retentionRules: {}, events: [], searchEntries: [],
  };
}

export function snapshot(state) {
  return structuredClone(state);
}

export function restore(state, saved) {
  for (const name of Object.keys(state)) delete state[name];
  Object.assign(state, structuredClone(saved));
}
