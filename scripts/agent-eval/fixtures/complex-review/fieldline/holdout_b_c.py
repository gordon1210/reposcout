"""Memoised derived reads: collection revisions, a memo helper and its call sites with tests and docs."""

VARIANT = "holdout-b-c"

MEMO = r'''
import type { CollectionName } from '../adapters/store/memory.ts';
import type { AppContext } from '../context.ts';

interface Entry {
  revisions: number[];
  value: unknown;
}

interface Cache {
  entries: Map<string, Entry>;
  hits: number;
  misses: number;
}

export interface MemoStats {
  entries: number;
  hits: number;
  misses: number;
}

const caches = new WeakMap<object, Cache>();

function cacheOf(ctx: AppContext): Cache {
  let cache = caches.get(ctx.store);
  if (cache === undefined) {
    cache = { entries: new Map(), hits: 0, misses: 0 };
    caches.set(ctx.store, cache);
  }
  return cache;
}

/**
 * Returns the result of `compute` for `key`, reusing the previous result until one of the `deps`
 * collections is written. Results are copied on the way in and out, so callers may change what
 * they receive.
 */
export function memoize<T>(ctx: AppContext, key: string, deps: readonly CollectionName[], compute: () => T): T {
  const cache = cacheOf(ctx);
  const revisions = deps.map((name) => ctx.store.revision(name));
  const entry = cache.entries.get(key);
  if (entry !== undefined && entry.revisions.length === revisions.length && entry.revisions.every((revision, index) => revision === revisions[index])) {
    cache.hits += 1;
    return structuredClone(entry.value) as T;
  }
  cache.misses += 1;
  const value = compute();
  cache.entries.set(key, { revisions, value: structuredClone(value) });
  return value;
}

export function memoStats(ctx: AppContext): MemoStats {
  const cache = cacheOf(ctx);
  return { entries: cache.entries.size, hits: cache.hits, misses: cache.misses };
}

/** Drops every memoised result of the application; the hit and miss counters restart. */
export function clearMemo(ctx: AppContext): void {
  caches.delete(ctx.store);
}
'''

MEMO_TEST = r'''
import { createApp } from '../../src/app.ts';
import { clearMemo, memoize, memoStats } from '../../src/shared/memo.ts';

const fresh = () => createApp({ now: '2026-03-02T08:00:00Z' }).ctx;

describe('memoize', () => {
  it('computes once while the dependencies stay unchanged', () => {
    const ctx = fresh();
    let calls = 0;
    const compute = () => ({ run: (calls += 1) });
    assert.deepEqual(memoize(ctx, 'k', ['parts'], compute), { run: 1 });
    assert.deepEqual(memoize(ctx, 'k', ['parts'], compute), { run: 1 });
    assert.equal(calls, 1);
  });

  it('recomputes after a write to a dependency', () => {
    const ctx = fresh();
    let calls = 0;
    const compute = () => ({ run: (calls += 1) });
    memoize(ctx, 'k', ['parts'], compute);
    ctx.store.collection('parts').insert({ id: 'P-1' });
    assert.deepEqual(memoize(ctx, 'k', ['parts'], compute), { run: 2 });
  });

  it('recomputes after an update or a removal', () => {
    const ctx = fresh();
    const parts = ctx.store.collection<{ id: string; name?: string }>('parts');
    parts.insert({ id: 'P-1' });
    let calls = 0;
    const compute = () => ({ run: (calls += 1) });
    memoize(ctx, 'k', ['parts'], compute);
    parts.update('P-1', { name: 'Valve' });
    assert.equal(memoize(ctx, 'k', ['parts'], compute).run, 2);
    parts.remove('P-1');
    assert.equal(memoize(ctx, 'k', ['parts'], compute).run, 3);
  });

  it('ignores writes to collections it does not depend on', () => {
    const ctx = fresh();
    let calls = 0;
    const compute = () => ({ run: (calls += 1) });
    memoize(ctx, 'k', ['parts'], compute);
    ctx.store.collection('stock').insert({ id: 'S-1' });
    assert.equal(memoize(ctx, 'k', ['parts'], compute).run, 1);
  });

  it('depends on every listed collection', () => {
    const ctx = fresh();
    let calls = 0;
    const compute = () => ({ run: (calls += 1) });
    memoize(ctx, 'k', ['parts', 'stock'], compute);
    ctx.store.collection('stock').insert({ id: 'S-1' });
    assert.equal(memoize(ctx, 'k', ['parts', 'stock'], compute).run, 2);
  });

  it('keeps one entry per key', () => {
    const ctx = fresh();
    assert.equal(memoize(ctx, 'a', [], () => 1), 1);
    assert.equal(memoize(ctx, 'b', [], () => 2), 2);
    assert.equal(memoize(ctx, 'a', [], () => 3), 1);
    assert.equal(memoStats(ctx).entries, 2);
  });

  it('hands out copies of the stored result', () => {
    const ctx = fresh();
    const first = memoize(ctx, 'k', [], () => ({ items: [1, 2] }));
    first.items.push(3);
    assert.deepEqual(memoize(ctx, 'k', [], () => ({ items: [] })), { items: [1, 2] });
  });

  it('counts hits and misses', () => {
    const ctx = fresh();
    memoize(ctx, 'k', ['parts'], () => 1);
    memoize(ctx, 'k', ['parts'], () => 1);
    memoize(ctx, 'k', ['parts'], () => 1);
    assert.deepEqual(memoStats(ctx), { entries: 1, hits: 2, misses: 1 });
  });

  it('forgets everything on clear', () => {
    const ctx = fresh();
    memoize(ctx, 'k', [], () => 1);
    clearMemo(ctx);
    assert.deepEqual(memoStats(ctx), { entries: 0, hits: 0, misses: 0 });
    assert.equal(memoize(ctx, 'k', [], () => 2), 2);
  });

  it('keeps applications apart', () => {
    const first = fresh();
    const second = fresh();
    memoize(first, 'k', [], () => 1);
    assert.equal(memoize(second, 'k', [], () => 2), 2);
  });
});
'''

STORE_TEST = r'''
describe('collection revisions', () => {
  let revisions: Store;

  beforeEach(() => {
    revisions = createMemoryStore();
  });

  it('starts every collection at revision zero', () => {
    assert.equal(revisions.revision('parts'), 0);
    assert.equal(revisions.revision('jobs'), 0);
  });

  it('advances on insert, update and removal', () => {
    const parts = revisions.collection<Row>('parts');
    parts.insert(row('P-1', 'Valve'));
    assert.equal(revisions.revision('parts'), 1);
    parts.update('P-1', { qty: 3 });
    assert.equal(revisions.revision('parts'), 2);
    parts.remove('P-1');
    assert.equal(revisions.revision('parts'), 3);
  });

  it('does not advance for reads or for removing nothing', () => {
    const parts = revisions.collection<Row>('parts');
    parts.insert(row('P-1', 'Valve'));
    parts.get('P-1');
    parts.all();
    parts.remove('P-9');
    assert.equal(revisions.revision('parts'), 1);
  });

  it('does not advance when an insert or update is refused', () => {
    const parts = revisions.collection<Row>('parts');
    parts.insert(row('P-1', 'Valve'));
    assert.throws(() => parts.insert(row('P-1', 'Again')));
    assert.throws(() => parts.update('P-1', { id: 'P-2' }));
    assert.equal(revisions.revision('parts'), 1);
  });

  it('counts each collection separately', () => {
    revisions.collection<Row>('parts').insert(row('P-1', 'Valve'));
    assert.equal(revisions.revision('stock'), 0);
  });
});
'''

