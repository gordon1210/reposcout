"""Tests for the reporting layer: CSV export, KPIs, utilisation, backlog, revenue, activity and report routes."""

from .checks_kit import DESCRIBE_IT, WITH_SETUP, Spec, register
from .checks_workorders import scene_spec

WORLD = "tests/support/world.ts"
REPORTS = "src/reports"

CSV = r'''
describe('toCsv', () => {
  it('writes a header row and one line per record', () => {
    const csv = toCsv([
      { region: 'metro-east', invoices: 3, netCents: 45000 },
      { region: 'lakeside', invoices: 1, netCents: 9800 },
    ]);
    assert.equal(csv, 'region,invoices,netCents\nmetro-east,3,45000\nlakeside,1,9800\n');
  });

  it('takes the header from the first record', () => {
    const csv = toCsv([{ a: 1 }, { a: 2, b: 3 }]);
    assert.equal(csv, 'a\n1\n2\n');
  });

  it('uses the requested columns in the requested order', () => {
    const rows = [{ id: 'INV-1', amount: 120, note: 'late' }];
    assert.equal(toCsv(rows, ['note', 'id']), 'note,id\nlate,INV-1\n');
  });

  it('leaves cells empty for missing columns and absent values', () => {
    const csv = toCsv([{ id: 'A', note: undefined, owner: null }, { id: 'B' }], ['id', 'note', 'owner', 'other']);
    assert.equal(csv, 'id,note,owner,other\nA,,,\nB,,,\n');
  });

  it('keeps zero and false as values', () => {
    assert.equal(toCsv([{ count: 0, active: false, label: '' }]), 'count,active,label\n0,false,\n');
  });

  it('quotes cells containing separators and doubles embedded quotes', () => {
    const csv = toCsv([{ site: 'Quay Street, Eastport', note: 'Called it "the beast"' }]);
    assert.equal(csv, 'site,note\n"Quay Street, Eastport","Called it ""the beast"""\n');
  });

  it('quotes cells containing line breaks', () => {
    assert.equal(toCsv([{ note: 'first\nsecond' }]), 'note\n"first\nsecond"\n');
  });

  it('serialises nested values as JSON', () => {
    const csv = toCsv([{ id: 1, tags: ['a', 'b'], where: { lat: 1.5 } }]);
    assert.equal(csv, 'id,tags,where\n1,"[""a"",""b""]","{""lat"":1.5}"\n');
  });

  it('quotes header cells like any other cell', () => {
    assert.equal(toCsv([{ 'net, cents': 5 }]), '"net, cents"\n5\n');
  });

  it('writes only the header when there are no records but columns are known', () => {
    assert.equal(toCsv([], ['region', 'month']), 'region,month\n');
  });

  it('writes a single empty line for an empty report without columns', () => {
    assert.equal(toCsv([]), '\n');
  });

  it('always ends with exactly one line feed', () => {
    for (const rows of [[], [{ a: 1 }], [{ a: 1 }, { a: 2 }]]) {
      const csv = toCsv(rows);
      assert.ok(csv.endsWith('\n'));
      assert.ok(!csv.endsWith('\n\n') || rows.length === 0);
    }
  });
});
'''

