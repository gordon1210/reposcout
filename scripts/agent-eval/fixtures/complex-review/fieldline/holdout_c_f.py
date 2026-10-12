"""Planned maintenance programmes: recurring service visits per asset, their routes, report, tests and docs."""

VARIANT = "holdout-c-f"

# (asset kind, export name, file stem, work type, summary, extra tag)
KINDS = (
    ("air-handler", "airHandlerVisit", "air-handler", "duct-cleaning", "Planned air handler cleaning", "ventilation"),
    ("backflow-preventer", "backflowPreventerVisit", "backflow-preventer", "backflow-test", "Planned backflow test", "statutory"),
    ("boiler", "boilerVisit", "boiler", "boiler-service", "Planned boiler service", "gas"),
    ("chiller", "chillerVisit", "chiller", "chiller-maintenance", "Planned chiller maintenance", "refrigerant"),
    ("elevator", "elevatorVisit", "elevator", "elevator-inspection", "Planned elevator inspection", "statutory"),
    ("fire-panel", "firePanelVisit", "fire-panel", "fire-panel-inspection", "Planned fire panel inspection", "life-safety"),
    ("generator", "generatorVisit", "generator", "generator-load-test", "Planned generator load test", "electrical"),
    ("heat-pump", "heatPumpVisit", "heat-pump", "heat-pump-service", "Planned heat pump service", "refrigerant"),
    ("solar-inverter", "solarInverterVisit", "solar-inverter", "general", "Planned solar inverter check", "electrical"),
    ("water-heater", "waterHeaterVisit", "water-heater", "general", "Planned water heater check", "hot-water"),
)

MODEL = r'''
import type { CalendarDate } from '../shared/time.ts';

/** Months between two visits of each cadence. */
export const CADENCE_MONTHS = { monthly: 1, quarterly: 3, 'semi-annual': 6, annual: 12 } as const;

export type Cadence = keyof typeof CADENCE_MONTHS;

export const CADENCES = Object.keys(CADENCE_MONTHS) as Cadence[];

export type ProgrammeStatus = 'active' | 'paused' | 'ended';

export interface Programme {
  id: string;
  customerId: string;
  siteId: string;
  name: string;
  cadence: Cadence;
  startsOn: CalendarDate;
  /** Visits due before this date are never generated; it moves to the resume date after a pause. */
  activeFrom: CalendarDate;
  assetIds: string[];
  status: ProgrammeStatus;
  /** Visit keys mapped to the work order that was opened for the visit. */
  generated: Record<string, string>;
  endedOn?: CalendarDate;
}

export interface Visit {
  key: string;
  assetId: string;
  dueOn: CalendarDate;
  workType: string;
  summary: string;
  jobId?: string;
}

export const visitKey = (assetId: string, dueOn: CalendarDate): string => `${assetId}@${dueOn}`;
'''

CADENCE = r'''
import { CADENCE_MONTHS } from './model.ts';
import type { Cadence } from './model.ts';
import type { CalendarDate } from '../shared/time.ts';

const pad = (value: number, width = 2): string => String(value).padStart(width, '0');

export function daysInMonth(year: number, month: number): number {
  return new Date(Date.UTC(year, month, 0)).getUTCDate();
}

/** Moves a date by whole months; a day that does not exist in the target month becomes its last day. */
export function shiftMonths(date: CalendarDate, months: number): CalendarDate {
  const [year, month, day] = date.split('-').map(Number);
  const index = year * 12 + (month - 1) + months;
  const targetYear = Math.floor(index / 12);
  const targetMonth = (index % 12) + 1;
  return `${pad(targetYear, 4)}-${pad(targetMonth)}-${pad(Math.min(day, daysInMonth(targetYear, targetMonth)))}`;
}

/**
 * The due dates of a cadence between `from` and `to`, both included. Every date is counted from
 * `startsOn`, so a clamped month end never moves the dates after it.
 */
export function dueDates(startsOn: CalendarDate, cadence: Cadence, from: CalendarDate, to: CalendarDate): CalendarDate[] {
  const step = CADENCE_MONTHS[cadence];
  const dates: CalendarDate[] = [];
  for (let count = 0; ; count += 1) {
    const due = shiftMonths(startsOn, count * step);
    if (due > to) break;
    if (due >= from) dates.push(due);
  }
  return dates;
}
'''

KIND_TYPES = r'''
import type { AssetKind } from '../../assets/model.ts';

/** How the planned visit of one asset kind is opened as a work order. */
export interface VisitKind {
  assetKind: AssetKind;
  workType: string;
  summary: string;
  tags: readonly string[];
}
'''

KIND_FILE = r'''
import type { VisitKind } from './types.ts';

export const §NAME§: VisitKind = {
  assetKind: '§ASSET§',
  workType: '§WORK§',
  summary: '§SUMMARY§',
  tags: ['planned', '§TAG§'],
};
'''

KIND_INDEX = r'''
import type { AssetKind } from '../../assets/model.ts';
§IMPORTS§
import type { VisitKind } from './types.ts';

const KINDS: readonly VisitKind[] = [
§LIST§
];

const BY_ASSET_KIND = new Map<AssetKind, VisitKind>(KINDS.map((kind) => [kind.assetKind, kind]));

export function visitKindFor(kind: AssetKind): VisitKind {
  const found = BY_ASSET_KIND.get(kind);
  if (found === undefined) throw new Error(`No planned visit is defined for ${kind} assets`);
  return found;
}

export function visitKinds(): readonly VisitKind[] {
  return KINDS;
}
'''

VALIDATION = r'''
import { array, date, object, oneOf, string } from '../shared/validate.ts';
import type { Parsed } from '../shared/validate.ts';
import { CADENCES } from './model.ts';

export const programmeInput = object({
  customerId: string({ min: 1 }),
  siteId: string({ min: 1 }),
  name: string({ min: 3, max: 80 }),
  cadence: oneOf(CADENCES),
  startsOn: date(),
  assetIds: array(string({ min: 1 }), { min: 1, max: 200 }),
});

export const assetReference = object({ assetId: string({ min: 1 }) });

export type ProgrammeInput = Parsed<typeof programmeInput>;
'''

REPOSITORY = r'''
import type { AppContext } from '../../context.ts';
import type { Programme } from '../../programmes/model.ts';
import type { Collection } from '../store/memory.ts';

export function programmes(ctx: AppContext): Collection<Programme> {
  return ctx.store.collection<Programme>('programmes');
}

export function programmesForCustomer(ctx: AppContext, customerId: string): Programme[] {
  return programmes(ctx)
    .find((programme) => programme.customerId === customerId)
    .sort((left, right) => left.id.localeCompare(right.id));
}

export function activeProgrammes(ctx: AppContext): Programme[] {
  return programmes(ctx)
    .find((programme) => programme.status === 'active')
    .sort((left, right) => left.id.localeCompare(right.id));
}

/** The programme that still plans visits for the asset, if any; ended programmes do not count. */
export function programmeCovering(ctx: AppContext, assetId: string): Programme | undefined {
  return programmes(ctx).first((programme) => programme.status !== 'ended' && programme.assetIds.includes(assetId));
}
'''

SERVICE = r'''
import { assets } from '../adapters/repositories/assets.ts';
import { customers } from '../adapters/repositories/customers.ts';
import { programmeCovering, programmes } from '../adapters/repositories/programmes.ts';
import { requireSiteOf } from '../adapters/repositories/sites.ts';
import { recordAudit } from '../adapters/store/audit.ts';
import type { AppContext } from '../context.ts';
import { ConflictError, DomainError } from '../shared/errors.ts';
import { dateOf } from '../shared/time.ts';
import { parseInput } from '../shared/validate.ts';
import type { Programme } from './model.ts';
import { programmeInput } from './validation.ts';

const today = (ctx: AppContext) => dateOf(ctx.clock.now());

function requireOpen(programme: Programme): void {
  if (programme.status === 'ended') {
    throw new ConflictError('programme-ended', `Programme ${programme.id} has ended`);
  }
}

function checkAsset(ctx: AppContext, siteId: string, assetId: string, ownId?: string): void {
  const asset = assets(ctx).require(assetId, 'asset');
  if (asset.siteId !== siteId) throw new DomainError('asset-not-at-site', `Asset ${asset.id} is not at site ${siteId}`);
  if (asset.retiredOn !== undefined) throw new DomainError('asset-retired', `Asset ${asset.id} has been retired`);
  const other = programmeCovering(ctx, asset.id);
  if (other !== undefined && other.id !== ownId) {
    throw new ConflictError('asset-in-programme', `Asset ${asset.id} already belongs to programme ${other.id}`, {
      programmeId: other.id,
    });
  }
}

function requireTooMany(ctx: AppContext, count: number): void {
  const limit = ctx.config.programmes.maxAssetsPerProgramme;
  if (count > limit) throw new DomainError('too-many-assets', `A programme covers at most ${limit} assets`);
}

export function createProgramme(ctx: AppContext, actorId: string, input: unknown): Programme {
  const data = parseInput(programmeInput, input);
  const customer = customers(ctx).require(data.customerId, 'customer');
  const site = requireSiteOf(ctx, customer.id, data.siteId);
  const assetIds = [...new Set(data.assetIds)].sort();
  requireTooMany(ctx, assetIds.length);
  for (const assetId of assetIds) checkAsset(ctx, site.id, assetId);
  const programme: Programme = {
    id: ctx.ids.next('PRG'),
    customerId: customer.id,
    siteId: site.id,
    name: data.name.trim(),
    cadence: data.cadence,
    startsOn: data.startsOn,
    activeFrom: today(ctx),
    assetIds,
    status: 'active',
    generated: {},
  };
  programmes(ctx).insert(programme);
  recordAudit(ctx, actorId, 'programme.created', programme.id, {
    customerId: programme.customerId,
    siteId: programme.siteId,
    cadence: programme.cadence,
    assets: assetIds.length,
  });
  return programme;
}

export function addProgrammeAsset(ctx: AppContext, actorId: string, programmeId: string, assetId: string): Programme {
  const programme = programmes(ctx).require(programmeId, 'programme');
  requireOpen(programme);
  if (programme.assetIds.includes(assetId)) return programme;
  requireTooMany(ctx, programme.assetIds.length + 1);
  checkAsset(ctx, programme.siteId, assetId, programme.id);
  const updated = programmes(ctx).update(programme.id, { assetIds: [...programme.assetIds, assetId].sort() });
  recordAudit(ctx, actorId, 'programme.asset-added', programme.id, { assetId });
  return updated;
}

export function removeProgrammeAsset(ctx: AppContext, actorId: string, programmeId: string, assetId: string): Programme {
  const programme = programmes(ctx).require(programmeId, 'programme');
  requireOpen(programme);
  if (!programme.assetIds.includes(assetId)) {
    throw new DomainError('asset-not-in-programme', `Asset ${assetId} is not part of programme ${programme.id}`, 404);
  }
  if (programme.assetIds.length === 1) {
    throw new ConflictError('programme-needs-assets', `Programme ${programme.id} must keep at least one asset`);
  }
  const updated = programmes(ctx).update(programme.id, {
    assetIds: programme.assetIds.filter((candidate) => candidate !== assetId),
  });
  recordAudit(ctx, actorId, 'programme.asset-removed', programme.id, { assetId });
  return updated;
}

export function pauseProgramme(ctx: AppContext, actorId: string, programmeId: string): Programme {
  const programme = programmes(ctx).require(programmeId, 'programme');
  if (programme.status !== 'active') {
    throw new ConflictError('invalid-programme-state', `Programme ${programme.id} is ${programme.status}, not active`);
  }
  const updated = programmes(ctx).update(programme.id, { status: 'paused' });
  recordAudit(ctx, actorId, 'programme.paused', programme.id);
  return updated;
}

/** A resumed programme plans from today on; visits that fell due while it was paused are not generated. */
export function resumeProgramme(ctx: AppContext, actorId: string, programmeId: string): Programme {
  const programme = programmes(ctx).require(programmeId, 'programme');
  if (programme.status !== 'paused') {
    throw new ConflictError('invalid-programme-state', `Programme ${programme.id} is ${programme.status}, not paused`);
  }
  const updated = programmes(ctx).update(programme.id, { status: 'active', activeFrom: today(ctx) });
  recordAudit(ctx, actorId, 'programme.resumed', programme.id);
  return updated;
}

export function endProgramme(ctx: AppContext, actorId: string, programmeId: string): Programme {
  const programme = programmes(ctx).require(programmeId, 'programme');
  requireOpen(programme);
  const updated = programmes(ctx).update(programme.id, { status: 'ended', endedOn: today(ctx) });
  recordAudit(ctx, actorId, 'programme.ended', programme.id);
  return updated;
}
'''

