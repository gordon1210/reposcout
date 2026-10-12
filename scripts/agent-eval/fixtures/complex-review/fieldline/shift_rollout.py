"""Shift patterns for technicians: rostered hours, shift changes and the shift-aware views."""

SHIFTS = r'''
import { activeTechnicians, techniciansInRegion } from '../adapters/repositories/technicians.ts';
import type { ShiftDefinition, ShiftPattern } from '../config/schema.ts';
import { businessCalendar } from '../context.ts';
import type { AppContext } from '../context.ts';
import { isWorkingDay, workingMinutesBetween } from '../shared/calendar.ts';
import type { BusinessCalendar } from '../shared/calendar.ts';
import { atMinute, formatClock } from '../shared/time.ts';
import type { CalendarDate } from '../shared/time.ts';
import type { Technician } from './model.ts';
import { isAvailable } from './service.ts';

export const SHIFT_PATTERNS: readonly ShiftPattern[] = ['early', 'standard'];

/** The technician's shift pattern; records created before shift patterns work the standard shift. */
export function shiftPatternOf(technician: Technician): ShiftPattern {
  return technician.shift ?? 'standard';
}

/** The configured hours of the technician's shift. */
export function shiftOf(ctx: AppContext, technician: Technician): ShiftDefinition {
  return ctx.config.scheduling.shifts[shiftPatternOf(technician)] ?? ctx.config.scheduling.shifts.standard;
}

/** The organisation's working days and holidays with the technician's shift hours. */
export function shiftCalendar(ctx: AppContext, technician: Technician): BusinessCalendar {
  const shift = shiftOf(ctx, technician);
  return { ...businessCalendar(ctx), dayStartMinute: shift.startMinute, dayEndMinute: shift.endMinute };
}

/** Rostered minutes on the dates `[from, to)`: the shift hours of every working day. */
export function rosteredMinutes(ctx: AppContext, technician: Technician, from: CalendarDate, to: CalendarDate): number {
  return workingMinutesBetween(shiftCalendar(ctx, technician), atMinute(from, 0), atMinute(to, 0));
}

export function describeShift(shift: ShiftDefinition): string {
  return `${formatClock(shift.startMinute)}–${formatClock(shift.endMinute)}`;
}

export interface RosterEntry {
  technicianId: string;
  name: string;
  region: string;
  shift: ShiftPattern;
  startMinute: number;
  endMinute: number;
  hours: string;
}

/** Technicians working on `date`, earliest shift first and then by name. */
export function rosterFor(ctx: AppContext, date: CalendarDate, region?: string): RosterEntry[] {
  if (!isWorkingDay(businessCalendar(ctx), date)) return [];
  const pool = region === undefined ? activeTechnicians(ctx) : techniciansInRegion(ctx, region);
  return pool
    .filter((technician) => isAvailable(technician, date))
    .map((technician) => {
      const shift = shiftOf(ctx, technician);
      return {
        technicianId: technician.id,
        name: technician.name,
        region: technician.region,
        shift: shiftPatternOf(technician),
        startMinute: shift.startMinute,
        endMinute: shift.endMinute,
        hours: describeShift(shift),
      };
    })
    .sort((left, right) => left.startMinute - right.startMinute || left.name.localeCompare(right.name));
}
'''

UTILISATION = r'''
import { activeTechnicians } from '../adapters/repositories/technicians.ts';
import { entriesForTechnician } from '../adapters/repositories/time-entries.ts';
import type { ShiftPattern } from '../config/schema.ts';
import type { AppContext } from '../context.ts';
import { roundToIncrement } from '../shared/rounding.ts';
import { atMinute } from '../shared/time.ts';
import type { CalendarDate } from '../shared/time.ts';
import { rosteredMinutes, shiftPatternOf } from '../technicians/shifts.ts';

export interface UtilisationRow {
  technicianId: string;
  name: string;
  shift: ShiftPattern;
  laborHours: number;
  travelHours: number;
  availableHours: number;
  utilisationPercent: number;
}

/**
 * Billable utilisation per technician: labour and travel time against the technician's rostered
 * shift hours in the period, in whole percent.
 */
export function utilisationReport(ctx: AppContext, from: CalendarDate, to: CalendarDate): UtilisationRow[] {
  return activeTechnicians(ctx)
    .map((technician) => {
      const availableMinutes = rosteredMinutes(ctx, technician, from, to);
      const entries = entriesForTechnician(ctx, technician.id, atMinute(from, 0), atMinute(to, 0)).filter(
        (entry) => entry.status === 'approved' || entry.status === 'submitted',
      );
      const labor = entries.filter((entry) => entry.kind === 'labor').reduce((sum, entry) => sum + entry.minutes, 0);
      const travel = entries.filter((entry) => entry.kind === 'travel').reduce((sum, entry) => sum + entry.minutes, 0);
      const ratio = availableMinutes === 0 ? 0 : ((labor + travel) / availableMinutes) * 100;
      return {
        technicianId: technician.id,
        name: technician.name,
        shift: shiftPatternOf(technician),
        laborHours: Math.round((labor / 60) * 10) / 10,
        travelHours: Math.round((travel / 60) * 10) / 10,
        availableHours: Math.round((availableMinutes / 60) * 10) / 10,
        utilisationPercent: roundToIncrement(ratio, 1, false),
      };
    })
    .sort((left, right) => right.utilisationPercent - left.utilisationPercent || left.name.localeCompare(right.name));
}
'''

CHANGE_SHIFT = r'''
/**
 * Moves a technician to another shift pattern. Booked visits from today onwards must fit the new
 * hours; dispatch reschedules them first.
 */
export function changeShift(ctx: AppContext, actorId: string, technicianId: string, shift: ShiftPattern): Technician {
  const technician = technicians(ctx).require(technicianId, 'technician');
  if (technician.shift === shift) return technician;
  const hours = ctx.config.scheduling.shifts[shift];
  const today = dateOf(ctx.clock.now());
  const outside = appointmentsForTechnician(ctx, technicianId)
    .filter((appointment) => appointment.status === 'booked' || appointment.status === 'confirmed')
    .filter((appointment) => dateOf(appointment.start) >= today)
    .filter((appointment) => minuteOfDay(appointment.start) < hours.startMinute || minuteOfDay(appointment.end) > hours.endMinute)
    .map((appointment) => appointment.id);
  if (outside.length > 0) {
    throw new ConflictError('appointments-outside-shift', `${outside.length} booked visits fall outside the ${shift} shift`, {
      appointmentIds: outside,
    });
  }
  const updated = technicians(ctx).update(technicianId, { shift });
  recordAudit(ctx, actorId, 'technician.shift', technicianId, { from: technician.shift ?? 'standard', to: shift });
  return updated;
}
'''

