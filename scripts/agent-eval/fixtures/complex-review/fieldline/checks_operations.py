"""Tests for timesheets and inventory: services, calculations and their API routes."""

from .checks_kit import WITH_SETUP, Spec, register
from .checks_workorders import scene_spec

WORLD = "tests/support/world.ts"
TS = "src/timesheets"
INV = "src/inventory"

CLOCK = r'''
function world() {
  const created = createWorld();
  return { ...created, ctx: created.app.ctx, tech: created.technician.id as string };
}
type Scene = ReturnType<typeof world>;

const job = (scene: Scene) => openWorkOrder(scene.app, scene.customer.id, scene.site.id, 'general');

describe('clockIn', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = world();
  });

  it('opens a labor entry against a work order', () => {
    const target = job(scene);
    const entry = clockIn(scene.ctx, scene.tech, 'labor', target.id);
    assert.equal(entry.id, 'TIME-000001');
    assert.equal(entry.status, 'open');
    assert.equal(entry.kind, 'labor');
    assert.equal(entry.jobId, target.id);
    assert.equal(entry.minutes, 0);
    assert.equal(entry.start, at(MONDAY, '08:00'));
    assert.equal(entry.end, undefined);
  });

  it('allows breaks and admin time without a work order', () => {
    assert.equal(clockIn(scene.ctx, scene.tech, 'break').jobId, undefined);
    clockOut(scene.ctx, 'TIME-000001');
    assert.equal(clockIn(scene.ctx, scene.tech, 'admin').kind, 'admin');
  });

  it('requires a work order for labor and travel', () => {
    for (const kind of ['labor', 'travel'] as const) {
      assert.throws(
        () => clockIn(scene.ctx, scene.tech, kind),
        (error: any) => error.code === 'job-required' && error.status === 422,
        kind,
      );
    }
  });

  it('rejects unknown technicians and work orders', () => {
    assert.throws(() => clockIn(scene.ctx, 'TEC-999999', 'break'), (error: any) => error.status === 404);
    assert.throws(() => clockIn(scene.ctx, scene.tech, 'labor', 'WO-999999'), (error: any) => error.status === 404);
  });

  it('allows only one open entry per technician', () => {
    clockIn(scene.ctx, scene.tech, 'break');
    assert.throws(
      () => clockIn(scene.ctx, scene.tech, 'admin'),
      (error: any) => error.code === 'already-clocked-in' && error.status === 409,
    );
  });

  it('lets other technicians clock in meanwhile', () => {
    const other = seedTechnician(scene.app, { skills: ['plumbing'] });
    clockIn(scene.ctx, scene.tech, 'break');
    assert.equal(clockIn(scene.ctx, other.id, 'break').technicianId, other.id);
  });

  const roundedStarts: Array<[string, string]> = [
    ['09:01', '09:00'],
    ['09:02', '09:00'],
    ['09:03', '09:05'],
    ['09:04', '09:05'],
    ['09:06', '09:05'],
    ['09:07', '09:05'],
    ['09:08', '09:10'],
    ['09:58', '10:00'],
  ];
  for (const [clock, expected] of roundedStarts) {
    it(`rounds a clock-in at ${clock} to ${expected}`, () => {
      scene.app.clock.set(at(MONDAY, clock));
      assert.equal(clockIn(scene.ctx, scene.tech, 'break').start, at(MONDAY, expected));
    });
  }
});

describe('clockOut', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = world();
    scene.app.clock.set(at(MONDAY, '09:00'));
  });

  it('closes the entry and submits it with the elapsed minutes', () => {
    const target = job(scene);
    const entry = clockIn(scene.ctx, scene.tech, 'labor', target.id);
    scene.app.clock.advance(95);
    const closed = clockOut(scene.ctx, entry.id, undefined, 'Replaced the valve');
    assert.equal(closed.status, 'submitted');
    assert.equal(closed.minutes, 95);
    assert.equal(closed.end, at(MONDAY, '10:35'));
    assert.equal(closed.note, 'Replaced the valve');
  });

  it('rounds the end to five minutes', () => {
    const entry = clockIn(scene.ctx, scene.tech, 'break');
    scene.app.clock.advance(31);
    assert.equal(clockOut(scene.ctx, entry.id).minutes, 30);
    const second = clockIn(scene.ctx, scene.tech, 'break');
    scene.app.clock.advance(33);
    assert.equal(clockOut(scene.ctx, second.id).minutes, 35);
  });

  it('records the distance on travel entries only', () => {
    const target = job(scene);
    const travel = clockIn(scene.ctx, scene.tech, 'travel', target.id);
    scene.app.clock.advance(30);
    const closed = clockOut(scene.ctx, travel.id, 14);
    assert.equal(closed.km, 14);
    const labor = clockIn(scene.ctx, scene.tech, 'labor', target.id);
    scene.app.clock.advance(30);
    assert.throws(() => clockOut(scene.ctx, labor.id, 5), (error: any) => error.code === 'km-on-travel-only');
    assert.equal(timeEntries(scene.ctx).require(labor.id, 'time entry').status, 'open');
  });

  it('does not close an entry twice', () => {
    const entry = clockIn(scene.ctx, scene.tech, 'break');
    scene.app.clock.advance(15);
    clockOut(scene.ctx, entry.id);
    assert.throws(() => clockOut(scene.ctx, entry.id), (error: any) => error.code === 'not-open' && error.status === 409);
  });

  it('allows clocking in again after clocking out', () => {
    const entry = clockIn(scene.ctx, scene.tech, 'break');
    scene.app.clock.advance(15);
    clockOut(scene.ctx, entry.id);
    assert.equal(clockIn(scene.ctx, scene.tech, 'admin').id, 'TIME-000002');
  });

  it('rejects entries longer than the shift limit', () => {
    const entry = clockIn(scene.ctx, scene.tech, 'break');
    scene.app.clock.advance(13 * 60);
    assert.throws(() => clockOut(scene.ctx, entry.id), (error: any) => error.code === 'entry-too-long');
    assert.equal(timeEntries(scene.ctx).require(entry.id, 'time entry').status, 'open');
  });

  it('accepts entries exactly at the shift limit', () => {
    const entry = clockIn(scene.ctx, scene.tech, 'break');
    scene.app.clock.advance(12 * 60);
    assert.equal(clockOut(scene.ctx, entry.id).minutes, 720);
  });

  it('never produces negative minutes when the clock moves back', () => {
    const entry = clockIn(scene.ctx, scene.tech, 'break');
    scene.app.clock.set(at(MONDAY, '08:30'));
    assert.equal(clockOut(scene.ctx, entry.id).minutes, 0);
  });

  it('follows the configured rounding step', () => {
    const custom = createWorld({ config: { timesheets: { clockRoundingMinutes: 15 } } });
    custom.app.clock.set(at(MONDAY, '09:06'));
    const entry = clockIn(custom.app.ctx, custom.technician.id, 'break');
    assert.equal(entry.start, at(MONDAY, '09:00'));
    custom.app.clock.advance(44);
    assert.equal(clockOut(custom.app.ctx, entry.id).end, at(MONDAY, '09:45'));
  });
});
'''