DAY_PLAN_TEST = r'''
import { memoStats } from '../../src/shared/memo.ts';

function fresh(options: Parameters<typeof createWorld>[0] = {}) {
  const world = createWorld(options);
  const ctx = world.app.ctx;
  const technician = technicians(ctx).require(world.technician.id, 'technician');
  const destination = world.site.address.location;
  return { ...world, ctx, technician, destination };
}

const labels = (slots: Array<{ start: string }>) => slots.map((slot) => slot.start.slice(11, 16));

const book = (world: ReturnType<typeof fresh>, start: string, technicianId = world.technician.id) => {
  const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
  return call(world.app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId, start, durationMinutes: 60 });
};

const slotsOf = (world: ReturnType<typeof fresh>, date = '2026-03-02') =>
  availableSlots(world.ctx, world.technician, date, 60, world.destination, appointmentLocations(world.ctx, world.technician.id, date));

describe('day plan reuse', () => {
  it('serves a repeated query from the cache', () => {
    const world = fresh();
    const first = slotsOf(world);
    const misses = memoStats(world.ctx).misses;
    assert.deepEqual(slotsOf(world), first);
    assert.equal(memoStats(world.ctx).misses, misses);
    assert.ok(memoStats(world.ctx).hits >= 1);
  });

  it('reflects a booking made after the first query', () => {
    const world = fresh();
    assert.ok(labels(slotsOf(world)).includes('09:00'));
    book(world, '2026-03-02T09:00:00Z');
    const after = labels(slotsOf(world));
    assert.equal(after.includes('09:00'), false);
    assert.equal(after.includes('09:30'), false);
  });

  it('reflects a cancellation made after the first query', () => {
    const world = fresh();
    const appointment = book(world, '2026-03-02T09:00:00Z');
    assert.equal(labels(slotsOf(world)).includes('09:00'), false);
    call(world.app, 'POST', `/appointments/${appointment.id}/cancel`, 'dispatch', { reason: 'Customer called' });
    assert.ok(labels(slotsOf(world)).includes('09:00'));
  });

  it('keeps technicians apart', () => {
    const world = fresh();
    const other = seedTechnician(world.app, { skills: ['plumbing'] });
    book(world, '2026-03-02T09:00:00Z', other.id);
    assert.ok(labels(slotsOf(world)).includes('09:00'));
  });

  it('keeps dates apart', () => {
    const world = fresh();
    book(world, '2026-03-02T09:00:00Z');
    assert.ok(labels(slotsOf(world, '2026-03-03')).includes('09:00'));
    assert.equal(labels(slotsOf(world, '2026-03-02')).includes('09:00'), false);
  });

  it('does not offer slots on a weekend from the cache either', () => {
    const world = fresh();
    assert.deepEqual(slotsOf(world, '2026-03-07'), []);
    assert.deepEqual(slotsOf(world, '2026-03-07'), []);
  });
});
'''

LOAD_TEST = r'''
import { recordAbsence } from '../../src/technicians/service.ts';
import { memoStats } from '../../src/shared/memo.ts';

function fresh() {
  const world = createWorld();
  const ctx = world.app.ctx;
  return { ...world, ctx, technician: technicians(ctx).require(world.technician.id, 'technician') };
}

const book = (world: ReturnType<typeof fresh>, start: string) => {
  const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
  return call(world.app, 'POST', '/appointments', 'dispatch', {
    jobId: job.id,
    technicianId: world.technician.id,
    start,
    durationMinutes: 60,
  });
};

describe('cached daily load', () => {
  it('serves a repeated query from the cache', () => {
    const world = fresh();
    const first = dailyLoad(world.ctx, world.technician, '2026-03-02');
    assert.deepEqual(dailyLoad(world.ctx, world.technician, '2026-03-02'), first);
    assert.equal(memoStats(world.ctx).hits, 1);
  });

  it('reflects bookings made after the first query', () => {
    const world = fresh();
    assert.equal(dailyLoad(world.ctx, world.technician, '2026-03-02').appointments, 0);
    book(world, '2026-03-02T09:00:00Z');
    assert.equal(dailyLoad(world.ctx, world.technician, '2026-03-02').appointments, 1);
    book(world, '2026-03-02T11:00:00Z');
    assert.equal(dailyLoad(world.ctx, world.technician, '2026-03-02').appointments, 2);
  });

  it('reflects an absence recorded after the first query', () => {
    const world = fresh();
    assert.equal(dailyLoad(world.ctx, world.technician, '2026-03-03').availableMinutes, 540);
    const away = recordAbsence(world.ctx, 'admin', world.technician.id, { from: '2026-03-03', to: '2026-03-03', kind: 'sick' });
    assert.equal(dailyLoad(world.ctx, away, '2026-03-03').availableMinutes, 0);
  });

  it('keeps technicians and dates apart', () => {
    const world = fresh();
    const other = seedTechnician(world.app, { skills: [] });
    book(world, '2026-03-02T09:00:00Z');
    assert.equal(dailyLoad(world.ctx, world.technician, '2026-03-03').appointments, 0);
    assert.equal(dailyLoad(world.ctx, technicians(world.ctx).require(other.id, 'technician'), '2026-03-02').appointments, 0);
  });

  it('feeds the least loaded ranking with current numbers', () => {
    const world = fresh();
    const other = seedTechnician(world.app, { skills: [] });
    assert.deepEqual(leastLoaded(world.ctx, 'metro-east', '2026-03-02').map((load) => load.technicianId), [world.technician.id, other.id]);
    book(world, '2026-03-02T09:00:00Z');
    assert.deepEqual(leastLoaded(world.ctx, 'metro-east', '2026-03-02').map((load) => load.technicianId), [other.id, world.technician.id]);
  });
});
'''

