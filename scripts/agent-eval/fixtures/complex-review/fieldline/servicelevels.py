"""Service level agreements: policies, clocks, pausing and credits."""

from .core import imports, render, rounding, rounding_names


def sla_model(ctx):
    return render(ctx, '''
«imports»

export type Coverage = 'business' | 'extended' | 'always';

export interface SlaPolicy {
  id: string;
  name: string;
  /** Working minutes on the coverage calendar until a technician must be on site. */
  responseMinutes: number;
  /** Working minutes on the coverage calendar until the work must be completed. */
  resolutionMinutes: number;
  coverage: Coverage;
  /** Credit per started hour of resolution breach, in basis points of the invoice. */
  creditBasisPointsPerHour: number;
}

export type ClockStatus = 'running' | 'paused' | 'met' | 'breached' | 'stopped';

export interface SlaClock {
  id: string;
  jobId: string;
  policyId: string;
  startedAt: Timestamp;
  status: ClockStatus;
  pausedSince?: Timestamp;
  /** Working minutes spent paused so far. */
  pausedMinutes: number;
  pauses: number;
  respondedAt?: Timestamp;
  responseBreached: boolean;
  resolvedAt?: Timestamp;
  breachMinutes?: number;
  creditBasisPoints?: number;
  warned: boolean;
}

export const SlaEvents = {
  started: 'sla.started',
  paused: 'sla.paused',
  resumed: 'sla.resumed',
  warning: 'sla.warning',
  met: 'sla.met',
  breached: 'sla.breached',
} as const;
''', imports=imports(ctx, "src/sla/model.ts", {"src/shared/time.ts": ["type Timestamp"]}))


def sla_policies(ctx):
    return render(ctx, '''
«imports»

export const SLA_POLICIES: Readonly<Record<string, SlaPolicy>> = {
  standard: {
    id: 'standard',
    name: 'Standard',
    responseMinutes: 1200,
    resolutionMinutes: 3000,
    coverage: 'business',
    creditBasisPointsPerHour: 50,
  },
  priority: {
    id: 'priority',
    name: 'Priority',
    responseMinutes: 240,
    resolutionMinutes: 960,
    coverage: 'extended',
    creditBasisPointsPerHour: 100,
  },
  premium: {
    id: 'premium',
    name: 'Premium',
    responseMinutes: 120,
    resolutionMinutes: 480,
    coverage: 'always',
    creditBasisPointsPerHour: 200,
  },
  emergency: {
    id: 'emergency',
    name: 'Emergency response',
    responseMinutes: 60,
    resolutionMinutes: 240,
    coverage: 'always',
    creditBasisPointsPerHour: 250,
  },
};

const COVERAGE_HOURS: Record<Exclude<Coverage, 'always'>, Omit<BusinessCalendar, 'holidays'>> = {
  business: { dayStartMinute: 480, dayEndMinute: 1080, workingDays: [1, 2, 3, 4, 5] },
  extended: { dayStartMinute: 420, dayEndMinute: 1260, workingDays: [1, 2, 3, 4, 5, 6] },
};

export function policyById(id: string): SlaPolicy {
  const policy = SLA_POLICIES[id];
  if (!policy) throw new DomainError('unknown-sla-policy', `Unknown SLA policy ${id}`);
  return policy;
}

/** Emergencies on any contract use the emergency policy when it is stricter. */
export function policyFor(contractPolicyId: string, emergency: boolean): SlaPolicy {
  const policy = policyById(contractPolicyId);
  if (!emergency) return policy;
  const urgent = SLA_POLICIES.emergency;
  return urgent.resolutionMinutes < policy.resolutionMinutes ? urgent : policy;
}

/** The calendar the policy's minutes are counted on; public holidays are excluded except for 24/7 cover. */
export function coverageCalendar(policy: SlaPolicy, holidays: readonly CalendarDate[]): BusinessCalendar {
  if (policy.coverage === 'always') return ALWAYS_OPEN;
  return { ...COVERAGE_HOURS[policy.coverage], holidays };
}
''', imports=imports(ctx, "src/sla/policies.ts", {
        "src/shared/calendar.ts": ["ALWAYS_OPEN", "type BusinessCalendar"],
        "src/shared/errors.ts": ["DomainError"],
        "src/shared/time.ts": ["type CalendarDate"],
        "src/sla/model.ts": ["type Coverage", "type SlaPolicy"],
    }))