ENTRIES = r'''
function world() {
  const created = createWorld();
  const target = openWorkOrder(created.app, created.customer.id, created.site.id, 'general');
  return { ...created, ctx: created.app.ctx, tech: created.technician.id as string, jobId: target.id as string };
}
type Scene = ReturnType<typeof world>;

const entry = (scene: Scene, overrides: Record<string, unknown> = {}) => ({
  technicianId: scene.tech,
  jobId: scene.jobId,
  kind: 'labor',
  start: at(MONDAY, '09:00'),
  end: at(MONDAY, '10:30'),
  ...overrides,
});

describe('logEntry', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = world();
  });

  it('stores a submitted entry with computed minutes', () => {
    const logged = logEntry(scene.ctx, entry(scene));
    assert.equal(logged.id, 'TIME-000001');
    assert.equal(logged.status, 'submitted');
    assert.equal(logged.minutes, 90);
    assert.equal(logged.end, at(MONDAY, '10:30'));
    assert.equal(logged.km, undefined);
  });

  it('keeps distance and notes on travel entries', () => {
    const logged = logEntry(scene.ctx, entry(scene, { kind: 'travel', km: 23.4, note: 'Van trip computer' }));
    assert.equal(logged.km, 23.4);
    assert.equal(logged.note, 'Van trip computer');
  });

  it('accepts breaks and admin time without a work order', () => {
    const logged = logEntry(scene.ctx, entry(scene, { kind: 'break', jobId: undefined }));
    assert.equal(logged.kind, 'break');
    assert.equal(logged.jobId, undefined);
  });

  const invalid: Array<[string, Record<string, unknown>, string]> = [
    ['an entry that ends where it starts', { end: at(MONDAY, '09:00') }, 'invalid-period'],
    ['an entry that ends before it starts', { end: at(MONDAY, '08:45') }, 'invalid-period'],
    ['distance on labor time', { km: 5 }, 'km-on-travel-only'],
    ['labor without a work order', { jobId: undefined }, 'job-required'],
    ['travel without a work order', { kind: 'travel', jobId: undefined }, 'job-required'],
    ['an entry beyond the shift limit', { end: at(MONDAY, '22:00') }, 'entry-too-long'],
    ['an unknown kind', { kind: 'nap' }, 'invalid-input'],
    ['a negative distance', { kind: 'travel', km: -1 }, 'invalid-input'],
    ['a malformed start', { start: '2026-03-02 09:00' }, 'invalid-input'],
  ];
  for (const [label, overrides, code] of invalid) {
    it(`rejects ${label}`, () => {
      assert.throws(() => logEntry(scene.ctx, entry(scene, overrides)), (error: any) => error.code === code && error.status === 422);
      assert.equal(timeEntries(scene.ctx).count(() => true), 0);
    });
  }

  it('rejects unknown technicians and work orders', () => {
    assert.throws(() => logEntry(scene.ctx, entry(scene, { technicianId: 'TEC-999999' })), (error: any) => error.status === 404);
    assert.throws(() => logEntry(scene.ctx, entry(scene, { jobId: 'WO-999999' })), (error: any) => error.status === 404);
  });

  it('rejects overlapping entries of the same technician', () => {
    const first = logEntry(scene.ctx, entry(scene));
    for (const [start, end] of [['09:30', '10:00'], ['08:00', '09:30'], ['10:00', '11:00'], ['08:00', '12:00']]) {
      assert.throws(
        () => logEntry(scene.ctx, entry(scene, { start: at(MONDAY, start), end: at(MONDAY, end) })),
        (error: any) => error.code === 'overlapping-entry' && error.status === 409 && error.details.entryId === first.id,
        `${start}-${end}`,
      );
    }
  });

  it('accepts adjacent entries and entries of other technicians', () => {
    logEntry(scene.ctx, entry(scene));
    assert.equal(logEntry(scene.ctx, entry(scene, { start: at(MONDAY, '10:30'), end: at(MONDAY, '11:30') })).minutes, 60);
    assert.equal(logEntry(scene.ctx, entry(scene, { start: at(MONDAY, '08:00'), end: at(MONDAY, '09:00') })).minutes, 60);
    const other = seedTechnician(scene.app, { skills: ['plumbing'] });
    assert.equal(logEntry(scene.ctx, entry(scene, { technicianId: other.id })).technicianId, other.id);
  });
});

describe('correctEntry', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = world();
  });

  it('changes minutes and moves the end accordingly', () => {
    const logged = logEntry(scene.ctx, entry(scene));
    const corrected = correctEntry(scene.ctx, logged.id, 75, 'Travel was shorter');
    assert.equal(corrected.minutes, 75);
    assert.equal(corrected.end, at(MONDAY, '10:15'));
    assert.equal(corrected.note, 'Travel was shorter');
    assert.equal(corrected.status, 'submitted');
  });

  it('keeps the old note for a blank correction note', () => {
    const logged = logEntry(scene.ctx, entry(scene, { note: 'Original' }));
    assert.equal(correctEntry(scene.ctx, logged.id, 60, '   ').note, 'Original');
  });

  it('resubmits rejected entries', () => {
    const logged = logEntry(scene.ctx, entry(scene));
    rejectEntry(scene.ctx, 'dispatch', logged.id, 'Wrong duration');
    assert.equal(correctEntry(scene.ctx, logged.id, 60, 'Fixed').status, 'submitted');
  });

  it('refuses approved entries', () => {
    const logged = logEntry(scene.ctx, entry(scene));
    approveEntries(scene.ctx, 'dispatch', [logged.id]);
    assert.throws(() => correctEntry(scene.ctx, logged.id, 60, 'Late fix'), (error: any) => error.code === 'entry-approved' && error.status === 409);
  });

  it('refuses invalid minutes', () => {
    const logged = logEntry(scene.ctx, entry(scene));
    for (const minutes of [-1, 1.5, Number.NaN]) {
      assert.throws(() => correctEntry(scene.ctx, logged.id, minutes, 'Bad'), (error: any) => error.code === 'invalid-minutes');
    }
    assert.equal(correctEntry(scene.ctx, logged.id, 0, 'Nothing worked').minutes, 0);
  });

  it('rejects unknown entries', () => {
    assert.throws(() => correctEntry(scene.ctx, 'TIME-999999', 30, 'Fix'), (error: any) => error.status === 404);
  });
});
'''

OVERTIME = r'''
const config = resolveConfig().timesheets;

let counter = 0;
const entry = (date: string, minutes: number, kind: TimeEntry['kind'] = 'labor', status: TimeEntry['status'] = 'approved'): TimeEntry => {
  counter += 1;
  return {
    id: `TIME-${counter.toString().padStart(6, '0')}`,
    technicianId: 'TEC-000001',
    jobId: 'WO-000001',
    kind,
    start: `${date}T09:00:00Z`,
    end: `${date}T18:00:00Z`,
    minutes,
    status,
  };
};

describe('dailyTotals', () => {
  it('returns nothing without entries', () => {
    assert.deepEqual(dailyTotals([], config), []);
  });

  it('sums the minutes of one day', () => {
    const [day] = dailyTotals([entry('2026-03-02', 120), entry('2026-03-02', 90)], config);
    assert.deepEqual(day, { date: '2026-03-02', workedMinutes: 210, overtimeMinutes: 0 });
  });

  it('counts travel and admin time but not breaks', () => {
    const [day] = dailyTotals([entry('2026-03-02', 200), entry('2026-03-02', 40, 'travel'), entry('2026-03-02', 20, 'admin'), entry('2026-03-02', 30, 'break')], config);
    assert.equal(day.workedMinutes, 260);
  });

  it('ignores rejected entries', () => {
    const [day] = dailyTotals([entry('2026-03-02', 120), entry('2026-03-02', 300, 'labor', 'rejected')], config);
    assert.equal(day.workedMinutes, 120);
  });

  it('pays no overtime up to the daily threshold', () => {
    assert.equal(dailyTotals([entry('2026-03-02', 480)], config)[0].overtimeMinutes, 0);
  });

  it('rounds overtime down to whole quarter hours', () => {
    const cases: Array<[number, number]> = [[494, 0], [495, 15], [509, 15], [510, 30], [540, 60], [600, 120]];
    for (const [worked, overtime] of cases) {
      assert.equal(dailyTotals([entry('2026-03-02', worked)], config)[0].overtimeMinutes, overtime, String(worked));
    }
  });

  it('separates days and sorts them chronologically', () => {
    const days = dailyTotals([entry('2026-03-04', 60), entry('2026-03-02', 500), entry('2026-03-03', 90)], config);
    assert.deepEqual(days.map((day) => [day.date, day.workedMinutes, day.overtimeMinutes]), [
      ['2026-03-02', 500, 15],
      ['2026-03-03', 90, 0],
      ['2026-03-04', 60, 0],
    ]);
  });

  it('follows a configured daily threshold and step', () => {
    const custom = { ...config, overtimeAfterMinutes: 420, overtimeStepMinutes: 30 };
    assert.equal(dailyTotals([entry('2026-03-02', 480)], custom)[0].overtimeMinutes, 60);
    assert.equal(dailyTotals([entry('2026-03-02', 449)], custom)[0].overtimeMinutes, 0);
  });
});

describe('weeklyOvertime', () => {
  const day = (date: string, workedMinutes: number, overtimeMinutes = 0) => ({ date, workedMinutes, overtimeMinutes });

  it('is zero for a normal week', () => {
    const days = ['02', '03', '04', '05', '06'].map((date) => day(`2026-03-${date}`, 480));
    assert.equal(weeklyOvertime(days, config), 0);
  });

  it('uses the summed daily overtime when it is larger', () => {
    const days = [day('2026-03-02', 600, 120), day('2026-03-03', 540, 60), day('2026-03-04', 300)];
    assert.equal(weeklyOvertime(days, config), 180);
  });

  it('uses the time beyond the weekly threshold when it is larger', () => {
    const days = ['02', '03', '04', '05', '06'].map((date) => day(`2026-03-${date}`, 495));
    assert.equal(weeklyOvertime(days, config), 75);
  });

  it('rounds the weekly figure down to quarter hours', () => {
    const days = [day('2026-03-02', 480), day('2026-03-03', 480), day('2026-03-04', 480), day('2026-03-05', 480), day('2026-03-06', 480), day('2026-03-07', 29)];
    assert.equal(weeklyOvertime(days, config), 15);
    const short = [...days.slice(0, 5), day('2026-03-07', 14)];
    assert.equal(weeklyOvertime(short, config), 0);
  });

  it('never pays the same hours twice', () => {
    const days = [day('2026-03-02', 720, 240), day('2026-03-03', 720, 240), day('2026-03-04', 720, 240), day('2026-03-05', 720, 240), day('2026-03-06', 720, 240)];
    assert.equal(weeklyOvertime(days, config), 1200);
  });

  it('is zero without days', () => {
    assert.equal(weeklyOvertime([], config), 0);
  });
});
'''

