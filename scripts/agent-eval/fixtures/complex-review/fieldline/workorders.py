"""Work orders ("jobs"): the central lifecycle module.

The base layout keeps the lifecycle in `src/jobs/jobs.ts` with numbering, priorities and notes
beside it. The module-split variant distributes the same code over `src/workorders/*` behind an
index barrel and renames the exported API from Job* to WorkOrder*.
"""

from .core import JOB_RENAMES, SPLIT_HOMES, TYPE_SYMBOLS, imports, render

# Internal helpers shared between lifecycle segments; module-private in the single-file layout.
HELPERS = ("applyTransition", "publish", "activeHold", "loadForUpdate")
HELPER_HOME = "src/workorders/transitions.ts"


class Segment:
    def __init__(self, module, code, external=None, uses=(), helpers=()):
        self.module = module
        self.code = code
        self.external = external or {}
        self.uses = uses
        self.helpers = helpers


def _model(ctx):
    prefix = "workorder" if ctx.split else "job"
    return Segment("src/workorders/model.ts", render(ctx, '''
export type «JobStatus» =
  | 'new'
  | 'scheduled'
  | 'assigned'
  | 'in_progress'
  | 'on_hold'
  | 'completed'
  | 'closed'
  | 'cancelled';

export type «JobPriority» = 'low' | 'normal' | 'high' | 'emergency';

export const «JOB_PRIORITIES»: readonly «JobPriority»[] = ['low', 'normal', 'high', 'emergency'];

export type HoldReason =
  | 'awaiting_parts'
  | 'customer_unavailable'
  | 'access_denied'
  | 'awaiting_approval'
  | 'weather'
  | 'safety';

export const HOLD_REASONS: readonly HoldReason[] = [
  'awaiting_parts',
  'customer_unavailable',
  'access_denied',
  'awaiting_approval',
  'weather',
  'safety',
];

export interface «JobHold» {
  reason: HoldReason;
  since: Timestamp;
  note?: string;
  releasedAt?: Timestamp;
  /** Status the work order returns to when the hold is released. */
  resumeTo: «JobStatus»;
}

export interface «JobHistoryEntry» {
  at: Timestamp;
  status: «JobStatus»;
  actorId: string;
  note?: string;
}

export interface CompletionReport {
  completedAt: Timestamp;
  resolution: string;
  partsUsed: Array<{ sku: string; quantity: number }>;
  signatureName?: string;
  followUpRequired: boolean;
  meterReading?: number;
}

export interface «Job» {
  id: string;
  number: string;
  customerId: string;
  siteId: string;
  assetId?: string;
  workType: string;
  priority: «JobPriority»;
  status: «JobStatus»;
  summary: string;
  description: string;
  source: 'phone' | 'portal' | 'contract' | 'api';
  requestedAt: Timestamp;
  scheduledStart?: Timestamp;
  scheduledEnd?: Timestamp;
  technicianId?: string;
  contractId?: string;
  startedAt?: Timestamp;
  completion?: CompletionReport;
  holds: «JobHold»[];
  history: «JobHistoryEntry»[];
  checklist: Record<string, boolean>;
  tags: string[];
  followUpOf?: string;
  cancelReason?: string;
  reopenCount: number;
}

export interface «OpenJobInput» {
  customerId: string;
  siteId: string;
  assetId?: string;
  workType: string;
  summary: string;
  description?: string;
  priority?: «JobPriority»;
  source?: «Job»['source'];
  tags?: string[];
}

export interface «JobFilter» {
  status?: «JobStatus»[];
  customerId?: string;
  siteId?: string;
  technicianId?: string;
  workType?: string;
  priority?: «JobPriority»;
  requestedFrom?: Timestamp;
  requestedTo?: Timestamp;
  text?: string;
}

export const OPEN_STATUSES: readonly «JobStatus»[] = ['new', 'scheduled', 'assigned', 'in_progress', 'on_hold'];

/** Event types published on the application bus for work-order lifecycle changes. */
export const «JobEvents» = {
  opened: '«p».opened',
  scheduled: '«p».scheduled',
  assigned: '«p».assigned',
  started: '«p».started',
  held: '«p».held',
  resumed: '«p».resumed',
  completed: '«p».completed',
  cancelled: '«p».cancelled',
  closed: '«p».closed',
  reopened: '«p».reopened',
} as const;

const TRANSITIONS: Record<«JobStatus», readonly «JobStatus»[]> = {
  new: ['scheduled', 'assigned', 'on_hold', 'cancelled'],
  scheduled: ['scheduled', 'assigned', 'on_hold', 'cancelled'],
  assigned: ['scheduled', 'assigned', 'in_progress', 'on_hold', 'cancelled'],
  in_progress: ['on_hold', 'completed', 'cancelled'],
  on_hold: ['new', 'scheduled', 'assigned', 'in_progress', 'cancelled'],
  completed: ['closed', 'new', 'assigned'],
  closed: ['new', 'assigned'],
  cancelled: [],
};

export function isOpenStatus(status: «JobStatus»): boolean {
  return OPEN_STATUSES.includes(status);
}

export function canTransition(from: «JobStatus», to: «JobStatus»): boolean {
  return TRANSITIONS[from].includes(to);
}
''', p=prefix), external={"src/shared/time.ts": ["type Timestamp"]})


