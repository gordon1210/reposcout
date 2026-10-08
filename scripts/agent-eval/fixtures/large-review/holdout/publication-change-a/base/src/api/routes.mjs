import * as structure from '../structure/service.mjs';
import * as structureDiscussion from '../structure/discussion.mjs';
import * as governancePolicies from '../governance/policies.mjs';
import * as governanceAssessments from '../governance/assessments.mjs';
import * as governancePlans from '../governance/plans.mjs';
import * as governancePublication from '../governance/publication.mjs';
import * as governanceEvolution from '../governance/evolution.mjs';
import { previewBatchImport } from '../exchange/import-plan.mjs';
import { applyBatchImport, getImportReceipt } from '../exchange/import-service.mjs';
import { exportReleaseArchive, exportWorkingArchive } from '../exchange/export-service.mjs';
import { inspectArchive } from '../exchange/archive.mjs';
import { previewSynchronization } from '../exchange/sync-plan.mjs';
import { importSynchronizationAnchors, applySynchronization, getSynchronization } from '../exchange/sync-service.mjs';
import * as collections from '../collections/service.mjs';
import * as documents from '../documents/service.mjs';
import * as documentQueries from '../documents/queries.mjs';
import { compareRevisions } from '../documents/diff.mjs';
import * as workflow from '../workflow/service.mjs';
import * as reviewQueries from '../workflow/queries.mjs';
import * as releases from '../releases/service.mjs';
import * as releaseQueries from '../releases/queries.mjs';
import * as queue from '../exports/queue.mjs';
import { runExport } from '../exports/runner.mjs';
import * as exportQueries from '../exports/queries.mjs';
import { rebuildReleaseIndex } from '../search/index.mjs';
import { search } from '../search/query.mjs';
import * as saved from '../search/saved.mjs';
import * as preview from '../preview/service.mjs';
import * as shares from '../shares/service.mjs';
import * as comments from '../comments/service.mjs';
import * as labels from '../labels/service.mjs';
import * as watches from '../subscriptions/watch.mjs';
import * as digest from '../subscriptions/digest.mjs';
import * as audit from '../audit/query.mjs';
import { setRetention } from '../retention/rules.mjs';
import * as retention from '../retention/service.mjs';
import * as templates from '../templates/service.mjs';
import * as importing from '../importing/service.mjs';
import { checkLinks } from '../content/links.mjs';
import { documentOutline } from '../content/outline.mjs';
import * as users from '../admin/users.mjs';
import * as statistics from '../admin/statistics.mjs';