REVENUE_TEST = r'''
import { memoStats } from '../../src/shared/memo.ts';
import { revenueByRegion } from '../../src/reports/revenue.ts';

const issued = (id: string, customerId: string, issuedOn: string, subtotalCents: number, extra: Record<string, unknown> = {}) => ({
  id,
  customerId,
  jobIds: [],
  status: 'issued',
  currency: 'EUR',
  lines: [],
  subtotalCents,
  taxCents: Math.round(subtotalCents * 0.19),
  totalCents: Math.round(subtotalCents * 1.19),
  paidCents: 0,
  createdAt: `${issuedOn}T08:00:00Z`,
  issuedOn,
  ...extra,
});

describe('cached revenue report', () => {
  it('serves a repeated query from the cache', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    ctx.store.collection<any>('invoices').insert(issued('INV-1', world.customer.id, '2026-03-02', 10000));
    const first = revenueByRegion(ctx, '2026-03-01', '2026-04-01');
    assert.deepEqual(revenueByRegion(ctx, '2026-03-01', '2026-04-01'), first);
    assert.equal(memoStats(ctx).hits, 1);
    assert.deepEqual(first, [{ region: 'metro-east', month: '2026-03', invoices: 1, netCents: 10000, taxCents: 1900 }]);
  });

  it('reflects invoices issued after the first query', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    const invoices = ctx.store.collection<any>('invoices');
    invoices.insert(issued('INV-1', world.customer.id, '2026-03-02', 10000));
    assert.equal(revenueByRegion(ctx, '2026-03-01', '2026-04-01')[0].invoices, 1);
    invoices.insert(issued('INV-2', world.customer.id, '2026-03-05', 5000));
    const [row] = revenueByRegion(ctx, '2026-03-01', '2026-04-01');
    assert.equal(row.invoices, 2);
    assert.equal(row.netCents, 15000);
  });

  it('drops an invoice that is voided later', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    const invoices = ctx.store.collection<any>('invoices');
    invoices.insert(issued('INV-1', world.customer.id, '2026-03-02', 10000));
    assert.equal(revenueByRegion(ctx, '2026-03-01', '2026-04-01').length, 1);
    invoices.update('INV-1', { status: 'void' });
    assert.deepEqual(revenueByRegion(ctx, '2026-03-01', '2026-04-01'), []);
  });

  it('follows a change of the customer billing region', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    ctx.store.collection<any>('invoices').insert(issued('INV-1', world.customer.id, '2026-03-02', 10000));
    assert.equal(revenueByRegion(ctx, '2026-03-01', '2026-04-01')[0].region, 'metro-east');
    ctx.store.collection<any>('customers').update(world.customer.id, { billingRegion: 'northern-hills' });
    assert.equal(revenueByRegion(ctx, '2026-03-01', '2026-04-01')[0].region, 'northern-hills');
  });

  it('keeps periods apart', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    ctx.store.collection<any>('invoices').insert(issued('INV-1', world.customer.id, '2026-03-02', 10000));
    assert.equal(revenueByRegion(ctx, '2026-03-01', '2026-04-01').length, 1);
    assert.deepEqual(revenueByRegion(ctx, '2026-04-01', '2026-05-01'), []);
  });
});
'''

UTILISATION_TEST = r'''
import { memoStats } from '../../src/shared/memo.ts';
import { recordAbsence } from '../../src/technicians/service.ts';
import { utilisationReport } from '../../src/reports/utilisation.ts';

const entry = (id: string, technicianId: string, kind: string, start: string, minutes: number, status = 'approved') => ({
  id,
  technicianId,
  kind,
  start,
  end: start,
  minutes,
  status,
});

describe('cached utilisation report', () => {
  it('serves a repeated query from the cache', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    ctx.store.collection<any>('timeEntries').insert(entry('TE-1', world.technician.id, 'labor', '2026-03-02T09:00:00Z', 120));
    const first = utilisationReport(ctx, '2026-03-02', '2026-03-03');
    assert.deepEqual(utilisationReport(ctx, '2026-03-02', '2026-03-03'), first);
    assert.equal(memoStats(ctx).hits, 1);
    assert.equal(first[0].laborHours, 2);
    assert.equal(first[0].availableHours, 9);
    assert.equal(first[0].utilisationPercent, 22);
  });

  it('reflects entries logged after the first query', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    const entries = ctx.store.collection<any>('timeEntries');
    entries.insert(entry('TE-1', world.technician.id, 'labor', '2026-03-02T09:00:00Z', 120));
    assert.equal(utilisationReport(ctx, '2026-03-02', '2026-03-03')[0].laborHours, 2);
    entries.insert(entry('TE-2', world.technician.id, 'travel', '2026-03-02T12:00:00Z', 60));
    const [row] = utilisationReport(ctx, '2026-03-02', '2026-03-03');
    assert.equal(row.travelHours, 1);
    assert.equal(row.utilisationPercent, 33);
  });

  it('ignores entries that a later rejection removes from the total', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    const entries = ctx.store.collection<any>('timeEntries');
    entries.insert(entry('TE-1', world.technician.id, 'labor', '2026-03-02T09:00:00Z', 120));
    assert.equal(utilisationReport(ctx, '2026-03-02', '2026-03-03')[0].laborHours, 2);
    entries.update('TE-1', { status: 'rejected' });
    assert.equal(utilisationReport(ctx, '2026-03-02', '2026-03-03')[0].laborHours, 0);
  });

  it('lists technicians hired after the first query', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    assert.equal(utilisationReport(ctx, '2026-03-02', '2026-03-03').length, 1);
    seedTechnician(world.app, { skills: [] });
    assert.equal(utilisationReport(ctx, '2026-03-02', '2026-03-03').length, 2);
  });

  it('drops technicians deactivated after the first query', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    assert.equal(utilisationReport(ctx, '2026-03-02', '2026-03-03').length, 1);
    call(world.app, 'POST', `/technicians/${world.technician.id}/deactivate`, 'admin');
    assert.equal(utilisationReport(ctx, '2026-03-02', '2026-03-03').length, 0);
  });

  it('keeps periods apart', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    ctx.store.collection<any>('timeEntries').insert(entry('TE-1', world.technician.id, 'labor', '2026-03-02T09:00:00Z', 120));
    assert.equal(utilisationReport(ctx, '2026-03-02', '2026-03-03')[0].laborHours, 2);
    assert.equal(utilisationReport(ctx, '2026-03-03', '2026-03-04')[0].laborHours, 0);
  });

  it('is unaffected by absences, which do not change rostered working time', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    const before = utilisationReport(ctx, '2026-03-02', '2026-03-03');
    recordAbsence(ctx, 'admin', world.technician.id, { from: '2026-03-02', to: '2026-03-02', kind: 'sick' });
    assert.deepEqual(utilisationReport(ctx, '2026-03-02', '2026-03-03'), before);
  });
});
'''