APPROVALS = r'''
function world() {
  const created = createWorld();
  const target = openWorkOrder(created.app, created.customer.id, created.site.id, 'general');
  const tech = created.technician.id as string;
  const log = (start: string, end: string, kind = 'labor') =>
    logEntry(created.app.ctx, { technicianId: tech, jobId: target.id, kind, start: at(MONDAY, start), end: at(MONDAY, end) });
  return { ...created, ctx: created.app.ctx, tech, log };
}
type Scene = ReturnType<typeof world>;

describe('approveEntries', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = world();
  });

  it('approves submitted entries and audits the batch', () => {
    const first = scene.log('09:00', '10:00');
    const second = scene.log('10:00', '11:00');
    const approved = approveEntries(scene.ctx, 'dispatch', [first.id, second.id]);
    assert.deepEqual(approved.map((entry) => entry.status), ['approved', 'approved']);
    const [audit] = auditTrail(scene.ctx, `${first.id},${second.id}`);
    assert.equal(audit.action, 'timesheet.approved');
    assert.deepEqual(audit.details, { count: 2 });
  });

  it('refuses the whole batch when one entry is not submitted', () => {
    const first = scene.log('09:00', '10:00');
    const second = scene.log('10:00', '11:00');
    rejectEntry(scene.ctx, 'dispatch', second.id, 'Wrong job');
    assert.throws(
      () => approveEntries(scene.ctx, 'dispatch', [first.id, second.id]),
      (error: any) => error.code === 'not-submitted' && error.status === 409 && error.message.includes(second.id),
    );
    assert.equal(timeEntries(scene.ctx).require(first.id, 'time entry').status, 'submitted');
  });

  it('refuses already approved entries', () => {
    const entry = scene.log('09:00', '10:00');
    approveEntries(scene.ctx, 'dispatch', [entry.id]);
    assert.throws(() => approveEntries(scene.ctx, 'dispatch', [entry.id]), (error: any) => error.code === 'not-submitted');
  });

  it('forbids approving your own time', () => {
    const entry = scene.log('09:00', '10:00');
    assert.throws(
      () => approveEntries(scene.ctx, scene.tech, [entry.id]),
      (error: any) => error.code === 'self-approval' && error.status === 409,
    );
  });

  it('rejects unknown entries', () => {
    assert.throws(() => approveEntries(scene.ctx, 'dispatch', ['TIME-999999']), (error: any) => error.status === 404);
  });
});

describe('rejectEntry', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = world();
  });

  it('stores the trimmed reason', () => {
    const entry = scene.log('09:00', '10:00');
    const rejected = rejectEntry(scene.ctx, 'dispatch', entry.id, '  Wrong job  ');
    assert.equal(rejected.status, 'rejected');
    assert.equal(rejected.rejectedReason, 'Wrong job');
    assert.equal(auditTrail(scene.ctx, entry.id)[0].action, 'timesheet.rejected');
  });

  it('requires a reason', () => {
    const entry = scene.log('09:00', '10:00');
    assert.throws(() => rejectEntry(scene.ctx, 'dispatch', entry.id, '   '), (error: any) => error.code === 'reason-required');
  });

  it('only rejects submitted entries', () => {
    const entry = scene.log('09:00', '10:00');
    rejectEntry(scene.ctx, 'dispatch', entry.id, 'Wrong job');
    assert.throws(() => rejectEntry(scene.ctx, 'dispatch', entry.id, 'Again'), (error: any) => error.code === 'not-submitted');
    const open = clockIn(scene.ctx, scene.tech, 'break');
    assert.throws(() => rejectEntry(scene.ctx, 'dispatch', open.id, 'Too early'), (error: any) => error.code === 'not-submitted');
  });
});

describe('pendingApprovals', () => {
  it('lists submitted entries oldest first', () => {
    const scene = world();
    const late = scene.log('13:00', '14:00');
    const early = scene.log('09:00', '10:00');
    const middle = scene.log('11:00', '12:00');
    assert.deepEqual(pendingApprovals(scene.ctx).map((entry) => entry.id), [early.id, middle.id, late.id]);
  });

  it('drops approved and rejected entries', () => {
    const scene = world();
    const approved = scene.log('09:00', '10:00');
    const rejected = scene.log('10:00', '11:00');
    const pending = scene.log('11:00', '12:00');
    approveEntries(scene.ctx, 'dispatch', [approved.id]);
    rejectEntry(scene.ctx, 'dispatch', rejected.id, 'Wrong job');
    assert.deepEqual(pendingApprovals(scene.ctx).map((entry) => entry.id), [pending.id]);
  });

  it('is empty when nothing was logged', () => {
    assert.deepEqual(pendingApprovals(world().ctx), []);
  });
});
'''

SUMMARY = r'''
function world() {
  const created = createWorld();
  const target = openWorkOrder(created.app, created.customer.id, created.site.id, 'general');
  const tech = created.technician.id as string;
  const log = (date: string, start: string, end: string, kind = 'labor', extra: Record<string, unknown> = {}) =>
    logEntry(created.app.ctx, { technicianId: tech, jobId: target.id, kind, start: at(date, start), end: at(date, end), ...extra });
  return { ...created, ctx: created.app.ctx, tech, log };
}

describe('weeklySummary', () => {
  it('reports hours per kind in quarter hours', () => {
    const scene = world();
    scene.log(MONDAY, '09:00', '12:00');
    scene.log(MONDAY, '12:00', '12:45', 'break');
    scene.log(MONDAY, '13:00', '14:30', 'travel');
    scene.log('2026-03-03', '09:00', '10:15', 'admin');
    const summary = weeklySummary(scene.ctx, scene.tech, MONDAY);
    assert.deepEqual(summary.hours, { labor: 3, travel: 1.5, break: 0.75, admin: 1.25 });
    assert.equal(summary.overtimeHours, 0);
    assert.equal(summary.weekStart, MONDAY);
    assert.equal(summary.days.length, 2);
  });

  it('is empty for a week without entries', () => {
    const scene = world();
    const summary = weeklySummary(scene.ctx, scene.tech, MONDAY);
    assert.deepEqual(summary.hours, { labor: 0, travel: 0, break: 0, admin: 0 });
    assert.deepEqual(summary.days, []);
  });

  it('only includes entries inside the week', () => {
    const scene = world();
    scene.log('2026-03-01', '09:00', '10:00');
    scene.log(MONDAY, '09:00', '10:00');
    scene.log('2026-03-08', '09:00', '10:00');
    scene.log('2026-03-09', '09:00', '10:00');
    assert.equal(weeklySummary(scene.ctx, scene.tech, MONDAY).hours.labor, 2);
    assert.equal(weeklySummary(scene.ctx, scene.tech, '2026-03-09').hours.labor, 1);
  });

  it('excludes rejected and still open entries', () => {
    const scene = world();
    const rejected = scene.log(MONDAY, '09:00', '10:00');
    rejectEntry(scene.ctx, 'dispatch', rejected.id, 'Wrong job');
    scene.log(MONDAY, '10:00', '11:00');
    scene.app.clock.set(at('2026-03-03', '09:00'));
    clockIn(scene.ctx, scene.tech, 'break');
    const summary = weeklySummary(scene.ctx, scene.tech, MONDAY);
    assert.equal(summary.hours.labor, 1);
    assert.equal(summary.hours.break, 0);
  });

  it('counts overtime in hours', () => {
    const scene = world();
    scene.log(MONDAY, '07:00', '18:30');
    const summary = weeklySummary(scene.ctx, scene.tech, MONDAY);
    assert.equal(summary.hours.labor, 11.5);
    assert.equal(summary.overtimeHours, 3.5);
  });

  it('only includes the technician requested', () => {
    const scene = world();
    scene.log(MONDAY, '09:00', '10:00');
    const other = seedTechnician(scene.app, { skills: ['plumbing'] });
    assert.equal(weeklySummary(scene.ctx, other.id, MONDAY).hours.labor, 0);
  });
});

describe('mileageClaim', () => {
  const travel = (technicianId: string, km: number | undefined, status: TimeEntry['status'] = 'approved', kind: TimeEntry['kind'] = 'travel'): TimeEntry => ({
    id: 'TIME-000001',
    technicianId,
    jobId: 'WO-000001',
    kind,
    start: '2026-03-02T09:00:00Z',
    end: '2026-03-02T10:00:00Z',
    minutes: 60,
    km,
    status,
  });

  it('adds up approved trips at the configured rate', () => {
    const claim = mileageClaim([travel('TEC-000001', 12), travel('TEC-000001', 8)], 'TEC-000001', 30);
    assert.deepEqual(claim, { technicianId: 'TEC-000001', trips: 2, km: 20, amountCents: 600 });
  });

  it('rounds every trip to a whole kilometre before adding', () => {
    const claim = mileageClaim([travel('TEC-000001', 12.3), travel('TEC-000001', 7.2), travel('TEC-000001', 9.8)], 'TEC-000001', 30);
    assert.equal(claim.km, 12 + 7 + 10);
    assert.equal(claim.amountCents, 870);
  });

  it('ignores trips that are not approved travel of that technician', () => {
    const entries = [
      travel('TEC-000001', 10),
      travel('TEC-000001', 10, 'submitted'),
      travel('TEC-000001', 10, 'rejected'),
      travel('TEC-000001', 10, 'approved', 'labor'),
      travel('TEC-000002', 10),
      travel('TEC-000001', undefined),
    ];
    assert.deepEqual(mileageClaim(entries, 'TEC-000001', 30), { technicianId: 'TEC-000001', trips: 1, km: 10, amountCents: 300 });
  });

  it('claims nothing without trips', () => {
    assert.deepEqual(mileageClaim([], 'TEC-000001', 30), { technicianId: 'TEC-000001', trips: 0, km: 0, amountCents: 0 });
  });

  it('scales with the rate', () => {
    assert.equal(mileageClaim([travel('TEC-000001', 40)], 'TEC-000001', 25).amountCents, 1000);
    assert.equal(mileageClaim([travel('TEC-000001', 40)], 'TEC-000001', 0).amountCents, 0);
  });
});
'''