KPIS = r'''
let scene: Scene;
const PERIOD = { from: at(MONDAY, '00:00'), to: at('2026-03-09', '00:00') };

beforeEach(() => {
  scene = start();
});

function finish(jobId: string, date: string, clock: string, input: Record<string, unknown> = {}) {
  scene.app.clock.set(at(date, clock));
  const job = «requireJob»(scene.ctx, jobId);
  if (!job.technicianId) «assignJob»(scene.ctx, 'dispatch', jobId, scene.tech);
  if (job.status !== 'in_progress') «startJob»(scene.ctx, scene.tech, jobId);
  for (const item of Object.keys(«requireJob»(scene.ctx, jobId).checklist)) «recordChecklistItem»(scene.ctx, scene.tech, jobId, item, true);
  return «completeJob»(scene.ctx, scene.tech, jobId, { resolution: 'Replaced the thermostat', ...input });
}

const contracted = () => {
  seedContract(scene.app, scene.customer.id, [scene.site.id], 'premium', ['general']);
};

describe('without finished work', () => {
  it('reports no rates', () => {
    assert.equal(firstTimeFixRate(scene.ctx, PERIOD), undefined);
    assert.equal(meanHoursToComplete(scene.ctx, PERIOD), undefined);
    assert.equal(slaCompliancePercent(scene.ctx, PERIOD), undefined);
  });

  it('counts only the open backlog in the snapshot', () => {
    open(scene);
    open(scene, 'general', { priority: 'emergency' });
    const snapshot = kpiSnapshot(scene.ctx, PERIOD);
    assert.deepEqual(snapshot, {
      period: PERIOD,
      completed: 0,
      firstTimeFixRate: undefined,
      meanHoursToComplete: undefined,
      slaCompliancePercent: undefined,
      openBacklog: 2,
    });
  });
});

describe('firstTimeFixRate', () => {
  it('is 100 when nothing needed a second visit', () => {
    finish(open(scene).id, MONDAY, '10:00');
    finish(open(scene).id, MONDAY, '11:00');
    assert.equal(firstTimeFixRate(scene.ctx, PERIOD), 100);
  });

  it('excludes jobs flagged for a follow-up visit', () => {
    finish(open(scene).id, MONDAY, '10:00');
    finish(open(scene).id, MONDAY, '11:00');
    finish(open(scene).id, MONDAY, '12:00', { followUpRequired: true });
    assert.equal(firstTimeFixRate(scene.ctx, PERIOD), 66.7);
  });

  it('excludes jobs that were reopened', () => {
    const repeat = open(scene);
    finish(repeat.id, MONDAY, '10:00');
    «reopenJob»(scene.ctx, 'dispatch', repeat.id, 'The fault returned');
    finish(repeat.id, MONDAY, '12:00');
    finish(open(scene).id, MONDAY, '13:00');
    assert.equal(firstTimeFixRate(scene.ctx, PERIOD), 50);
  });

  it('is 0 when every job needed more work', () => {
    finish(open(scene).id, MONDAY, '10:00', { followUpRequired: true });
    assert.equal(firstTimeFixRate(scene.ctx, PERIOD), 0);
  });

  it('rounds to one decimal', () => {
    for (const clock of ['09:00', '10:00', '11:00', '12:00', '13:00', '14:00']) finish(open(scene).id, MONDAY, clock);
    finish(open(scene).id, MONDAY, '15:00', { followUpRequired: true });
    assert.equal(firstTimeFixRate(scene.ctx, PERIOD), 85.7);
  });

  it('ignores cancelled and unfinished work', () => {
    finish(open(scene).id, MONDAY, '10:00');
    «cancelJob»(scene.ctx, 'dispatch', open(scene).id, 'Raised twice by mistake');
    open(scene);
    assert.equal(firstTimeFixRate(scene.ctx, PERIOD), 100);
  });
});

describe('period boundaries', () => {
  it('includes the first instant and excludes the last', () => {
    finish(open(scene).id, MONDAY, '12:00');
    const completed = (from: string, to: string) => kpiSnapshot(scene.ctx, { from, to }).completed;
    assert.equal(completed(at(MONDAY, '12:00'), at(MONDAY, '13:00')), 1);
    assert.equal(completed(at(MONDAY, '11:00'), at(MONDAY, '12:00')), 0);
    assert.equal(completed(at(MONDAY, '12:01'), at(MONDAY, '13:00')), 0);
  });

  it('only looks at work completed inside the period', () => {
    finish(open(scene).id, MONDAY, '10:00');
    finish(open(scene).id, '2026-03-10', '10:00', { followUpRequired: true });
    assert.equal(firstTimeFixRate(scene.ctx, PERIOD), 100);
    assert.equal(kpiSnapshot(scene.ctx, { from: at('2026-03-09', '00:00'), to: at('2026-03-16', '00:00') }).completed, 1);
  });
});

describe('meanHoursToComplete', () => {
  it('averages the time from request to completion', () => {
    const slow = open(scene);
    const quick = open(scene);
    finish(slow.id, MONDAY, '10:30');
    finish(quick.id, MONDAY, '09:00');
    assert.equal(meanHoursToComplete(scene.ctx, PERIOD), 1.8);
  });

  it('spans days', () => {
    finish(open(scene).id, '2026-03-04', '08:00');
    assert.equal(meanHoursToComplete(scene.ctx, PERIOD), 48);
  });

  it('leaves out the time spent waiting for parts', () => {
    const job = open(scene);
    scene.app.clock.set(at(MONDAY, '09:00'));
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    scene.app.clock.set(at(MONDAY, '10:00'));
    «holdJob»(scene.ctx, scene.tech, job.id, 'awaiting_parts');
    scene.app.clock.set(at('2026-03-03', '10:00'));
    «resumeJob»(scene.ctx, scene.tech, job.id);
    finish(job.id, '2026-03-03', '11:00');
    assert.equal(meanHoursToComplete(scene.ctx, PERIOD), 3);
  });

  it('measures reopened work up to its latest completion', () => {
    const job = open(scene);
    finish(job.id, MONDAY, '10:00');
    «reopenJob»(scene.ctx, 'dispatch', job.id, 'Leak is back');
    finish(job.id, MONDAY, '12:00');
    assert.equal(meanHoursToComplete(scene.ctx, PERIOD), 4);
  });
});

describe('slaCompliancePercent', () => {
  it('is the share of met clocks in the period', () => {
    contracted();
    finish(open(scene).id, MONDAY, '10:00');
    finish(open(scene).id, MONDAY, '11:00');
    finish(open(scene).id, MONDAY, '20:00');
    assert.equal(slaCompliancePercent(scene.ctx, PERIOD), 66.7);
  });

  it('reaches 100 and 0 at the extremes', () => {
    contracted();
    finish(open(scene).id, MONDAY, '10:00');
    assert.equal(slaCompliancePercent(scene.ctx, PERIOD), 100);
    const late = open(scene);
    scene.app.clock.set(at('2026-03-04', '08:00'));
    finish(late.id, '2026-03-04', '20:00');
    assert.equal(slaCompliancePercent(scene.ctx, { from: at('2026-03-04', '00:00'), to: at('2026-03-05', '00:00') }), 0);
  });

  it('ignores work without a service level', () => {
    finish(open(scene).id, MONDAY, '10:00');
    assert.equal(slaCompliancePercent(scene.ctx, PERIOD), undefined);
  });

  it('feeds the snapshot', () => {
    contracted();
    const [first, second] = [open(scene), open(scene)];
    open(scene);
    finish(first.id, MONDAY, '10:00');
    finish(second.id, MONDAY, '11:00', { followUpRequired: true });
    assert.deepEqual(kpiSnapshot(scene.ctx, PERIOD), {
      period: PERIOD,
      completed: 2,
      firstTimeFixRate: 50,
      meanHoursToComplete: 2.5,
      slaCompliancePercent: 100,
      openBacklog: 1,
    });
  });
});
'''