BACKLOG_TEST = r'''
import { memoStats } from '../../src/shared/memo.ts';
import { backlogByRegion } from '../../src/reports/backlog.ts';

describe('cached backlog by region', () => {
  it('serves a repeated query from the cache', () => {
    const world = createWorld();
    openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    const first = backlogByRegion(world.app.ctx);
    assert.deepEqual(backlogByRegion(world.app.ctx), first);
    assert.deepEqual(first, { 'metro-east': { new: 1 } });
    assert.equal(memoStats(world.app.ctx).hits, 1);
  });

  it('reflects work orders opened after the first query', () => {
    const world = createWorld();
    openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    assert.deepEqual(backlogByRegion(world.app.ctx), { 'metro-east': { new: 1 } });
    openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    assert.deepEqual(backlogByRegion(world.app.ctx), { 'metro-east': { new: 2 } });
  });

  it('reflects status changes', () => {
    const world = createWorld();
    const created = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    assert.deepEqual(backlogByRegion(world.app.ctx), { 'metro-east': { new: 1 } });
    call(world.app, 'POST', `${JOBS}/${created.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    assert.deepEqual(backlogByRegion(world.app.ctx), { 'metro-east': { assigned: 1 } });
    call(world.app, 'POST', `${JOBS}/${created.id}/cancel`, 'dispatch', { reason: 'Customer withdrew the request' });
    assert.deepEqual(backlogByRegion(world.app.ctx), {});
  });

  it('follows a change of the customer billing region', () => {
    const world = createWorld();
    openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    assert.deepEqual(backlogByRegion(world.app.ctx), { 'metro-east': { new: 1 } });
    world.app.ctx.store.collection<any>('customers').update(world.customer.id, { billingRegion: 'valley' });
    assert.deepEqual(backlogByRegion(world.app.ctx), { valley: { new: 1 } });
  });
});
'''

STATEMENT_TEST = r'''
import { memoStats } from '../../src/shared/memo.ts';

const invoice = (customerId: string, id = 'INV-1', dueOn = '2026-03-03', totalCents = 11900) => ({
  id,
  number: `FL-${id}`,
  customerId,
  jobIds: [],
  status: 'issued',
  currency: 'EUR',
  lines: [],
  subtotalCents: Math.round(totalCents / 1.19),
  taxCents: totalCents - Math.round(totalCents / 1.19),
  totalCents,
  paidCents: 0,
  createdAt: '2026-02-01T08:00:00Z',
  issuedOn: '2026-02-01',
  dueOn,
});

describe('cached customer statement', () => {
  it('serves a repeated query from the cache', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    ctx.store.collection<any>('invoices').insert(invoice(world.customer.id));
    const first = customerStatement(ctx, world.customer.id, '2026-03-10');
    assert.deepEqual(customerStatement(ctx, world.customer.id, '2026-03-10'), first);
    assert.equal(memoStats(ctx).hits, 1);
    assert.equal(first.buckets['1-30'], 11900);
  });

  it('reflects credit notes issued after the first query', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    ctx.store.collection<any>('invoices').insert(invoice(world.customer.id));
    assert.equal(customerStatement(ctx, world.customer.id, '2026-03-10').totalOutstandingCents, 11900);
    ctx.store.collection<any>('creditNotes').insert({
      id: 'CN-1',
      number: 'FL-CN-1',
      invoiceId: 'INV-1',
      customerId: world.customer.id,
      amountCents: 1900,
      reason: 'Goodwill',
      issuedOn: '2026-03-05',
    });
    assert.equal(customerStatement(ctx, world.customer.id, '2026-03-10').totalOutstandingCents, 10000);
  });

  it('reflects payments recorded after the first query', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    const invoices = ctx.store.collection<any>('invoices');
    invoices.insert(invoice(world.customer.id));
    assert.equal(customerStatement(ctx, world.customer.id, '2026-03-10').lines.length, 1);
    invoices.update('INV-1', { paidCents: 11900, status: 'paid' });
    assert.equal(customerStatement(ctx, world.customer.id, '2026-03-10').lines.length, 0);
  });

  it('keeps customers and dates apart', () => {
    const world = createWorld();
    const ctx = world.app.ctx;
    ctx.store.collection<any>('invoices').insert(invoice(world.customer.id));
    assert.equal(customerStatement(ctx, world.customer.id, '2026-03-10').buckets['1-30'], 11900);
    assert.equal(customerStatement(ctx, world.customer.id, '2026-03-01').buckets.current, 11900);
    assert.equal(customerStatement(ctx, 'CUS-999999', '2026-03-10').totalOutstandingCents, 0);
  });
});
'''

VALUATION_TEST = r'''
import { memoStats } from '../../src/shared/memo.ts';

function fresh() {
  const app = createApp({ now: '2026-03-02T08:00:00Z' });
  const ctx = app.ctx;
  addPart(ctx, 'warehouse', { id: 'PUMP-CIRC', name: 'Circulation pump', category: 'heating', unitCostCents: 200, listPriceCents: 500 });
  return { app, ctx };
}

describe('cached stock valuation', () => {
  it('serves a repeated query from the cache', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 200);
    const first = stockValuation(ctx);
    assert.deepEqual(stockValuation(ctx), first);
    assert.equal(memoStats(ctx).hits, 1);
    assert.equal(first.totalCents, 1000);
  });

  it('reflects stock received after the first query', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 200);
    assert.equal(stockValuation(ctx).totalCents, 1000);
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 200);
    assert.equal(stockValuation(ctx).totalCents, 2000);
  });

  it('reflects a changed unit cost', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 200);
    assert.equal(stockValuation(ctx).totalCents, 1000);
    ctx.store.collection<any>('parts').update('PUMP-CIRC', { unitCostCents: 300 });
    assert.equal(stockValuation(ctx).totalCents, 1500);
  });

  it('keeps locations apart', () => {
    const { ctx } = fresh();
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 200);
    assert.equal(stockValuation(ctx, 'WH-CENTRAL').totalCents, 1000);
    assert.deepEqual(stockValuation(ctx, 'VAN-000001'), { lines: [], totalCents: 0 });
    assert.equal(stockValuation(ctx).totalCents, 1000);
  });
});
'''

SUMMARY_TEST = r'''
import { memoStats } from '../../src/shared/memo.ts';

function scene() {
  const created = createWorld();
  const target = openWorkOrder(created.app, created.customer.id, created.site.id, 'general');
  const tech = created.technician.id as string;
  const log = (date: string, start: string, end: string, kind = 'labor') =>
    logEntry(created.app.ctx, { technicianId: tech, jobId: target.id, kind, start: at(date, start), end: at(date, end) });
  return { ...created, ctx: created.app.ctx, tech, log };
}

describe('cached weekly summary', () => {
  it('serves a repeated query from the cache', () => {
    const world = scene();
    world.log(MONDAY, '09:00', '12:00');
    const first = weeklySummary(world.ctx, world.tech, MONDAY);
    assert.deepEqual(weeklySummary(world.ctx, world.tech, MONDAY), first);
    assert.equal(memoStats(world.ctx).hits, 1);
    assert.equal(first.hours.labor, 3);
  });

  it('reflects entries logged after the first query', () => {
    const world = scene();
    world.log(MONDAY, '09:00', '12:00');
    assert.equal(weeklySummary(world.ctx, world.tech, MONDAY).hours.labor, 3);
    world.log(MONDAY, '13:00', '14:00');
    assert.equal(weeklySummary(world.ctx, world.tech, MONDAY).hours.labor, 4);
  });

  it('reflects a rejection', () => {
    const world = scene();
    const entry = world.log(MONDAY, '09:00', '12:00');
    assert.equal(weeklySummary(world.ctx, world.tech, MONDAY).hours.labor, 3);
    rejectEntry(world.ctx, 'admin', entry.id, 'Wrong job');
    assert.equal(weeklySummary(world.ctx, world.tech, MONDAY).hours.labor, 0);
  });

  it('keeps technicians and weeks apart', () => {
    const world = scene();
    const other = seedTechnician(world.app, { skills: [] });
    world.log(MONDAY, '09:00', '12:00');
    assert.equal(weeklySummary(world.ctx, world.tech, MONDAY).hours.labor, 3);
    assert.equal(weeklySummary(world.ctx, other.id, MONDAY).hours.labor, 0);
    assert.equal(weeklySummary(world.ctx, world.tech, '2026-03-09').hours.labor, 0);
  });
});
'''