CATALOG = r'''
function fresh() {
  const app = createApp({ now: '2026-03-02T08:00:00Z' });
  return { app, ctx: app.ctx };
}

const part = (overrides: Record<string, unknown> = {}) => ({
  id: 'VLV-15MM',
  name: '15mm isolation valve',
  category: 'plumbing',
  unitCostCents: 450,
  listPriceCents: 900,
  ...overrides,
});

describe('addPart', () => {
  it('stores the part with defaults', () => {
    const { ctx } = fresh();
    const added = addPart(ctx, 'warehouse', part());
    assert.deepEqual(added, {
      id: 'VLV-15MM',
      name: '15mm isolation valve',
      category: 'plumbing',
      unitCostCents: 450,
      listPriceCents: 900,
      packSize: 1,
      reorderPoint: 0,
      reorderQuantity: 0,
      warrantyMonths: 12,
      discontinued: false,
    });
    assert.equal(parts(ctx).get('VLV-15MM')?.name, '15mm isolation valve');
  });

  it('keeps explicit pack, reorder and warranty settings', () => {
    const { ctx } = fresh();
    const added = addPart(ctx, 'warehouse', part({ packSize: 10, reorderPoint: 5, reorderQuantity: 20, warrantyMonths: 24 }));
    assert.equal(added.packSize, 10);
    assert.equal(added.reorderPoint, 5);
    assert.equal(added.reorderQuantity, 20);
    assert.equal(added.warrantyMonths, 24);
  });

  it('rejects duplicate skus', () => {
    const { ctx } = fresh();
    addPart(ctx, 'warehouse', part());
    assert.throws(() => addPart(ctx, 'warehouse', part()), (error: any) => error.code === 'duplicate-sku' && error.status === 409);
  });

  it('rejects a list price below cost but accepts equal prices', () => {
    const { ctx } = fresh();
    assert.throws(() => addPart(ctx, 'warehouse', part({ listPriceCents: 449 })), (error: any) => error.code === 'price-below-cost');
    assert.equal(addPart(ctx, 'warehouse', part({ listPriceCents: 450 })).listPriceCents, 450);
  });

  const invalid: Array<[string, Record<string, unknown>]> = [
    ['lower-case skus', { id: 'vlv-15mm' }],
    ['skus with spaces', { id: 'VLV 15MM' }],
    ['very short skus', { id: 'V1' }],
    ['a one-letter name', { name: 'V' }],
    ['a negative cost', { unitCostCents: -1 }],
    ['a fractional price', { listPriceCents: 9.5 }],
    ['a zero pack size', { packSize: 0 }],
    ['a pack size above 1000', { packSize: 1001 }],
    ['a warranty beyond ten years', { warrantyMonths: 121 }],
  ];
  for (const [label, overrides] of invalid) {
    it(`rejects ${label}`, () => {
      const { ctx } = fresh();
      assert.throws(() => addPart(ctx, 'warehouse', part(overrides)), (error: any) => error.code === 'invalid-input' && error.status === 422);
    });
  }

  it('audits the addition', () => {
    const { ctx } = fresh();
    addPart(ctx, 'warehouse', part());
    assert.equal(auditTrail(ctx, 'VLV-15MM')[0].action, 'part.added');
  });
});

describe('updateListPrice', () => {
  it('changes the price and audits the old and new value', () => {
    const { ctx } = fresh();
    addPart(ctx, 'warehouse', part());
    const updated = updateListPrice(ctx, 'admin', 'VLV-15MM', 1100);
    assert.equal(updated.listPriceCents, 1100);
    assert.deepEqual(auditTrail(ctx, 'VLV-15MM').at(-1)?.details, { from: 900, to: 1100 });
  });

  it('refuses prices below cost', () => {
    const { ctx } = fresh();
    addPart(ctx, 'warehouse', part());
    assert.throws(() => updateListPrice(ctx, 'admin', 'VLV-15MM', 449), (error: any) => error.code === 'price-below-cost');
    assert.throws(() => updateListPrice(ctx, 'admin', 'VLV-15MM', 600.5), (error: any) => error.code === 'price-below-cost');
    assert.equal(parts(ctx).get('VLV-15MM')?.listPriceCents, 900);
  });

  it('rejects unknown parts', () => {
    const { ctx } = fresh();
    assert.throws(() => updateListPrice(ctx, 'admin', 'NOPE-1', 1000), (error: any) => error.status === 404);
  });
});

describe('discontinuePart', () => {
  it('flags the part and clears its reorder settings', () => {
    const { ctx } = fresh();
    addPart(ctx, 'warehouse', part({ reorderPoint: 5, reorderQuantity: 20 }));
    const updated = discontinuePart(ctx, 'admin', 'VLV-15MM');
    assert.equal(updated.discontinued, true);
    assert.equal(updated.reorderPoint, 0);
    assert.equal(updated.reorderQuantity, 0);
    assert.equal(auditTrail(ctx, 'VLV-15MM').at(-1)?.action, 'part.discontinued');
  });

  it('hides the part from the category listing', () => {
    const { ctx } = fresh();
    addPart(ctx, 'warehouse', part());
    addPart(ctx, 'warehouse', part({ id: 'VLV-22MM', name: '22mm isolation valve' }));
    discontinuePart(ctx, 'admin', 'VLV-15MM');
    assert.deepEqual(partsInCategory(ctx, 'plumbing').map((entry) => entry.id), ['VLV-22MM']);
    assert.deepEqual(partsInCategory(ctx, 'electrical'), []);
  });
});
'''

STOCK = r'''
function fresh() {
  const app = createApp({ now: '2026-03-02T08:00:00Z' });
  const ctx = app.ctx;
  addPart(ctx, 'warehouse', {
    id: 'PUMP-CIRC',
    name: 'Circulation pump',
    category: 'heating',
    unitCostCents: 10000,
    listPriceCents: 18000,
    reorderPoint: 2,
    reorderQuantity: 6,
  });
  return { app, ctx };
}

describe('averageCost', () => {
  it('blends the current and received cost by quantity', () => {
    assert.equal(averageCost(10, 100, 10, 200), 150);
    assert.equal(averageCost(3, 100, 1, 200), 125);
    assert.equal(averageCost(1, 100, 3, 200), 175);
  });

  it('rounds to whole cents', () => {
    assert.equal(averageCost(3, 100, 2, 201), 140);
    assert.equal(averageCost(1, 100, 2, 101), 101);
  });

  it('takes the new cost when nothing was on hand', () => {
    assert.equal(averageCost(0, 100, 5, 240), 240);
  });

  it('treats negative stock as empty', () => {
    assert.equal(averageCost(-4, 100, 5, 240), 240);
  });

  it('takes the received cost when nothing is received', () => {
    assert.equal(averageCost(0, 100, 0, 240), 240);
  });
});

describe('receiveStock', () => {
  it('books the stock and creates a receipt', () => {
    const { ctx } = fresh();
    const receipt = receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 4, 10000, 'PO-1001');
    assert.equal(receipt.id, 'RCV-000001');
    assert.equal(receipt.reference, 'PO-1001');
    assert.equal(receipt.receivedAt, '2026-03-02T08:00:00Z');
    assert.equal(levelAt(ctx, 'WH-CENTRAL', 'PUMP-CIRC').onHand, 4);
    assert.equal(levelAt(ctx, 'WH-CENTRAL', 'PUMP-CIRC').reserved, 0);
  });

  it('re-averages the part cost over all locations', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 2, 10000);
    receiveStock(ctx, 'warehouse', 'VAN-000001', 'PUMP-CIRC', 2, 12000);
    assert.equal(parts(ctx).get('PUMP-CIRC')?.unitCostCents, 11000);
  });

  it('accumulates stock at a location', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 2, 10000);
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 3, 10000);
    assert.equal(levelAt(ctx, 'WH-CENTRAL', 'PUMP-CIRC').onHand, 5);
  });

  it('rejects invalid quantities and unknown parts', () => {
    const { ctx } = fresh();
    for (const quantity of [0, -1, 1.5]) {
      assert.throws(() => receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', quantity, 10000), (error: any) => error.code === 'invalid-quantity');
    }
    assert.throws(() => receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'NOPE-1', 1, 100), (error: any) => error.status === 404);
  });

  it('audits the receipt', () => {
    const { ctx } = fresh();
    const receipt = receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 2, 10000);
    const [entry] = auditTrail(ctx, receipt.id);
    assert.equal(entry.action, 'stock.received');
    assert.deepEqual(entry.details, { sku: 'PUMP-CIRC', quantity: 2, locationId: 'WH-CENTRAL' });
  });
});

describe('transferStock', () => {
  it('moves stock between locations', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 10000);
    transferStock(ctx, 'warehouse', 'WH-CENTRAL', 'VAN-000001', 'PUMP-CIRC', 2);
    assert.equal(levelAt(ctx, 'WH-CENTRAL', 'PUMP-CIRC').onHand, 3);
    assert.equal(levelAt(ctx, 'VAN-000001', 'PUMP-CIRC').onHand, 2);
  });

  it('rejects identical locations', () => {
    const { ctx } = fresh();
    assert.throws(() => transferStock(ctx, 'warehouse', 'WH-CENTRAL', 'WH-CENTRAL', 'PUMP-CIRC', 1), (error: any) => error.code === 'same-location');
  });

  it('refuses to move more than is available', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 2, 10000);
    assert.throws(
      () => transferStock(ctx, 'warehouse', 'WH-CENTRAL', 'VAN-000001', 'PUMP-CIRC', 3),
      (error: any) => error.code === 'insufficient-stock' && error.status === 409,
    );
    assert.equal(levelAt(ctx, 'WH-CENTRAL', 'PUMP-CIRC').onHand, 2);
  });

  it('does not move reserved stock', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 3, 10000);
    saveLevel(ctx, { ...levelAt(ctx, 'WH-CENTRAL', 'PUMP-CIRC'), reserved: 2 });
    assert.throws(() => transferStock(ctx, 'warehouse', 'WH-CENTRAL', 'VAN-000001', 'PUMP-CIRC', 2), (error: any) => error.code === 'insufficient-stock');
    transferStock(ctx, 'warehouse', 'WH-CENTRAL', 'VAN-000001', 'PUMP-CIRC', 1);
    assert.equal(levelAt(ctx, 'VAN-000001', 'PUMP-CIRC').onHand, 1);
  });
});

describe('adjustCount', () => {
  it('sets the counted quantity and audits the change', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 10, 10000);
    const level = adjustCount(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 8);
    assert.equal(level.onHand, 8);
    const entry = auditTrail(ctx, 'WH-CENTRAL:PUMP-CIRC')[0];
    assert.deepEqual(entry.details, { from: 10, to: 8 });
  });

  it('cannot count below the reserved quantity', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 10, 10000);
    saveLevel(ctx, { ...levelAt(ctx, 'WH-CENTRAL', 'PUMP-CIRC'), reserved: 4 });
    assert.throws(() => adjustCount(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 3), (error: any) => error.code === 'below-reserved');
    assert.equal(adjustCount(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 4).onHand, 4);
  });

  it('announces low stock at or below the reorder point', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 10, 10000);
    adjustCount(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 3);
    assert.equal(ctx.bus.recent(50).filter((event) => event.type === 'stock.low').length, 0);
    adjustCount(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 2);
    const [low] = ctx.bus.recent(50).filter((event) => event.type === 'stock.low');
    assert.equal(low.sku, 'PUMP-CIRC');
    assert.equal(low.available, 2);
  });
});
'''