def _transitions(ctx):
    export = "export " if ctx.split else ""
    return Segment(HELPER_HOME, render(ctx, '''
«export»function loadForUpdate(ctx: AppContext, jobId: string): «Job» {
  return jobs(ctx).require(jobId, 'work order');
}

«export»function activeHold(job: «Job»): «JobHold» | undefined {
  return job.holds.find((hold) => hold.releasedAt === undefined);
}

«export»function applyTransition(
  ctx: AppContext,
  job: «Job»,
  to: «JobStatus»,
  actorId: string,
  change: Partial<«Job»> = {},
  note?: string,
): «Job» {
  if (!canTransition(job.status, to)) {
    throw new ConflictError('invalid-transition', `Work order ${job.number} cannot move from ${job.status} to ${to}`, {
      from: job.status,
      to,
    });
  }
  const entry: «JobHistoryEntry» = { at: ctx.clock.now(), status: to, actorId, ...(note ? { note } : {}) };
  return jobs(ctx).update(job.id, (current) => ({
    ...current,
    ...change,
    status: to,
    history: [...current.history, entry],
  }));
}

«export»function publish(
  ctx: AppContext,
  type: string,
  job: «Job»,
  actorId: string,
  extra: Record<string, unknown> = {},
): void {
  ctx.bus.emit({
    type,
    at: ctx.clock.now(),
    jobId: job.id,
    number: job.number,
    customerId: job.customerId,
    siteId: job.siteId,
    status: job.status,
    actorId,
    ...extra,
  });
}
''', export=export), external={
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/jobs.ts": ["jobs"],
        "src/shared/errors.ts": ["ConflictError"],
    }, uses=("type Job", "type JobHold", "type JobHistoryEntry", "type JobStatus", "canTransition"))