UTILISATION = r'''
const WEEK = { from: MONDAY, to: '2026-03-09' };
let scene: Scene;
let jobId: string;

beforeEach(() => {
  scene = start();
  jobId = open(scene).id;
});

const book = (technicianId: string, kind: string, from: string, to: string, date = MONDAY, extra: Record<string, unknown> = {}) =>
  logEntry(scene.ctx, { technicianId, kind, jobId, start: at(date, from), end: at(date, to), ...extra });

const rowOf = (technicianId: string, period = WEEK) =>
  utilisationReport(scene.ctx, period.from, period.to).find((row) => row.technicianId === technicianId)!;

const dayMinutes = () => scene.ctx.config.scheduling.dayEndMinute - scene.ctx.config.scheduling.dayStartMinute;
const workdays = (count: number) => Math.round((count * dayMinutes()) / 6) / 10;
const share = (minutes: number, days: number) => Math.round((minutes / (days * dayMinutes())) * 100);

describe('available time', () => {
  it('counts the standard working time of the period', () => {
    assert.equal(rowOf(scene.tech).availableHours, workdays(5));
  });

  it('skips weekends', () => {
    assert.equal(rowOf(scene.tech, { from: '2026-03-07', to: '2026-03-09' }).availableHours, 0);
    assert.equal(rowOf(scene.tech, { from: '2026-03-06', to: '2026-03-09' }).availableHours, workdays(1));
  });

  it('skips public holidays', () => {
    assert.equal(rowOf(scene.tech, { from: '2026-03-30', to: '2026-04-06' }).availableHours, workdays(4));
  });

  it('covers a single day', () => {
    assert.equal(rowOf(scene.tech, { from: '2026-03-03', to: '2026-03-04' }).availableHours, workdays(1));
  });

  it('is empty for an empty or reversed period', () => {
    assert.equal(rowOf(scene.tech, { from: MONDAY, to: MONDAY }).availableHours, 0);
    assert.equal(rowOf(scene.tech, { from: '2026-03-09', to: MONDAY }).availableHours, 0);
  });
});

describe('booked time', () => {
  it('starts at zero', () => {
    assert.deepEqual(rowOf(scene.tech), {
      technicianId: scene.tech,
      name: scene.technician.name,
      laborHours: 0,
      travelHours: 0,
      availableHours: workdays(5),
      utilisationPercent: 0,
    });
  });

  it('adds labour and travel against the available time', () => {
    book(scene.tech, 'labor', '09:00', '10:30');
    book(scene.tech, 'travel', '10:30', '11:00');
    book(scene.tech, 'labor', '13:00', '14:00', '2026-03-03');
    assert.deepEqual(rowOf(scene.tech), {
      technicianId: scene.tech,
      name: scene.technician.name,
      laborHours: 2.5,
      travelHours: 0.5,
      availableHours: workdays(5),
      utilisationPercent: share(180, 5),
    });
  });

  it('rounds hours to one decimal and the percentage to whole numbers', () => {
    book(scene.tech, 'labor', '09:00', '10:40');
    const row = rowOf(scene.tech);
    assert.equal(row.laborHours, 1.7);
    assert.equal(row.utilisationPercent, share(100, 5));
  });

  it('does not count breaks and administration', () => {
    logEntry(scene.ctx, { technicianId: scene.tech, kind: 'break', start: at(MONDAY, '12:00'), end: at(MONDAY, '12:30') });
    logEntry(scene.ctx, { technicianId: scene.tech, kind: 'admin', start: at(MONDAY, '15:00'), end: at(MONDAY, '16:00') });
    const row = rowOf(scene.tech);
    assert.equal(row.laborHours + row.travelHours, 0);
    assert.equal(row.utilisationPercent, 0);
  });

  it('counts approved entries but not rejected ones', () => {
    const kept = book(scene.tech, 'labor', '09:00', '10:00');
    const dropped = book(scene.tech, 'labor', '10:00', '11:00');
    approveEntries(scene.ctx, 'dispatch', [kept.id]);
    rejectEntry(scene.ctx, 'dispatch', dropped.id, 'Booked on the wrong job');
    assert.equal(rowOf(scene.tech).laborHours, 1);
  });

  it('only looks at entries inside the period', () => {
    book(scene.tech, 'labor', '09:00', '10:00', '2026-03-09');
    book(scene.tech, 'labor', '09:00', '11:00', '2026-03-06');
    assert.equal(rowOf(scene.tech).laborHours, 2);
    assert.equal(rowOf(scene.tech, { from: '2026-03-09', to: '2026-03-10' }).laborHours, 1);
  });

  it('does not divide by zero outside working time', () => {
    book(scene.tech, 'labor', '09:00', '10:00', '2026-03-07');
    const row = rowOf(scene.tech, { from: '2026-03-07', to: '2026-03-09' });
    assert.equal(row.laborHours, 1);
    assert.equal(row.utilisationPercent, 0);
  });
});

describe('ranking', () => {
  const hire = (name: string) => seedTechnician(scene.app, { name }).id as string;

  it('puts the busiest technician first and ties in name order', () => {
    const blake = hire('Blake Morgan');
    const avery = hire('Avery Collins');
    const casey = hire('Casey Lin');
    book(blake, 'labor', '09:00', '11:00');
    book(casey, 'labor', '09:00', '12:00');
    book(avery, 'labor', '09:00', '11:00');
    const names = utilisationReport(scene.ctx, WEEK.from, WEEK.to).filter((row) => row.technicianId !== scene.tech).map((row) => row.name);
    assert.deepEqual(names, ['Casey Lin', 'Avery Collins', 'Blake Morgan']);
  });

  it('lists idle technicians in name order after the busy ones', () => {
    const zoe = hire('Zoe Hart');
    const abe = hire('Abe Ford');
    book(zoe, 'travel', '09:00', '10:00');
    const rows = utilisationReport(scene.ctx, WEEK.from, WEEK.to);
    assert.equal(rows[0].technicianId, zoe);
    assert.deepEqual(rows.slice(1).map((row) => row.name), [abe, scene.tech].map((id) => rows.find((row) => row.technicianId === id)!.name).sort());
  });

  it('leaves out deactivated technicians', () => {
    const leaver = hire('Leah Brandt');
    call(scene.app, 'POST', `/technicians/${leaver}/deactivate`, 'admin');
    assert.ok(utilisationReport(scene.ctx, WEEK.from, WEEK.to).every((row) => row.technicianId !== leaver));
  });
});
'''