RESERVATIONS = r'''
const fixture = () => {
  const scene = start();
  addPart(scene.ctx, 'warehouse', { id: 'PUMP-CIRC', name: 'Circulation pump', category: 'heating', unitCostCents: 10000, listPriceCents: 18000 });
  addPart(scene.ctx, 'warehouse', { id: 'GSK-KIT', name: 'Gasket kit', category: 'heating', unitCostCents: 300, listPriceCents: 700 });
  return scene;
};

describe('reservePartsForJob', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = fixture();
  });

  it('holds stock in the warehouse for an unassigned work order', () => {
    receiveStock(scene.ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 3, 10000);
    const job = open(scene);
    const [reservation] = reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'PUMP-CIRC', quantity: 2 }]);
    assert.equal(reservation.id, 'RES-000001');
    assert.equal(reservation.status, 'held');
    assert.equal(reservation.locationId, 'WH-CENTRAL');
    assert.equal(levelAt(scene.ctx, 'WH-CENTRAL', 'PUMP-CIRC').reserved, 2);
    assert.equal(eventsOf(scene, 'parts.reserved').length, 1);
  });

  it('prefers the van of the assigned technician', () => {
    receiveStock(scene.ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 3, 10000);
    receiveStock(scene.ctx, 'warehouse', 'VAN-000001', 'PUMP-CIRC', 1, 10000);
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    const [reservation] = reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'PUMP-CIRC', quantity: 1 }]);
    assert.equal(reservation.locationId, scene.technician.vanLocationId);
  });

  it('falls back to the warehouse when the van lacks stock', () => {
    receiveStock(scene.ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 3, 10000);
    receiveStock(scene.ctx, 'warehouse', scene.technician.vanLocationId, 'PUMP-CIRC', 1, 10000);
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    const [reservation] = reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'PUMP-CIRC', quantity: 2 }]);
    assert.equal(reservation.locationId, 'WH-CENTRAL');
  });

  it('backorders unavailable parts', () => {
    const job = open(scene);
    const [reservation] = reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'PUMP-CIRC', quantity: 1 }]);
    assert.equal(reservation.status, 'backordered');
    assert.equal(reservation.locationId, undefined);
    assert.equal(eventsOf(scene, 'parts.backordered').length, 1);
  });

  it('puts an assigned work order on hold when parts are backordered', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'PUMP-CIRC', quantity: 1 }]);
    const held = «requireJob»(scene.ctx, job.id);
    assert.equal(held.status, 'on_hold');
    assert.equal(held.holds.at(-1)?.reason, 'awaiting_parts');
  });

  it('leaves new work orders as they are when parts are backordered', () => {
    const job = open(scene);
    reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'PUMP-CIRC', quantity: 1 }]);
    assert.equal(«requireJob»(scene.ctx, job.id).status, 'new');
  });

  it('handles several lines independently', () => {
    receiveStock(scene.ctx, 'warehouse', 'WH-CENTRAL', 'GSK-KIT', 5, 300);
    const job = open(scene);
    const created = reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'GSK-KIT', quantity: 2 }, { sku: 'PUMP-CIRC', quantity: 1 }]);
    assert.deepEqual(created.map((reservation) => reservation.status), ['held', 'backordered']);
  });

  it('refuses discontinued parts, invalid quantities and unknown parts', () => {
    const job = open(scene);
    discontinuePart(scene.ctx, 'admin', 'GSK-KIT');
    assert.throws(() => reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'GSK-KIT', quantity: 1 }]), (error: any) => error.code === 'discontinued');
    for (const quantity of [0, -2, 1.5]) {
      assert.throws(() => reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'PUMP-CIRC', quantity }]), (error: any) => error.code === 'invalid-quantity');
    }
    assert.throws(() => reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'NOPE-1', quantity: 1 }]), (error: any) => error.status === 404);
  });
});

describe('releaseReservations', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = fixture();
  });

  it('frees held stock and counts the released reservations', () => {
    receiveStock(scene.ctx, 'warehouse', 'WH-CENTRAL', 'GSK-KIT', 5, 300);
    const job = open(scene);
    reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'GSK-KIT', quantity: 2 }, { sku: 'PUMP-CIRC', quantity: 1 }]);
    assert.equal(releaseReservations(scene.ctx, job.id), 2);
    assert.equal(levelAt(scene.ctx, 'WH-CENTRAL', 'GSK-KIT').reserved, 0);
    assert.deepEqual(reservationsForJob(scene.ctx, job.id), []);
  });

  it('is a no-op without reservations', () => {
    assert.equal(releaseReservations(scene.ctx, open(scene).id), 0);
  });
});

describe('inventory subscriptions', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = fixture();
  });

  it('releases reservations when the work order is cancelled', () => {
    receiveStock(scene.ctx, 'warehouse', 'WH-CENTRAL', 'GSK-KIT', 5, 300);
    const job = open(scene);
    reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'GSK-KIT', quantity: 2 }]);
    «cancelJob»(scene.ctx, 'dispatch', job.id, 'Customer withdrew the request');
    assert.equal(levelAt(scene.ctx, 'WH-CENTRAL', 'GSK-KIT').reserved, 0);
    assert.equal(levelAt(scene.ctx, 'WH-CENTRAL', 'GSK-KIT').onHand, 5);
  });

  it('consumes reserved stock when the work order completes', () => {
    receiveStock(scene.ctx, 'warehouse', 'WH-CENTRAL', 'GSK-KIT', 5, 300);
    const job = open(scene);
    reservePartsForJob(scene.ctx, 'dispatch', job.id, [{ sku: 'GSK-KIT', quantity: 2 }]);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    for (const item of Object.keys(«requireJob»(scene.ctx, job.id).checklist)) «recordChecklistItem»(scene.ctx, scene.tech, job.id, item, true);
    «completeJob»(scene.ctx, scene.tech, job.id, { resolution: 'Replaced the gasket on the pump', partsUsed: [{ sku: 'GSK-KIT', quantity: 2 }] });
    const level = levelAt(scene.ctx, 'WH-CENTRAL', 'GSK-KIT');
    assert.equal(level.onHand, 3);
    assert.equal(level.reserved, 0);
    assert.equal(eventsOf(scene, 'parts.consumed').length, 1);
  });

  it('takes unreserved parts from the technician van', () => {
    receiveStock(scene.ctx, 'warehouse', scene.technician.vanLocationId, 'GSK-KIT', 4, 300);
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    for (const item of Object.keys(«requireJob»(scene.ctx, job.id).checklist)) «recordChecklistItem»(scene.ctx, scene.tech, job.id, item, true);
    «completeJob»(scene.ctx, scene.tech, job.id, { resolution: 'Replaced the gasket on the pump', partsUsed: [{ sku: 'GSK-KIT', quantity: 3 }] });
    assert.equal(levelAt(scene.ctx, scene.technician.vanLocationId, 'GSK-KIT').onHand, 1);
  });
});
'''