export const routes = {
  'collections.create': collections.createCollection, 'collections.move': collections.moveCollection,
  'collections.list': collections.listCollections, 'members.assign': collections.assignMember,
  'members.remove': collections.removeMember,
  'documents.create': documents.createDocument, 'documents.revise': documents.reviseDocument,
  'documents.move': documents.moveDocument, 'documents.archive': documents.archiveDocument,
  'documents.owner': documents.transferOwnership, 'documents.get': documentQueries.getDocument,
  'documents.list': documentQueries.listDocuments, 'documents.history': documentQueries.history,
  'documents.diff': compareRevisions, 'documents.links': checkLinks, 'documents.outline': documentOutline,
  'reviews.open': governancePlans.governedOpenReview, 'reviews.assign': workflow.assignReview,
  'reviews.decide': governancePlans.governedDecideReview, 'reviews.get': reviewQueries.getReview,
  'reviews.queue': reviewQueries.reviewQueue,
  'releases.publish': governancePublication.governedPublishRelease, 'releases.withdraw': releases.withdrawRelease,
  'releases.get': releaseQueries.getRelease, 'releases.list': releaseQueries.listReleases,
  'exports.create': queue.enqueueExport, 'exports.cancel': queue.cancelExport,
  'exports.run': runExport, 'exports.get': exportQueries.getExport, 'exports.list': exportQueries.listExports,
  'search.rebuild': rebuildReleaseIndex, 'search.query': search, 'search.save': saved.saveSearch,
  'search.saved': saved.runSavedSearch, 'search.delete': saved.deleteSavedSearch,
  'preview.document': preview.previewDocument, 'preview.bundle': preview.previewBundle,
  'shares.create': shares.createShare, 'shares.read': shares.readShare, 'shares.revoke': shares.revokeShare,
  'comments.add': comments.addComment, 'comments.resolve': comments.resolveComment,
  'comments.list': comments.listComments, 'labels.set': labels.labelDocument,
  'labels.find': labels.findLabel, 'labels.counts': labels.labelCounts,
  'watches.add': watches.watchDocument, 'watches.remove': watches.unwatchDocument,
  'watches.list': watches.listWatches, 'digest.preview': digest.previewDigest, 'digest.deliver': digest.deliverDigest,
  'audit.list': audit.auditEvents, 'audit.export': audit.exportAudit,
  'retention.set': setRetention, 'retention.preview': retention.previewRetention, 'retention.apply': retention.applyRetention,
  'templates.create': templates.createTemplate, 'templates.instantiate': templates.instantiateTemplate,
  'templates.list': templates.listTemplates, 'import.preview': importing.previewImport,
  'import.apply': importing.importDocument, 'users.create': users.createUser,
  'users.active': users.setUserActive, 'users.list': users.listUsers,
  'workspace.statistics': statistics.workspaceStatistics, 'workspace.integrity': statistics.integrityReport,
  'structure.get': structure.getStructure,
  'structure.diff': structure.compareStructure,
  'structure.preview': structure.previewSections,
  'structure.apply': structure.applySections,
  'structure.merge.open': structure.openMerge,
  'structure.merge.get': structure.getMerge,
  'structure.merge.resolve': structure.resolveMerge,
  'structure.merge.commit': structure.commitMerge,
  'structure.merge.abandon': structure.abandonMerge,
  'structure.compare': structureDiscussion.compareRevisionStructure,
  'structure.anchors.capture': structureDiscussion.captureDiscussionAnchor,
  'structure.anchors.get': structureDiscussion.getDiscussionAnchor,
  'structure.anchors.preview': structureDiscussion.previewAnchorMap,
  'structure.anchors.followup': structureDiscussion.createAnchorFollowups,
  'structure.anchors.followups': structureDiscussion.listAnchorFollowups,
  'governance.policies.create': governancePolicies.createPolicy,
  'governance.policies.revise': governancePolicies.revisePolicy,
  'governance.policies.get': governancePolicies.getPolicy,
  'governance.policies.assign': governancePolicies.assignPolicy,
  'governance.policies.effective': governancePolicies.inspectPolicy,
  'governance.assessments.preview': governanceAssessments.previewAssessment,
  'governance.assessments.create': governanceAssessments.createAssessment,
  'governance.assessments.get': governanceAssessments.getAssessment,
  'governance.plans.create': governancePlans.createPlan,
  'governance.plans.get': governancePlans.getPlan,
  'governance.submit': governancePlans.submitStage,
  'governance.checklist.record': governancePlans.recordChecklist,
  'governance.publication.prepare': governancePublication.preparePublication,
  'governance.policies.compare': governanceEvolution.comparePolicyVersions,
  'governance.assignments.preview': governanceEvolution.previewAssignment,
  'governance.assignments.apply': governanceEvolution.applyAssignmentPreview,
  'exchange.import.preview': previewBatchImport,
  'exchange.import.apply': applyBatchImport,
  'exchange.import.get': getImportReceipt,
  'exchange.export.release': exportReleaseArchive,
  'exchange.export.working': exportWorkingArchive,
  'exchange.archive.inspect': inspectArchive,
  'exchange.import.anchors': importSynchronizationAnchors,
  'exchange.sync.preview': previewSynchronization,
  'exchange.sync.apply': applySynchronization,
  'exchange.sync.get': getSynchronization,
};