VISITS = r'''
import { assets } from '../adapters/repositories/assets.ts';
import type { AppContext } from '../context.ts';
import type { CalendarDate } from '../shared/time.ts';
import { dueDates } from './cadence.ts';
import { visitKindFor } from './kinds/index.ts';
import { visitKey } from './model.ts';
import type { Programme, Visit } from './model.ts';

export interface PlannedVisit extends Visit {
  tags: readonly string[];
}

/**
 * The visits of a programme that fall due between `from` and `to`, ordered by due date and asset.
 * Retired assets have no visits; visits that already have a work order carry its id.
 */
export function plannedVisits(ctx: AppContext, programme: Programme, from: CalendarDate, to: CalendarDate): PlannedVisit[] {
  const visits: PlannedVisit[] = [];
  for (const dueOn of dueDates(programme.startsOn, programme.cadence, from, to)) {
    for (const assetId of programme.assetIds) {
      const asset = assets(ctx).get(assetId);
      if (asset === undefined || asset.retiredOn !== undefined) continue;
      const kind = visitKindFor(asset.kind);
      const key = visitKey(assetId, dueOn);
      visits.push({
        key,
        assetId,
        dueOn,
        workType: kind.workType,
        summary: `${kind.summary} due ${dueOn}`,
        tags: kind.tags,
        jobId: programme.generated[key],
      });
    }
  }
  return visits;
}
'''

GENERATE = r'''
import { programmes } from '../adapters/repositories/programmes.ts';
import { recordAudit } from '../adapters/store/audit.ts';
import type { AppContext } from '../context.ts';
import { openJob } from '../jobs/jobs.ts';
import { ConflictError } from '../shared/errors.ts';
import { addDays, dateOf } from '../shared/time.ts';
import type { CalendarDate } from '../shared/time.ts';
import { plannedVisits } from './visits.ts';

export interface GeneratedVisit {
  key: string;
  assetId: string;
  dueOn: CalendarDate;
  workType: string;
  jobId: string;
}

export interface Generation {
  programmeId: string;
  on: CalendarDate;
  opened: GeneratedVisit[];
}

/**
 * Opens a work order for every visit that is due by `on` (plus the configured lead days), has been
 * due since the programme became active and has no work order yet. Each opened visit is recorded
 * before the next one starts, so a failed run resumes where it stopped.
 */
export function generateVisits(
  ctx: AppContext,
  actorId: string,
  programmeId: string,
  on: CalendarDate = dateOf(ctx.clock.now()),
): Generation {
  const programme = programmes(ctx).require(programmeId, 'programme');
  if (programme.status !== 'active') {
    throw new ConflictError('programme-not-active', `Programme ${programme.id} is ${programme.status}`);
  }
  const until = addDays(on, ctx.config.programmes.leadDays);
  const pending = plannedVisits(ctx, programme, programme.activeFrom, until).filter((visit) => visit.jobId === undefined);
  const opened: GeneratedVisit[] = [];
  for (const visit of pending) {
    const job = openJob(ctx, actorId, {
      customerId: programme.customerId,
      siteId: programme.siteId,
      assetId: visit.assetId,
      workType: visit.workType,
      summary: visit.summary,
      description: `${programme.name}: planned visit due ${visit.dueOn}`,
      source: 'programme',
      tags: [...visit.tags],
    });
    programmes(ctx).update(programme.id, (current) => ({
      ...current,
      generated: { ...current.generated, [visit.key]: job.id },
    }));
    opened.push({ key: visit.key, assetId: visit.assetId, dueOn: visit.dueOn, workType: visit.workType, jobId: job.id });
  }
  if (opened.length > 0) recordAudit(ctx, actorId, 'programme.generated', programme.id, { on, opened: opened.length });
  return { programmeId: programme.id, on, opened };
}
'''

COVERAGE = r'''
import { activeProgrammes } from '../adapters/repositories/programmes.ts';
import { assetsDueForService } from '../assets/service.ts';
import type { Asset } from '../assets/model.ts';
import type { AppContext } from '../context.ts';
import type { CalendarDate } from '../shared/time.ts';

/** Assets that are due for service within `withinDays` of `on` and belong to no active programme. */
export function unprogrammedDueAssets(ctx: AppContext, on: CalendarDate, withinDays: number): Asset[] {
  const covered = new Set(activeProgrammes(ctx).flatMap((programme) => programme.assetIds));
  return assetsDueForService(ctx, on, withinDays).filter((asset) => !covered.has(asset.id));
}
'''

VIEW = r'''
import type { Programme } from './model.ts';

/** A programme as the API shows it: the visit ledger is reduced to the number of opened visits. */
export function programmeView(programme: Programme) {
  const { generated, ...rest } = programme;
  return { ...rest, openedVisits: Object.keys(generated).length };
}
'''

SCHEDULE_REPORT = r'''
import { activeProgrammes } from '../adapters/repositories/programmes.ts';
import type { AppContext } from '../context.ts';
import { plannedVisits } from '../programmes/visits.ts';
import type { CalendarDate } from '../shared/time.ts';

export interface ScheduleRow {
  programmeId: string;
  programme: string;
  customerId: string;
  siteId: string;
  assetId: string;
  dueOn: CalendarDate;
  workType: string;
  opened: boolean;
  jobId?: string;
}

/** Planned visits of the active programmes that fall due in the period, soonest first. */
export function programmeSchedule(ctx: AppContext, from: CalendarDate, to: CalendarDate): ScheduleRow[] {
  const rows: ScheduleRow[] = [];
  for (const programme of activeProgrammes(ctx)) {
    for (const visit of plannedVisits(ctx, programme, from, to)) {
      rows.push({
        programmeId: programme.id,
        programme: programme.name,
        customerId: programme.customerId,
        siteId: programme.siteId,
        assetId: visit.assetId,
        dueOn: visit.dueOn,
        workType: visit.workType,
        opened: visit.jobId !== undefined,
        jobId: visit.jobId,
      });
    }
  }
  return rows.sort(
    (left, right) =>
      left.dueOn.localeCompare(right.dueOn) ||
      left.programmeId.localeCompare(right.programmeId) ||
      left.assetId.localeCompare(right.assetId),
  );
}
'''

ROUTES = r'''
import { programmes } from '../../adapters/repositories/programmes.ts';
import { authorize } from '../../auth/permissions.ts';
import { generateVisits } from '../../programmes/generate.ts';
import { unprogrammedDueAssets } from '../../programmes/coverage.ts';
import {
  addProgrammeAsset,
  createProgramme,
  endProgramme,
  pauseProgramme,
  removeProgrammeAsset,
  resumeProgramme,
} from '../../programmes/service.ts';
import { assetReference } from '../../programmes/validation.ts';
import { programmeView } from '../../programmes/view.ts';
import { plannedVisits } from '../../programmes/visits.ts';
import { DomainError } from '../../shared/errors.ts';
import { addDays, dateOf } from '../../shared/time.ts';
import { parseInput } from '../../shared/validate.ts';
import { created, ok, optionalQuery, param, queryDate, queryInteger } from '../http.ts';
import type { Router } from '../router.ts';

export function registerProgrammeRoutes(router: Router): void {
  router.post('/programmes', (ctx, request) => {
    authorize(request.actor, 'programmes:write');
    return created(programmeView(createProgramme(ctx, request.actor.id, request.body)));
  });

  router.get('/programmes', (ctx, request) => {
    authorize(request.actor, 'jobs:schedule');
    const customerId = optionalQuery(request, 'customerId');
    const status = optionalQuery(request, 'status');
    const items = programmes(ctx)
      .find((programme) => (customerId === undefined || programme.customerId === customerId) && (status === undefined || programme.status === status))
      .sort((left, right) => left.id.localeCompare(right.id));
    return ok({ items: items.map(programmeView) });
  });

  router.get('/programmes/gaps', (ctx, request) => {
    authorize(request.actor, 'jobs:schedule');
    const on = queryDate(request, 'on', dateOf(ctx.clock.now()));
    const within = queryInteger(request, 'within', 30, { min: 0, max: 365 });
    return ok({ items: unprogrammedDueAssets(ctx, on, within) });
  });

  router.get('/programmes/:id', (ctx, request) => {
    authorize(request.actor, 'jobs:schedule');
    return ok(programmeView(programmes(ctx).require(param(request, 'id'), 'programme')));
  });

  router.post('/programmes/:id/pause', (ctx, request) => {
    authorize(request.actor, 'programmes:write');
    return ok(programmeView(pauseProgramme(ctx, request.actor.id, param(request, 'id'))));
  });

  router.post('/programmes/:id/resume', (ctx, request) => {
    authorize(request.actor, 'programmes:write');
    return ok(programmeView(resumeProgramme(ctx, request.actor.id, param(request, 'id'))));
  });

  router.post('/programmes/:id/end', (ctx, request) => {
    authorize(request.actor, 'programmes:write');
    return ok(programmeView(endProgramme(ctx, request.actor.id, param(request, 'id'))));
  });

  router.post('/programmes/:id/assets', (ctx, request) => {
    authorize(request.actor, 'programmes:write');
    const { assetId } = parseInput(assetReference, request.body);
    return ok(programmeView(addProgrammeAsset(ctx, request.actor.id, param(request, 'id'), assetId)));
  });

  router.delete('/programmes/:id/assets/:assetId', (ctx, request) => {
    authorize(request.actor, 'programmes:write');
    return ok(programmeView(removeProgrammeAsset(ctx, request.actor.id, param(request, 'id'), param(request, 'assetId'))));
  });

  router.get('/programmes/:id/visits', (ctx, request) => {
    authorize(request.actor, 'jobs:schedule');
    const programme = programmes(ctx).require(param(request, 'id'), 'programme');
    const today = dateOf(ctx.clock.now());
    const from = queryDate(request, 'from', today);
    const to = queryDate(request, 'to', addDays(from, 365));
    if (to < from) throw new DomainError('invalid-range', 'The end of the period is before its start');
    return ok({ items: plannedVisits(ctx, programme, from, to) });
  });

  router.post('/programmes/:id/generate', (ctx, request) => {
    authorize(request.actor, 'programmes:write');
    const on = queryDate(request, 'on', dateOf(ctx.clock.now()));
    return ok(generateVisits(ctx, request.actor.id, param(request, 'id'), on));
  });
}
'''