REORDER = r'''
function fresh() {
  const app = createApp({ now: '2026-03-02T08:00:00Z' });
  const ctx = app.ctx;
  const add = (id: string, extra: Record<string, unknown> = {}) =>
    addPart(ctx, 'warehouse', { id, name: `Part ${id}`, category: 'heating', unitCostCents: 200, listPriceCents: 500, ...extra });
  return { app, ctx, add };
}

describe('reorderSuggestions', () => {
  it('suggests parts at or below their reorder point', () => {
    const { ctx, add } = fresh();
    add('PUMP-CIRC', { reorderPoint: 2, reorderQuantity: 6 });
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 200);
    assert.deepEqual(reorderSuggestions(ctx, 'WH-CENTRAL'), []);
    adjustCount(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 2);
    assert.deepEqual(reorderSuggestions(ctx, 'WH-CENTRAL'), [{ sku: 'PUMP-CIRC', locationId: 'WH-CENTRAL', available: 2, quantity: 6, packs: 6 }]);
  });

  it('rounds the quantity up to whole packs', () => {
    const { ctx, add } = fresh();
    add('SCRW-BOX', { reorderPoint: 5, reorderQuantity: 12, packSize: 10 });
    const [suggestion] = reorderSuggestions(ctx, 'WH-CENTRAL');
    assert.equal(suggestion.quantity, 20);
    assert.equal(suggestion.packs, 2);
  });

  it('adds open backorders to the suggested quantity, rounded up to packs', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    const part = (id: string, extra: Record<string, unknown>) =>
      addPart(ctx, 'warehouse', { id, name: `Part ${id}`, category: 'heating', unitCostCents: 200, listPriceCents: 500, reorderPoint: 0, reorderQuantity: 4, ...extra });
    part('PUMP-CIRC', {});
    part('SCRW-BOX', { packSize: 5 });
    const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    reservePartsForJob(ctx, 'dispatch', job.id, [{ sku: 'PUMP-CIRC', quantity: 3 }, { sku: 'SCRW-BOX', quantity: 3 }]);
    const byPart = new Map(reorderSuggestions(ctx, 'WH-CENTRAL').map((entry) => [entry.sku, entry]));
    assert.equal(byPart.get('PUMP-CIRC')?.quantity, 7);
    assert.equal(byPart.get('SCRW-BOX')?.quantity, 10);
    assert.equal(byPart.get('SCRW-BOX')?.packs, 2);
  });

  it('skips parts without a reorder quantity and discontinued parts', () => {
    const { ctx, add } = fresh();
    add('NO-REORDER');
    add('OLD-PART', { reorderPoint: 2, reorderQuantity: 4 });
    discontinuePart(ctx, 'admin', 'OLD-PART');
    assert.deepEqual(reorderSuggestions(ctx, 'WH-CENTRAL'), []);
  });

  it('sorts by availability and then by sku', () => {
    const { ctx, add } = fresh();
    add('BBB-1', { reorderPoint: 5, reorderQuantity: 5 });
    add('AAA-1', { reorderPoint: 5, reorderQuantity: 5 });
    add('CCC-1', { reorderPoint: 5, reorderQuantity: 5 });
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'BBB-1', 1, 200);
    assert.deepEqual(reorderSuggestions(ctx, 'WH-CENTRAL').map((entry) => entry.sku), ['AAA-1', 'CCC-1', 'BBB-1']);
  });

  it('is evaluated per location', () => {
    const { ctx, add } = fresh();
    add('PUMP-CIRC', { reorderPoint: 2, reorderQuantity: 6 });
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 10, 200);
    assert.equal(reorderSuggestions(ctx, 'WH-CENTRAL').length, 0);
    assert.equal(reorderSuggestions(ctx, 'VAN-000001').length, 1);
  });
});

describe('vanTopUp', () => {
  it('tops up to half of the recent usage, in whole packs', () => {
    const { ctx, add } = fresh();
    add('GSK-KIT', { packSize: 2 });
    assert.deepEqual(vanTopUp(ctx, 'VAN-000001', new Map([['GSK-KIT', 10]])), [{ sku: 'GSK-KIT', quantity: 4 }]);
  });

  it('never targets less than the safety stock', () => {
    const { ctx, add } = fresh();
    add('GSK-KIT');
    assert.deepEqual(vanTopUp(ctx, 'VAN-000001', new Map([['GSK-KIT', 1]])), [{ sku: 'GSK-KIT', quantity: 1 }]);
  });

  it('subtracts stock already in the van', () => {
    const { ctx, add } = fresh();
    add('GSK-KIT');
    receiveStock(ctx, 'warehouse', 'VAN-000001', 'GSK-KIT', 3, 200);
    assert.deepEqual(vanTopUp(ctx, 'VAN-000001', new Map([['GSK-KIT', 10]])), [{ sku: 'GSK-KIT', quantity: 2 }]);
    receiveStock(ctx, 'warehouse', 'VAN-000001', 'GSK-KIT', 2, 200);
    assert.deepEqual(vanTopUp(ctx, 'VAN-000001', new Map([['GSK-KIT', 10]])), []);
  });

  it('moves nothing when the shortfall is smaller than a pack', () => {
    const { ctx, add } = fresh();
    add('SCRW-BOX', { packSize: 10 });
    assert.deepEqual(vanTopUp(ctx, 'VAN-000001', new Map([['SCRW-BOX', 6]])), []);
  });

  it('skips unknown and discontinued parts', () => {
    const { ctx, add } = fresh();
    add('OLD-PART');
    discontinuePart(ctx, 'admin', 'OLD-PART');
    assert.deepEqual(vanTopUp(ctx, 'VAN-000001', new Map([['OLD-PART', 10], ['NOPE-1', 10]])), []);
  });

  it('sorts suggestions by sku', () => {
    const { ctx, add } = fresh();
    add('ZZZ-1');
    add('AAA-1');
    assert.deepEqual(vanTopUp(ctx, 'VAN-000001', new Map([['ZZZ-1', 4], ['AAA-1', 4]])).map((entry) => entry.sku), ['AAA-1', 'ZZZ-1']);
  });
});

describe('stockValuation', () => {
  it('is empty without stock', () => {
    assert.deepEqual(stockValuation(fresh().ctx), { lines: [], totalCents: 0 });
  });

  it('values stock at the current average cost', () => {
    const { ctx, add } = fresh();
    add('PUMP-CIRC', { unitCostCents: 10000, listPriceCents: 18000 });
    add('GSK-KIT', { unitCostCents: 300, listPriceCents: 700 });
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 2, 10000);
    receiveStock(ctx, 'warehouse', 'VAN-000001', 'GSK-KIT', 4, 300);
    assert.deepEqual(stockValuation(ctx), {
      lines: [
        { sku: 'GSK-KIT', onHand: 4, unitCostCents: 300, valueCents: 1200 },
        { sku: 'PUMP-CIRC', onHand: 2, unitCostCents: 10000, valueCents: 20000 },
      ],
      totalCents: 21200,
    });
  });

  it('can be limited to one location', () => {
    const { ctx, add } = fresh();
    add('GSK-KIT', { unitCostCents: 300, listPriceCents: 700 });
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'GSK-KIT', 3, 300);
    receiveStock(ctx, 'warehouse', 'VAN-000001', 'GSK-KIT', 4, 300);
    assert.equal(stockValuation(ctx, 'VAN-000001').lines[0].onHand, 4);
    assert.equal(stockValuation(ctx, 'WH-CENTRAL').totalCents, 900);
    assert.equal(stockValuation(ctx).lines[0].onHand, 7);
  });

  it('omits sold-out lines', () => {
    const { ctx, add } = fresh();
    add('GSK-KIT');
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'GSK-KIT', 3, 200);
    adjustCount(ctx, 'warehouse', 'WH-CENTRAL', 'GSK-KIT', 0);
    assert.deepEqual(stockValuation(ctx).lines, []);
  });
});
'''