BACKLOG = r'''
let scene: Scene;

beforeEach(() => {
  scene = start();
});

const on = (date: string, clock: string) => scene.app.clock.set(at(date, clock));

function finishNow(jobId: string) {
  «assignJob»(scene.ctx, 'dispatch', jobId, scene.tech);
  «startJob»(scene.ctx, scene.tech, jobId);
  for (const item of Object.keys(«requireJob»(scene.ctx, jobId).checklist)) «recordChecklistItem»(scene.ctx, scene.tech, jobId, item, true);
  «completeJob»(scene.ctx, scene.tech, jobId, { resolution: 'Tightened the compression fitting' });
}

describe('backlogAging', () => {
  it('is empty without open work', () => {
    assert.deepEqual(backlogAging(scene.ctx), []);
  });

  it('groups open work by whole days, oldest last', () => {
    open(scene);
    on('2026-03-04', '08:00');
    open(scene);
    open(scene, 'general', { priority: 'emergency' });
    on('2026-03-06', '12:00');
    assert.deepEqual(backlogAging(scene.ctx), [
      { ageDays: 2, count: 2, emergencies: 1 },
      { ageDays: 4, count: 1, emergencies: 0 },
    ]);
  });

  it('moves to the next day exactly after 24 hours', () => {
    open(scene);
    on(MONDAY, '23:59');
    assert.deepEqual(backlogAging(scene.ctx), [{ ageDays: 0, count: 1, emergencies: 0 }]);
    on('2026-03-03', '07:59');
    assert.deepEqual(backlogAging(scene.ctx).map((bucket) => bucket.ageDays), [0]);
    on('2026-03-03', '08:00');
    assert.deepEqual(backlogAging(scene.ctx).map((bucket) => bucket.ageDays), [1]);
  });

  it('counts emergencies per bucket', () => {
    open(scene, 'general', { priority: 'emergency' });
    open(scene, 'general', { priority: 'emergency' });
    open(scene);
    on('2026-03-03', '09:00');
    assert.deepEqual(backlogAging(scene.ctx), [{ ageDays: 1, count: 3, emergencies: 2 }]);
  });

  it('excludes time on hold from the age', () => {
    const waiting = open(scene);
    on(MONDAY, '09:00');
    «assignJob»(scene.ctx, 'dispatch', waiting.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, waiting.id);
    «holdJob»(scene.ctx, scene.tech, waiting.id, 'awaiting_parts');
    on('2026-03-06', '09:00');
    assert.deepEqual(backlogAging(scene.ctx), [{ ageDays: 0, count: 1, emergencies: 0 }]);
    «resumeJob»(scene.ctx, scene.tech, waiting.id);
    on('2026-03-08', '10:00');
    assert.deepEqual(backlogAging(scene.ctx).map((bucket) => bucket.ageDays), [2]);
  });

  it('leaves out finished and cancelled work', () => {
    finishNow(open(scene).id);
    «cancelJob»(scene.ctx, 'dispatch', open(scene).id, 'Customer solved it themselves');
    const remaining = open(scene);
    on('2026-03-05', '08:00');
    assert.deepEqual(backlogAging(scene.ctx), [{ ageDays: 3, count: 1, emergencies: 0 }]);
    assert.equal(«requireJob»(scene.ctx, remaining.id).status, 'new');
  });
});

describe('backlogByRegion', () => {
  it('is empty without open work', () => {
    assert.deepEqual(backlogByRegion(scene.ctx), {});
  });

  it('counts open work per region and status', () => {
    const lakeside = seedCustomer(scene.app, { name: 'Brightwater Dental', region: 'lakeside' });
    const openLakeside = () => open(scene, 'general', { customerId: lakeside.customer.id, siteId: lakeside.site.id });
    open(scene);
    open(scene);
    const started = open(scene);
    «assignJob»(scene.ctx, 'dispatch', started.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, started.id);
    openLakeside();
    const held = openLakeside();
    «assignJob»(scene.ctx, 'dispatch', held.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, held.id);
    «holdJob»(scene.ctx, scene.tech, held.id, 'awaiting_parts');
    assert.deepEqual(backlogByRegion(scene.ctx), {
      'metro-east': { new: 2, in_progress: 1 },
      lakeside: { new: 1, on_hold: 1 },
    });
  });

  it('drops work that is no longer open', () => {
    const job = open(scene);
    finishNow(job.id);
    assert.deepEqual(backlogByRegion(scene.ctx), {});
    «reopenJob»(scene.ctx, 'dispatch', job.id, 'Fitting is leaking again');
    assert.deepEqual(backlogByRegion(scene.ctx), { 'metro-east': { assigned: 1 } });
  });
});
'''