DOC_PROGRAMMES = r'''
# Planned maintenance programmes

A programme repeats the service visits of a set of assets at one site on a fixed cadence:
`monthly`, `quarterly`, `semi-annual` or `annual`. Programmes are staff-only and are created
with `POST /programmes` (`programmes:write`: admin and dispatcher).

Every due date is counted from the start date, so a start on the 31st gives visits on the
last day of shorter months without moving the dates after them. The visit of an asset is a work
order of the work type that suits its kind (a boiler service for boilers, a load test for
generators); kinds without a dedicated work type use general maintenance.

## Generating visits

`POST /programmes/:id/generate` opens a work order for every visit that is due by the given day
(`on`, default today) and has no work order yet. Visits are generated through the ordinary
work-order path, so coverage, included visits, SLA clocks, notifications and webhooks behave
exactly as for a work order opened by dispatch, with one exception: planned visits run no SLA
clock. They are scheduled against their due date, so response and resolution targets would only
measure the gap between opening and the visit. The work orders have the source `programme` and
the tag `planned`.

- Visits already generated are never opened again; calling the operation twice is harmless.
- A programme plans from the day it was created or last resumed. Visits that fell due before that
  day, for example while the programme was paused, are not generated afterwards.
- `programmes.leadDays` opens work orders ahead of their due date. The default is 0: a visit is
  opened on its due date.
- Retired assets have no visits. A suspended or closed customer cannot receive generated work.

## Lifecycle

Only active programmes generate visits. A programme can be paused and resumed, and ended for
good. An asset can belong to one programme that has not ended; a programme keeps at least one
asset and covers at most `programmes.maxAssetsPerProgramme` assets.

`GET /programmes/gaps` lists assets that are due for service and belong to no active programme.
`GET /reports/programme-schedule` lists the planned visits of active programmes for a period.
'''

PERMISSION_PATCHES = (
    ("  | 'quotes:write'\n", "  | 'quotes:write'\n  | 'programmes:write'\n"),
)

TEST_SUPPORT = r'''
import { call } from './world.ts';
import type { World } from './world.ts';

export const START = '2026-03-09';

/** Creates an active programme for the world's site; the first visit falls due on `START`. */
export function startProgramme(world: World, assetIds: string[], extra: Record<string, unknown> = {}) {
  return call(world.app, 'POST', '/programmes', 'dispatch', {
    customerId: world.customer.id,
    siteId: world.site.id,
    name: 'Plant room upkeep',
    cadence: 'quarterly',
    startsOn: START,
    assetIds,
    ...extra,
  });
}

export const generate = (world: World, programmeId: string, on?: string) =>
  call(world.app, 'POST', `/programmes/${programmeId}/generate${on === undefined ? '' : `?on=${on}`}`, 'dispatch');
'''

KIND_TEST = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { at, call, createWorld, seedAsset } from '../../support/world.ts';
import { generate, START, startProgramme } from '../../support/programmes.ts';
import { §NAME§ } from '../../../src/programmes/kinds/§STEM§.ts';
import { visitKindFor } from '../../../src/programmes/kinds/index.ts';

describe('§ASSET§ visit kind', () => {
  it('is the kind used for §ASSET§ assets', () => {
    assert.equal(visitKindFor('§ASSET§'), §NAME§);
    assert.equal(§NAME§.assetKind, '§ASSET§');
  });

  it('names a registered work type', () => {
    const { app } = createWorld();
    assert.equal(app.ctx.workTypes.lookup(§NAME§.workType)?.key, '§WORK§');
  });

  it('marks its work orders as planned', () => {
    assert.deepEqual(§NAME§.tags, ['planned', '§TAG§']);
    assert.match(§NAME§.summary, /^Planned /);
  });

  it('opens a §WORK§ work order for the asset on the due date', () => {
    const world = createWorld();
    const asset = seedAsset(world.app, world.site.id, '§ASSET§');
    const programme = startProgramme(world, [asset.id], { cadence: 'annual' });
    world.app.clock.set(at(START, '08:00'));
    const { opened } = generate(world, programme.id);
    assert.equal(opened.length, 1);
    const job = call(world.app, 'GET', `/jobs/${opened[0].jobId}`, 'dispatch');
    assert.equal(job.workType, '§WORK§');
    assert.equal(job.assetId, asset.id);
    assert.equal(job.summary, `§SUMMARY§ due ${START}`);
    assert.deepEqual([...job.tags].sort(), [...§NAME§.tags].sort());
  });

  it('plans the next visit one cadence later', () => {
    const world = createWorld();
    const asset = seedAsset(world.app, world.site.id, '§ASSET§');
    const programme = startProgramme(world, [asset.id], { cadence: 'semi-annual' });
    const visits = call(world.app, 'GET', `/programmes/${programme.id}/visits?from=${START}&to=2027-03-09`, 'dispatch').items;
    assert.deepEqual(visits.map((visit: any) => visit.dueOn), [START, '2026-09-09', '2027-03-09']);
    assert.ok(visits.every((visit: any) => visit.workType === '§WORK§'));
  });

  it('shows up in the schedule report with the work type of the kind', () => {
    const world = createWorld();
    const asset = seedAsset(world.app, world.site.id, '§ASSET§');
    const programme = startProgramme(world, [asset.id], { cadence: 'annual' });
    const rows = call(world.app, 'GET', `/reports/programme-schedule?from=${START}&to=${START}`, 'admin').items;
    assert.deepEqual(rows.map((row: any) => [row.programmeId, row.assetId, row.workType, row.opened]), [
      [programme.id, asset.id, '§WORK§', false],
    ]);
  });

  it('is not listed as a gap while its programme is active', () => {
    const world = createWorld();
    const asset = seedAsset(world.app, world.site.id, '§ASSET§');
    const before = call(world.app, 'GET', '/programmes/gaps?on=2030-01-01&within=30', 'dispatch').items.map((item: any) => item.id);
    const programme = startProgramme(world, [asset.id], { cadence: 'annual' });
    const during = call(world.app, 'GET', '/programmes/gaps?on=2030-01-01&within=30', 'dispatch').items.map((item: any) => item.id);
    assert.deepEqual(before.filter((id: string) => id !== asset.id), during);
    call(world.app, 'POST', `/programmes/${programme.id}/end`, 'dispatch');
    assert.deepEqual(call(world.app, 'GET', '/programmes/gaps?on=2030-01-01&within=30', 'dispatch').items.map((item: any) => item.id), before);
  });

  it('stops producing visits once the asset is retired', () => {
    const world = createWorld();
    const asset = seedAsset(world.app, world.site.id, '§ASSET§');
    const other = seedAsset(world.app, world.site.id, 'boiler');
    const programme = startProgramme(world, [asset.id, other.id], { cadence: 'annual' });
    call(world.app, 'POST', `/assets/${asset.id}/retire`, 'admin');
    world.app.clock.set(at(START, '08:00'));
    assert.deepEqual(generate(world, programme.id).opened.map((visit: any) => visit.assetId), [other.id]);
  });
});
'''

CADENCE_TEST = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { daysInMonth, dueDates, shiftMonths } from '../../src/programmes/cadence.ts';

describe('daysInMonth', () => {
  it('knows the length of every month, leap years included', () => {
    assert.deepEqual([1, 2, 3, 4, 6, 12].map((month) => daysInMonth(2026, month)), [31, 28, 31, 30, 30, 31]);
    assert.equal(daysInMonth(2028, 2), 29);
    assert.equal(daysInMonth(2100, 2), 28);
    assert.equal(daysInMonth(2000, 2), 29);
  });
});

describe('shiftMonths', () => {
  it('keeps the day of the month when it exists', () => {
    assert.equal(shiftMonths('2026-03-09', 1), '2026-04-09');
    assert.equal(shiftMonths('2026-03-09', 12), '2027-03-09');
  });

  it('clamps to the last day of shorter months', () => {
    assert.equal(shiftMonths('2026-01-31', 1), '2026-02-28');
    assert.equal(shiftMonths('2028-01-31', 1), '2028-02-29');
    assert.equal(shiftMonths('2026-03-31', 1), '2026-04-30');
    assert.equal(shiftMonths('2026-08-30', 6), '2027-02-28');
  });

  it('crosses year boundaries in both directions', () => {
    assert.equal(shiftMonths('2026-11-15', 3), '2027-02-15');
    assert.equal(shiftMonths('2026-02-15', -3), '2025-11-15');
    assert.equal(shiftMonths('2026-12-31', 2), '2027-02-28');
  });

  it('leaves a date alone for zero months', () => {
    assert.equal(shiftMonths('2026-05-31', 0), '2026-05-31');
  });
});

describe('dueDates', () => {
  it('lists the dates inside the window, both ends included', () => {
    assert.deepEqual(dueDates('2026-03-09', 'quarterly', '2026-03-09', '2026-12-09'), ['2026-03-09', '2026-06-09', '2026-09-09', '2026-12-09']);
    assert.deepEqual(dueDates('2026-03-09', 'quarterly', '2026-03-10', '2026-12-08'), ['2026-06-09', '2026-09-09']);
  });

  it('starts at the start date even when the window opens earlier', () => {
    assert.deepEqual(dueDates('2026-03-09', 'annual', '2020-01-01', '2028-12-31'), ['2026-03-09', '2027-03-09', '2028-03-09']);
  });

  it('is empty before the start and for windows without a due date', () => {
    assert.deepEqual(dueDates('2026-03-09', 'monthly', '2026-01-01', '2026-03-08'), []);
    assert.deepEqual(dueDates('2026-03-09', 'annual', '2026-03-10', '2027-03-08'), []);
  });

  it('counts every date from the start so clamped dates do not drift', () => {
    assert.deepEqual(dueDates('2026-01-31', 'monthly', '2026-01-01', '2026-05-31'), [
      '2026-01-31', '2026-02-28', '2026-03-31', '2026-04-30', '2026-05-31',
    ]);
  });

  it('spaces the cadences by 1, 3, 6 and 12 months', () => {
    const window = ['2026-01-15', '2027-01-15'] as const;
    assert.equal(dueDates('2026-01-15', 'monthly', ...window).length, 13);
    assert.equal(dueDates('2026-01-15', 'quarterly', ...window).length, 5);
    assert.equal(dueDates('2026-01-15', 'semi-annual', ...window).length, 3);
    assert.equal(dueDates('2026-01-15', 'annual', ...window).length, 2);
  });
});
'''