SHIFT_TESTS = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { technicians } from '../../src/adapters/repositories/technicians.ts';
import { createApp } from '../../src/app.ts';
import type { AppOptions } from '../../src/app.ts';
import type { AppContext } from '../../src/context.ts';
import { createTechnician, recordAbsence } from '../../src/technicians/service.ts';
import {
  describeShift,
  rosteredMinutes,
  rosterFor,
  shiftCalendar,
  shiftOf,
  shiftPatternOf,
} from '../../src/technicians/shifts.ts';

function fresh(options: AppOptions = {}) {
  const app = createApp({ now: '2026-03-02T08:00:00Z', ...options });
  return { app, ctx: app.ctx };
}

let hired = 0;

function hire(ctx: AppContext, name: string, extra: Record<string, unknown> = {}) {
  hired += 1;
  return createTechnician(ctx, 'admin', {
    name,
    email: `shift${hired}@fieldline.example`,
    region: 'metro-east',
    home: { lat: 52.5, lon: 13.38 },
    hourlyCostCents: 3100,
    ...extra,
  });
}

describe('shiftOf', () => {
  it('returns the hours of the standard shift by default', () => {
    const { ctx } = fresh();
    assert.deepEqual(shiftOf(ctx, hire(ctx, 'Robin Hale')), { startMinute: 480, endMinute: 1020 });
  });

  it('returns the hours of the early shift', () => {
    const { ctx } = fresh();
    assert.deepEqual(shiftOf(ctx, hire(ctx, 'Robin Hale', { shift: 'early' })), { startMinute: 420, endMinute: 900 });
  });

  it('treats records without a shift as standard', () => {
    const { ctx } = fresh();
    const stored = technicians(ctx).update(hire(ctx, 'Robin Hale', { shift: 'early' }).id, { shift: undefined });
    assert.equal(shiftPatternOf(stored), 'standard');
    assert.deepEqual(shiftOf(ctx, stored), { startMinute: 480, endMinute: 1020 });
  });

  it('follows configured shift hours', () => {
    const { ctx } = fresh({ config: { scheduling: { shifts: { early: { startMinute: 450 } } } } });
    assert.deepEqual(shiftOf(ctx, hire(ctx, 'Robin Hale', { shift: 'early' })), { startMinute: 450, endMinute: 900 });
  });
});

describe('shiftCalendar', () => {
  it('keeps the working days and holidays of the organisation', () => {
    const { ctx } = fresh();
    const calendar = shiftCalendar(ctx, hire(ctx, 'Robin Hale', { shift: 'early' }));
    assert.equal(calendar.dayStartMinute, 420);
    assert.equal(calendar.dayEndMinute, 900);
    assert.deepEqual(calendar.workingDays, [1, 2, 3, 4, 5]);
    assert.deepEqual(calendar.holidays, ctx.config.holidays);
  });
});

describe('rosteredMinutes', () => {
  it('counts five standard days in a week', () => {
    const { ctx } = fresh();
    assert.equal(rosteredMinutes(ctx, hire(ctx, 'Robin Hale'), '2026-03-02', '2026-03-09'), 2700);
  });

  it('counts five early days in a week', () => {
    const { ctx } = fresh();
    assert.equal(rosteredMinutes(ctx, hire(ctx, 'Robin Hale', { shift: 'early' }), '2026-03-02', '2026-03-09'), 2400);
  });

  it('skips public holidays', () => {
    const { ctx } = fresh();
    assert.equal(rosteredMinutes(ctx, hire(ctx, 'Robin Hale'), '2026-03-30', '2026-04-06'), 2160);
    assert.equal(rosteredMinutes(ctx, hire(ctx, 'Sam Ortiz', { shift: 'early' }), '2026-03-30', '2026-04-06'), 1920);
  });

  it('is zero for an empty period', () => {
    const { ctx } = fresh();
    assert.equal(rosteredMinutes(ctx, hire(ctx, 'Robin Hale'), '2026-03-02', '2026-03-02'), 0);
  });
});

describe('describeShift', () => {
  it('prints the shift hours', () => {
    assert.equal(describeShift({ startMinute: 420, endMinute: 900 }), '07:00–15:00');
    assert.equal(describeShift({ startMinute: 480, endMinute: 1020 }), '08:00–17:00');
  });
});