REVENUE = r'''
let scene: Scene;

beforeEach(() => {
  scene = start();
});

const lakeside = () => seedCustomer(scene.app, { name: 'Brightwater Dental', region: 'lakeside' });

function invoice(customer: { customer: any; site: any }, date: string, issue = true) {
  scene.app.clock.set(at(date, '10:00'));
  const job = open(scene, 'general', { customerId: customer.customer.id, siteId: customer.site.id });
  work(scene, job.id);
  const draft = draftInvoiceForJob(scene.ctx, 'billing', job.id);
  return issue ? issueInvoice(scene.ctx, 'billing', draft.id) : draft;
}

const home = () => ({ customer: scene.customer, site: scene.site });
const sum = (invoices: Array<{ subtotalCents: number; taxCents: number }>) => ({
  netCents: invoices.reduce((total, entry) => total + entry.subtotalCents, 0),
  taxCents: invoices.reduce((total, entry) => total + entry.taxCents, 0),
});

describe('revenueByRegion', () => {
  it('is empty without issued invoices', () => {
    assert.deepEqual(revenueByRegion(scene.ctx, '2026-01-01', '2027-01-01'), []);
    invoice(home(), MONDAY, false);
    assert.deepEqual(revenueByRegion(scene.ctx, '2026-01-01', '2027-01-01'), []);
  });

  it('adds up invoices per region and month of issue', () => {
    const second = lakeside();
    const a = invoice(home(), '2026-03-03');
    const b = invoice(home(), '2026-03-17');
    const c = invoice(second, '2026-03-20');
    const d = invoice(home(), '2026-04-07');
    assert.deepEqual(revenueByRegion(scene.ctx, '2026-03-01', '2026-05-01'), [
      { region: 'lakeside', month: '2026-03', invoices: 1, ...sum([c]) },
      { region: 'metro-east', month: '2026-03', invoices: 2, ...sum([a, b]) },
      { region: 'metro-east', month: '2026-04', invoices: 1, ...sum([d]) },
    ]);
  });

  it('sorts by month before region', () => {
    const second = lakeside();
    invoice(second, '2026-04-08');
    invoice(home(), '2026-04-09');
    invoice(second, '2026-03-30');
    const rows = revenueByRegion(scene.ctx, '2026-03-01', '2026-05-01');
    assert.deepEqual(rows.map((row) => `${row.month} ${row.region}`), ['2026-03 lakeside', '2026-04 lakeside', '2026-04 metro-east']);
  });

  it('includes the start date and excludes the end date', () => {
    invoice(home(), '2026-03-10');
    assert.equal(revenueByRegion(scene.ctx, '2026-03-10', '2026-03-11').length, 1);
    assert.equal(revenueByRegion(scene.ctx, '2026-03-09', '2026-03-10').length, 0);
    assert.equal(revenueByRegion(scene.ctx, '2026-03-11', '2026-03-12').length, 0);
  });

  it('keeps paid invoices and drops void ones', () => {
    const paid = invoice(home(), '2026-03-03');
    recordPayment(scene.ctx, 'billing', paid.id, paid.totalCents, 'transfer');
    const voided = invoice(home(), '2026-03-04');
    voidInvoice(scene.ctx, 'billing', voided.id, 'Raised against the wrong site');
    const [row] = revenueByRegion(scene.ctx, '2026-03-01', '2026-04-01');
    assert.equal(row.invoices, 1);
    assert.deepEqual({ netCents: row.netCents, taxCents: row.taxCents }, sum([paid]));
  });

  it('reports no tax for exempt customers', () => {
    const exempt = seedCustomer(scene.app, { name: 'Marlow Hospice', region: 'lakeside', taxExempt: true });
    const issued = invoice(exempt, '2026-03-12');
    assert.equal(issued.taxCents, 0);
    assert.deepEqual(revenueByRegion(scene.ctx, '2026-03-01', '2026-04-01'), [
      { region: 'lakeside', month: '2026-03', invoices: 1, netCents: issued.subtotalCents, taxCents: 0 },
    ]);
  });
});
'''