MODEL_TEST = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { CADENCE_MONTHS, CADENCES, visitKey } from '../../src/programmes/model.ts';

describe('cadences', () => {
  it('are named for the months between visits', () => {
    assert.deepEqual(CADENCE_MONTHS, { monthly: 1, quarterly: 3, 'semi-annual': 6, annual: 12 });
    assert.deepEqual(CADENCES, ['monthly', 'quarterly', 'semi-annual', 'annual']);
  });

  it('divide the year evenly', () => {
    for (const months of Object.values(CADENCE_MONTHS)) assert.equal(12 % months, 0);
  });
});

describe('visitKey', () => {
  it('joins the asset and the due date', () => {
    assert.equal(visitKey('AST-000004', '2026-03-09'), 'AST-000004@2026-03-09');
  });

  it('differs per asset and per date', () => {
    const keys = new Set([visitKey('AST-000001', '2026-03-09'), visitKey('AST-000002', '2026-03-09'), visitKey('AST-000001', '2026-06-09')]);
    assert.equal(keys.size, 3);
  });
});
'''

KINDS_INDEX_TEST = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createApp } from '../../../src/app.ts';
import { ASSET_KINDS } from '../../../src/assets/model.ts';
import { visitKindFor, visitKinds } from '../../../src/programmes/kinds/index.ts';

describe('visit kinds', () => {
  it('define exactly one visit for every asset kind', () => {
    assert.deepEqual(visitKinds().map((kind) => kind.assetKind).sort(), [...ASSET_KINDS].sort());
    for (const kind of ASSET_KINDS) assert.equal(visitKindFor(kind).assetKind, kind);
  });

  it('name registered work types', () => {
    const { ctx } = createApp();
    for (const kind of visitKinds()) assert.ok(ctx.workTypes.lookup(kind.workType), `${kind.assetKind} -> ${kind.workType}`);
  });

  it('use dedicated work types for the statutory kinds', () => {
    assert.equal(visitKindFor('elevator').workType, 'elevator-inspection');
    assert.equal(visitKindFor('fire-panel').workType, 'fire-panel-inspection');
    assert.equal(visitKindFor('backflow-preventer').workType, 'backflow-test');
  });

  it('fall back to general maintenance where no dedicated work type exists', () => {
    assert.deepEqual(
      visitKinds().filter((kind) => kind.workType === 'general').map((kind) => kind.assetKind).sort(),
      ['solar-inverter', 'water-heater'],
    );
  });

  it('tag every planned visit as planned', () => {
    for (const kind of visitKinds()) {
      assert.equal(kind.tags[0], 'planned', kind.assetKind);
      assert.match(kind.summary, /^Planned /, kind.assetKind);
    }
  });

  it('are known by their asset kind only', () => {
    assert.throws(() => visitKindFor('tumble-dryer' as any), /No planned visit is defined/);
  });
});
'''

SERVICE_TEST = r'''
import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';
import { call, createWorld, expectStatus, seedAsset, seedCustomer } from '../support/world.ts';
import type { World } from '../support/world.ts';
import { startProgramme, START } from '../support/programmes.ts';

let world: World;

beforeEach(() => {
  world = createWorld();
});

const body = (assetIds: string[], extra: Record<string, unknown> = {}) => ({
  customerId: world.customer.id,
  siteId: world.site.id,
  name: 'Plant room upkeep',
  cadence: 'quarterly',
  startsOn: START,
  assetIds,
  ...extra,
});

describe('createProgramme', () => {
  it('starts an active programme for the assets of a site', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const chiller = seedAsset(world.app, world.site.id, 'chiller');
    const programme = startProgramme(world, [chiller.id, boiler.id]);
    assert.match(programme.id, /^PRG-\d{6}$/);
    assert.equal(programme.status, 'active');
    assert.deepEqual(programme.assetIds, [boiler.id, chiller.id]);
    assert.equal(programme.activeFrom, '2026-03-02');
    assert.equal(programme.openedVisits, 0);
    assert.equal(programme.generated, undefined);
  });

  it('numbers programmes independently of the other records', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const other = seedAsset(world.app, world.site.id, 'elevator');
    assert.equal(startProgramme(world, [boiler.id]).id, 'PRG-000001');
    assert.equal(startProgramme(world, [other.id]).id, 'PRG-000002');
    assert.equal(call(world.app, 'POST', '/customers', 'admin', { name: 'Fenwick Pharmacy', segment: 'commercial', billingRegion: 'metro-east' }).id, 'CUS-000002');
  });

  it('keeps duplicate asset ids once', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    assert.deepEqual(startProgramme(world, [boiler.id, boiler.id]).assetIds, [boiler.id]);
  });

  it('trims the name', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    assert.equal(startProgramme(world, [boiler.id], { name: '  Plant room upkeep  ' }).name, 'Plant room upkeep');
  });

  it('rejects invalid input with the offending fields', () => {
    const response = expectStatus(world.app, 422, 'POST', '/programmes', 'dispatch', { customerId: world.customer.id, cadence: 'weekly', assetIds: [] });
    const paths = response.error.details.issues.map((issue: any) => issue.path).sort();
    assert.deepEqual(paths, ['assetIds', 'cadence', 'name', 'siteId', 'startsOn']);
  });

  it('rejects unknown customers, foreign sites and unknown assets', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    expectStatus(world.app, 404, 'POST', '/programmes', 'dispatch', body([boiler.id], { customerId: 'CUS-999999' }));
    const { site: foreign } = seedCustomer(world.app, { name: 'Fenwick Pharmacy' });
    expectStatus(world.app, 404, 'POST', '/programmes', 'dispatch', body([boiler.id], { siteId: foreign.id }));
    expectStatus(world.app, 404, 'POST', '/programmes', 'dispatch', body(['AST-999999']));
  });

  it('rejects assets of another site and retired assets', () => {
    const other = call(world.app, 'POST', `/customers/${world.customer.id}/sites`, 'admin', {
      label: 'Annex',
      address: { line1: '14 Quay Street', city: 'Eastport', postcode: 'EP1 4QS', region: 'metro-east', location: { lat: 52.52, lon: 13.4 } },
    });
    const away = seedAsset(world.app, other.id, 'boiler');
    assert.equal(expectStatus(world.app, 422, 'POST', '/programmes', 'dispatch', body([away.id])).error.code, 'asset-not-at-site');
    const old = seedAsset(world.app, world.site.id, 'boiler');
    call(world.app, 'POST', `/assets/${old.id}/retire`, 'admin');
    assert.equal(expectStatus(world.app, 422, 'POST', '/programmes', 'dispatch', body([old.id])).error.code, 'asset-retired');
  });

  it('keeps an asset in one programme at a time', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const first = startProgramme(world, [boiler.id]);
    const clash = expectStatus(world.app, 409, 'POST', '/programmes', 'dispatch', body([boiler.id]));
    assert.equal(clash.error.code, 'asset-in-programme');
    assert.equal(clash.error.details.programmeId, first.id);
    call(world.app, 'POST', `/programmes/${first.id}/end`, 'dispatch');
    assert.equal(startProgramme(world, [boiler.id]).status, 'active');
  });

  it('limits the assets of one programme', () => {
    const world = createWorld({ config: { programmes: { maxAssetsPerProgramme: 2 } } });
    const ids = ['boiler', 'chiller', 'elevator'].map((kind) => seedAsset(world.app, world.site.id, kind).id);
    assert.equal(
      expectStatus(world.app, 422, 'POST', '/programmes', 'dispatch', {
        customerId: world.customer.id, siteId: world.site.id, name: 'Plant room upkeep', cadence: 'annual', startsOn: START, assetIds: ids,
      }).error.code,
      'too-many-assets',
    );
  });

  it('records the creation in the audit trail', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const programme = startProgramme(world, [boiler.id]);
    const trail = call(world.app, 'GET', `/audit/${programme.id}`, 'admin').items;
    assert.deepEqual(trail.map((entry: any) => entry.action), ['programme.created']);
    assert.equal(trail[0].details.assets, 1);
  });
});

describe('programme assets', () => {
  it('adds an asset of the same site', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const chiller = seedAsset(world.app, world.site.id, 'chiller');
    const programme = startProgramme(world, [boiler.id]);
    const updated = call(world.app, 'POST', `/programmes/${programme.id}/assets`, 'dispatch', { assetId: chiller.id });
    assert.deepEqual(updated.assetIds, [boiler.id, chiller.id]);
    assert.deepEqual(call(world.app, 'POST', `/programmes/${programme.id}/assets`, 'dispatch', { assetId: chiller.id }).assetIds, [boiler.id, chiller.id]);
  });

  it('refuses assets that belong to another programme', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const chiller = seedAsset(world.app, world.site.id, 'chiller');
    startProgramme(world, [chiller.id]);
    const programme = startProgramme(world, [boiler.id]);
    assert.equal(expectStatus(world.app, 409, 'POST', `/programmes/${programme.id}/assets`, 'dispatch', { assetId: chiller.id }).error.code, 'asset-in-programme');
  });

  it('removes an asset but keeps at least one', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const chiller = seedAsset(world.app, world.site.id, 'chiller');
    const programme = startProgramme(world, [boiler.id, chiller.id]);
    assert.deepEqual(call(world.app, 'DELETE', `/programmes/${programme.id}/assets/${boiler.id}`, 'dispatch').assetIds, [chiller.id]);
    assert.equal(expectStatus(world.app, 409, 'DELETE', `/programmes/${programme.id}/assets/${chiller.id}`, 'dispatch').error.code, 'programme-needs-assets');
    assert.equal(expectStatus(world.app, 404, 'DELETE', `/programmes/${programme.id}/assets/${boiler.id}`, 'dispatch').error.code, 'asset-not-in-programme');
  });

  it('respects the asset limit when adding', () => {
    const world = createWorld({ config: { programmes: { maxAssetsPerProgramme: 1 } } });
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const chiller = seedAsset(world.app, world.site.id, 'chiller');
    const programme = startProgramme(world, [boiler.id]);
    assert.equal(expectStatus(world.app, 422, 'POST', `/programmes/${programme.id}/assets`, 'dispatch', { assetId: chiller.id }).error.code, 'too-many-assets');
  });
});
'''