def _queries(ctx):
    return Segment("src/workorders/queries.ts", render(ctx, '''
export function «getJob»(ctx: AppContext, jobId: string): «Job» | undefined {
  return jobs(ctx).get(jobId);
}

export function «requireJob»(ctx: AppContext, jobId: string): «Job» {
  return jobs(ctx).require(jobId, 'work order');
}

function matchesText(job: «Job», text: string): boolean {
  const needle = text.trim().toLowerCase();
  if (!needle) return true;
  return (
    job.number.toLowerCase().includes(needle) ||
    job.summary.toLowerCase().includes(needle) ||
    job.tags.some((tag) => tag.toLowerCase() === needle)
  );
}

const PRIORITY_ORDER: Record<«JobPriority», number> = { emergency: 0, high: 1, normal: 2, low: 3 };

/** Work orders matching every given criterion, most urgent first, then oldest request first. */
export function «listJobs»(ctx: AppContext, filter: «JobFilter» = {}): «Job»[] {
  const statuses = filter.status && filter.status.length > 0 ? new Set(filter.status) : undefined;
  return jobs(ctx)
    .find(
      (job) =>
        (statuses === undefined || statuses.has(job.status)) &&
        (filter.customerId === undefined || job.customerId === filter.customerId) &&
        (filter.siteId === undefined || job.siteId === filter.siteId) &&
        (filter.technicianId === undefined || job.technicianId === filter.technicianId) &&
        (filter.workType === undefined || job.workType === filter.workType) &&
        (filter.priority === undefined || job.priority === filter.priority) &&
        (filter.requestedFrom === undefined || job.requestedAt >= filter.requestedFrom) &&
        (filter.requestedTo === undefined || job.requestedAt < filter.requestedTo) &&
        (filter.text === undefined || matchesText(job, filter.text)),
    )
    .sort(
      (left, right) =>
        PRIORITY_ORDER[left.priority] - PRIORITY_ORDER[right.priority] ||
        left.requestedAt.localeCompare(right.requestedAt) ||
        left.id.localeCompare(right.id),
    );
}

export interface TimelineItem {
  at: Timestamp;
  kind: 'status' | 'hold' | 'release';
  label: string;
  actorId?: string;
}

/** Status changes and hold periods in chronological order. */
export function «jobTimeline»(ctx: AppContext, jobId: string): TimelineItem[] {
  const job = jobs(ctx).require(jobId, 'work order');
  const items: TimelineItem[] = job.history.map((entry) => ({
    at: entry.at,
    kind: 'status',
    label: entry.note ? `${entry.status}: ${entry.note}` : entry.status,
    actorId: entry.actorId,
  }));
  for (const hold of job.holds) {
    items.push({ at: hold.since, kind: 'hold', label: `on hold (${hold.reason.replace(/_/g, ' ')})` });
    if (hold.releasedAt) items.push({ at: hold.releasedAt, kind: 'release', label: 'hold released' });
  }
  const rank = { status: 0, hold: 1, release: 2 } as const;
  return items.sort((left, right) => compareTimestamps(left.at, right.at) || rank[left.kind] - rank[right.kind]);
}

/** Minutes since the request, excluding time spent on hold. */
export function «jobAge»(job: «Job», at: Timestamp): number {
  const end = job.completion?.completedAt ?? at;
  let held = 0;
  for (const hold of job.holds) {
    held += minutesBetween(hold.since, hold.releasedAt ?? end);
  }
  return Math.max(0, minutesBetween(job.requestedAt, end) - held);
}
'''), external={
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/jobs.ts": ["jobs"],
        "src/shared/time.ts": ["compareTimestamps", "minutesBetween", "type Timestamp"],
    }, uses=("type Job", "type JobFilter", "type JobPriority"))