describe('rosterFor', () => {
  it('lists working technicians with the earliest shift first', () => {
    const { ctx } = fresh();
    hire(ctx, 'Blake Morgan');
    hire(ctx, 'Casey Lin', { shift: 'early' });
    hire(ctx, 'Avery Collins', { shift: 'early' });
    const roster = rosterFor(ctx, '2026-03-03');
    assert.deepEqual(roster.map((entry) => [entry.name, entry.shift, entry.hours]), [
      ['Avery Collins', 'early', '07:00–15:00'],
      ['Casey Lin', 'early', '07:00–15:00'],
      ['Blake Morgan', 'standard', '08:00–17:00'],
    ]);
  });

  it('leaves out absent and deactivated technicians', () => {
    const { ctx } = fresh();
    const away = hire(ctx, 'Blake Morgan');
    const gone = hire(ctx, 'Casey Lin');
    recordAbsence(ctx, 'admin', away.id, { from: '2026-03-03', to: '2026-03-04', kind: 'leave' });
    technicians(ctx).update(gone.id, { active: false });
    assert.deepEqual(rosterFor(ctx, '2026-03-03'), []);
    assert.deepEqual(rosterFor(ctx, '2026-03-05').map((entry) => entry.name), ['Blake Morgan']);
  });

  it('is empty at weekends and on public holidays', () => {
    const { ctx } = fresh();
    hire(ctx, 'Blake Morgan');
    assert.deepEqual(rosterFor(ctx, '2026-03-07'), []);
    assert.deepEqual(rosterFor(ctx, '2026-04-03'), []);
  });

  it('filters by region including roaming technicians', () => {
    const { ctx } = fresh();
    hire(ctx, 'Blake Morgan');
    hire(ctx, 'Casey Lin', { region: 'metro-west' });
    hire(ctx, 'Avery Collins', { region: 'metro-west', roaming: true });
    assert.deepEqual(rosterFor(ctx, '2026-03-03', 'metro-east').map((entry) => entry.name), ['Avery Collins', 'Blake Morgan']);
  });
});
'''


def patch_sources(ctx):
    ctx.add("src/technicians/shifts.ts", SHIFTS)
    ctx.replace("src/reports/utilisation.ts", UTILISATION)

    ctx.patch(
        "src/scheduling/slots.ts",
        "import { isAvailable } from '../technicians/service.ts';\n",
        "import { isAvailable } from '../technicians/service.ts';\nimport { shiftOf } from '../technicians/shifts.ts';\n",
    )
    ctx.patch(
        "src/scheduling/slots.ts",
        "  const shift = ctx.config.scheduling.shifts[technician.shift] ?? ctx.config.scheduling.shifts.standard;\n",
        "  const shift = shiftOf(ctx, technician);\n",
    )

    ctx.patch(
        "src/technicians/model.ts",
        "  shift: ShiftPattern;\n",
        "  /** Shift pattern that sets the technician's bookable and rostered hours. */\n  shift: ShiftPattern;\n",
    )

    service = "src/technicians/service.ts"
    ctx.patch(
        service,
        "import { technicians } from '../adapters/repositories/technicians.ts';\n",
        "import { appointmentsForTechnician } from '../adapters/repositories/appointments.ts';\n"
        "import { technicians } from '../adapters/repositories/technicians.ts';\n",
    )
    ctx.patch(
        service,
        "import { recordAudit } from '../adapters/store/audit.ts';\n",
        "import { recordAudit } from '../adapters/store/audit.ts';\n"
        "import type { ShiftPattern } from '../config/schema.ts';\n",
    )
    ctx.patch(
        service,
        "import type { CalendarDate } from '../shared/time.ts';\n",
        "import { dateOf, minuteOfDay } from '../shared/time.ts';\nimport type { CalendarDate } from '../shared/time.ts';\n",
    )
    ctx.patch(
        service,
        "\nexport function deactivateTechnician(",
        CHANGE_SHIFT + "\nexport function deactivateTechnician(",
    )

    repository = "src/adapters/repositories/technicians.ts"
    ctx.patch(
        repository,
        "import type { AppContext } from '../../context.ts';\n",
        "import type { ShiftPattern } from '../../config/schema.ts';\nimport type { AppContext } from '../../context.ts';\n",
    )
    ctx.append(repository, '''