LIFECYCLE_TEST = r'''
import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';
import { at, call, createWorld, expectStatus, seedAsset } from '../support/world.ts';
import type { World } from '../support/world.ts';
import { generate, START, startProgramme } from '../support/programmes.ts';

let world: World;
let programme: any;

beforeEach(() => {
  world = createWorld();
  programme = startProgramme(world, [seedAsset(world.app, world.site.id, 'boiler').id], { cadence: 'monthly' });
});

const post = (action: string, actor = 'dispatch') => call(world.app, 'POST', `/programmes/${programme.id}/${action}`, actor);

describe('programme lifecycle', () => {
  it('pauses and resumes', () => {
    assert.equal(post('pause').status, 'paused');
    assert.equal(post('resume').status, 'active');
  });

  it('refuses transitions the status does not allow', () => {
    assert.equal(expectStatus(world.app, 409, 'POST', `/programmes/${programme.id}/resume`, 'dispatch').error.code, 'invalid-programme-state');
    post('pause');
    assert.equal(expectStatus(world.app, 409, 'POST', `/programmes/${programme.id}/pause`, 'dispatch').error.code, 'invalid-programme-state');
  });

  it('ends for good', () => {
    world.app.clock.set(at('2026-03-04', '09:00'));
    const ended = post('end');
    assert.equal(ended.status, 'ended');
    assert.equal(ended.endedOn, '2026-03-04');
    for (const action of ['end', 'pause', 'resume']) {
      assert.ok([409].includes(world.app.request('POST', `/programmes/${programme.id}/${action}`, { actor: 'dispatch' }).status), action);
    }
  });

  it('can end a paused programme', () => {
    post('pause');
    assert.equal(post('end').status, 'ended');
  });

  it('refuses to generate while paused or after the end', () => {
    post('pause');
    assert.equal(expectStatus(world.app, 409, 'POST', `/programmes/${programme.id}/generate`, 'dispatch').error.code, 'programme-not-active');
    post('resume');
    post('end');
    assert.equal(expectStatus(world.app, 409, 'POST', `/programmes/${programme.id}/generate`, 'dispatch').error.code, 'programme-not-active');
  });

  it('moves the planning start to the resume date', () => {
    post('pause');
    world.app.clock.set(at('2026-05-20', '09:00'));
    assert.equal(post('resume').activeFrom, '2026-05-20');
  });

  it('does not generate the visits that fell due during the pause', () => {
    post('pause');
    world.app.clock.set(at('2026-05-20', '09:00'));
    post('resume');
    const { opened } = generate(world, programme.id);
    assert.deepEqual(opened.map((visit: any) => visit.dueOn), []);
    assert.deepEqual(generate(world, programme.id, '2026-06-09').opened.map((visit: any) => visit.dueOn), ['2026-06-09']);
  });

  it('writes every transition to the audit trail', () => {
    post('pause');
    post('resume');
    post('end');
    const actions = call(world.app, 'GET', `/audit/${programme.id}`, 'admin').items.map((entry: any) => entry.action);
    assert.deepEqual(actions, ['programme.created', 'programme.paused', 'programme.resumed', 'programme.ended']);
  });

  it('keeps the first visit date of the cadence after the start', () => {
    assert.equal(programme.startsOn, START);
    assert.equal(call(world.app, 'GET', `/programmes/${programme.id}`, 'dispatch').startsOn, START);
  });
});
'''

VISITS_TEST = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { call, createWorld, seedAsset } from '../support/world.ts';
import { generate, START, startProgramme } from '../support/programmes.ts';

const visitsOf = (world: ReturnType<typeof createWorld>, id: string, query = '') =>
  call(world.app, 'GET', `/programmes/${id}/visits${query}`, 'dispatch').items;

describe('planned visits', () => {
  it('list every asset on every due date, ordered by date and asset id', () => {
    const world = createWorld();
    const chiller = seedAsset(world.app, world.site.id, 'chiller');
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const programme = startProgramme(world, [chiller.id, boiler.id], { cadence: 'quarterly' });
    const visits = visitsOf(world, programme.id, `?from=${START}&to=2026-06-30`);
    assert.deepEqual(visits.map((visit: any) => [visit.dueOn, visit.assetId]), [
      [START, chiller.id], [START, boiler.id], ['2026-06-09', chiller.id], ['2026-06-09', boiler.id],
    ]);
  });

  it('carry the work type and summary of the asset kind', () => {
    const world = createWorld();
    const lift = seedAsset(world.app, world.site.id, 'elevator');
    const programme = startProgramme(world, [lift.id]);
    const [visit] = visitsOf(world, programme.id, `?from=${START}&to=${START}`);
    assert.equal(visit.workType, 'elevator-inspection');
    assert.equal(visit.summary, `Planned elevator inspection due ${START}`);
    assert.deepEqual(visit.tags, ['planned', 'statutory']);
    assert.equal(visit.key, `${lift.id}@${START}`);
    assert.equal(visit.jobId, undefined);
  });

  it('show the work order of a visit that was generated', () => {
    const world = createWorld();
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const programme = startProgramme(world, [boiler.id]);
    const { opened } = generate(world, programme.id, START);
    const [visit] = visitsOf(world, programme.id, `?from=${START}&to=${START}`);
    assert.equal(visit.jobId, opened[0].jobId);
  });

  it('default to the coming year', () => {
    const world = createWorld();
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const programme = startProgramme(world, [boiler.id], { cadence: 'quarterly' });
    assert.deepEqual(visitsOf(world, programme.id).map((visit: any) => visit.dueOn), [START, '2026-06-09', '2026-09-09', '2026-12-09', '2027-03-02'].slice(0, 4));
  });

  it('leave out retired assets', () => {
    const world = createWorld();
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const chiller = seedAsset(world.app, world.site.id, 'chiller');
    const programme = startProgramme(world, [boiler.id, chiller.id]);
    call(world.app, 'POST', `/assets/${chiller.id}/retire`, 'admin');
    assert.deepEqual(visitsOf(world, programme.id, `?from=${START}&to=${START}`).map((visit: any) => visit.assetId), [boiler.id]);
  });

  it('reject periods that end before they start', () => {
    const world = createWorld();
    const programme = startProgramme(world, [seedAsset(world.app, world.site.id, 'boiler').id]);
    const response = world.app.request('GET', `/programmes/${programme.id}/visits?from=2026-06-01&to=2026-05-01`, { actor: 'dispatch' });
    assert.equal(response.status, 422);
    assert.equal((response.body as any).error.code, 'invalid-range');
  });
});
'''

GENERATE_TEST = r'''
import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';
import { at, call, createWorld, expectStatus, seedAsset } from '../support/world.ts';
import type { World } from '../support/world.ts';
import { generate, START, startProgramme } from '../support/programmes.ts';

let world: World;
let boiler: any;
let chiller: any;
let programme: any;

beforeEach(() => {
  world = createWorld();
  boiler = seedAsset(world.app, world.site.id, 'boiler');
  chiller = seedAsset(world.app, world.site.id, 'chiller');
  programme = startProgramme(world, [boiler.id, chiller.id], { cadence: 'quarterly' });
});

const jobs = () => call(world.app, 'GET', '/jobs?pageSize=200', 'dispatch').items;