def sla_clock(ctx):
    resume = '''export function resumeClock(ctx: AppContext, jobId: string): SlaClock | undefined {
  const clock = clockForJob(ctx, jobId);
  if (!clock || clock.status !== 'paused' || clock.pausedSince === undefined) return clock;
  const now = ctx.clock.now();
  const paused = workingMinutesBetween(calendarOf(ctx, clock), clock.pausedSince, now);
  const updated = slaClocks(ctx).update(clock.id, (current) => {
    const { pausedSince: _released, ...rest } = current;
    return { ...rest, status: 'running', pausedMinutes: current.pausedMinutes + paused };
  });
  ctx.bus.emit({ type: SlaEvents.resumed, at: now, jobId, pausedMinutes: paused });
  return updated;
}''' if not ctx.refactor else '''export function resumeClock(ctx: AppContext, jobId: string): SlaClock | undefined {
  const clock = clockForJob(ctx, jobId);
  if (clock?.status !== 'paused' || clock.pausedSince === undefined) return clock;
  const now = ctx.clock.now();
  const paused = workingMinutesBetween(calendarOf(ctx, clock), clock.pausedSince, now);
  const updated = slaClocks(ctx).update(clock.id, (current) => {
    const { pausedSince: _released, ...rest } = current;
    return { ...rest, status: 'running', pausedMinutes: current.pausedMinutes + paused };
  });
  ctx.bus.emit({ type: SlaEvents.resumed, at: now, jobId, pausedMinutes: paused });
  return updated;
}'''
    return render(ctx, '''
«imports»

export function calendarOf(ctx: AppContext, clock: SlaClock): BusinessCalendar {
  return coverageCalendar(policyById(clock.policyId), ctx.config.holidays);
}

export function startClock(ctx: AppContext, jobId: string, policy: SlaPolicy, at: Timestamp): SlaClock {
  const clock: SlaClock = {
    id: `SLA-${jobId}`,
    jobId,
    policyId: policy.id,
    startedAt: at,
    status: 'running',
    pausedMinutes: 0,
    pauses: 0,
    responseBreached: false,
    warned: false,
  };
  slaClocks(ctx).insert(clock);
  ctx.bus.emit({ type: SlaEvents.started, at, jobId, policyId: policy.id });
  return clock;
}

/** Pausing is idempotent: a paused or finished clock is left unchanged. */
export function pauseClock(ctx: AppContext, jobId: string, cause: string): SlaClock | undefined {
  const clock = clockForJob(ctx, jobId);
  if (!clock || clock.status !== 'running') return clock;
  const now = ctx.clock.now();
  const updated = slaClocks(ctx).update(clock.id, { status: 'paused', pausedSince: now, pauses: clock.pauses + 1 });
  ctx.bus.emit({ type: SlaEvents.paused, at: now, jobId, cause });
  return updated;
}

«resume»

/** Working minutes counted against the policy at `at`, excluding paused time. */
export function elapsedMinutes(ctx: AppContext, clock: SlaClock, at: Timestamp): number {
  const calendar = calendarOf(ctx, clock);
  const end = clock.resolvedAt ?? at;
  const pausedNow = clock.status === 'paused' && clock.pausedSince !== undefined ? workingMinutesBetween(calendar, clock.pausedSince, end) : 0;
  return Math.max(0, workingMinutesBetween(calendar, clock.startedAt, end) - clock.pausedMinutes - pausedNow);
}

export function recordResponse(ctx: AppContext, jobId: string): SlaClock | undefined {
  const clock = clockForJob(ctx, jobId);
  if (!clock || clock.respondedAt !== undefined) return clock;
  const now = ctx.clock.now();
  const policy = policyById(clock.policyId);
  const responseBreached = elapsedMinutes(ctx, clock, now) > policy.responseMinutes;
  return slaClocks(ctx).update(clock.id, { respondedAt: now, responseBreached });
}

/**
 * Stops the clock when work is completed. Resolution beyond the policy target is a breach; the
 * breach is measured in started quarter hours and earns a credit per started hour, capped by
 * `sla.creditCapBasisPoints`.
 */
export function resolveClock(ctx: AppContext, jobId: string): SlaClock | undefined {
  const paused = resumeClock(ctx, jobId);
  if (!paused || (paused.status !== 'running' && paused.status !== 'paused')) return paused;
  const now = ctx.clock.now();
  const policy = policyById(paused.policyId);
  const elapsed = elapsedMinutes(ctx, { ...paused, resolvedAt: now }, now);
  const over = elapsed - policy.resolutionMinutes;
  if (over <= 0) {
    const met = slaClocks(ctx).update(paused.id, { status: 'met', resolvedAt: now, breachMinutes: 0 });
    ctx.bus.emit({ type: SlaEvents.met, at: now, jobId, elapsedMinutes: elapsed });
    return met;
  }
  const breachMinutes = «breach»;
  const creditBasisPoints = slaCreditBasisPoints(policy, breachMinutes, ctx.config.sla.creditCapBasisPoints);
  const breached = slaClocks(ctx).update(paused.id, { status: 'breached', resolvedAt: now, breachMinutes, creditBasisPoints });
  ctx.bus.emit({ type: SlaEvents.breached, at: now, jobId, breachMinutes, creditBasisPoints });
  return breached;
}

export function stopClock(ctx: AppContext, jobId: string): SlaClock | undefined {
  const clock = clockForJob(ctx, jobId);
  if (!clock || clock.status === 'met' || clock.status === 'breached' || clock.status === 'stopped') return clock;
  return slaClocks(ctx).update(clock.id, { status: 'stopped', resolvedAt: ctx.clock.now() });
}

/** Reopened work continues against the original target; earlier pauses still count. */
export function reopenClock(ctx: AppContext, jobId: string): SlaClock | undefined {
  const clock = clockForJob(ctx, jobId);
  if (!clock || (clock.status !== 'met' && clock.status !== 'breached')) return clock;
  return slaClocks(ctx).update(clock.id, (current) => {
    const { resolvedAt: _resolved, breachMinutes: _breach, creditBasisPoints: _credit, ...rest } = current;
    return { ...rest, status: 'running' };
  });
}
''', resume=resume, breach=rounding(ctx, "over", 15, "up"),
                  imports=imports(ctx, "src/sla/clock.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/adapters/repositories/sla-clocks.ts": ["clockForJob", "slaClocks"],
                      "src/shared/calendar.ts": ["workingMinutesBetween", "type BusinessCalendar"],
                      "src/shared/rounding.ts": rounding_names(ctx, "up"),
                      "src/shared/time.ts": ["type Timestamp"],
                      "src/sla/credits.ts": ["slaCreditBasisPoints"],
                      "src/sla/model.ts": ["SlaEvents", "type SlaClock", "type SlaPolicy"],
                      "src/sla/policies.ts": ["coverageCalendar", "policyById"],
                  }))