ACTIVITY = r'''
const emit = (ctx: AppContext, type: string, clock: string) => ctx.bus.emit({ type, at: at(MONDAY, clock) });

describe('recentActivity', () => {
  it('reports nothing for a quiet system', () => {
    const { ctx } = createApp({ now: at(MONDAY, '09:00') });
    assert.deepEqual(recentActivity(ctx), { workOrderEvents: 0, byType: {}, lastEventAt: undefined });
  });

  it('counts events by type', () => {
    const { ctx } = createApp({ now: at(MONDAY, '09:00') });
    emit(ctx, 'probe.alpha', '09:00');
    emit(ctx, 'probe.beta', '09:05');
    emit(ctx, 'probe.alpha', '09:10');
    emit(ctx, 'probe.alpha', '09:15');
    assert.deepEqual(recentActivity(ctx).byType, { 'probe.alpha': 3, 'probe.beta': 1 });
  });

  it('reports the time of the latest event', () => {
    const { ctx } = createApp({ now: at(MONDAY, '09:00') });
    emit(ctx, 'probe.alpha', '09:00');
    emit(ctx, 'probe.beta', '09:45');
    assert.equal(recentActivity(ctx).lastEventAt, at(MONDAY, '09:45'));
  });

  it('only looks at the most recent events', () => {
    const { ctx } = createApp({ now: at(MONDAY, '09:00') });
    for (const [index, type] of ['probe.a', 'probe.b', 'probe.c', 'probe.d', 'probe.e'].entries()) {
      emit(ctx, type, `10:0${index}`);
    }
    const summary = recentActivity(ctx, 2);
    assert.deepEqual(summary.byType, { 'probe.d': 1, 'probe.e': 1 });
    assert.equal(summary.lastEventAt, at(MONDAY, '10:04'));
  });

  it('keeps no more than the retained journal', () => {
    const { ctx } = createApp({ now: at(MONDAY, '09:00') });
    for (let index = 0; index < 260; index += 1) emit(ctx, index < 50 ? 'probe.early' : 'probe.late', '11:00');
    const summary = recentActivity(ctx, 500);
    assert.equal(summary.byType['probe.early'], undefined);
    assert.equal(summary.byType['probe.late'], 200);
  });

  it('defaults to the latest two hundred events', () => {
    const { ctx } = createApp({ now: at(MONDAY, '09:00') });
    for (let index = 0; index < 230; index += 1) emit(ctx, index < 40 ? 'probe.old' : 'probe.new', '12:00');
    assert.deepEqual(recentActivity(ctx).byType, { 'probe.new': 190, 'probe.old': 10 });
  });
});
'''