export function techniciansOnShift(ctx: AppContext, shift: ShiftPattern): Technician[] {
  return activeTechnicians(ctx).filter((technician) => (technician.shift ?? 'standard') === shift);
}
''')

    capacity = "src/scheduling/capacity.ts"
    ctx.patch(
        capacity,
        "import type { AppContext } from '../context.ts';\n",
        "import type { ShiftPattern } from '../config/schema.ts';\nimport type { AppContext } from '../context.ts';\n",
    )
    ctx.patch(
        capacity,
        "import type { Technician } from '../technicians/model.ts';\n",
        "import type { Technician } from '../technicians/model.ts';\nimport { shiftPatternOf } from '../technicians/shifts.ts';\n",
    )
    ctx.patch(capacity, "  date: CalendarDate;\n  appointments: number;\n",
              "  date: CalendarDate;\n  shift: ShiftPattern;\n  appointments: number;\n")
    ctx.patch(capacity, "    date,\n    appointments: live.length,\n",
              "    date,\n    shift: shiftPatternOf(technician),\n    appointments: live.length,\n")

    board = "src/dispatch/board.ts"
    ctx.patch(
        board,
        "import type { AppContext } from '../context.ts';\n",
        "import type { ShiftPattern } from '../config/schema.ts';\nimport type { AppContext } from '../context.ts';\n",
    )
    ctx.patch(
        board,
        "import type { CalendarDate } from '../shared/time.ts';\n",
        "import type { CalendarDate } from '../shared/time.ts';\n"
        "import { describeShift, shiftOf, shiftPatternOf } from '../technicians/shifts.ts';\n",
    )
    ctx.patch(board, "  name: string;\n  load: DailyLoad;\n", "  name: string;\n  shift: string;\n  load: DailyLoad;\n")
    ctx.patch(
        board,
        "export function dispatchBoard(ctx: AppContext, date: CalendarDate, region?: string): DispatchBoard {",
        "/** Work waiting for dispatch and one column per technician; `shift` limits the columns to one shift pattern. */\n"
        "export function dispatchBoard(ctx: AppContext, date: CalendarDate, region?: string, shift?: ShiftPattern): DispatchBoard {",
    )
    ctx.patch(
        board,
        "  const columns = roster\n    .map((technician) => ({\n      technicianId: technician.id,\n      name: technician.name,\n",
        "  const columns = roster\n    .filter((technician) => shift === undefined || shiftPatternOf(technician) === shift)\n"
        "    .map((technician) => ({\n      technicianId: technician.id,\n      name: technician.name,\n"
        "      shift: describeShift(shiftOf(ctx, technician)),\n",
    )

    routes = "src/api/routes/technicians.ts"
    ctx.patch(
        routes,
        "import {\n  createTechnician,\n  deactivateTechnician,\n  recordAbsence,\n  updateCertifications,\n} from '../../technicians/service.ts';\n",
        "import {\n  changeShift,\n  createTechnician,\n  deactivateTechnician,\n  recordAbsence,\n  updateCertifications,\n} from '../../technicians/service.ts';\n"
        "import { rosterFor, SHIFT_PATTERNS, shiftPatternOf } from '../../technicians/shifts.ts';\n",
    )
    ctx.patch(
        routes,
        "import { created, ok, optionalQuery, param, queryInteger } from '../http.ts';\n",
        "import { created, ok, optionalQuery, param, queryDate, queryInteger } from '../http.ts';\n",
    )
    ctx.patch(
        routes,
        "export function registerTechnicianRoutes(router: Router): void {\n",
        "const shiftInput = object({ shift: oneOf(SHIFT_PATTERNS) });\n\n"
        "const shiftFilter = object({ shift: optional(oneOf(SHIFT_PATTERNS)) });\n\n"
        "export function registerTechnicianRoutes(router: Router): void {\n",
    )
    ctx.patch(
        routes,
        "    const items = region === undefined ? activeTechnicians(ctx) : techniciansInRegion(ctx, region);\n    return ok({ items });\n  });\n",
        "    const { shift } = parseInput(shiftFilter, { shift: optionalQuery(request, 'shift') });\n"
        "    const pool = region === undefined ? activeTechnicians(ctx) : techniciansInRegion(ctx, region);\n"
        "    const items = shift === undefined ? pool : pool.filter((technician) => shiftPatternOf(technician) === shift);\n"
        "    return ok({ items });\n  });\n\n"
        "  router.get('/technicians/roster', (ctx, request) => {\n"
        "    authorize(request.actor, 'jobs:schedule');\n"
        "    const date = queryDate(request, 'date', dateOf(ctx.clock.now()));\n"
        "    return ok({ date, items: rosterFor(ctx, date, optionalQuery(request, 'region')) });\n  });\n\n"
        "  router.put('/technicians/:id/shift', (ctx, request) => {\n"
        "    authorize(request.actor, 'technicians:write');\n"
        "    const { shift } = parseInput(shiftInput, request.body);\n"
        "    return ok(changeShift(ctx, request.actor.id, param(request, 'id'), shift));\n  });\n",
    )

    dispatch = "src/api/routes/dispatch.ts"
    ctx.patch(
        dispatch,
        "import { dateOf } from '../../shared/time.ts';\n",
        "import { dateOf } from '../../shared/time.ts';\n"
        "import { object, oneOf, optional, parseInput } from '../../shared/validate.ts';\n"
        "import { SHIFT_PATTERNS } from '../../technicians/shifts.ts';\n",
    )
    ctx.patch(
        dispatch,
        "export function registerDispatchRoutes(router: Router): void {\n",
        "const boardFilter = object({ shift: optional(oneOf(SHIFT_PATTERNS)) });\n\n"
        "export function registerDispatchRoutes(router: Router): void {\n",
    )
    ctx.patch(
        dispatch,
        "    return ok(dispatchBoard(ctx, date, optionalQuery(request, 'region')));\n",
        "    const { shift } = parseInput(boardFilter, { shift: optionalQuery(request, 'shift') });\n"
        "    return ok(dispatchBoard(ctx, date, optionalQuery(request, 'region'), shift));\n",
    )

    reports = "src/api/routes/reports.ts"
    ctx.patch(
        reports,
        "import { atMinute } from '../../shared/time.ts';\n",
        "import { atMinute } from '../../shared/time.ts';\n"
        "import { object, oneOf, optional, parseInput } from '../../shared/validate.ts';\n"
        "import { SHIFT_PATTERNS } from '../../technicians/shifts.ts';\n",
    )
    ctx.patch(
        reports,
        "export function registerReportRoutes(router: Router): void {\n",
        "const utilisationFilter = object({ shift: optional(oneOf(SHIFT_PATTERNS)) });\n\n"
        "export function registerReportRoutes(router: Router): void {\n",
    )
    ctx.patch(
        reports,
        "    return ok({ items: utilisationReport(ctx, queryDate(request, 'from'), queryDate(request, 'to')) });\n",
        "    const { shift } = parseInput(utilisationFilter, { shift: optionalQuery(request, 'shift') });\n"
        "    const rows = utilisationReport(ctx, queryDate(request, 'from'), queryDate(request, 'to'));\n"
        "    return ok({ items: shift === undefined ? rows : rows.filter((row) => row.shift === shift) });\n",
    )


def patch_tests(ctx):
    ctx.add("tests/technicians/shifts.test.ts", SHIFT_TESTS)

    world = "tests/support/world.ts"
    ctx.patch(world, "  skills?: string[];\n  home?: { lat: number; lon: number };\n}\n",
              "  skills?: string[];\n  home?: { lat: number; lon: number };\n  shift?: string;\n}\n")
    ctx.patch(world, "    certifications: (seed.skills ?? []).map((skill) => ({ skill })),\n  });\n",
              "    certifications: (seed.skills ?? []).map((skill) => ({ skill })),\n"
              "    ...(seed.shift === undefined ? {} : { shift: seed.shift }),\n  });\n")

    defaults = "tests/config/defaults.test.ts"
    ctx.patch(
        defaults,
        "  it('weights the dispatch criteria to a hundred', () => {\n",
        "  it('opens the scheduling day for the early shift', () => {\n"
        "    const { scheduling } = DEFAULT_CONFIG;\n"
        "    assert.deepEqual(scheduling.shifts, {\n"
        "      early: { startMinute: 420, endMinute: 900 },\n"
        "      standard: { startMinute: 480, endMinute: 1020 },\n"
        "    });\n"
        "    assert.equal(scheduling.defaultShift, 'standard');\n"
        "    assert.equal(Math.min(...Object.values(scheduling.shifts).map((shift) => shift.startMinute)), scheduling.dayStartMinute);\n"
        "  });\n\n"
        "  it('weights the dispatch criteria to a hundred', () => {\n",
    )

    schema = "tests/config/schema.test.ts"
    ctx.patch(
        schema,
        "    ['a lowercase currency', { organization: { currency: 'eur' } }, 'organization.currency must be an ISO code'],\n",
        "    ['a lowercase currency', { organization: { currency: 'eur' } }, 'organization.currency must be an ISO code'],\n"
        "    ['a shift before the scheduling day', { scheduling: { shifts: { early: { startMinute: 360 } } } }, 'scheduling.shifts.early must fall within the scheduling day'],\n"
        "    ['a shift after the scheduling day', { scheduling: { shifts: { standard: { endMinute: 1080 } } } }, 'scheduling.shifts.standard must fall within the scheduling day'],\n"
        "    ['a shift shorter than a slot', { scheduling: { shifts: { early: { startMinute: 420, endMinute: 435 } } } }, 'scheduling.shifts.early must be at least one slot long'],\n"
        "    ['a default shift that does not exist', { scheduling: { defaultShift: 'night' as never } }, 'scheduling.defaultShift must name a configured shift'],\n",
    )
    ctx.patch(
        schema,
        "  it('accepts a custom but consistent configuration', () => {\n",
        "  it('accepts shifts that use the whole scheduling day', () => {\n"
        "    const problems = problemsFor({\n"
        "      scheduling: { dayStartMinute: 360, shifts: { early: { startMinute: 360, endMinute: 840 } }, defaultShift: 'early' },\n"
        "    });\n"
        "    assert.deepEqual(problems, []);\n"
        "  });\n\n"
        "  it('accepts a custom but consistent configuration', () => {\n",
    )

    service = "tests/technicians/service.test.ts"
    ctx.patch(
        service,
        "import { createTechnician, deactivateTechnician } from '../../src/technicians/service.ts';\n",
        "import { appointments } from '../../src/adapters/repositories/appointments.ts';\n"
        "import { changeShift, createTechnician, deactivateTechnician } from '../../src/technicians/service.ts';\n",
    )
    ctx.patch(
        service,
        "    assert.equal(technician.maxDailyAppointments, undefined);\n",
        "    assert.equal(technician.maxDailyAppointments, undefined);\n    assert.equal(technician.shift, 'standard');\n",
    )
    ctx.patch(
        service,
        "  it('records an audit entry', () => {\n",
        "  it('keeps a requested shift', () => {\n"
        "    const { ctx } = fresh();\n"
        "    assert.equal(createTechnician(ctx, 'admin', input({ shift: 'early' })).shift, 'early');\n"
        "  });\n\n"
        "  it('uses the configured default shift', () => {\n"
        "    const app = createApp({ now: '2026-03-02T08:00:00Z', config: { scheduling: { defaultShift: 'early' } } });\n"
        "    assert.equal(createTechnician(app.ctx, 'admin', input()).shift, 'early');\n"
        "  });\n\n"
        "  it('records an audit entry', () => {\n",
    )
    ctx.patch(
        service,
        "    ['a malformed expiry date', { certifications: [{ skill: 'gas-safe', expiresOn: '31/01/2027' }] }, 'certifications'],\n",
        "    ['a malformed expiry date', { certifications: [{ skill: 'gas-safe', expiresOn: '31/01/2027' }] }, 'certifications'],\n"
        "    ['an unknown shift', { shift: 'night' }, 'shift'],\n",
    )
    ctx.patch(
        service,
        "describe('deactivateTechnician', () => {\n",
        r'''describe('changeShift', () => {
  const visit = (ctx: AppContext, technicianId: string, start: string, end: string, status = 'booked') =>
    appointments(ctx).insert({
      id: ctx.ids.next('APT'),
      jobId: 'JOB-000001',
      technicianId,
      start,
      end,
      windowStart: start,
      windowEnd: end,
      travelMinutes: 10,
      status: status as never,
      createdAt: '2026-03-01T12:00:00Z',
    });

  it('moves the technician and records the change', () => {
    const { ctx } = fresh();
    const technician = createTechnician(ctx, 'admin', input());
    const moved = changeShift(ctx, 'dispatch', technician.id, 'early');
    assert.equal(moved.shift, 'early');
    assert.equal(technicians(ctx).get(technician.id)?.shift, 'early');
    const entry = auditTrail(ctx, technician.id).at(-1)!;
    assert.equal(entry.action, 'technician.shift');
    assert.equal(entry.actorId, 'dispatch');
    assert.deepEqual(entry.details, { from: 'standard', to: 'early' });
  });

  it('leaves an unchanged shift alone', () => {
    const { ctx } = fresh();
    const technician = createTechnician(ctx, 'admin', input());
    changeShift(ctx, 'dispatch', technician.id, 'standard');
    assert.equal(auditTrail(ctx, technician.id).length, 1);
  });

  it('refuses while booked visits fall outside the new hours', () => {
    const { ctx } = fresh();
    const technician = createTechnician(ctx, 'admin', input());
    const late = visit(ctx, technician.id, '2026-03-03T15:00:00Z', '2026-03-03T16:00:00Z');
    visit(ctx, technician.id, '2026-03-03T09:00:00Z', '2026-03-03T10:00:00Z');
    assert.throws(
      () => changeShift(ctx, 'dispatch', technician.id, 'early'),
      (error: any) => error.code === 'appointments-outside-shift' && error.status === 409 && error.details.appointmentIds.join() === late.id,
    );
    assert.equal(technicians(ctx).get(technician.id)?.shift, 'standard');
  });

  it('counts confirmed visits that end after the new shift', () => {
    const { ctx } = fresh();
    const technician = createTechnician(ctx, 'admin', input());
    visit(ctx, technician.id, '2026-03-04T14:30:00Z', '2026-03-04T15:30:00Z', 'confirmed');
    assert.throws(() => changeShift(ctx, 'dispatch', technician.id, 'early'), (error: any) => error.status === 409);
  });

  it('ignores earlier days and visits that no longer take place', () => {
    const { ctx } = fresh();
    const technician = createTechnician(ctx, 'admin', input());
    visit(ctx, technician.id, '2026-02-27T16:00:00Z', '2026-02-27T17:00:00Z');
    visit(ctx, technician.id, '2026-03-03T16:00:00Z', '2026-03-03T17:00:00Z', 'cancelled');
    visit(ctx, technician.id, '2026-03-03T15:00:00Z', '2026-03-03T16:00:00Z', 'missed');
    assert.equal(changeShift(ctx, 'dispatch', technician.id, 'early').shift, 'early');
  });

  it('accepts visits that end when the new shift ends', () => {
    const { ctx } = fresh();
    const technician = createTechnician(ctx, 'admin', input({ shift: 'early' }));
    visit(ctx, technician.id, '2026-03-03T07:00:00Z', '2026-03-03T08:00:00Z');
    visit(ctx, technician.id, '2026-03-03T14:00:00Z', '2026-03-03T15:00:00Z');
    assert.throws(() => changeShift(ctx, 'dispatch', technician.id, 'standard'), (error: any) => error.status === 409);
    appointments(ctx).remove('APT-000001');
    assert.equal(changeShift(ctx, 'dispatch', technician.id, 'standard').shift, 'standard');
  });
});