def sla_credits(ctx):
    return render(ctx, '''
«imports»

/**
 * Credit for a resolution breach: the policy rate for every started hour of breach, in whole
 * percent steps, never more than the cap.
 */
export function slaCreditBasisPoints(policy: SlaPolicy, breachMinutes: number, capBasisPoints: number): number {
  if (breachMinutes <= 0) return 0;
  const hours = Math.ceil(breachMinutes / 60);
  const raw = hours * policy.creditBasisPointsPerHour;
  return Math.min(capBasisPoints, «steps»);
}
''', steps=rounding(ctx, "raw", 100, "down"), imports=imports(ctx, "src/sla/credits.ts", {
        "src/shared/rounding.ts": rounding_names(ctx, "down"),
        "src/sla/model.ts": ["type SlaPolicy"],
    }))


def sla_subscriptions(ctx):
    return render(ctx, '''
«imports»

/** Holds caused by the customer stop the resolution clock; provider-side holds do not. */
export const CUSTOMER_CAUSED_HOLDS: readonly HoldReason[] = ['customer_unavailable', 'access_denied', 'awaiting_approval'];

interface PauseTrigger {
  /** When present, the event's `reason` must be one of these for the clock to pause. */
  reasons?: readonly string[];
}

/** Events that pause the clock of the work order named in their `jobId`. */
const PAUSE_TRIGGERS: Readonly<Record<string, PauseTrigger>> = {
  'job.held': { reasons: CUSTOMER_CAUSED_HOLDS },
  'appointment.no-access': {},
};

function startFor(ctx: AppContext, event: DomainEvent): void {
  const contractId = typeof event.contractId === 'string' ? event.contractId : undefined;
  if (!contractId) return;
  const contract = contracts(ctx).get(contractId);
  if (!contract) return;
  const policy = policyFor(contract.slaPolicyId, event.priority === 'emergency');
  startClock(ctx, String(event.jobId), policy, event.at);
}

export function registerSlaSubscriptions(ctx: AppContext): void {
  ctx.bus.on(«JobEvents».opened, (event) => startFor(ctx, event));
  ctx.bus.on(«JobEvents».started, (event) => {
    recordResponse(ctx, String(event.jobId));
  });
  for (const [type, trigger] of Object.entries(PAUSE_TRIGGERS)) {
    ctx.bus.on(type, (event) => {
      if (trigger.reasons && !trigger.reasons.includes(String(event.reason))) return;
      pauseClock(ctx, String(event.jobId), type);
    });
  }
  ctx.bus.on(«JobEvents».resumed, (event) => {
    resumeClock(ctx, String(event.jobId));
  });
  ctx.bus.on(AppointmentEvents.rebooked, (event) => {
    resumeClock(ctx, String(event.jobId));
  });
  ctx.bus.on(«JobEvents».completed, (event) => {
    resolveClock(ctx, String(event.jobId));
  });
  ctx.bus.on(«JobEvents».cancelled, (event) => {
    stopClock(ctx, String(event.jobId));
  });
  ctx.bus.on(«JobEvents».reopened, (event) => {
    reopenClock(ctx, String(event.jobId));
  });
}
''', imports=imports(ctx, "src/sla/subscriptions.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/contracts.ts": ["contracts"],
        "src/scheduling/model.ts": ["AppointmentEvents"],
        "src/shared/events.ts": ["type DomainEvent"],
        "src/sla/clock.ts": ["pauseClock", "recordResponse", "reopenClock", "resolveClock", "resumeClock",
                             "startClock", "stopClock"],
        "src/sla/policies.ts": ["policyFor"],
    }, jobs=("JobEvents", "type HoldReason")))