describe('generating visits', () => {
  it('opens nothing before the first due date', () => {
    assert.deepEqual(generate(world, programme.id, '2026-03-08').opened, []);
    assert.equal(jobs().length, 0);
  });

  it('opens one work order per asset on the due date', () => {
    const { opened } = generate(world, programme.id, START);
    assert.deepEqual(opened.map((visit: any) => [visit.assetId, visit.dueOn, visit.workType]), [
      [boiler.id, START, 'boiler-service'],
      [chiller.id, START, 'chiller-maintenance'],
    ]);
    assert.equal(jobs().length, 2);
  });

  it('opens ordinary work orders for the programme customer and site', () => {
    const { opened } = generate(world, programme.id, START);
    const job = call(world.app, 'GET', `/jobs/${opened[0].jobId}`, 'dispatch');
    assert.equal(job.customerId, world.customer.id);
    assert.equal(job.siteId, world.site.id);
    assert.equal(job.status, 'new');
    assert.equal(job.description, `Plant room upkeep: planned visit due ${START}`);
    assert.match(job.number, /^FL-2026-\d+$/);
  });

  it('never opens a visit twice', () => {
    generate(world, programme.id, START);
    assert.deepEqual(generate(world, programme.id, START).opened, []);
    assert.deepEqual(generate(world, programme.id, '2026-06-08').opened, []);
    assert.equal(jobs().length, 2);
    assert.equal(call(world.app, 'GET', `/programmes/${programme.id}`, 'dispatch').openedVisits, 2);
  });

  it('opens the later visits when their dates are reached', () => {
    generate(world, programme.id, START);
    const june = generate(world, programme.id, '2026-06-09');
    assert.deepEqual(june.opened.map((visit: any) => visit.dueOn), ['2026-06-09', '2026-06-09']);
    assert.equal(jobs().length, 4);
  });

  it('catches up visits that were missed while the programme was active', () => {
    const late = generate(world, programme.id, '2026-09-20');
    assert.deepEqual(late.opened.map((visit: any) => visit.dueOn), [START, START, '2026-06-09', '2026-06-09', '2026-09-09', '2026-09-09']);
  });

  it('plans from the creation date, not from the start date', () => {
    world.app.clock.set(at('2026-07-01', '08:00'));
    const seeded = startProgramme(world, [seedAsset(world.app, world.site.id, 'elevator').id], { name: 'Lift upkeep', startsOn: '2026-01-15', cadence: 'monthly' });
    const { opened } = generate(world, seeded.id, '2026-08-20');
    assert.deepEqual(opened.map((visit: any) => visit.dueOn), ['2026-07-15', '2026-08-15']);
  });

  it('defaults to the date of the clock', () => {
    world.app.clock.set(at(START, '09:00'));
    assert.equal(generate(world, programme.id).opened.length, 2);
  });

  it('opens visits ahead of time by the configured lead days', () => {
    const early = createWorld({ config: { programmes: { leadDays: 7 } } });
    const asset = seedAsset(early.app, early.site.id, 'boiler');
    const own = startProgramme(early, [asset.id]);
    assert.deepEqual(generate(early, own.id, '2026-03-01').opened, []);
    assert.equal(generate(early, own.id, '2026-03-02').opened.length, 1);
  });

  it('skips retired assets', () => {
    call(world.app, 'POST', `/assets/${chiller.id}/retire`, 'admin');
    const { opened } = generate(world, programme.id, START);
    assert.deepEqual(opened.map((visit: any) => visit.assetId), [boiler.id]);
  });

  it('refuses work for suspended customers and keeps the ledger empty', () => {
    call(world.app, 'POST', `/customers/${world.customer.id}/suspend`, 'admin', { reason: 'Unpaid invoices' });
    const response = expectStatus(world.app, 409, 'POST', `/programmes/${programme.id}/generate?on=${START}`, 'dispatch');
    assert.equal(response.error.code, 'customer-not-accepting-work');
    assert.equal(call(world.app, 'GET', `/programmes/${programme.id}`, 'dispatch').openedVisits, 0);
    call(world.app, 'POST', `/customers/${world.customer.id}/reactivate`, 'admin');
    assert.equal(generate(world, programme.id, START).opened.length, 2);
  });

  it('records one audit entry per run that opened work', () => {
    generate(world, programme.id, START);
    generate(world, programme.id, START);
    const actions = call(world.app, 'GET', `/audit/${programme.id}`, 'admin').items.map((entry: any) => entry.action);
    assert.deepEqual(actions, ['programme.created', 'programme.generated']);
  });

  it('clamps month-end dates without drifting', () => {
    const lift = seedAsset(world.app, world.site.id, 'elevator');
    const monthly = startProgramme(world, [lift.id], { name: 'Lift upkeep', startsOn: '2026-03-31', cadence: 'monthly' });
    const dates = call(world.app, 'GET', `/programmes/${monthly.id}/visits?from=2026-03-01&to=2026-06-30`, 'dispatch').items.map((visit: any) => visit.dueOn);
    assert.deepEqual(dates, ['2026-03-31', '2026-04-30', '2026-05-31', '2026-06-30']);
  });
});
'''

COEXISTENCE_TEST = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { at, call, completeChecklist, createWorld, JOBS, seedAsset, seedContract } from '../support/world.ts';
import { generate, START, startProgramme } from '../support/programmes.ts';

function scene() {
  const world = createWorld();
  const boiler = seedAsset(world.app, world.site.id, 'boiler');
  const contract = seedContract(world.app, world.customer.id, [world.site.id], 'basic', ['boiler-service']);
  const programme = startProgramme(world, [boiler.id], { cadence: 'annual' });
  world.app.clock.set(at(START, '09:00'));
  const job = call(world.app, 'GET', `/jobs/${generate(world, programme.id).opened[0].jobId}`, 'dispatch');
  return { world, boiler, contract, programme, job };
}

describe('programme work orders on a contract site', () => {
  it('are covered by the contract like any work order', () => {
    const { job, contract } = scene();
    assert.equal(job.contractId, contract.id);
    assert.ok(job.tags.includes('included-visit'));
    assert.ok(job.tags.includes('planned'));
  });

  it('use up the included visits of the contract', () => {
    const { world, contract } = scene();
    const again = call(world.app, 'POST', JOBS, 'dispatch', {
      customerId: world.customer.id, siteId: world.site.id, workType: 'boiler-service', assetId: call(world.app, 'GET', `/sites/${world.site.id}/assets`, 'dispatch').items[0].id, summary: 'Second boiler service',
    });
    assert.equal(again.contractId, contract.id);
    assert.ok(!again.tags.includes('included-visit'));
  });

  it('run no SLA clock, while other contract work orders do', () => {
    const { world, job, boiler } = scene();
    assert.equal(world.app.request('GET', `${JOBS}/${job.id}/sla`, { actor: 'dispatch' }).status, 404);
    const callout = call(world.app, 'POST', JOBS, 'dispatch', {
      customerId: world.customer.id, siteId: world.site.id, workType: 'boiler-service', assetId: boiler.id, summary: 'Boiler lockout',
    });
    assert.equal(call(world.app, 'GET', `${JOBS}/${callout.id}/sla`, 'dispatch').policyId, 'standard');
  });

  it('are completed and invoiced without a callout fee for the included visit', () => {
    const { world, job } = scene();
    call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    completeChecklist(world.app, job.id, world.technician.id);
    call(world.app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Serviced the boiler and tested', signatureName: 'Avery Collins' });
    const invoice = call(world.app, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
    assert.ok(invoice.lines.every((line: any) => line.kind !== 'callout'));
  });

  it('update the service history of the asset when completed', () => {
    const { world, job, boiler } = scene();
    call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    completeChecklist(world.app, job.id, world.technician.id);
    call(world.app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Serviced the boiler and tested', signatureName: 'Avery Collins' });
    const stored = call(world.app, 'GET', `/sites/${world.site.id}/assets`, 'dispatch').items.find((asset: any) => asset.id === boiler.id);
    assert.equal(stored.lastServicedOn, START);
  });

  it('leave their follow-ups as ordinary contract work orders with a clock', () => {
    const { world, job } = scene();
    call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    completeChecklist(world.app, job.id, world.technician.id);
    call(world.app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Serviced the boiler and tested', signatureName: 'Avery Collins' });
    const followUp = call(world.app, 'POST', `${JOBS}/${job.id}/follow-ups`, 'dispatch', { summary: 'Replace the expansion vessel' });
    assert.equal(call(world.app, 'GET', `${JOBS}/${followUp.id}`, 'dispatch').source, 'phone');
    assert.equal(call(world.app, 'GET', `${JOBS}/${followUp.id}/sla`, 'dispatch').policyId, 'standard');
  });

  it('show up in the backlog while they are open', () => {
    const { world } = scene();
    const backlog = call(world.app, 'GET', '/reports/backlog', 'admin');
    assert.equal(backlog.aging.reduce((sum: number, bucket: any) => sum + bucket.count, 0), 1);
  });

  it('keep the customer from closing while a visit is open', () => {
    const { world } = scene();
    const response = world.app.request('POST', `/customers/${world.customer.id}/close`, { actor: 'admin', body: undefined });
    assert.equal(response.status, 409);
  });
});
'''

API_TEST = r'''
import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';
import { call, createWorld, expectStatus, seedAsset } from '../support/world.ts';
import type { World } from '../support/world.ts';
import { startProgramme } from '../support/programmes.ts';

let world: World;
let portal: string;

beforeEach(() => {
  world = createWorld();
  portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
});

const asset = (kind = 'boiler') => seedAsset(world.app, world.site.id, kind).id;

describe('GET /programmes', () => {
  it('lists programmes by id without their visit ledger', () => {
    const first = startProgramme(world, [asset('boiler')]);
    const second = startProgramme(world, [asset('chiller')], { name: 'Chiller upkeep' });
    const list = call(world.app, 'GET', '/programmes', 'dispatch');
    assert.deepEqual(list.items.map((item: any) => item.id), [first.id, second.id]);
    assert.ok(list.items.every((item: any) => item.generated === undefined && item.openedVisits === 0));
  });

  it('filters by customer and status', () => {
    const first = startProgramme(world, [asset('boiler')]);
    startProgramme(world, [asset('chiller')], { name: 'Chiller upkeep' });
    call(world.app, 'POST', `/programmes/${first.id}/pause`, 'dispatch');
    assert.deepEqual(call(world.app, 'GET', '/programmes?status=paused', 'dispatch').items.map((item: any) => item.id), [first.id]);
    assert.equal(call(world.app, 'GET', `/programmes?customerId=${world.customer.id}`, 'dispatch').items.length, 2);
    assert.equal(call(world.app, 'GET', '/programmes?customerId=CUS-999999', 'dispatch').items.length, 0);
  });
});

describe('GET /programmes/:id', () => {
  it('returns the programme', () => {
    const programme = startProgramme(world, [asset()]);
    assert.deepEqual(call(world.app, 'GET', `/programmes/${programme.id}`, 'dispatch'), programme);
  });

  it('reports unknown programmes as missing', () => {
    assert.equal(expectStatus(world.app, 404, 'GET', '/programmes/PRG-999999', 'dispatch').error.details.entity, 'programme');
  });
});

describe('programme access', () => {
  it('lets staff with scheduling rights read and dispatch and admin change programmes', () => {
    const programme = startProgramme(world, [asset()]);
    for (const actor of ['admin', 'dispatch']) expectStatus(world.app, 200, 'GET', `/programmes/${programme.id}`, actor);
    expectStatus(world.app, 200, 'POST', `/programmes/${programme.id}/pause`, 'admin');
    expectStatus(world.app, 200, 'POST', `/programmes/${programme.id}/resume`, 'dispatch');
  });

  it('refuses every other role', () => {
    const programme = startProgramme(world, [asset()]);
    for (const actor of ['billing', 'warehouse', world.technician.id, portal]) {
      assert.equal(expectStatus(world.app, 403, 'GET', `/programmes/${programme.id}`, actor).error.code, 'forbidden', actor);
      assert.equal(expectStatus(world.app, 403, 'POST', `/programmes/${programme.id}/pause`, actor).error.code, 'forbidden', actor);
      assert.equal(expectStatus(world.app, 403, 'GET', '/programmes', actor).error.code, 'forbidden', actor);
    }
  });

  it('answers unknown actors with 401', () => {
    expectStatus(world.app, 401, 'GET', '/programmes', 'nobody');
  });
});

describe('GET /programmes/gaps', () => {
  it('lists due assets that no active programme covers', () => {
    const covered = asset('boiler');
    const open = asset('elevator');
    startProgramme(world, [covered]);
    const gaps = call(world.app, 'GET', '/programmes/gaps?on=2027-06-01', 'dispatch').items;
    assert.deepEqual(gaps.map((item: any) => item.id), [open]);
  });

  it('lists an asset again once its programme is paused or ended', () => {
    const boiler = asset('boiler');
    const programme = startProgramme(world, [boiler]);
    call(world.app, 'POST', `/programmes/${programme.id}/pause`, 'dispatch');
    assert.deepEqual(call(world.app, 'GET', '/programmes/gaps?on=2027-06-01', 'dispatch').items.map((item: any) => item.id), [boiler]);
  });

  it('leaves out assets that are not due yet', () => {
    asset('boiler');
    assert.deepEqual(call(world.app, 'GET', '/programmes/gaps?on=2024-07-01&within=0', 'dispatch').items, []);
  });

  it('is not mistaken for a programme id', () => {
    assert.equal(world.app.request('GET', '/programmes/gaps', { actor: 'dispatch' }).status, 200);
  });
});
'''

GENERATE_API_TEST = r'''
import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';
import { call, createWorld, expectStatus, seedAsset } from '../support/world.ts';
import type { World } from '../support/world.ts';
import { START, startProgramme } from '../support/programmes.ts';

let world: World;
let programme: any;

beforeEach(() => {
  world = createWorld();
  programme = startProgramme(world, [seedAsset(world.app, world.site.id, 'generator').id], { cadence: 'semi-annual' });
});

describe('POST /programmes/:id/generate', () => {
  it('answers with the opened visits', () => {
    const body = expectStatus(world.app, 200, 'POST', `/programmes/${programme.id}/generate?on=${START}`, 'dispatch');
    assert.equal(body.programmeId, programme.id);
    assert.equal(body.on, START);
    assert.equal(body.opened.length, 1);
    assert.equal(body.opened[0].workType, 'generator-load-test');
    assert.match(body.opened[0].jobId, /^WO-\d{6}$/);
  });

  it('answers an empty list when everything was generated already', () => {
    call(world.app, 'POST', `/programmes/${programme.id}/generate?on=${START}`, 'dispatch');
    assert.deepEqual(expectStatus(world.app, 200, 'POST', `/programmes/${programme.id}/generate?on=${START}`, 'dispatch').opened, []);
  });

  it('rejects malformed dates', () => {
    assert.equal(expectStatus(world.app, 422, 'POST', `/programmes/${programme.id}/generate?on=tomorrow`, 'dispatch').error.code, 'invalid-input');
  });

  it('reports unknown programmes as missing', () => {
    expectStatus(world.app, 404, 'POST', '/programmes/PRG-999999/generate', 'dispatch');
  });

  it('is limited to the roles that may change programmes', () => {
    for (const actor of ['billing', 'warehouse', world.technician.id]) {
      expectStatus(world.app, 403, 'POST', `/programmes/${programme.id}/generate`, actor);
    }
  });

  it('opens work orders that the job routes list', () => {
    const { opened } = call(world.app, 'POST', `/programmes/${programme.id}/generate?on=${START}`, 'dispatch');
    const listed = call(world.app, 'GET', `/jobs?pageSize=200`, 'dispatch').items.map((item: any) => item.id);
    assert.deepEqual(listed, [opened[0].jobId]);
    assert.equal(call(world.app, 'GET', `/jobs/${opened[0].jobId}`, 'dispatch').source, 'programme');
  });
});
'''