API_TIMESHEETS = r'''
let world: World;
let jobId: string;
let tech: string;

beforeEach(() => {
  world = createWorld();
  tech = world.technician.id;
  jobId = openWorkOrder(world.app, world.customer.id, world.site.id, 'general').id;
});

const entry = (overrides: Record<string, unknown> = {}) => ({
  technicianId: tech,
  jobId,
  kind: 'labor',
  start: at(MONDAY, '09:00'),
  end: at(MONDAY, '10:30'),
  ...overrides,
});

describe('POST /time-entries', () => {
  it('lets a technician log their own time', () => {
    const body = expectStatus(world.app, 201, 'POST', '/time-entries', tech, entry());
    assert.equal(body.minutes, 90);
    assert.equal(body.status, 'submitted');
  });

  it('forbids logging time for someone else', () => {
    const other = seedTechnician(world.app, { skills: ['plumbing'] });
    expectStatus(world.app, 403, 'POST', '/time-entries', other.id, entry());
  });

  it('is limited to roles with timesheet access', () => {
    expectStatus(world.app, 403, 'POST', '/time-entries', 'billing', entry());
    expectStatus(world.app, 403, 'POST', '/time-entries', 'warehouse', entry());
    expectStatus(world.app, 201, 'POST', '/time-entries', 'admin', entry());
  });

  it('answers overlaps with a conflict and invalid periods with 422', () => {
    call(world.app, 'POST', '/time-entries', tech, entry());
    assert.equal(expectStatus(world.app, 409, 'POST', '/time-entries', tech, entry({ start: at(MONDAY, '10:00'), end: at(MONDAY, '11:00') })).error.code, 'overlapping-entry');
    assert.equal(expectStatus(world.app, 422, 'POST', '/time-entries', tech, entry({ end: at(MONDAY, '08:00') })).error.code, 'invalid-period');
  });
});

describe('clock routes', () => {
  it('clocks in and out', () => {
    world.app.clock.set(at(MONDAY, '09:00'));
    const opened = expectStatus(world.app, 201, 'POST', '/clock/in', tech, { technicianId: tech, kind: 'labor', jobId });
    assert.equal(opened.status, 'open');
    world.app.clock.advance(60);
    const closed = expectStatus(world.app, 200, 'POST', '/clock/out', tech, { entryId: opened.id, note: 'Done for now' });
    assert.equal(closed.minutes, 60);
    assert.equal(closed.status, 'submitted');
  });

  it('refuses a second open entry', () => {
    call(world.app, 'POST', '/clock/in', tech, { technicianId: tech, kind: 'break' });
    assert.equal(expectStatus(world.app, 409, 'POST', '/clock/in', tech, { technicianId: tech, kind: 'admin' }).error.code, 'already-clocked-in');
  });

  it('keeps technicians to their own clock', () => {
    const other = seedTechnician(world.app, { skills: ['plumbing'] });
    expectStatus(world.app, 403, 'POST', '/clock/in', other.id, { technicianId: tech, kind: 'break' });
    const opened = call(world.app, 'POST', '/clock/in', tech, { technicianId: tech, kind: 'break' });
    expectStatus(world.app, 403, 'POST', '/clock/out', other.id, { entryId: opened.id });
    expectStatus(world.app, 200, 'POST', '/clock/out', 'admin', { entryId: opened.id });
  });

  it('validates the input', () => {
    expectStatus(world.app, 422, 'POST', '/clock/in', tech, { technicianId: tech, kind: 'nap' });
    expectStatus(world.app, 422, 'POST', '/clock/in', tech, { technicianId: tech, kind: 'labor' });
    expectStatus(world.app, 404, 'POST', '/clock/out', tech, { entryId: 'TIME-999999' });
  });
});

describe('approval routes', () => {
  it('approves and rejects through dispatch', () => {
    const first = call(world.app, 'POST', '/time-entries', tech, entry());
    const second = call(world.app, 'POST', '/time-entries', tech, entry({ start: at(MONDAY, '11:00'), end: at(MONDAY, '12:00') }));
    const pending = call(world.app, 'GET', '/time-entries/pending', 'dispatch').items;
    assert.deepEqual(pending.map((item: any) => item.id), [first.id, second.id]);
    const approved = call(world.app, 'POST', '/time-entries/approve', 'dispatch', { entryIds: [first.id] }).items;
    assert.equal(approved[0].status, 'approved');
    const rejected = call(world.app, 'POST', `/time-entries/${second.id}/reject`, 'dispatch', { reason: 'Wrong job' });
    assert.equal(rejected.rejectedReason, 'Wrong job');
    assert.deepEqual(call(world.app, 'GET', '/time-entries/pending', 'dispatch').items, []);
  });

  it('forbids technicians from approving anything', () => {
    const logged = call(world.app, 'POST', '/time-entries', tech, entry());
    expectStatus(world.app, 403, 'POST', '/time-entries/approve', tech, { entryIds: [logged.id] });
    expectStatus(world.app, 403, 'POST', `/time-entries/${logged.id}/reject`, tech, { reason: 'Nope' });
    expectStatus(world.app, 403, 'GET', '/time-entries/pending', tech);
    expectStatus(world.app, 403, 'POST', `/time-entries/${logged.id}/correct`, tech, { minutes: 30, note: 'Fix' });
  });

  it('validates approval payloads', () => {
    expectStatus(world.app, 422, 'POST', '/time-entries/approve', 'dispatch', { entryIds: [] });
    expectStatus(world.app, 422, 'POST', '/time-entries/approve', 'dispatch', {});
    expectStatus(world.app, 422, 'POST', '/time-entries/TIME-000001/reject', 'dispatch', { reason: 'x' });
  });

  it('corrects entries until they are approved', () => {
    const logged = call(world.app, 'POST', '/time-entries', tech, entry());
    const corrected = expectStatus(world.app, 200, 'POST', `/time-entries/${logged.id}/correct`, 'dispatch', { minutes: 45, note: 'Shorter visit' });
    assert.equal(corrected.minutes, 45);
    call(world.app, 'POST', '/time-entries/approve', 'dispatch', { entryIds: [logged.id] });
    assert.equal(expectStatus(world.app, 409, 'POST', `/time-entries/${logged.id}/correct`, 'dispatch', { minutes: 30, note: 'Too late' }).error.code, 'entry-approved');
    expectStatus(world.app, 422, 'POST', `/time-entries/${logged.id}/correct`, 'dispatch', { minutes: 0, note: 'Zero' });
  });
});

describe('technician timesheet routes', () => {
  it('returns the weekly summary to the technician and to dispatch', () => {
    call(world.app, 'POST', '/time-entries', tech, entry());
    const own = call(world.app, 'GET', `/technicians/${tech}/timesheet?week=${MONDAY}`, tech);
    assert.equal(own.hours.labor, 1.5);
    const staff = call(world.app, 'GET', `/technicians/${tech}/timesheet?week=${MONDAY}`, 'dispatch');
    assert.deepEqual(staff, own);
  });

  it('hides the summary from other technicians and unrelated roles', () => {
    const other = seedTechnician(world.app, { skills: ['plumbing'] });
    expectStatus(world.app, 403, 'GET', `/technicians/${tech}/timesheet?week=${MONDAY}`, other.id);
    expectStatus(world.app, 403, 'GET', `/technicians/${tech}/timesheet?week=${MONDAY}`, 'billing');
    expectStatus(world.app, 422, 'GET', `/technicians/${tech}/timesheet`, 'dispatch');
  });

  it('computes the mileage claim for approved travel', () => {
    const travel = call(world.app, 'POST', '/time-entries', tech, entry({ kind: 'travel', km: 25 }));
    call(world.app, 'POST', '/time-entries/approve', 'dispatch', { entryIds: [travel.id] });
    const claim = call(world.app, 'GET', `/technicians/${tech}/mileage?from=2026-03-01&to=2026-03-08`, 'dispatch');
    assert.deepEqual(claim, { technicianId: tech, trips: 1, km: 25, amountCents: 750 });
    expectStatus(world.app, 403, 'GET', `/technicians/${tech}/mileage?from=2026-03-01&to=2026-03-08`, tech);
    expectStatus(world.app, 422, 'GET', `/technicians/${tech}/mileage?from=2026-03-01`, 'dispatch');
  });
});
'''