CACHE_API_TEST = r'''
import { call, createWorld, expectStatus, seedTechnician } from '../support/world.ts';
import type { World } from '../support/world.ts';

describe('GET /cache/stats', () => {
  let world: World;

  beforeEach(() => {
    world = createWorld();
  });

  it('starts empty', () => {
    assert.deepEqual(call(world.app, 'GET', '/cache/stats', 'admin'), { entries: 0, hits: 0, misses: 0 });
  });

  it('counts derived reads served from the cache', () => {
    const route = `/technicians/${world.technician.id}/load?date=2026-03-02`;
    call(world.app, 'GET', route, 'dispatch');
    call(world.app, 'GET', route, 'dispatch');
    call(world.app, 'GET', route, 'dispatch');
    assert.deepEqual(call(world.app, 'GET', '/cache/stats', 'admin'), { entries: 1, hits: 2, misses: 1 });
  });

  it('serves the load report of a technician from current data', () => {
    const route = `/technicians/${world.technician.id}/load?date=2026-03-02`;
    assert.equal(call(world.app, 'GET', route, 'dispatch').appointments, 0);
    const job = call(world.app, 'POST', '/jobs', 'dispatch', {
      customerId: world.customer.id,
      siteId: world.site.id,
      workType: 'general',
      summary: 'Investigate reported fault',
    });
    call(world.app, 'POST', '/appointments', 'dispatch', {
      jobId: job.id,
      technicianId: world.technician.id,
      start: '2026-03-02T09:00:00Z',
    });
    assert.equal(call(world.app, 'GET', route, 'dispatch').appointments, 1);
  });

  it('is reserved for administrators', () => {
    for (const actor of ['dispatch', 'billing', 'warehouse', world.technician.id]) {
      assert.equal(expectStatus(world.app, 403, 'GET', '/cache/stats', actor).error.code, 'forbidden', actor);
    }
  });

  it('is not affected by other technicians', () => {
    seedTechnician(world.app, { skills: [] });
    assert.equal(call(world.app, 'GET', '/cache/stats', 'admin').misses, 0);
  });
});
'''

REORDER_TEST = r'''
import { memoStats } from '../../src/shared/memo.ts';

function fresh() {
  const app = createApp({ now: '2026-03-02T08:00:00Z' });
  const ctx = app.ctx;
  const add = (id: string, extra: Record<string, unknown> = {}) =>
    addPart(ctx, 'warehouse', { id, name: `Part ${id}`, category: 'heating', unitCostCents: 200, listPriceCents: 500, ...extra });
  return { app, ctx, add };
}

describe('cached reorder suggestions', () => {
  it('serves a repeated query from the cache', () => {
    const { ctx, add } = fresh();
    add('PUMP-CIRC', { reorderPoint: 2, reorderQuantity: 6 });
    const first = reorderSuggestions(ctx, 'WH-CENTRAL');
    assert.deepEqual(reorderSuggestions(ctx, 'WH-CENTRAL'), first);
    assert.equal(memoStats(ctx).hits, 1);
    assert.equal(first.length, 1);
  });

  it('reflects stock received after the first query', () => {
    const { ctx, add } = fresh();
    add('PUMP-CIRC', { reorderPoint: 2, reorderQuantity: 6 });
    assert.equal(reorderSuggestions(ctx, 'WH-CENTRAL').length, 1);
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 200);
    assert.deepEqual(reorderSuggestions(ctx, 'WH-CENTRAL'), []);
  });

  it('reflects a changed reorder point', () => {
    const { ctx, add } = fresh();
    add('PUMP-CIRC', { reorderPoint: 2, reorderQuantity: 6 });
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 200);
    assert.deepEqual(reorderSuggestions(ctx, 'WH-CENTRAL'), []);
    ctx.store.collection<any>('parts').update('PUMP-CIRC', { reorderPoint: 8 });
    assert.equal(reorderSuggestions(ctx, 'WH-CENTRAL').length, 1);
  });

  it('reflects backorders raised after the first query', () => {
    const { ctx, add } = fresh();
    add('PUMP-CIRC', { reorderPoint: 2, reorderQuantity: 6 });
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 200);
    assert.deepEqual(reorderSuggestions(ctx, 'WH-CENTRAL'), []);
    ctx.store.collection<any>('reservations').insert({
      id: 'RSV-1',
      jobId: 'WO-000001',
      sku: 'PUMP-CIRC',
      quantity: 4,
      status: 'backordered',
      locationId: 'WH-CENTRAL',
    });
    const [suggestion] = reorderSuggestions(ctx, 'WH-CENTRAL');
    assert.equal(suggestion.quantity, 10);
  });

  it('keeps locations apart', () => {
    const { ctx, add } = fresh();
    add('PUMP-CIRC', { reorderPoint: 2, reorderQuantity: 6 });
    receiveStock(ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 5, 200);
    assert.deepEqual(reorderSuggestions(ctx, 'WH-CENTRAL'), []);
    assert.equal(reorderSuggestions(ctx, 'VAN-000001').length, 1);
  });
});
'''

CERTIFICATION_TEST = r'''
import { memoStats } from '../../src/shared/memo.ts';

function fresh() {
  const app = createApp({ now: '2026-03-02T08:00:00Z' });
  const technician = createTechnician(app.ctx, 'admin', {
    name: 'Robin Hale',
    email: 'robin@fieldline.example',
    region: 'metro-east',
    home: { lat: 52.5, lon: 13.38 },
    hourlyCostCents: 3100,
    certifications: [{ skill: 'gas-safe', expiresOn: '2026-03-20' }],
  });
  return { ctx: app.ctx, technician };
}

describe('cached expiring certifications', () => {
  it('serves a repeated query from the cache', () => {
    const { ctx } = fresh();
    const first = expiringCertifications(ctx, '2026-03-02', 30);
    assert.deepEqual(expiringCertifications(ctx, '2026-03-02', 30), first);
    assert.equal(memoStats(ctx).hits, 1);
    assert.deepEqual(first.map((entry) => entry.skill), ['gas-safe']);
  });

  it('reflects renewed certifications', () => {
    const { ctx, technician } = fresh();
    assert.equal(expiringCertifications(ctx, '2026-03-02', 30).length, 1);
    updateCertifications(ctx, 'admin', technician.id, [{ skill: 'gas-safe', expiresOn: '2027-03-20' }]);
    assert.deepEqual(expiringCertifications(ctx, '2026-03-02', 30), []);
  });

  it('drops technicians who are deactivated', () => {
    const { ctx, technician } = fresh();
    assert.equal(expiringCertifications(ctx, '2026-03-02', 30).length, 1);
    deactivateTechnician(ctx, 'admin', technician.id, 0);
    assert.deepEqual(expiringCertifications(ctx, '2026-03-02', 30), []);
  });

  it('keeps windows apart', () => {
    const { ctx } = fresh();
    assert.equal(expiringCertifications(ctx, '2026-03-02', 10).length, 0);
    assert.equal(expiringCertifications(ctx, '2026-03-02', 30).length, 1);
    assert.equal(expiringCertifications(ctx, '2026-03-25', 30).length, 0);
  });
});
'''