def sla_monitor(ctx):
    return render(ctx, '''
«imports»

export interface SlaStatus {
  jobId: string;
  policyId: string;
  status: SlaClock['status'];
  elapsedMinutes: number;
  remainingMinutes: number;
  pausedMinutes: number;
  responseBreached: boolean;
  breachMinutes: number;
  creditBasisPoints: number;
}

export function slaStatus(ctx: AppContext, clock: SlaClock, at: Timestamp): SlaStatus {
  const policy = policyById(clock.policyId);
  const elapsed = elapsedMinutes(ctx, clock, at);
  return {
    jobId: clock.jobId,
    policyId: clock.policyId,
    status: clock.status,
    elapsedMinutes: elapsed,
    remainingMinutes: Math.max(0, policy.resolutionMinutes - elapsed),
    pausedMinutes: clock.pausedMinutes,
    responseBreached: clock.responseBreached,
    breachMinutes: clock.breachMinutes ?? 0,
    creditBasisPoints: clock.creditBasisPoints ?? 0,
  };
}

/**
 * Emits one `sla.warning` per running clock once the configured share of its resolution time
 * has been used.
 */
export function scanForWarnings(ctx: AppContext): string[] {
  const now = ctx.clock.now();
  const warned: string[] = [];
  for (const clock of slaClocks(ctx).find((candidate) => candidate.status === 'running' && !candidate.warned)) {
    const policy = policyById(clock.policyId);
    const used = elapsedMinutes(ctx, clock, now) / policy.resolutionMinutes;
    if (used < ctx.config.sla.warningRatio) continue;
    slaClocks(ctx).update(clock.id, { warned: true });
    ctx.bus.emit({ type: SlaEvents.warning, at: now, jobId: clock.jobId, usedRatio: Math.round(used * 100) / 100 });
    warned.push(clock.jobId);
  }
  return warned;
}

export function complianceRate(ctx: AppContext, from: Timestamp, to: Timestamp): number | undefined {
  const finished = slaClocks(ctx).find(
    (clock) => (clock.status === 'met' || clock.status === 'breached') && clock.resolvedAt !== undefined && clock.resolvedAt >= from && clock.resolvedAt < to,
  );
  if (finished.length === 0) return undefined;
  return finished.filter((clock) => clock.status === 'met').length / finished.length;
}
''', imports=imports(ctx, "src/sla/monitor.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/sla-clocks.ts": ["slaClocks"],
        "src/shared/time.ts": ["type Timestamp"],
        "src/sla/clock.ts": ["elapsedMinutes"],
        "src/sla/model.ts": ["SlaEvents", "type SlaClock"],
        "src/sla/policies.ts": ["policyById"],
    }))


def generate(ctx):
    ctx.add("src/sla/model.ts", sla_model(ctx))
    ctx.add("src/sla/policies.ts", sla_policies(ctx))
    ctx.add("src/sla/clock.ts", sla_clock(ctx))
    ctx.add("src/sla/credits.ts", sla_credits(ctx))
    ctx.add("src/sla/subscriptions.ts", sla_subscriptions(ctx))
    ctx.add("src/sla/monitor.ts", sla_monitor(ctx))