describe('deactivateTechnician', () => {
''',
    )
    ctx.patch(
        service,
        "import { createApp } from '../../src/app.ts';\n",
        "import { createApp } from '../../src/app.ts';\nimport type { AppContext } from '../../src/context.ts';\n",
    )

    slots = "tests/scheduling/slots.test.ts"
    ctx.patch(
        slots,
        "    assert.deepEqual(workingBounds(ctx, away, '2026-03-04'), [480, 1020]);\n  });\n",
        "    assert.deepEqual(workingBounds(ctx, away, '2026-03-04'), [480, 1020]);\n  });\n\n"
        "  it('uses the hours of the early shift', () => {\n"
        "    const { app, ctx } = fresh();\n"
        "    const early = technicians(ctx).require(seedTechnician(app, { shift: 'early' }).id, 'technician');\n"
        "    assert.deepEqual(workingBounds(ctx, early, '2026-03-02'), [420, 900]);\n"
        "  });\n",
    )
    ctx.patch(
        slots,
        "  it('shortens the list for longer visits', () => {\n",
        "  it('offers the hours of the early shift', () => {\n"
        "    const { app, ctx, destination } = fresh();\n"
        "    const early = technicians(ctx).require(seedTechnician(app, { shift: 'early' }).id, 'technician');\n"
        "    const slots = availableSlots(ctx, early, '2026-03-03', 60, destination, new Map());\n"
        "    assert.equal(slots.length, 15);\n"
        "    assert.equal(slots[0].start, '2026-03-03T07:00:00Z');\n"
        "    assert.equal(slots.at(-1)?.end, '2026-03-03T15:00:00Z');\n"
        "  });\n\n"
        "  it('shortens the list for longer visits', () => {\n",
    )

    capacity = "tests/scheduling/capacity.test.ts"
    ctx.patch(capacity, "      date: '2026-03-02',\n      appointments: 0,\n",
              "      date: '2026-03-02',\n      shift: 'standard',\n      appointments: 0,\n")
    ctx.patch(
        capacity,
        "  it('adds booked and travel minutes', () => {\n",
        "  it('measures an early shift against its own hours', () => {\n"
        "    const world = fresh();\n"
        "    const early = technicians(world.ctx).require(seedTechnician(world.app, { shift: 'early' }).id, 'technician');\n"
        "    const load = dailyLoad(world.ctx, early, '2026-03-03');\n"
        "    assert.equal(load.shift, 'early');\n"
        "    assert.equal(load.availableMinutes, 480);\n"
        "  });\n\n"
        "  it('adds booked and travel minutes', () => {\n",
    )

    windows = "tests/scheduling/windows.test.ts"
    ctx.patch(
        windows,
        "  it('aligns starts on a window boundary to that window', () => {\n",
        "  it('starts the first window of the day with the scheduling day', () => {\n"
        "    assert.deepEqual(arrivalWindow(ctx(), '2026-03-02T07:00:00Z'), {\n"
        "      windowStart: '2026-03-02T07:00:00Z',\n"
        "      windowEnd: '2026-03-02T09:00:00Z',\n"
        "    });\n"
        "    assert.deepEqual(arrivalWindow(ctx(), '2026-03-02T07:30:00Z'), {\n"
        "      windowStart: '2026-03-02T07:00:00Z',\n"
        "      windowEnd: '2026-03-02T09:00:00Z',\n"
        "    });\n"
        "  });\n\n"
        "  it('aligns starts on a window boundary to that window', () => {\n",
    )

    booking = "tests/scheduling/service.test.ts"
    ctx.patch(
        booking,
        "  it('rejects unavailable slots', () => {\n",
        "  it('books an early-shift technician from seven', () => {\n"
        "    const early = seedTechnician(scene.app, { shift: 'early' });\n"
        "    const appointment = book(scene, open(scene).id, at('2026-03-03', '07:00'), early.id);\n"
        "    assert.equal(appointment.windowStart, at('2026-03-03', '07:00'));\n"
        "    assert.equal(appointment.windowEnd, at('2026-03-03', '09:00'));\n"
        "  });\n\n"
        "  it('keeps standard-shift technicians from being booked before eight', () => {\n"
        "    assert.throws(\n"
        "      () => book(scene, open(scene).id, at('2026-03-03', '07:30')),\n"
        "      (error: any) => error.code === 'slot-unavailable' && error.status === 409,\n"
        "    );\n"
        "  });\n\n"
        "  it('rejects unavailable slots', () => {\n",
    )

    utilisation = "tests/reports/utilisation.test.ts"
    ctx.patch(
        utilisation,
        "const dayMinutes = () => scene.ctx.config.scheduling.dayEndMinute - scene.ctx.config.scheduling.dayStartMinute;\n"
        "const workdays = (count: number) => Math.round((count * dayMinutes()) / 6) / 10;\n"
        "const share = (minutes: number, days: number) => Math.round((minutes / (days * dayMinutes())) * 100);\n",
        "const shiftMinutes = (pattern: 'early' | 'standard' = 'standard') => {\n"
        "  const shift = scene.ctx.config.scheduling.shifts[pattern];\n"
        "  return shift.endMinute - shift.startMinute;\n"
        "};\n"
        "const workdays = (count: number, pattern: 'early' | 'standard' = 'standard') => Math.round((count * shiftMinutes(pattern)) / 6) / 10;\n"
        "const share = (minutes: number, days: number) => Math.round((minutes / (days * shiftMinutes())) * 100);\n",
    )
    ctx.patch(utilisation, "  it('counts the standard working time of the period', () => {\n",
              "  it('counts the rostered standard shifts of the period', () => {\n")
    ctx.patch(
        utilisation,
        "      technicianId: scene.tech,\n      name: scene.technician.name,\n      laborHours",
        "      technicianId: scene.tech,\n      name: scene.technician.name,\n      shift: 'standard',\n      laborHours",
        count=2,
    )
    ctx.patch(
        utilisation,
        "describe('booked time', () => {\n",
        "describe('shifts', () => {\n"
        "  it('counts the hours of an early shift', () => {\n"
        "    const early = seedTechnician(scene.app, { shift: 'early' }).id as string;\n"
        "    const row = rowOf(early);\n"
        "    assert.equal(row.shift, 'early');\n"
        "    assert.equal(row.availableHours, workdays(5, 'early'));\n"
        "    assert.equal(row.availableHours, 40);\n"
        "  });\n\n"
        "  it('rates the same work higher on the shorter shift', () => {\n"
        "    const early = seedTechnician(scene.app, { shift: 'early' }).id as string;\n"
        "    book(scene.tech, 'labor', '09:00', '13:00');\n"
        "    book(early, 'labor', '09:00', '13:00');\n"
        "    assert.equal(rowOf(scene.tech).utilisationPercent, 9);\n"
        "    assert.equal(rowOf(early).utilisationPercent, 10);\n"
        "  });\n\n"
        "  it('follows a shift change', () => {\n"
        "    call(scene.app, 'PUT', `/technicians/${scene.tech}/shift`, 'dispatch', { shift: 'early' });\n"
        "    assert.equal(rowOf(scene.tech).availableHours, 40);\n"
        "  });\n"
        "});\n\n"
        "describe('booked time', () => {\n",
    )

    api_reports = "tests/api/reports.test.ts"
    ctx.patch(
        api_reports,
        "const dayMinutes = () => world.app.ctx.config.scheduling.dayEndMinute - world.app.ctx.config.scheduling.dayStartMinute;\n",
        "const shiftMinutes = () => {\n"
        "  const { standard } = world.app.ctx.config.scheduling.shifts;\n"
        "  return standard.endMinute - standard.startMinute;\n"
        "};\n",
    )
    ctx.patch(api_reports, "row.availableHours === (5 * dayMinutes()) / 60",
              "row.availableHours === (5 * shiftMinutes()) / 60")
    ctx.patch(api_reports, "Math.round((180 / (5 * dayMinutes())) * 100)",
              "Math.round((180 / (5 * shiftMinutes())) * 100)")

    ctx.patch(
        api_reports,
        "  it('requires a period', () => {\n    expectStatus(world.app, 422, 'GET', '/reports/utilisation', 'admin');\n",
        "  it('measures early-shift technicians against their own hours', () => {\n"
        "    const early = seedTechnician(world.app, { name: 'Avery Early', shift: 'early' });\n"
        "    const row = get(`/reports/utilisation?${RANGE}`).items.find((item: any) => item.technicianId === early.id);\n"
        "    assert.equal(row.shift, 'early');\n"
        "    assert.equal(row.availableHours, 40);\n"
        "  });\n\n"
        "  it('filters by shift', () => {\n"
        "    const early = seedTechnician(world.app, { name: 'Avery Early', shift: 'early' });\n"
        "    assert.deepEqual(get(`/reports/utilisation?${RANGE}&shift=early`).items.map((row: any) => row.technicianId), [early.id]);\n"
        "    assert.deepEqual(get(`/reports/utilisation?${RANGE}&shift=standard`).items.map((row: any) => row.technicianId), [world.technician.id]);\n"
        "    expectStatus(world.app, 422, 'GET', `/reports/utilisation?${RANGE}&shift=night`, 'admin');\n"
        "  });\n\n"
        "  it('requires a period', () => {\n    expectStatus(world.app, 422, 'GET', '/reports/utilisation', 'admin');\n",
    )

    permissions = "tests/api/security/permissions.test.ts"
    ctx.patch(
        permissions,
        "      ['POST', `/technicians/${MISSING}/deactivate`], ['GET', '/technicians/certifications/expiring'],\n",
        "      ['POST', `/technicians/${MISSING}/deactivate`], ['GET', '/technicians/certifications/expiring'],\n"
        "      ['PUT', `/technicians/${MISSING}/shift`],\n",
    )
    ctx.patch(
        permissions,
        "      ['GET', '/dispatch/board'], ['GET', '/dispatch/capacity'], ['GET', '/dispatch/rules'],",
        "      ['GET', '/technicians/roster'], ['GET', '/dispatch/board'], ['GET', '/dispatch/capacity'], ['GET', '/dispatch/rules'],",
    )

    appointments = "tests/api/appointments.test.ts"
    ctx.patch(
        appointments,
        "  it('honours an explicit duration', () => {\n",
        "  it('lists slots from seven for an early-shift technician', () => {\n"
        "    const early = seedTechnician(world.app, { shift: 'early' });\n"
        "    const body = call(world.app, 'GET', `/technicians/${early.id}/slots?date=2026-03-03&jobId=${job.id}`, 'dispatch');\n"
        "    assert.equal(body.slots.length, 15);\n"
        "    assert.equal(body.slots[0].start, '2026-03-03T07:00:00Z');\n"
        "    assert.equal(body.slots.at(-1).end, '2026-03-03T15:00:00Z');\n"
        "  });\n\n"
        "  it('honours an explicit duration', () => {\n",
    )

    api = "tests/api/technicians.test.ts"
    ctx.patch(
        api,
        "  it('lets dispatch create technicians', () => {\n",
        "  it('puts new technicians on the standard shift unless told otherwise', () => {\n"
        "    assert.equal(expectStatus(world.app, 201, 'POST', '/technicians', 'admin', payload()).shift, 'standard');\n"
        "    const early = expectStatus(world.app, 201, 'POST', '/technicians', 'admin', payload({ email: 'early@fieldline.example', shift: 'early' }));\n"
        "    assert.equal(early.shift, 'early');\n"
        "  });\n\n"
        "  it('rejects an unknown shift', () => {\n"
        "    const body = expectStatus(world.app, 422, 'POST', '/technicians', 'admin', payload({ shift: 'night' }));\n"
        "    assert.deepEqual(body.error.details.issues.map((issue: any) => issue.path), ['shift']);\n"
        "  });\n\n"
        "  it('lets dispatch create technicians', () => {\n",
    )
    ctx.patch(
        api,
        "  it('hides deactivated technicians', () => {\n",
        "  it('filters by shift', () => {\n"
        "    const early = call(world.app, 'POST', '/technicians', 'admin', payload({ shift: 'early' }));\n"
        "    assert.deepEqual(call(world.app, 'GET', '/technicians?shift=early', 'dispatch').items.map((item: any) => item.id), [early.id]);\n"
        "    assert.deepEqual(call(world.app, 'GET', '/technicians?shift=standard', 'dispatch').items.map((item: any) => item.id), [world.technician.id]);\n"
        "    expectStatus(world.app, 422, 'GET', '/technicians?shift=night', 'dispatch');\n"
        "  });\n\n"
        "  it('hides deactivated technicians', () => {\n",
    )
    ctx.patch(
        api,
        "describe('GET /technicians/certifications/expiring', () => {\n",
        r'''describe('PUT /technicians/:id/shift', () => {
  const book = (start: string, technicianId = world.technician.id) => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    return call(world.app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId, start, durationMinutes: 60 });
  };

  it('moves a technician to the early shift', () => {
    const body = expectStatus(world.app, 200, 'PUT', `/technicians/${world.technician.id}/shift`, 'dispatch', { shift: 'early' });
    assert.equal(body.shift, 'early');
    assert.deepEqual(call(world.app, 'GET', '/technicians?shift=early', 'dispatch').items.map((item: any) => item.id), [world.technician.id]);
  });

  it('opens early slots after the change', () => {
    call(world.app, 'PUT', `/technicians/${world.technician.id}/shift`, 'dispatch', { shift: 'early' });
    assert.equal(book('2026-03-03T07:00:00Z').windowStart, '2026-03-03T07:00:00Z');
  });

  it('refuses while booked visits fall outside the new hours', () => {
    const late = book('2026-03-03T15:30:00Z');
    book('2026-03-03T09:00:00Z');
    const body = expectStatus(world.app, 409, 'PUT', `/technicians/${world.technician.id}/shift`, 'dispatch', { shift: 'early' });
    assert.equal(body.error.code, 'appointments-outside-shift');
    assert.deepEqual(body.error.details.appointmentIds, [late.id]);
  });

  it('accepts the change once the late visit is rescheduled', () => {
    const late = book('2026-03-03T15:30:00Z');
    call(world.app, 'POST', `/appointments/${late.id}/reschedule`, 'dispatch', { start: '2026-03-03T11:00:00Z' });
    expectStatus(world.app, 200, 'PUT', `/technicians/${world.technician.id}/shift`, 'dispatch', { shift: 'early' });
  });

  it('rejects unknown shifts', () => {
    const body = expectStatus(world.app, 422, 'PUT', `/technicians/${world.technician.id}/shift`, 'dispatch', { shift: 'night' });
    assert.equal(body.error.code, 'invalid-input');
  });

  it('answers unknown technicians with 404', () => {
    expectStatus(world.app, 404, 'PUT', '/technicians/TEC-999999/shift', 'dispatch', { shift: 'early' });
  });

  it('is limited to technician managers', () => {
    for (const actor of ['billing', 'warehouse', world.technician.id]) {
      expectStatus(world.app, 403, 'PUT', `/technicians/${world.technician.id}/shift`, actor, { shift: 'early' });
    }
  });
});