ARCHITECTURE_SECTION = '''
## Derived data

Expensive derived reads are memoised per application with `memoize` (`src/shared/memo.ts`):
daily plans and loads of technicians, stock valuation, customer statements, weekly timesheet
summaries, reorder suggestions, expiring certifications and the revenue, utilisation and
backlog-by-region reports. A cached result is reused
until a collection named in its dependency list is written; the store counts writes per
collection (`store.revision`). A dependency list must name every collection the computation
reads, directly or through the technician or customer records it is handed. Reads that depend on
the clock are not memoised. `GET /cache/stats` (administrators) reports entries, hits and misses.
'''


def generate(ctx):
    if ctx.variant != VARIANT:
        return

    # Store: per-collection write counters.
    ctx.patch("src/adapters/store/memory.ts",
              "  collection<T extends Entity>(name: CollectionName): Collection<T>;\n  snapshot(): Record<string, Entity[]>;",
              "  collection<T extends Entity>(name: CollectionName): Collection<T>;\n"
              "  /** Number of writes the collection has received; reads never change it. */\n"
              "  revision(name: CollectionName): number;\n"
              "  snapshot(): Record<string, Entity[]>;")
    ctx.patch("src/adapters/store/memory.ts",
              "function createCollection<T extends Entity>(name: string): Collection<T> {",
              "function createCollection<T extends Entity>(name: string, onWrite: () => void): Collection<T> {")
    ctx.patch("src/adapters/store/memory.ts",
              "      records.set(record.id, structuredClone(record));\n      return structuredClone(record);",
              "      records.set(record.id, structuredClone(record));\n      onWrite();\n      return structuredClone(record);")
    ctx.patch("src/adapters/store/memory.ts",
              "      records.set(id, structuredClone(next));\n      return structuredClone(next);",
              "      records.set(id, structuredClone(next));\n      onWrite();\n      return structuredClone(next);")
    ctx.patch("src/adapters/store/memory.ts",
              "    remove(id) {\n      return records.delete(id);\n    },",
              "    remove(id) {\n      const removed = records.delete(id);\n      if (removed) onWrite();\n      return removed;\n    },")
    ctx.patch("src/adapters/store/memory.ts",
              "  const collections = new Map<string, Collection<Entity>>();\n  return {",
              "  const collections = new Map<string, Collection<Entity>>();\n"
              "  const revisions = new Map<string, number>();\n  return {")
    ctx.patch("src/adapters/store/memory.ts",
              "        collection = createCollection<Entity>(name);",
              "        collection = createCollection<Entity>(name, () => revisions.set(name, (revisions.get(name) ?? 0) + 1));")
    ctx.patch("src/adapters/store/memory.ts",
              "    snapshot() {",
              "    revision(name: CollectionName): number {\n      return revisions.get(name) ?? 0;\n    },\n    snapshot() {")

    ctx.add("src/shared/memo.ts", MEMO)

    # Slots: the technician's day plan.
    path = "src/scheduling/slots.ts"
    ctx.patch(path,
              "import { appointmentsForTechnician } from '../adapters/repositories/appointments.ts';\n",
              "import { appointmentsForTechnician } from '../adapters/repositories/appointments.ts';\n"
              "import type { CollectionName } from '../adapters/store/memory.ts';\n")
    ctx.patch(path,
              "import type { Coordinates } from '../shared/geo.ts';\n",
              "import type { Coordinates } from '../shared/geo.ts';\nimport { memoize } from '../shared/memo.ts';\n")
    ctx.patch(path,
              "interface Busy {\n  from: number;\n  to: number;\n  location: Coordinates;\n}\n\n"
              "function busyPeriods(ctx: AppContext, technicianId: string, date: CalendarDate, sites: Map<string, Coordinates>): Busy[] {\n"
              "  const dayStart = toEpochMinutes(atMinute(date, 0));\n"
              "  return appointmentsForTechnician(ctx, technicianId, date)\n"
              "    .filter((appointment) => appointment.status !== 'missed')\n"
              "    .map((appointment) => ({\n"
              "      from: toEpochMinutes(appointment.start) - dayStart,\n"
              "      to: toEpochMinutes(appointment.end) - dayStart,\n"
              "      location: sites.get(appointment.jobId) ?? { lat: 0, lon: 0 },\n"
              "    }));\n}\n",
              "interface Busy {\n  from: number;\n  to: number;\n  location: Coordinates;\n}\n\n"
              "interface DayPlan {\n"
              "  bounds: [number, number] | undefined;\n"
              "  periods: Array<{ from: number; to: number; jobId: string }>;\n"
              "}\n\n"
              "/** Collections a technician's day plan is derived from. */\n"
              "const DAY_PLAN_SOURCES: readonly CollectionName[] = ['appointments'];\n\n"
              "/** Working bounds and live appointment periods of a technician on a date, in minutes of that day. */\n"
              "function dayPlan(ctx: AppContext, technician: Technician, date: CalendarDate): DayPlan {\n"
              "  return memoize(ctx, `day-plan:${technician.id}:${date}`, DAY_PLAN_SOURCES, () => {\n"
              "    const dayStart = toEpochMinutes(atMinute(date, 0));\n"
              "    return {\n"
              "      bounds: workingBounds(ctx, technician, date),\n"
              "      periods: appointmentsForTechnician(ctx, technician.id, date)\n"
              "        .filter((appointment) => appointment.status !== 'missed')\n"
              "        .map((appointment) => ({\n"
              "          from: toEpochMinutes(appointment.start) - dayStart,\n"
              "          to: toEpochMinutes(appointment.end) - dayStart,\n"
              "          jobId: appointment.jobId,\n"
              "        })),\n"
              "    };\n"
              "  });\n}\n\n"
              "function busyPeriods(plan: DayPlan, sites: Map<string, Coordinates>): Busy[] {\n"
              "  return plan.periods.map((period) => ({\n"
              "    from: period.from,\n"
              "    to: period.to,\n"
              "    location: sites.get(period.jobId) ?? { lat: 0, lon: 0 },\n"
              "  }));\n}\n")
    ctx.patch(path,
              "  const bounds = workingBounds(ctx, technician, date);\n  if (!bounds) return [];\n"
              "  const { slotMinutes, bufferMinutes, travelSpeedKmh, maxDailyAppointments } = ctx.config.scheduling;\n"
              "  const busy = busyPeriods(ctx, technician.id, date, jobLocations);",
              "  const plan = dayPlan(ctx, technician, date);\n  const bounds = plan.bounds;\n  if (!bounds) return [];\n"
              "  const { slotMinutes, bufferMinutes, travelSpeedKmh, maxDailyAppointments } = ctx.config.scheduling;\n"
              "  const busy = busyPeriods(plan, jobLocations);")

    # Capacity: the cached daily load.
    path = "src/scheduling/capacity.ts"
    ctx.patch(path,
              "import type { AppContext } from '../context.ts';\n",
              "import type { AppContext } from '../context.ts';\nimport { memoize } from '../shared/memo.ts';\n")
    ctx.patch(path,
              "export function dailyLoad(ctx: AppContext, technician: Technician, date: CalendarDate): DailyLoad {\n"
              "  const bounds",
              "export function dailyLoad(ctx: AppContext, technician: Technician, date: CalendarDate): DailyLoad {\n"
              "  return memoize(ctx, `daily-load:${technician.id}:${date}`, ['appointments', 'technicians'], () =>\n"
              "    computeDailyLoad(ctx, technician, date),\n  );\n}\n\n"
              "function computeDailyLoad(ctx: AppContext, technician: Technician, date: CalendarDate): DailyLoad {\n"
              "  const bounds")

    # Reports.
    path = "src/reports/revenue.ts"
    ctx.patch(path, "import type { Cents } from '../shared/money.ts';\n",
              "import { memoize } from '../shared/memo.ts';\nimport type { Cents } from '../shared/money.ts';\n")
    ctx.patch(path,
              "export function revenueByRegion(ctx: AppContext, from: CalendarDate, to: CalendarDate): RevenueRow[] {\n"
              "  const issued",
              "export function revenueByRegion(ctx: AppContext, from: CalendarDate, to: CalendarDate): RevenueRow[] {\n"
              "  return memoize(ctx, `revenue:${from}:${to}`, ['invoices', 'customers'], () => computeRevenue(ctx, from, to));\n}\n\n"
              "function computeRevenue(ctx: AppContext, from: CalendarDate, to: CalendarDate): RevenueRow[] {\n"
              "  const issued")

    path = "src/reports/utilisation.ts"
    ctx.patch(path, "import { roundToIncrement } from '../shared/rounding.ts';\n",
              "import { memoize } from '../shared/memo.ts';\nimport { roundToIncrement } from '../shared/rounding.ts';\n")
    ctx.patch(path,
              "export function utilisationReport(ctx: AppContext, from: CalendarDate, to: CalendarDate): UtilisationRow[] {\n"
              "  const calendar",
              "export function utilisationReport(ctx: AppContext, from: CalendarDate, to: CalendarDate): UtilisationRow[] {\n"
              "  return memoize(ctx, `utilisation:${from}:${to}`, ['technicians', 'timeEntries'], () => computeUtilisation(ctx, from, to));\n}\n\n"
              "function computeUtilisation(ctx: AppContext, from: CalendarDate, to: CalendarDate): UtilisationRow[] {\n"
              "  const calendar")

    path = "src/reports/backlog.ts"
    ctx.patch(path, "import { jobAge, listJobs, OPEN_STATUSES } from '../jobs/jobs.ts';\n",
              "import { jobAge, listJobs, OPEN_STATUSES } from '../jobs/jobs.ts';\nimport { memoize } from '../shared/memo.ts';\n")
    ctx.patch(path,
              "export function backlogByRegion(ctx: AppContext): Record<string, Record<string, number>> {\n"
              "  const result",
              "export function backlogByRegion(ctx: AppContext): Record<string, Record<string, number>> {\n"
              "  return memoize(ctx, 'backlog-by-region', ['jobs', 'customers'], () => computeBacklogByRegion(ctx));\n}\n\n"
              "function computeBacklogByRegion(ctx: AppContext): Record<string, Record<string, number>> {\n"
              "  const result")

    # Inventory, billing and timesheets.
    path = "src/inventory/valuation.ts"
    ctx.patch(path, "import { multiplyCents, sumCents } from '../shared/money.ts';\n",
              "import { memoize } from '../shared/memo.ts';\nimport { multiplyCents, sumCents } from '../shared/money.ts';\n")
    ctx.patch(path,
              "export function stockValuation(ctx: AppContext, locationId?: string): { lines: ValuationLine[]; totalCents: Cents } {\n"
              "  const levels",
              "export function stockValuation(ctx: AppContext, locationId?: string): { lines: ValuationLine[]; totalCents: Cents } {\n"
              "  return memoize(ctx, `valuation:${locationId ?? '*'}`, ['stock', 'parts'], () => computeValuation(ctx, locationId));\n}\n\n"
              "function computeValuation(ctx: AppContext, locationId?: string): { lines: ValuationLine[]; totalCents: Cents } {\n"
              "  const levels")

    path = "src/billing/statements.ts"
    ctx.patch(path, "import type { Cents } from '../shared/money.ts';\n",
              "import { memoize } from '../shared/memo.ts';\nimport type { Cents } from '../shared/money.ts';\n")
    ctx.patch(path,
              "export function customerStatement(ctx: AppContext, customerId: string, asOf: CalendarDate): Statement {\n"
              "  const lines",
              "export function customerStatement(ctx: AppContext, customerId: string, asOf: CalendarDate): Statement {\n"
              "  return memoize(ctx, `statement:${customerId}:${asOf}`, ['invoices', 'creditNotes'], () =>\n"
              "    computeStatement(ctx, customerId, asOf),\n  );\n}\n\n"
              "function computeStatement(ctx: AppContext, customerId: string, asOf: CalendarDate): Statement {\n"
              "  const lines")

    path = "src/inventory/reorder.ts"
    ctx.patch(path, "import { floorToIncrement, incrementsToCover, roundToIncrement } from '../shared/rounding.ts';\n",
              "import { memoize } from '../shared/memo.ts';\n"
              "import { floorToIncrement, incrementsToCover, roundToIncrement } from '../shared/rounding.ts';\n")
    ctx.patch(path,
              "export function reorderSuggestions(ctx: AppContext, locationId: string): ReorderSuggestion[] {\n"
              "  const result",
              "export function reorderSuggestions(ctx: AppContext, locationId: string): ReorderSuggestion[] {\n"
              "  return memoize(ctx, `reorder:${locationId}`, ['parts', 'stock', 'reservations'], () => computeReorder(ctx, locationId));\n}\n\n"
              "function computeReorder(ctx: AppContext, locationId: string): ReorderSuggestion[] {\n"
              "  const result")

    path = "src/technicians/skills.ts"
    ctx.patch(path, "import { daysBetween } from '../shared/time.ts';\n",
              "import { memoize } from '../shared/memo.ts';\nimport { daysBetween } from '../shared/time.ts';\n")
    ctx.patch(path,
              "export function expiringCertifications(ctx: AppContext, on: CalendarDate, withinDays: number): ExpiringCertification[] {\n"
              "  const result",
              "export function expiringCertifications(ctx: AppContext, on: CalendarDate, withinDays: number): ExpiringCertification[] {\n"
              "  return memoize(ctx, `expiring-certifications:${on}:${withinDays}`, ['technicians'], () =>\n"
              "    computeExpiring(ctx, on, withinDays),\n  );\n}\n\n"
              "function computeExpiring(ctx: AppContext, on: CalendarDate, withinDays: number): ExpiringCertification[] {\n"
              "  const result")

    path = "src/timesheets/summary.ts"
    ctx.patch(path, "import type { AppContext } from '../context.ts';\n",
              "import type { AppContext } from '../context.ts';\nimport { memoize } from '../shared/memo.ts';\n")
    ctx.patch(path,
              "export function weeklySummary(ctx: AppContext, technicianId: string, weekStart: CalendarDate): WeeklySummary {\n"
              "  const entries",
              "export function weeklySummary(ctx: AppContext, technicianId: string, weekStart: CalendarDate): WeeklySummary {\n"
              "  return memoize(ctx, `weekly-summary:${technicianId}:${weekStart}`, ['timeEntries'], () =>\n"
              "    computeWeeklySummary(ctx, technicianId, weekStart),\n  );\n}\n\n"
              "function computeWeeklySummary(ctx: AppContext, technicianId: string, weekStart: CalendarDate): WeeklySummary {\n"
              "  const entries")

    # Diagnostics route.
    path = "src/api/routes/system.ts"
    ctx.patch(path, "import { authorize } from '../../auth/permissions.ts';\n",
              "import { authorize } from '../../auth/permissions.ts';\nimport { memoStats } from '../../shared/memo.ts';\n")
    ctx.patch(path,
              "  router.get('/work-types', (ctx, request) => {",
              "  router.get('/cache/stats', (ctx, request) => {\n"
              "    authorize(request.actor, 'system:admin');\n"
              "    return ok(memoStats(ctx));\n"
              "  });\n\n"
              "  router.get('/work-types', (ctx, request) => {")
    ctx.patch("tests/api/security/permissions.test.ts",
              "      ['GET', '/config'], ['GET', `/audit/${MISSING}`],",
              "      ['GET', '/config'], ['GET', '/cache/stats'], ['GET', `/audit/${MISSING}`],")

    # Tests.
    ctx.append("tests/adapters/memory-store.test.ts", STORE_TEST)
    ctx.add("tests/shared/memo.test.ts", _test(MEMO_TEST, imports_head=True))
    ctx.add("tests/scheduling/day-plan.test.ts", _test(
        DAY_PLAN_TEST,
        world=["call", "createWorld", "openWorkOrder", "seedTechnician"],
        extra=("import { technicians } from '../../src/adapters/repositories/technicians.ts';\n"
               "import { appointmentLocations } from '../../src/scheduling/locations.ts';\n"
               "import { availableSlots } from '../../src/scheduling/slots.ts';\n")))
    ctx.add("tests/scheduling/load-cache.test.ts", _test(
        LOAD_TEST,
        world=["call", "createWorld", "openWorkOrder", "seedTechnician"],
        extra=("import { technicians } from '../../src/adapters/repositories/technicians.ts';\n"
               "import { dailyLoad, leastLoaded } from '../../src/scheduling/capacity.ts';\n")))
    ctx.add("tests/reports/revenue-cache.test.ts", _test(REVENUE_TEST, world=["createWorld"]))
    ctx.add("tests/reports/utilisation-cache.test.ts", _test(UTILISATION_TEST, world=["call", "createWorld", "seedTechnician"]))
    ctx.add("tests/reports/backlog-cache.test.ts", _test(BACKLOG_TEST, world=["call", "createWorld", "JOBS", "openWorkOrder"]))
    ctx.add("tests/billing/statement-cache.test.ts", _test(
        STATEMENT_TEST, world=["createWorld"],
        extra="import { customerStatement } from '../../src/billing/statements.ts';\n"))
    ctx.add("tests/inventory/valuation-cache.test.ts", _test(
        VALUATION_TEST, world=[],
        extra=("import { createApp } from '../../src/app.ts';\n"
               "import { addPart } from '../../src/inventory/catalog.ts';\n"
               "import { receiveStock } from '../../src/inventory/stock.ts';\n"
               "import { stockValuation } from '../../src/inventory/valuation.ts';\n")))
    ctx.add("tests/timesheets/summary-cache.test.ts", _test(
        SUMMARY_TEST, world=["at", "createWorld", "MONDAY", "openWorkOrder", "seedTechnician"],
        extra=("import { rejectEntry } from '../../src/timesheets/approvals.ts';\n"
               "import { logEntry } from '../../src/timesheets/entries.ts';\n"
               "import { weeklySummary } from '../../src/timesheets/summary.ts';\n")))
    ctx.add("tests/inventory/reorder-cache.test.ts", _test(
        REORDER_TEST, world=[],
        extra=("import { createApp } from '../../src/app.ts';\n"
               "import { addPart } from '../../src/inventory/catalog.ts';\n"
               "import { reorderSuggestions } from '../../src/inventory/reorder.ts';\n"
               "import { receiveStock } from '../../src/inventory/stock.ts';\n")))
    ctx.add("tests/technicians/certifications-cache.test.ts", _test(
        CERTIFICATION_TEST, world=[],
        extra=("import { createApp } from '../../src/app.ts';\n"
               "import { createTechnician, deactivateTechnician, updateCertifications } from '../../src/technicians/service.ts';\n"
               "import { expiringCertifications } from '../../src/technicians/skills.ts';\n")))
    ctx.add("tests/api/cache-stats.test.ts", _test(CACHE_API_TEST, world=None, setup=True))

    # Documentation.
    ctx.append("docs/architecture.md", ARCHITECTURE_SECTION)
    ctx.patch("docs/scheduling.md",
              "## Slots\n",
              "## Slots\n\nDaily plans and loads are derived data and are memoised (see the architecture notes).\n")
    ctx.patch("docs/reports.md",
              "All reports require `reports:read`.\n",
              "All reports require `reports:read`. Utilisation, revenue and the backlog by region are memoised\n"
              "per application and always reflect the data at the time of the request.\n")


def _test(body, world=(), extra="", imports_head=False, setup=False):
    """A test file: node imports, the world helpers when requested, extra imports, then the body."""
    head = "import assert from 'node:assert/strict';\n"
    head += "import { beforeEach, describe, it } from 'node:test';\n" if setup else "import { describe, it } from 'node:test';\n"
    if world:
        names = ", ".join(sorted(world, key=str.lower))
        head += f"import {{ {names} }} from '../support/world.ts';\n"
    head += extra
    return head + body