API = r'''
let world: World;
let portal: string;

const REPORTS = ['/reports/kpis', '/reports/utilisation', '/reports/revenue', '/reports/backlog', '/reports/activity'];
const RANGE = 'from=2026-03-02&to=2026-03-09';

beforeEach(() => {
  world = createWorld();
  portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
});

const get = (path: string, actor = 'admin') => call(world.app, 'GET', path, actor);
const dayMinutes = () => world.app.ctx.config.scheduling.dayEndMinute - world.app.ctx.config.scheduling.dayStartMinute;

function completeOne(jobTime: string) {
  const job = openWorkOrder(world.app, world.customer.id, world.site.id);
  call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
  call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
  completeChecklist(world.app, job.id, world.technician.id);
  world.app.clock.set(at(MONDAY, jobTime));
  call(world.app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Bled and rebalanced the radiators' });
  return job;
}

describe('access', () => {
  it('is open to administrators, dispatchers and billing staff', () => {
    for (const path of REPORTS) {
      for (const actor of ['admin', 'dispatch', 'billing']) expectStatus(world.app, 200, 'GET', `${path}?${RANGE}`, actor);
    }
  });

  it('is closed to everyone else', () => {
    for (const path of REPORTS) {
      for (const actor of ['warehouse', world.technician.id, portal]) {
        assert.equal(expectStatus(world.app, 403, 'GET', `${path}?${RANGE}`, actor).error.code, 'forbidden', `${path} as ${actor}`);
      }
    }
  });

  it('only answers GET', () => {
    for (const path of REPORTS) assert.equal(world.app.request('POST', path, { actor: 'admin' }).status, 405);
  });
});

describe('GET /reports/kpis', () => {
  it('reports the period and the open backlog', () => {
    openWorkOrder(world.app, world.customer.id, world.site.id);
    assert.deepEqual(get(`/reports/kpis?${RANGE}`), {
      period: { from: at(MONDAY, '00:00'), to: at('2026-03-09', '00:00') },
      completed: 0,
      openBacklog: 1,
    });
  });

  it('summarises completed work', () => {
    completeOne('10:30');
    completeOne('11:00');
    const body = get(`/reports/kpis?${RANGE}`);
    assert.equal(body.completed, 2);
    assert.equal(body.firstTimeFixRate, 100);
    assert.equal(body.meanHoursToComplete, 1.5);
    assert.equal(body.openBacklog, 0);
  });

  it('requires a valid period', () => {
    for (const query of ['', 'from=2026-03-02', 'to=2026-03-09', 'from=yesterday&to=2026-03-09', 'from=2026-03-02&to=2026-13-40']) {
      const body = expectStatus(world.app, 422, 'GET', `/reports/kpis?${query}`, 'admin');
      assert.equal(body.error.code, 'invalid-input', query);
    }
  });

  it('names the missing parameter', () => {
    const body = expectStatus(world.app, 422, 'GET', '/reports/kpis?to=2026-03-09', 'admin');
    assert.deepEqual(body.error.details.issues, [{ path: 'from', message: 'is required' }]);
  });
});

describe('GET /reports/utilisation', () => {
  it('lists every active technician', () => {
    const second = seedTechnician(world.app, { name: 'Casey Lin' });
    const { items } = get(`/reports/utilisation?${RANGE}`);
    assert.deepEqual(items.map((row: any) => row.technicianId).sort(), [world.technician.id, second.id].sort());
    assert.ok(items.every((row: any) => row.availableHours === (5 * dayMinutes()) / 60 && row.utilisationPercent === 0));
  });

  it('includes booked time', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id);
    call(world.app, 'POST', '/time-entries', 'admin', {
      technicianId: world.technician.id,
      kind: 'labor',
      jobId: job.id,
      start: at(MONDAY, '09:00'),
      end: at(MONDAY, '12:00'),
    });
    const [row] = get(`/reports/utilisation?${RANGE}`).items;
    assert.equal(row.laborHours, 3);
    assert.equal(row.utilisationPercent, Math.round((180 / (5 * dayMinutes())) * 100));
  });

  it('requires a period', () => {
    expectStatus(world.app, 422, 'GET', '/reports/utilisation', 'admin');
    expectStatus(world.app, 422, 'GET', '/reports/utilisation?from=2026-03-02', 'admin');
  });
});

describe('GET /reports/backlog', () => {
  it('returns aging buckets and regional counts', () => {
    openWorkOrder(world.app, world.customer.id, world.site.id);
    openWorkOrder(world.app, world.customer.id, world.site.id, 'general', { priority: 'emergency' });
    assert.deepEqual(get('/reports/backlog'), {
      aging: [{ ageDays: 0, count: 2, emergencies: 1 }],
      byRegion: { 'metro-east': { new: 2 } },
    });
  });

  it('is empty without open work', () => {
    assert.deepEqual(get('/reports/backlog'), { aging: [], byRegion: {} });
  });
});

describe('GET /reports/revenue', () => {
  function issueInvoice() {
    const job = completeOne('10:00');
    const draft = call(world.app, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
    return call(world.app, 'POST', `/invoices/${draft.id}/issue`, 'billing');
  }

  it('returns rows per region and month', () => {
    const issued = issueInvoice();
    assert.deepEqual(get('/reports/revenue?from=2026-03-01&to=2026-04-01', 'billing').items, [
      { region: 'metro-east', month: '2026-03', invoices: 1, netCents: issued.subtotalCents, taxCents: issued.taxCents },
    ]);
  });

  it('exports CSV on request', () => {
    const issued = issueInvoice();
    const response = world.app.request('GET', '/reports/revenue?from=2026-03-01&to=2026-04-01&format=csv', { actor: 'billing' });
    assert.equal(response.status, 200);
    assert.equal(response.headers?.['content-type'], 'text/csv');
    assert.equal(
      response.body,
      `region,month,invoices,netCents,taxCents\nmetro-east,2026-03,1,${issued.subtotalCents},${issued.taxCents}\n`,
    );
  });

  it('exports a lone line feed when nothing was issued', () => {
    const response = world.app.request('GET', '/reports/revenue?from=2026-03-01&to=2026-04-01&format=csv', { actor: 'admin' });
    assert.equal(response.body, '\n');
  });

  it('falls back to JSON for other formats', () => {
    assert.deepEqual(get('/reports/revenue?from=2026-03-01&to=2026-04-01&format=xml').items, []);
  });

  it('requires a period', () => {
    expectStatus(world.app, 422, 'GET', '/reports/revenue?from=2026-03-01', 'admin');
  });
});

describe('GET /reports/activity', () => {
  it('counts recent events by type', () => {
    openWorkOrder(world.app, world.customer.id, world.site.id);
    const body = get('/reports/activity');
    assert.ok(Object.values(body.byType as Record<string, number>).every((count) => Number.isInteger(count) && count > 0));
    assert.equal(typeof body.lastEventAt, 'string');
  });

  it('limits the window', () => {
    for (let index = 0; index < 4; index += 1) openWorkOrder(world.app, world.customer.id, world.site.id);
    const total = (limit: number) =>
      Object.values(get(`/reports/activity?limit=${limit}`).byType as Record<string, number>).reduce((sum, count) => sum + count, 0);
    assert.equal(total(1), 1);
    assert.equal(total(3), 3);
    assert.ok(total(200) >= 4);
  });

  it('rejects limits outside 1 to 200', () => {
    for (const limit of ['0', '-5', '201', '2.5', 'many']) {
      assert.equal(expectStatus(world.app, 422, 'GET', `/reports/activity?limit=${limit}`, 'admin').error.code, 'invalid-input', limit);
    }
  });
});
'''