def _lifecycle(ctx):
    return Segment("src/workorders/lifecycle.ts", render(ctx, '''
/**
 * Opens a work order for a customer site. Suspended customers may only raise emergencies, the
 * work type must be registered and an asset, when given, must be installed at the site.
 */
export function «openJob»(ctx: AppContext, actorId: string, input: «OpenJobInput»): «Job» {
  const customer = customers(ctx).require(input.customerId, 'customer');
  const site = requireSiteOf(ctx, customer.id, input.siteId);
  const handler = ctx.workTypes.lookup(input.workType);
  if (!handler) throw new DomainError('unknown-work-type', `Unknown work type ${input.workType}`);
  const now = ctx.clock.now();
  const coverage = coverageFor(ctx, site.id, handler.key, dateOf(now));
  const priority = input.priority ?? «defaultPriorityFor»(handler, coverage?.contract);
  if (!acceptsNewWork(customer, priority === 'emergency')) {
    throw new ConflictError('customer-not-accepting-work', `Customer ${customer.id} is ${customer.status}`);
  }
  if (input.assetId !== undefined) {
    const asset = assets(ctx).require(input.assetId, 'asset');
    if (asset.siteId !== site.id) throw new DomainError('asset-not-at-site', `Asset ${asset.id} is not at site ${site.id}`);
    if (asset.retiredOn !== undefined) throw new DomainError('asset-retired', `Asset ${asset.id} has been retired`);
  } else if (handler.requiresAsset) {
    throw new DomainError('asset-required', `Work type ${handler.key} requires an asset`);
  }
  const tags = new Set((input.tags ?? []).map((tag) => tag.trim().toLowerCase()).filter(Boolean));
  if (coverage?.includedVisitAvailable && handler.includedInPlans) tags.add('included-visit');
  const job: «Job» = {
    id: ctx.ids.next('WO'),
    number: «nextJobNumber»(ctx, dateOf(now)),
    customerId: customer.id,
    siteId: site.id,
    assetId: input.assetId,
    workType: handler.key,
    priority,
    status: 'new',
    summary: input.summary.trim(),
    description: input.description?.trim() ?? '',
    source: input.source ?? 'phone',
    requestedAt: now,
    contractId: coverage?.contract.id,
    holds: [],
    history: [{ at: now, status: 'new', actorId }],
    checklist: Object.fromEntries(handler.checklist.map((item) => [item, false])),
    tags: [...tags].sort(),
    reopenCount: 0,
  };
  jobs(ctx).insert(job);
  publish(ctx, «JobEvents».opened, job, actorId, { workType: job.workType, priority: job.priority, contractId: job.contractId });
  return job;
}

export function «scheduleJob»(
  ctx: AppContext,
  actorId: string,
  jobId: string,
  start: Timestamp,
  durationMinutes: number,
): «Job» {
  if (!Number.isInteger(durationMinutes) || durationMinutes <= 0) {
    throw new DomainError('invalid-duration', 'Duration must be a positive number of minutes');
  }
  const job = loadForUpdate(ctx, jobId);
  const status = job.technicianId && job.status !== 'new' ? 'assigned' : 'scheduled';
  const updated = applyTransition(ctx, job, status, actorId, {
    scheduledStart: start,
    scheduledEnd: addMinutes(start, durationMinutes),
  });
  publish(ctx, «JobEvents».scheduled, updated, actorId, { start, end: updated.scheduledEnd });
  return updated;
}

export function «assignJob»(ctx: AppContext, actorId: string, jobId: string, technicianId: string): «Job» {
  const job = loadForUpdate(ctx, jobId);
  const technician = technicians(ctx).require(technicianId, 'technician');
  if (!technician.active) throw new ConflictError('technician-inactive', `Technician ${technicianId} is not active`);
  const handler = ctx.workTypes.resolve(job.workType);
  const day = dateOf(job.scheduledStart ?? ctx.clock.now());
  const missing = missingSkills(technician, handler.skills, day);
  if (missing.length > 0) {
    throw new ConflictError('technician-not-qualified', `Technician ${technicianId} lacks ${missing.join(', ')}`, {
      missing,
    });
  }
  const updated = applyTransition(ctx, job, 'assigned', actorId, { technicianId });
  publish(ctx, «JobEvents».assigned, updated, actorId, { technicianId, previousTechnicianId: job.technicianId });
  return updated;
}

export function «startJob»(ctx: AppContext, actorId: string, jobId: string): «Job» {
  const job = loadForUpdate(ctx, jobId);
  if (!job.technicianId) throw new ConflictError('unassigned', `Work order ${job.number} has no technician`);
  const now = ctx.clock.now();
  const updated = applyTransition(ctx, job, 'in_progress', actorId, { startedAt: job.startedAt ?? now });
  publish(ctx, «JobEvents».started, updated, actorId, { firstStart: job.startedAt === undefined });
  return updated;
}

export function «cancelJob»(ctx: AppContext, actorId: string, jobId: string, reason: string): «Job» {
  const job = loadForUpdate(ctx, jobId);
  if (!reason.trim()) throw new DomainError('reason-required', 'A cancellation reason is required');
  const hold = activeHold(job);
  const now = ctx.clock.now();
  const updated = applyTransition(
    ctx,
    job,
    'cancelled',
    actorId,
    {
      cancelReason: reason.trim(),
      holds: hold ? job.holds.map((entry) => (entry === hold ? { ...entry, releasedAt: now } : entry)) : job.holds,
    },
    reason.trim(),
  );
  publish(ctx, «JobEvents».cancelled, updated, actorId, { reason: reason.trim(), wasStarted: job.startedAt !== undefined });
  return updated;
}

export function «closeJob»(ctx: AppContext, actorId: string, jobId: string): «Job» {
  const job = loadForUpdate(ctx, jobId);
  if (job.status !== 'completed') {
    throw new ConflictError('not-completed', `Work order ${job.number} must be completed before it is closed`);
  }
  const updated = applyTransition(ctx, job, 'closed', actorId);
  publish(ctx, «JobEvents».closed, updated, actorId);
  return updated;
}

/** Reopens completed or closed work; a work order can be reopened at most three times. */
export function «reopenJob»(ctx: AppContext, actorId: string, jobId: string, note: string): «Job» {
  const job = loadForUpdate(ctx, jobId);
  if (job.reopenCount >= 3) {
    throw new ConflictError('reopen-limit', `Work order ${job.number} was already reopened ${job.reopenCount} times`);
  }
  const target = job.technicianId ? 'assigned' : 'new';
  const updated = applyTransition(
    ctx,
    job,
    target,
    actorId,
    { reopenCount: job.reopenCount + 1, completion: undefined },
    note,
  );
  publish(ctx, «JobEvents».reopened, updated, actorId, { note });
  return updated;
}
'''), external={
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/assets.ts": ["assets"],
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/adapters/repositories/jobs.ts": ["jobs"],
        "src/adapters/repositories/sites.ts": ["requireSiteOf"],
        "src/adapters/repositories/technicians.ts": ["technicians"],
        "src/contracts/entitlements.ts": ["coverageFor"],
        "src/customers/service.ts": ["acceptsNewWork"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/time.ts": ["addMinutes", "dateOf", "type Timestamp"],
        "src/technicians/skills.ts": ["missingSkills"],
    }, uses=("type Job", "type OpenJobInput", "JobEvents", "defaultPriorityFor", "nextJobNumber"),
        helpers=("applyTransition", "publish", "activeHold", "loadForUpdate"))


def _holds(ctx):
    return Segment("src/workorders/holds.ts", render(ctx, '''
/** Puts open work on hold; only one hold can be active at a time. */
export function «holdJob»(
  ctx: AppContext,
  actorId: string,
  jobId: string,
  reason: HoldReason,
  note?: string,
): «Job» {
  if (!HOLD_REASONS.includes(reason)) throw new DomainError('invalid-hold-reason', `Unknown hold reason ${reason}`);
  const job = loadForUpdate(ctx, jobId);
  if (activeHold(job)) throw new ConflictError('already-on-hold', `Work order ${job.number} is already on hold`);
  if (job.status === 'new' && reason === 'awaiting_parts') {
    throw new ConflictError('not-diagnosed', 'Parts holds require the work to be scheduled first');
  }
  const hold: «JobHold» = { reason, since: ctx.clock.now(), resumeTo: job.status, ...(note ? { note } : {}) };
  const updated = applyTransition(ctx, job, 'on_hold', actorId, { holds: [...job.holds, hold] }, note);
  publish(ctx, «JobEvents».held, updated, actorId, { reason, note });
  return updated;
}

export function «resumeJob»(ctx: AppContext, actorId: string, jobId: string, note?: string): «Job» {
  const job = loadForUpdate(ctx, jobId);
  const hold = activeHold(job);
  if (!hold || job.status !== 'on_hold') {
    throw new ConflictError('not-on-hold', `Work order ${job.number} is not on hold`);
  }
  const now = ctx.clock.now();
  const updated = applyTransition(
    ctx,
    job,
    hold.resumeTo,
    actorId,
    { holds: job.holds.map((entry) => (entry === hold ? { ...entry, releasedAt: now } : entry)) },
    note,
  );
  publish(ctx, «JobEvents».resumed, updated, actorId, {
    reason: hold.reason,
    heldMinutes: minutesBetween(hold.since, now),
  });
  return updated;
}

/**
 * Releases parts holds automatically once every reservation for the work order is fulfilled.
 * Controlled by `features.partsAutoResume`.
 */
export function «registerJobSubscriptions»(ctx: AppContext): void {
  ctx.bus.on(InventoryEvents.available, (event) => {
    if (!ctx.config.features.partsAutoResume) return;
    const jobId = String(event.jobId);
    const job = jobs(ctx).get(jobId);
    if (!job || job.status !== 'on_hold') return;
    if (activeHold(job)?.reason !== 'awaiting_parts') return;
    const stillWaiting = reservationsForJob(ctx, jobId).some((reservation) => reservation.status === 'backordered');
    if (!stillWaiting) «resumeJob»(ctx, SYSTEM_ACTOR.id, jobId, 'parts available');
  });
}
'''), external={
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/jobs.ts": ["jobs"],
        "src/adapters/repositories/reservations.ts": ["reservationsForJob"],
        "src/auth/roles.ts": ["SYSTEM_ACTOR"],
        "src/inventory/events.ts": ["InventoryEvents"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/time.ts": ["minutesBetween"],
    }, uses=("type Job", "type JobHold", "type HoldReason", "HOLD_REASONS", "JobEvents"),
        helpers=("applyTransition", "publish", "activeHold", "loadForUpdate"))


def _completion(ctx):
    return Segment("src/workorders/completion.ts", render(ctx, '''
export function recordChecklistItem(ctx: AppContext, actorId: string, jobId: string, item: string, done: boolean): «Job» {
  const job = loadForUpdate(ctx, jobId);
  if (!(item in job.checklist)) {
    throw new DomainError('unknown-checklist-item', `Work order ${job.number} has no checklist item "${item}"`);
  }
  if (job.status !== 'in_progress') {
    throw new ConflictError('not-in-progress', 'Checklist items can only be recorded while work is in progress');
  }
  ctx.log.debug('checklist item recorded', { jobId, item, done, actorId });
  return jobs(ctx).update(jobId, (current) => ({ ...current, checklist: { ...current.checklist, [item]: done } }));
}

export interface CompletionInput {
  resolution: string;
  partsUsed?: Array<{ sku: string; quantity: number }>;
  signatureName?: string;
  followUpRequired?: boolean;
  meterReading?: number;
}

/**
 * Completes work in progress. Every checklist item must be done and the work type handler must
 * accept the report; parts used are published with the completion event for stock consumption.
 */
export function «completeJob»(ctx: AppContext, actorId: string, jobId: string, input: CompletionInput): «Job» {
  const job = loadForUpdate(ctx, jobId);
  if (job.status !== 'in_progress') {
    throw new ConflictError('not-in-progress', `Work order ${job.number} is ${job.status}`);
  }
  const open = Object.entries(job.checklist)
    .filter(([, done]) => !done)
    .map(([item]) => item);
  if (open.length > 0) {
    throw new ConflictError('checklist-incomplete', `Checklist items still open: ${open.join(', ')}`, { open });
  }
  const report: CompletionReport = {
    completedAt: ctx.clock.now(),
    resolution: input.resolution.trim(),
    partsUsed: (input.partsUsed ?? []).filter((line) => line.quantity > 0),
    signatureName: input.signatureName?.trim() || undefined,
    followUpRequired: input.followUpRequired ?? false,
    meterReading: input.meterReading,
  };
  if (!report.resolution) throw new DomainError('resolution-required', 'A resolution summary is required');
  const handler = ctx.workTypes.resolve(job.workType);
  const problems = handler.validateCompletion(job, report);
  if (problems.length > 0) {
    throw new DomainError('completion-rejected', problems.join('; '), 422, { problems });
  }
  const updated = applyTransition(ctx, job, 'completed', actorId, { completion: report });
  if (job.assetId) recordService(ctx, job.assetId, dateOf(report.completedAt), report.meterReading);
  publish(ctx, «JobEvents».completed, updated, actorId, {
    technicianId: updated.technicianId,
    partsUsed: report.partsUsed,
    followUpRequired: report.followUpRequired,
  });
  return updated;
}
'''), external={
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/jobs.ts": ["jobs"],
        "src/assets/service.ts": ["recordService"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/time.ts": ["dateOf"],
    }, uses=("type Job", "type CompletionReport", "JobEvents"),
        helpers=("applyTransition", "publish", "loadForUpdate"))


def _followups(ctx):
    return Segment("src/workorders/followups.ts", render(ctx, '''
/**
 * Opens a follow-up visit for completed work. The follow-up inherits site, asset and contract
 * and keeps the original technician when they are still active.
 */
export function createFollowUp(ctx: AppContext, actorId: string, jobId: string, summary: string): «Job» {
  const original = «requireJob»(ctx, jobId);
  if (original.status !== 'completed' && original.status !== 'closed') {
    throw new ConflictError('not-completed', `Follow-ups require completed work (${original.status})`);
  }
  const existing = jobs(ctx).first((job) => job.followUpOf === original.id && job.status !== 'cancelled');
  if (existing) {
    throw new ConflictError('follow-up-exists', `Work order ${original.number} already has follow-up ${existing.number}`, {
      followUpId: existing.id,
    });
  }
  const followUp = «openJob»(ctx, actorId, {
    customerId: original.customerId,
    siteId: original.siteId,
    assetId: original.assetId,
    workType: original.workType,
    summary,
    description: `Follow-up to ${original.number}: ${original.summary}`,
    priority: original.priority === 'low' ? 'normal' : original.priority,
    source: original.source,
    tags: ['follow-up'],
  });
  const linked = jobs(ctx).update(followUp.id, { followUpOf: original.id });
  const technician = original.technicianId ? technicians(ctx).get(original.technicianId) : undefined;
  return technician?.active ? «assignJob»(ctx, actorId, linked.id, technician.id) : linked;
}
'''), external={
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/jobs.ts": ["jobs"],
        "src/adapters/repositories/technicians.ts": ["technicians"],
        "src/shared/errors.ts": ["ConflictError"],
    }, uses=("type Job", "requireJob", "openJob", "assignJob"))


def _segments(ctx):
    return [_model(ctx), _transitions(ctx), _queries(ctx), _lifecycle(ctx), _holds(ctx), _completion(ctx),
            _followups(ctx)]


def _single_file(ctx):
    here = "src/jobs/jobs.ts"
    external = {}
    for segment in _segments(ctx):
        for module, names in segment.external.items():
            external.setdefault(module, []).extend(names)
    side = []
    for segment in _segments(ctx):
        for name in segment.uses:
            bare = name[5:] if name.startswith("type ") else name
            if ctx.job_home(bare) != here:
                side.append(name)
    header = imports(ctx, here, external, jobs=sorted(set(side)))
    body = "\n".join(segment.code for segment in _segments(ctx))
    return header + "\n" + body


def _split_files(ctx):
    for segment in _segments(ctx):
        groups = {module: list(names) for module, names in segment.external.items()}
        if segment.helpers:
            groups.setdefault(HELPER_HOME, []).extend(segment.helpers)
        jobs_used = [name for name in segment.uses
                     if ctx.job_home(name[5:] if name.startswith("type ") else name) != segment.module]
        header = imports(ctx, segment.module, groups, jobs=jobs_used, barrel=False)
        ctx.add(segment.module, header + "\n" + segment.code)


def numbering(ctx):
    here = f"{ctx.jobs_dir}/numbering.ts"
    return render(ctx, '''
«imports»

/**
 * Human-facing work order numbers: `FL-<year>-<sequence>`. Sequences restart every calendar year
 * and are never reused, even when a work order is cancelled.
 */
export function «nextJobNumber»(ctx: AppContext, on: CalendarDate): string {
  const year = on.slice(0, 4);
  const sequence = sequenceOf(ctx.ids.next(`FL${year}`));
  return `FL-${year}-${sequence.toString().padStart(5, '0')}`;
}

export function parseJobNumber(number: string): { year: number; sequence: number } | undefined {
  const match = /^FL-(\\d{4})-(\\d{5,})$/.exec(number.trim().toUpperCase());
  return match ? { year: Number(match[1]), sequence: Number(match[2]) } : undefined;
}
''', imports=imports(ctx, here, {
        "src/context.ts": ["type AppContext"],
        "src/shared/ids.ts": ["sequenceOf"],
        "src/shared/time.ts": ["type CalendarDate"],
    }))


def priorities(ctx):
    here = f"{ctx.jobs_dir}/priorities.ts"
    return render(ctx, '''
«imports»

const RANK: Record<«JobPriority», number> = { low: 0, normal: 1, high: 2, emergency: 3 };

export function priorityRank(priority: «JobPriority»): number {
  return RANK[priority];
}

/**
 * The work type's default priority, raised one level for premium contracts. Emergencies are
 * never assigned by default; they are always an explicit choice of the caller.
 */
export function defaultPriorityFor(handler: WorkTypeHandler, contract?: Contract): «JobPriority» {
  const base = handler.defaultPriority;
  if (contract?.plan !== 'premium') return base;
  return base === 'high' ? 'high' : escalatePriority(base);
}

export function escalatePriority(priority: «JobPriority»): «JobPriority» {
  const next = «JOB_PRIORITIES»[Math.min(RANK[priority] + 1, «JOB_PRIORITIES».length - 1)];
  return next;
}
''', imports=imports(ctx, here, {
        "src/contracts/model.ts": ["type Contract"],
        "src/worktypes/types.ts": ["type WorkTypeHandler"],
    }, jobs=("type JobPriority", "JOB_PRIORITIES"), barrel=False))


def notes(ctx):
    here = f"{ctx.jobs_dir}/notes.ts"
    return render(ctx, '''
«imports»

export interface «JobNote» {
  id: string;
  jobId: string;
  authorId: string;
  at: Timestamp;
  text: string;
  visibility: 'internal' | 'customer';
}

function noteStore(ctx: AppContext) {
  return ctx.store.collection<«JobNote»>('notes');
}

export function «addJobNote»(
  ctx: AppContext,
  jobId: string,
  authorId: string,
  text: string,
  visibility: «JobNote»['visibility'] = 'internal',
): «JobNote» {
  const job = jobs(ctx).require(jobId, 'work order');
  const trimmed = text.trim();
  if (!trimmed) throw new DomainError('empty-note', 'A note needs text');
  if (trimmed.length > 4000) throw new DomainError('note-too-long', 'Notes are limited to 4000 characters');
  return noteStore(ctx).insert({
    id: ctx.ids.next('NOTE'),
    jobId: job.id,
    authorId,
    at: ctx.clock.now(),
    text: trimmed,
    visibility,
  });
}

/** Customers only see notes shared with them; staff see everything. */
export function «listJobNotes»(ctx: AppContext, jobId: string, audience: 'staff' | 'customer'): «JobNote»[] {
  return noteStore(ctx)
    .find((note) => note.jobId === jobId && (audience === 'staff' || note.visibility === 'customer'))
    .sort((left, right) => left.at.localeCompare(right.at) || left.id.localeCompare(right.id));
}
''', imports=imports(ctx, here, {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/jobs.ts": ["jobs"],
        "src/shared/errors.ts": ["DomainError"],
        "src/shared/time.ts": ["type Timestamp"],
    }))


def barrel(ctx):
    lines = []
    modules = ["model", "lifecycle", "holds", "completion", "followups", "queries", "numbering",
               "priorities", "notes"]
    for module in modules:
        path = f"src/workorders/{module}.ts"
        names = SPLIT_HOMES[path]
        values = sorted((JOB_RENAMES[name] for name in names if name not in TYPE_SYMBOLS), key=str.lower)
        types = sorted((JOB_RENAMES[name] for name in names if name in TYPE_SYMBOLS), key=str.lower)
        if module == "queries":
            types.append("TimelineItem")
        if module == "completion":
            types.append("CompletionInput")
        if values:
            lines.append(_export_statement("export", values, f"./{module}.ts"))
        if types:
            lines.append(_export_statement("export type", sorted(types, key=str.lower), f"./{module}.ts"))
    lines.append("export { parseJobNumber } from './numbering.ts';")
    return "/** Public work-order API; modules outside src/workorders import from here. */\n" + "\n".join(lines)


def _export_statement(keyword, names, specifier):
    single = f"{keyword} {{ {', '.join(names)} }} from '{specifier}';"
    if len(single) <= 100:
        return single
    body = "".join(f"  {name},\n" for name in names)
    return f"{keyword} {{\n{body}}} from '{specifier}';"


def generate(ctx):
    if ctx.split:
        _split_files(ctx)
        ctx.add("src/workorders/index.ts", barrel(ctx))
    else:
        ctx.add("src/jobs/jobs.ts", _single_file(ctx))
    ctx.add(f"{ctx.jobs_dir}/numbering.ts", numbering(ctx))
    ctx.add(f"{ctx.jobs_dir}/priorities.ts", priorities(ctx))
    ctx.add(f"{ctx.jobs_dir}/notes.ts", notes(ctx))