REPORT_TEST = r'''
import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';
import { call, createWorld, expectStatus, seedAsset } from '../support/world.ts';
import type { World } from '../support/world.ts';
import { generate, START, startProgramme } from '../support/programmes.ts';

let world: World;

beforeEach(() => {
  world = createWorld();
});

const schedule = (query: string, actor = 'admin') => call(world.app, 'GET', `/reports/programme-schedule?${query}`, actor).items;

describe('GET /reports/programme-schedule', () => {
  it('lists the planned visits of active programmes soonest first', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const lift = seedAsset(world.app, world.site.id, 'elevator');
    startProgramme(world, [boiler.id], { cadence: 'quarterly' });
    startProgramme(world, [lift.id], { name: 'Lift upkeep', cadence: 'monthly' });
    const rows = schedule('from=2026-03-01&to=2026-05-31');
    assert.deepEqual(rows.map((row: any) => [row.dueOn, row.programmeId, row.assetId, row.workType]), [
      [START, 'PRG-000001', boiler.id, 'boiler-service'],
      [START, 'PRG-000002', lift.id, 'elevator-inspection'],
      ['2026-04-09', 'PRG-000002', lift.id, 'elevator-inspection'],
      ['2026-05-09', 'PRG-000002', lift.id, 'elevator-inspection'],
    ]);
  });

  it('marks visits that already have a work order', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const programme = startProgramme(world, [boiler.id], { cadence: 'monthly' });
    const { opened } = generate(world, programme.id, START);
    const rows = schedule('from=2026-03-01&to=2026-04-30');
    assert.deepEqual(rows.map((row: any) => [row.opened, row.jobId]), [[true, opened[0].jobId], [false, undefined]]);
  });

  it('leaves out paused and ended programmes', () => {
    const first = startProgramme(world, [seedAsset(world.app, world.site.id, 'boiler').id]);
    const second = startProgramme(world, [seedAsset(world.app, world.site.id, 'chiller').id], { name: 'Chiller upkeep' });
    call(world.app, 'POST', `/programmes/${first.id}/pause`, 'dispatch');
    call(world.app, 'POST', `/programmes/${second.id}/end`, 'dispatch');
    assert.deepEqual(schedule('from=2026-03-01&to=2026-12-31'), []);
  });

  it('requires both ends of the period', () => {
    expectStatus(world.app, 422, 'GET', '/reports/programme-schedule?from=2026-03-01', 'admin');
    expectStatus(world.app, 422, 'GET', '/reports/programme-schedule?to=2026-03-01', 'admin');
  });

  it('is open to the reporting roles only', () => {
    for (const actor of ['admin', 'dispatch', 'billing']) expectStatus(world.app, 200, 'GET', '/reports/programme-schedule?from=2026-03-01&to=2026-03-31', actor);
    for (const actor of ['warehouse', world.technician.id]) expectStatus(world.app, 403, 'GET', '/reports/programme-schedule?from=2026-03-01&to=2026-03-31', actor);
  });
});
'''

CONFIG_TEST = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { DEFAULT_CONFIG } from '../../src/config/defaults.ts';
import { resolveConfig } from '../../src/config/load.ts';
import { configProblems } from '../../src/config/schema.ts';

describe('programme settings', () => {
  it('open work orders on the due date and cover up to 25 assets by default', () => {
    assert.equal(DEFAULT_CONFIG.programmes.leadDays, 0);
    assert.equal(DEFAULT_CONFIG.programmes.maxAssetsPerProgramme, 25);
  });

  it('are valid in the defaults', () => {
    assert.deepEqual(configProblems(DEFAULT_CONFIG), []);
  });

  it('merge key by key', () => {
    const config = resolveConfig({ programmes: { leadDays: 5 } });
    assert.equal(config.programmes.leadDays, 5);
    assert.equal(config.programmes.maxAssetsPerProgramme, 25);
  });

  it('reject lead days outside 0 to 30', () => {
    for (const leadDays of [-1, 31, 1.5]) {
      assert.throws(() => resolveConfig({ programmes: { leadDays } }), /programmes\.leadDays/, String(leadDays));
    }
    assert.equal(resolveConfig({ programmes: { leadDays: 30 } }).programmes.leadDays, 30);
  });

  it('reject asset limits outside 1 to 200', () => {
    for (const maxAssetsPerProgramme of [0, 201, 2.5]) {
      assert.throws(() => resolveConfig({ programmes: { maxAssetsPerProgramme } }), /programmes\.maxAssetsPerProgramme/, String(maxAssetsPerProgramme));
    }
  });

  it('reject unknown programme settings', () => {
    assert.throws(() => resolveConfig({ programmes: { cadence: 'weekly' } as any }), /cadence/);
  });

  it('leave the other sections untouched', () => {
    const config = resolveConfig({ programmes: { leadDays: 3 } });
    assert.deepEqual(config.scheduling, DEFAULT_CONFIG.scheduling);
    assert.deepEqual(config.features, DEFAULT_CONFIG.features);
  });
});
'''

ADAPTER_TEST = r'''
import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';
import { activeProgrammes, programmeCovering, programmes, programmesForCustomer } from '../../src/adapters/repositories/programmes.ts';
import { call, createWorld, seedAsset, seedCustomer } from '../support/world.ts';
import type { World } from '../support/world.ts';
import { startProgramme } from '../support/programmes.ts';

let world: World;

beforeEach(() => {
  world = createWorld();
});

describe('programme repository', () => {
  it('stores programmes in the programmes collection', () => {
    const programme = startProgramme(world, [seedAsset(world.app, world.site.id, 'boiler').id]);
    assert.equal(programmes(world.app.ctx).get(programme.id)?.status, 'active');
    assert.deepEqual(Object.keys(world.app.ctx.store.snapshot()).includes('programmes'), true);
  });

  it('keeps the visit ledger out of the API view but in the record', () => {
    const programme = startProgramme(world, [seedAsset(world.app, world.site.id, 'boiler').id]);
    assert.deepEqual(programmes(world.app.ctx).require(programme.id).generated, {});
  });

  it('lists the programmes of a customer by id', () => {
    const first = startProgramme(world, [seedAsset(world.app, world.site.id, 'boiler').id]);
    const second = startProgramme(world, [seedAsset(world.app, world.site.id, 'chiller').id], { name: 'Chiller upkeep' });
    const { customer, site } = seedCustomer(world.app, { name: 'Fenwick Pharmacy' });
    call(world.app, 'POST', '/programmes', 'dispatch', {
      customerId: customer.id, siteId: site.id, name: 'Lift upkeep', cadence: 'annual', startsOn: '2026-04-01', assetIds: [seedAsset(world.app, site.id, 'elevator').id],
    });
    assert.deepEqual(programmesForCustomer(world.app.ctx, world.customer.id).map((programme) => programme.id), [first.id, second.id]);
  });

  it('lists active programmes only', () => {
    const first = startProgramme(world, [seedAsset(world.app, world.site.id, 'boiler').id]);
    const second = startProgramme(world, [seedAsset(world.app, world.site.id, 'chiller').id], { name: 'Chiller upkeep' });
    call(world.app, 'POST', `/programmes/${first.id}/pause`, 'dispatch');
    assert.deepEqual(activeProgrammes(world.app.ctx).map((programme) => programme.id), [second.id]);
  });

  it('finds the programme that still plans visits for an asset', () => {
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const programme = startProgramme(world, [boiler.id]);
    assert.equal(programmeCovering(world.app.ctx, boiler.id)?.id, programme.id);
    call(world.app, 'POST', `/programmes/${programme.id}/pause`, 'dispatch');
    assert.equal(programmeCovering(world.app.ctx, boiler.id)?.id, programme.id);
    call(world.app, 'POST', `/programmes/${programme.id}/end`, 'dispatch');
    assert.equal(programmeCovering(world.app.ctx, boiler.id), undefined);
  });
});
'''

GAPS_TEST = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { unprogrammedDueAssets } from '../../src/programmes/coverage.ts';
import { assetsDueForService } from '../../src/assets/service.ts';
import { call, createWorld, seedAsset } from '../support/world.ts';
import { startProgramme } from '../support/programmes.ts';

describe('unprogrammedDueAssets', () => {
  it('equals the due assets when no programme exists', () => {
    const world = createWorld();
    seedAsset(world.app, world.site.id, 'boiler');
    seedAsset(world.app, world.site.id, 'elevator');
    const ctx = world.app.ctx;
    assert.deepEqual(unprogrammedDueAssets(ctx, '2027-06-01', 30), assetsDueForService(ctx, '2027-06-01', 30));
  });

  it('keeps the order of the due list', () => {
    const world = createWorld();
    const ids = ['boiler', 'elevator', 'chiller', 'solar-inverter'].map((kind) => seedAsset(world.app, world.site.id, kind).id);
    startProgramme(world, [ids[2]]);
    const gaps = unprogrammedDueAssets(world.app.ctx, '2027-06-01', 30).map((asset) => asset.id);
    assert.deepEqual(gaps, assetsDueForService(world.app.ctx, '2027-06-01', 30).map((asset) => asset.id).filter((id) => id !== ids[2]));
  });

  it('ignores programmes that are not active', () => {
    const world = createWorld();
    const boiler = seedAsset(world.app, world.site.id, 'boiler');
    const programme = startProgramme(world, [boiler.id]);
    assert.deepEqual(unprogrammedDueAssets(world.app.ctx, '2027-06-01', 30), []);
    call(world.app, 'POST', `/programmes/${programme.id}/end`, 'dispatch');
    assert.deepEqual(unprogrammedDueAssets(world.app.ctx, '2027-06-01', 30).map((asset) => asset.id), [boiler.id]);
  });

  it('respects the look-ahead window', () => {
    const world = createWorld();
    const lift = seedAsset(world.app, world.site.id, 'elevator');
    assert.deepEqual(unprogrammedDueAssets(world.app.ctx, '2024-06-20', 0), []);
    assert.deepEqual(unprogrammedDueAssets(world.app.ctx, '2024-06-20', 15).map((asset) => asset.id), [lift.id]);
  });
});
'''