FILES = {
    "tests/reports/csv.test.ts": Spec({f"{REPORTS}/csv.ts": ["toCsv"]}, CSV),
    "tests/reports/kpis.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY", "seedContract"],
         f"{REPORTS}/kpis.ts": ["firstTimeFixRate", "kpiSnapshot", "meanHoursToComplete", "slaCompliancePercent"]},
        KPIS, jobs=["cancelJob", "holdJob", "reopenJob", "resumeJob"]),
    "tests/reports/utilisation.test.ts": scene_spec(
        {WORLD: ["at", "call", "MONDAY", "seedTechnician"], f"{REPORTS}/utilisation.ts": ["utilisationReport"],
         "src/timesheets/approvals.ts": ["approveEntries", "rejectEntry"],
         "src/timesheets/entries.ts": ["logEntry"]},
        UTILISATION),
    "tests/reports/backlog.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY", "seedCustomer"], f"{REPORTS}/backlog.ts": ["backlogAging", "backlogByRegion"]},
        BACKLOG, jobs=["cancelJob", "holdJob", "reopenJob", "resumeJob"]),
    "tests/reports/revenue.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY", "seedCustomer"], f"{REPORTS}/revenue.ts": ["revenueByRegion"],
         "src/billing/invoices.ts": ["draftInvoiceForJob", "issueInvoice", "voidInvoice"],
         "src/billing/payments.ts": ["recordPayment"]},
        REVENUE),
    "tests/reports/activity.test.ts": Spec(
        {"src/app.ts": ["createApp"], f"{REPORTS}/activity.ts": ["recentActivity"], WORLD: ["at", "MONDAY"],
         "src/context.ts": ["type AppContext"]}, ACTIVITY),
    "tests/api/reports.test.ts": Spec(
        {WORLD: ["at", "call", "completeChecklist", "createWorld", "expectStatus", "JOBS", "MONDAY",
                 "openWorkOrder", "seedTechnician", "type World"]}, API, node=WITH_SETUP),
}


def generate(ctx):
    register(ctx, FILES)