describe('GET /technicians/roster', () => {
  it('lists the technicians working on a day with their hours', () => {
    const early = call(world.app, 'POST', '/technicians', 'admin', payload({ name: 'Avery Early', shift: 'early' }));
    const roster = call(world.app, 'GET', '/technicians/roster?date=2026-03-03', 'dispatch');
    assert.equal(roster.date, '2026-03-03');
    assert.deepEqual(roster.items.map((entry: any) => [entry.technicianId, entry.hours]), [
      [early.id, '07:00–15:00'],
      [world.technician.id, '08:00–17:00'],
    ]);
  });

  it('defaults to the current date and is empty at weekends', () => {
    assert.equal(call(world.app, 'GET', '/technicians/roster', 'dispatch').date, '2026-03-02');
    assert.deepEqual(call(world.app, 'GET', '/technicians/roster?date=2026-03-07', 'dispatch').items, []);
  });

  it('is limited to roles that schedule work', () => {
    expectStatus(world.app, 403, 'GET', '/technicians/roster', 'billing');
    expectStatus(world.app, 403, 'GET', '/technicians/roster', world.technician.id);
  });
});

describe('GET /technicians/certifications/expiring', () => {
''',
    )

    board = "tests/dispatch/board.test.ts"
    ctx.patch(
        board,
        "  it('lists new work orders as unassigned with their age', () => {\n",
        "  it('shows the shift hours of each technician', () => {\n"
        "    const early = seedTechnician(scene.app, { name: 'Avery Early', shift: 'early' });\n"
        "    const columns = dispatchBoard(scene.ctx, MONDAY).columns;\n"
        "    assert.equal(columns.find((column) => column.technicianId === early.id)?.shift, '07:00–15:00');\n"
        "    assert.equal(columns.find((column) => column.technicianId === scene.tech)?.shift, '08:00–17:00');\n"
        "    assert.equal(columns.find((column) => column.technicianId === early.id)?.load.availableMinutes, 480);\n"
        "  });\n\n"
        "  it('limits the columns to one shift', () => {\n"
        "    const early = seedTechnician(scene.app, { name: 'Avery Early', shift: 'early' });\n"
        "    open(scene);\n"
        "    const board = dispatchBoard(scene.ctx, MONDAY, undefined, 'early');\n"
        "    assert.deepEqual(board.columns.map((column) => column.technicianId), [early.id]);\n"
        "    assert.equal(board.unassigned.length, 1);\n"
        "  });\n\n"
        "  it('lists new work orders as unassigned with their age', () => {\n",
    )

    dispatch = "tests/api/dispatch.test.ts"
    ctx.patch(
        dispatch,
        "  it('rejects a malformed date', () => {\n    expectStatus(world.app, 422, 'GET', '/dispatch/board?date=02-03-2026', 'dispatch');\n  });\n",
        "  it('rejects a malformed date', () => {\n    expectStatus(world.app, 422, 'GET', '/dispatch/board?date=02-03-2026', 'dispatch');\n  });\n\n"
        "  it('filters the columns by shift', () => {\n"
        "    const early = seedTechnician(world.app, { shift: 'early' });\n"
        "    const board = call(world.app, 'GET', '/dispatch/board?shift=early', 'dispatch');\n"
        "    assert.deepEqual(board.columns.map((column: any) => column.technicianId), [early.id]);\n"
        "    assert.equal(board.columns[0].shift, '07:00–15:00');\n"
        "    expectStatus(world.app, 422, 'GET', '/dispatch/board?shift=late', 'dispatch');\n"
        "  });\n",
    )


def generate(ctx):
    if not ctx.early:
        return
    patch_sources(ctx)
    patch_tests(ctx)