API_INVENTORY = r'''
let world: World;

beforeEach(() => {
  world = createWorld();
});

const part = (overrides: Record<string, unknown> = {}) => ({
  id: 'PUMP-CIRC',
  name: 'Circulation pump',
  category: 'heating',
  unitCostCents: 10000,
  listPriceCents: 18000,
  ...overrides,
});

const receive = (sku: string, quantity: number, locationId = 'WH-CENTRAL', unitCostCents = 10000) =>
  call(world.app, 'POST', '/stock/receipts', 'warehouse', { locationId, sku, quantity, unitCostCents });

describe('part routes', () => {
  it('adds parts and lists them by sku', () => {
    expectStatus(world.app, 201, 'POST', '/parts', 'warehouse', part({ id: 'ZZZ-1' }));
    expectStatus(world.app, 201, 'POST', '/parts', 'warehouse', part());
    assert.deepEqual(call(world.app, 'GET', '/parts', 'warehouse').items.map((item: any) => item.id), ['PUMP-CIRC', 'ZZZ-1']);
  });

  it('filters by category', () => {
    call(world.app, 'POST', '/parts', 'warehouse', part());
    call(world.app, 'POST', '/parts', 'warehouse', part({ id: 'VLV-15MM', category: 'plumbing' }));
    assert.deepEqual(call(world.app, 'GET', '/parts?category=plumbing', 'warehouse').items.map((item: any) => item.id), ['VLV-15MM']);
  });

  it('rejects duplicates and invalid parts', () => {
    call(world.app, 'POST', '/parts', 'warehouse', part());
    assert.equal(expectStatus(world.app, 409, 'POST', '/parts', 'warehouse', part()).error.code, 'duplicate-sku');
    assert.equal(expectStatus(world.app, 422, 'POST', '/parts', 'warehouse', part({ id: 'bad id' })).error.code, 'invalid-input');
    assert.equal(expectStatus(world.app, 422, 'POST', '/parts', 'warehouse', part({ id: 'CHEAP-1', listPriceCents: 10 })).error.code, 'price-below-cost');
  });

  it('reprices and discontinues', () => {
    call(world.app, 'POST', '/parts', 'warehouse', part());
    assert.equal(call(world.app, 'PATCH', '/parts/PUMP-CIRC/price', 'warehouse', { listPriceCents: 20000 }).listPriceCents, 20000);
    assert.equal(expectStatus(world.app, 422, 'PATCH', '/parts/PUMP-CIRC/price', 'warehouse', { listPriceCents: 100 }).error.code, 'price-below-cost');
    assert.equal(call(world.app, 'POST', '/parts/PUMP-CIRC/discontinue', 'warehouse').discontinued, true);
    expectStatus(world.app, 404, 'PATCH', '/parts/NOPE-1/price', 'warehouse', { listPriceCents: 100 });
  });

  it('applies the role matrix', () => {
    expectStatus(world.app, 403, 'POST', '/parts', 'dispatch', part());
    expectStatus(world.app, 403, 'POST', '/parts', world.technician.id, part());
    expectStatus(world.app, 200, 'GET', '/parts', 'dispatch');
    expectStatus(world.app, 200, 'GET', '/parts', world.technician.id);
    expectStatus(world.app, 403, 'GET', '/parts', 'billing');
  });
});

describe('stock routes', () => {
  beforeEach(() => {
    call(world.app, 'POST', '/parts', 'warehouse', part({ reorderPoint: 2, reorderQuantity: 6 }));
  });

  it('books receipts', () => {
    const receipt = expectStatus(world.app, 201, 'POST', '/stock/receipts', 'warehouse', { locationId: 'WH-CENTRAL', sku: 'PUMP-CIRC', quantity: 4, unitCostCents: 10000, reference: 'PO-7' });
    assert.equal(receipt.reference, 'PO-7');
    const level = call(world.app, 'GET', '/stock/WH-CENTRAL', 'warehouse');
    assert.deepEqual(level.items.map((item: any) => [item.sku, item.onHand, item.available]), [['PUMP-CIRC', 4, 4]]);
  });

  it('validates receipts', () => {
    expectStatus(world.app, 422, 'POST', '/stock/receipts', 'warehouse', { locationId: 'WH-CENTRAL', sku: 'PUMP-CIRC', quantity: 0, unitCostCents: 100 });
    expectStatus(world.app, 422, 'POST', '/stock/receipts', 'warehouse', { locationId: 'WH-CENTRAL', sku: 'PUMP-CIRC', quantity: 100001, unitCostCents: 100 });
    expectStatus(world.app, 404, 'POST', '/stock/receipts', 'warehouse', { locationId: 'WH-CENTRAL', sku: 'NOPE-1', quantity: 1, unitCostCents: 100 });
  });

  it('transfers stock and returns both levels', () => {
    receive('PUMP-CIRC', 5);
    const body = call(world.app, 'POST', '/stock/transfers', 'warehouse', { from: 'WH-CENTRAL', to: 'VAN-000001', sku: 'PUMP-CIRC', quantity: 2 });
    assert.equal(body.from.onHand, 3);
    assert.equal(body.to.onHand, 2);
    assert.equal(expectStatus(world.app, 409, 'POST', '/stock/transfers', 'warehouse', { from: 'WH-CENTRAL', to: 'VAN-000001', sku: 'PUMP-CIRC', quantity: 9 }).error.code, 'insufficient-stock');
    assert.equal(expectStatus(world.app, 422, 'POST', '/stock/transfers', 'warehouse', { from: 'WH-CENTRAL', to: 'WH-CENTRAL', sku: 'PUMP-CIRC', quantity: 1 }).error.code, 'same-location');
  });

  it('records stocktakes', () => {
    receive('PUMP-CIRC', 5);
    const level = call(world.app, 'POST', '/stock/counts', 'warehouse', { locationId: 'WH-CENTRAL', sku: 'PUMP-CIRC', counted: 4 });
    assert.equal(level.onHand, 4);
    expectStatus(world.app, 422, 'POST', '/stock/counts', 'warehouse', { locationId: 'WH-CENTRAL', sku: 'PUMP-CIRC', counted: -1 });
  });

  it('suggests reorders per location', () => {
    receive('PUMP-CIRC', 1);
    const { items } = call(world.app, 'GET', '/stock/WH-CENTRAL/reorder', 'warehouse');
    assert.deepEqual(items.map((item: any) => [item.sku, item.available, item.quantity]), [['PUMP-CIRC', 1, 6]]);
  });

  it('values the inventory', () => {
    receive('PUMP-CIRC', 2);
    receive('PUMP-CIRC', 1, 'VAN-000001');
    assert.equal(call(world.app, 'GET', '/inventory/valuation', 'warehouse').totalCents, 30000);
    assert.equal(call(world.app, 'GET', '/inventory/valuation?location=VAN-000001', 'warehouse').totalCents, 10000);
  });

  it('applies the role matrix', () => {
    expectStatus(world.app, 403, 'POST', '/stock/receipts', 'dispatch', { locationId: 'WH-CENTRAL', sku: 'PUMP-CIRC', quantity: 1, unitCostCents: 100 });
    expectStatus(world.app, 403, 'POST', '/stock/counts', world.technician.id, { locationId: 'WH-CENTRAL', sku: 'PUMP-CIRC', counted: 1 });
    expectStatus(world.app, 200, 'GET', '/stock/WH-CENTRAL', world.technician.id);
    expectStatus(world.app, 403, 'GET', '/inventory/valuation', 'billing');
  });
});

describe('work order reservation routes', () => {
  let jobId: string;

  beforeEach(() => {
    call(world.app, 'POST', '/parts', 'warehouse', part());
    jobId = openWorkOrder(world.app, world.customer.id, world.site.id, 'general').id;
  });

  it('reserves parts and lists the reservations', () => {
    receive('PUMP-CIRC', 2);
    const body = expectStatus(world.app, 201, 'POST', `${JOBS}/${jobId}/reservations`, 'dispatch', { parts: [{ sku: 'PUMP-CIRC', quantity: 1 }] });
    assert.equal(body.items[0].status, 'held');
    const listed = call(world.app, 'GET', `${JOBS}/${jobId}/reservations`, 'dispatch').items;
    assert.deepEqual(listed.map((item: any) => item.sku), ['PUMP-CIRC']);
  });

  it('backorders parts without stock', () => {
    const body = call(world.app, 'POST', `${JOBS}/${jobId}/reservations`, 'dispatch', { parts: [{ sku: 'PUMP-CIRC', quantity: 1 }] });
    assert.equal(body.items[0].status, 'backordered');
  });

  it('validates the request', () => {
    expectStatus(world.app, 422, 'POST', `${JOBS}/${jobId}/reservations`, 'dispatch', { parts: [] });
    expectStatus(world.app, 422, 'POST', `${JOBS}/${jobId}/reservations`, 'dispatch', { parts: [{ sku: 'PUMP-CIRC', quantity: 501 }] });
    expectStatus(world.app, 404, 'POST', `${JOBS}/WO-999999/reservations`, 'dispatch', { parts: [{ sku: 'PUMP-CIRC', quantity: 1 }] });
  });

  it('keeps reserving to roles that schedule work', () => {
    expectStatus(world.app, 403, 'POST', `${JOBS}/${jobId}/reservations`, world.technician.id, { parts: [{ sku: 'PUMP-CIRC', quantity: 1 }] });
    expectStatus(world.app, 403, 'POST', `${JOBS}/${jobId}/reservations`, 'warehouse', { parts: [{ sku: 'PUMP-CIRC', quantity: 1 }] });
  });

  it('shows reservations only to those who may see the work order', () => {
    const other = seedTechnician(world.app, { skills: ['plumbing'] });
    expectStatus(world.app, 404, 'GET', `${JOBS}/${jobId}/reservations`, other.id);
    expectStatus(world.app, 200, 'GET', `${JOBS}/${jobId}/reservations`, 'warehouse');
  });
});
'''

TS_WORLD = ["MONDAY", "at", "createWorld", "openWorkOrder", "seedTechnician"]
API_WORLD = ["MONDAY", "at", "call", "createWorld", "expectStatus", "JOBS", "openWorkOrder", "seedTechnician", "type World"]

FILES = {
    "tests/timesheets/clock.test.ts": Spec(
        {WORLD: TS_WORLD, "src/adapters/repositories/time-entries.ts": ["timeEntries"], f"{TS}/clock.ts": ["clockIn", "clockOut"]},
        CLOCK, node=WITH_SETUP),
    "tests/timesheets/entries.test.ts": Spec(
        {WORLD: ["MONDAY", "at", "createWorld", "openWorkOrder", "seedTechnician"],
         "src/adapters/repositories/time-entries.ts": ["timeEntries"], f"{TS}/approvals.ts": ["approveEntries", "rejectEntry"],
         f"{TS}/entries.ts": ["correctEntry", "logEntry"]},
        ENTRIES, node=WITH_SETUP),
    "tests/timesheets/overtime.test.ts": Spec(
        {"src/config/load.ts": ["resolveConfig"], f"{TS}/model.ts": ["type TimeEntry"],
         f"{TS}/overtime.ts": ["dailyTotals", "weeklyOvertime"]},
        OVERTIME),
    "tests/timesheets/approvals.test.ts": Spec(
        {WORLD: ["MONDAY", "at", "createWorld", "openWorkOrder"], "src/adapters/repositories/time-entries.ts": ["timeEntries"],
         "src/adapters/store/audit.ts": ["auditTrail"],
         f"{TS}/approvals.ts": ["approveEntries", "pendingApprovals", "rejectEntry"], f"{TS}/clock.ts": ["clockIn"],
         f"{TS}/entries.ts": ["logEntry"]},
        APPROVALS, node=WITH_SETUP),
    "tests/timesheets/summary.test.ts": Spec(
        {WORLD: ["MONDAY", "at", "createWorld", "openWorkOrder", "seedTechnician"], f"{TS}/approvals.ts": ["rejectEntry"],
         f"{TS}/clock.ts": ["clockIn"], f"{TS}/entries.ts": ["logEntry"], f"{TS}/mileage.ts": ["mileageClaim"],
         f"{TS}/model.ts": ["type TimeEntry"], f"{TS}/summary.ts": ["weeklySummary"]},
        SUMMARY),
    "tests/inventory/catalog.test.ts": Spec(
        {"src/app.ts": ["createApp"], "src/adapters/repositories/parts.ts": ["parts", "partsInCategory"],
         "src/adapters/store/audit.ts": ["auditTrail"], f"{INV}/catalog.ts": ["addPart", "discontinuePart", "updateListPrice"]},
        CATALOG),
    "tests/inventory/stock.test.ts": Spec(
        {"src/app.ts": ["createApp"], "src/adapters/repositories/parts.ts": ["parts"],
         "src/adapters/repositories/stock.ts": ["levelAt", "saveLevel"], "src/adapters/store/audit.ts": ["auditTrail"],
         f"{INV}/catalog.ts": ["addPart"],
         f"{INV}/stock.ts": ["adjustCount", "averageCost", "receiveStock", "transferStock"]},
        STOCK),
    "tests/inventory/reservations.test.ts": scene_spec(
        {"src/adapters/repositories/reservations.ts": ["reservationsForJob"], "src/adapters/repositories/stock.ts": ["levelAt"],
         f"{INV}/catalog.ts": ["addPart", "discontinuePart"],
         f"{INV}/reservations.ts": ["releaseReservations", "reservePartsForJob"], f"{INV}/stock.ts": ["receiveStock"]},
        RESERVATIONS, jobs=("cancelJob",)),
    "tests/inventory/reorder.test.ts": Spec(
        {"src/app.ts": ["createApp"], WORLD: ["createWorld", "openWorkOrder"], f"{INV}/catalog.ts": ["addPart", "discontinuePart"],
         f"{INV}/reorder.ts": ["reorderSuggestions", "vanTopUp"], f"{INV}/reservations.ts": ["reservePartsForJob"], f"{INV}/stock.ts": ["adjustCount", "receiveStock"],
         f"{INV}/valuation.ts": ["stockValuation"]},
        REORDER),
    "tests/api/timesheets.test.ts": Spec({WORLD: API_WORLD}, API_TIMESHEETS, node=WITH_SETUP),
    "tests/api/inventory.test.ts": Spec({WORLD: API_WORLD}, API_INVENTORY, node=WITH_SETUP),
}


def generate(ctx):
    register(ctx, FILES)