PERMISSION_GRANTS = r'''
  'programmes:write': {
    roles: ['admin', 'dispatcher'],
    routes: [
      ['POST', '/programmes'], ['POST', `/programmes/${MISSING}/pause`], ['POST', `/programmes/${MISSING}/resume`],
      ['POST', `/programmes/${MISSING}/end`], ['POST', `/programmes/${MISSING}/assets`], ['DELETE', `/programmes/${MISSING}/assets/${MISSING}`],
      ['POST', `/programmes/${MISSING}/generate`],
    ],
  },
'''

CONFIG_DOC_ROWS = (
    "| `programmes.leadDays` | 0 | Days before its due date on which the work order of a planned visit is opened. |\n"
    "| `programmes.maxAssetsPerProgramme` | 25 | Assets one maintenance programme can cover. |\n"
)


def generate(ctx):
    if ctx.variant != VARIANT:
        return
    _programme_sources(ctx)
    _integration(ctx)
    _tests(ctx)
    _docs(ctx)


def _programme_sources(ctx):
    ctx.add("src/programmes/model.ts", MODEL)
    ctx.add("src/programmes/cadence.ts", CADENCE)
    ctx.add("src/programmes/kinds/types.ts", KIND_TYPES)
    for asset, name, stem, work, summary, tag in KINDS:
        ctx.add(f"src/programmes/kinds/{stem}.ts", (KIND_FILE.replace("§NAME§", name).replace("§ASSET§", asset)
                                                     .replace("§WORK§", work).replace("§SUMMARY§", summary).replace("§TAG§", tag)))
    imports = "\n".join(f"import {{ {name} }} from './{stem}.ts';" for _, name, stem, *_ in KINDS)
    listing = "\n".join(f"  {name}," for _, name, *_ in KINDS)
    ctx.add("src/programmes/kinds/index.ts", KIND_INDEX.replace("§IMPORTS§", imports).replace("§LIST§", listing))
    ctx.add("src/programmes/validation.ts", VALIDATION)
    ctx.add("src/adapters/repositories/programmes.ts", REPOSITORY)
    ctx.add("src/programmes/service.ts", SERVICE)
    ctx.add("src/programmes/visits.ts", VISITS)
    ctx.add("src/programmes/generate.ts", GENERATE)
    ctx.add("src/programmes/coverage.ts", COVERAGE)
    ctx.add("src/programmes/view.ts", VIEW)
    ctx.add("src/reports/programme-schedule.ts", SCHEDULE_REPORT)
    ctx.add("src/api/routes/programmes.ts", ROUTES)


def _integration(ctx):
    ctx.patch("src/adapters/store/memory.ts", "  | 'notifications'\n", "  | 'notifications'\n  | 'programmes'\n")
    ctx.patch("src/jobs/jobs.ts", "source: 'phone' | 'portal' | 'contract' | 'api';", "source: 'phone' | 'portal' | 'contract' | 'api' | 'programme';")
    ctx.patch("src/jobs/jobs.ts", "    source: original.source,\n    tags: ['follow-up'],",
              "    // A follow-up is unplanned work and runs the contract clock even when the visit it follows was planned.\n"
              "    source: original.source === 'programme' ? 'phone' : original.source,\n    tags: ['follow-up'],")
    ctx.patch("src/auth/permissions.ts", "  | 'quotes:write'\n", "  | 'quotes:write'\n  | 'programmes:write'\n")
    ctx.patch("src/auth/permissions.ts", "'timesheets:approve', 'billing:read', 'billing:write', 'reports:read', 'quotes:write', 'notifications:read',\n    'system:admin',",
              "'timesheets:approve', 'billing:read', 'billing:write', 'reports:read', 'quotes:write', 'notifications:read',\n    'programmes:write', 'system:admin',")
    ctx.patch("src/auth/permissions.ts", "'inventory:read', 'timesheets:approve', 'reports:read', 'quotes:write', 'notifications:read',\n  ],",
              "'inventory:read', 'timesheets:approve', 'reports:read', 'quotes:write', 'notifications:read', 'programmes:write',\n  ],")
    ctx.patch("src/api/routes/index.ts", "import { registerNotificationRoutes } from './notifications.ts';\n",
              "import { registerNotificationRoutes } from './notifications.ts';\nimport { registerProgrammeRoutes } from './programmes.ts';\n")
    ctx.patch("src/api/routes/index.ts", "  registerQuoteRoutes(router);\n", "  registerQuoteRoutes(router);\n  registerProgrammeRoutes(router);\n")
    ctx.patch("src/api/routes/reports.ts", "import { kpiSnapshot } from '../../reports/kpis.ts';\n",
              "import { kpiSnapshot } from '../../reports/kpis.ts';\nimport { programmeSchedule } from '../../reports/programme-schedule.ts';\n")
    ctx.patch("src/api/routes/reports.ts", "  router.get('/reports/backlog', (ctx, request) => {",
              "  router.get('/reports/programme-schedule', (ctx, request) => {\n    authorize(request.actor, 'reports:read');\n"
              "    return ok({ items: programmeSchedule(ctx, queryDate(request, 'from'), queryDate(request, 'to')) });\n  });\n\n"
              "  router.get('/reports/backlog', (ctx, request) => {")
    ctx.patch("src/config/schema.ts", "  holidays: CalendarDate[];\n",
              "  programmes: {\n    /** Days before its due date on which the work order of a planned visit is opened. */\n    leadDays: number;\n    maxAssetsPerProgramme: number;\n  };\n  holidays: CalendarDate[];\n")
    ctx.patch("src/config/schema.ts", "  if (!/^[A-Z]{3}$/.test(config.organization.currency)) problems.push('organization.currency must be an ISO code');\n",
              "  if (!/^[A-Z]{3}$/.test(config.organization.currency)) problems.push('organization.currency must be an ISO code');\n"
              "  const { leadDays, maxAssetsPerProgramme } = config.programmes;\n"
              "  if (!Number.isInteger(leadDays) || leadDays < 0 || leadDays > 30) {\n    problems.push('programmes.leadDays must be a whole number of days from 0 to 30');\n  }\n"
              "  if (!Number.isInteger(maxAssetsPerProgramme) || maxAssetsPerProgramme < 1 || maxAssetsPerProgramme > 200) {\n"
              "    problems.push('programmes.maxAssetsPerProgramme must be a whole number from 1 to 200');\n  }\n")
    ctx.patch("src/sla/subscriptions.ts", "import { contracts } from '../adapters/repositories/contracts.ts';\n",
              "import { contracts } from '../adapters/repositories/contracts.ts';\nimport { jobs } from '../adapters/repositories/jobs.ts';\n")
    ctx.patch("src/sla/subscriptions.ts", "  if (!contract) return;\n  const policy = policyFor(",
              "  if (!contract) return;\n  if (jobs(ctx).get(String(event.jobId))?.source === 'programme') return;\n  const policy = policyFor(")
    ctx.patch("src/sla/subscriptions.ts", "function startFor(", "/** Planned programme visits are scheduled against their due date and run no clock. */\nfunction startFor(")
    ctx.patch("docs/service-levels.md", "- The clock starts when a contract work order is opened. Response is met when work starts.",
              "- The clock starts when a contract work order is opened. Response is met when work starts.\n  Planned programme visits (source `programme`) run no clock.")
    ctx.patch("src/config/defaults.ts", "  holidays: ['2026-01-01',",
              "  programmes: {\n    leadDays: 0,\n    maxAssetsPerProgramme: 25,\n  },\n  holidays: ['2026-01-01',")


def _tests(ctx):
    ctx.add("tests/support/programmes.ts", TEST_SUPPORT)
    ctx.add("tests/programmes/cadence.test.ts", CADENCE_TEST)
    ctx.add("tests/programmes/model.test.ts", MODEL_TEST)
    ctx.add("tests/programmes/kinds/index.test.ts", KINDS_INDEX_TEST)
    for asset, name, stem, work, summary, tag in KINDS:
        ctx.add(f"tests/programmes/kinds/{stem}.test.ts", (KIND_TEST.replace("§NAME§", name).replace("§STEM§", stem)
                                                            .replace("§ASSET§", asset).replace("§WORK§", work)
                                                            .replace("§SUMMARY§", summary).replace("§TAG§", tag)))
    ctx.add("tests/programmes/service.test.ts", SERVICE_TEST)
    ctx.add("tests/programmes/lifecycle.test.ts", LIFECYCLE_TEST)
    ctx.add("tests/programmes/visits.test.ts", VISITS_TEST)
    ctx.add("tests/programmes/generate.test.ts", GENERATE_TEST)
    ctx.add("tests/programmes/coexistence.test.ts", COEXISTENCE_TEST)
    ctx.add("tests/programmes/gaps.test.ts", GAPS_TEST)
    ctx.add("tests/api/programmes.test.ts", API_TEST)
    ctx.add("tests/api/programme-generate.test.ts", GENERATE_API_TEST)
    ctx.add("tests/reports/programme-schedule.test.ts", REPORT_TEST)
    ctx.add("tests/config/programmes.test.ts", CONFIG_TEST)
    ctx.add("tests/adapters/programmes.test.ts", ADAPTER_TEST)

    path = "tests/api/security/permissions.test.ts"
    ctx.patch(path, "['GET', '/dispatch/board'], ['GET', '/dispatch/capacity'], ['GET', '/dispatch/rules'],",
              "['GET', '/dispatch/board'], ['GET', '/dispatch/capacity'], ['GET', '/dispatch/rules'], ['GET', '/programmes'],\n      "
              "['GET', '/programmes/gaps'], ['GET', `/programmes/${MISSING}`], ['GET', `/programmes/${MISSING}/visits`],")
    ctx.patch(path, "['GET', '/reports/activity'], ['GET', '/sla/compliance'],", "['GET', '/reports/activity'], ['GET', '/sla/compliance'], ['GET', '/reports/programme-schedule'],")
    ctx.patch(path, "  'notifications:read': { roles: ['admin', 'dispatcher', 'customer'], routes: [['GET', '/notifications']] },\n",
              "  'notifications:read': { roles: ['admin', 'dispatcher', 'customer'], routes: [['GET', '/notifications']] },\n" + PERMISSION_GRANTS.lstrip("\n"))


def _docs(ctx):
    ctx.add("docs/programmes.md", DOC_PROGRAMMES)
    ctx.patch("docs/access.md", "timesheets:approve, reports:read, quotes:write, notifications:read |",
              "timesheets:approve, reports:read, quotes:write, notifications:read, programmes:write |")
    ctx.patch("docs/configuration.md", "| `features.autoInvoiceOnCompletion` | false |", CONFIG_DOC_ROWS + "| `features.autoInvoiceOnCompletion` | false |")
    ctx.patch("docs/architecture.md", "`reports` and\n`integrations`.", "`reports`,\n`integrations` and `programmes`.")
    ctx.append("docs/reports.md", "**Programme schedule** lists the planned visits of active maintenance programmes that fall due in a\nperiod and whether their work orders have been opened.")
