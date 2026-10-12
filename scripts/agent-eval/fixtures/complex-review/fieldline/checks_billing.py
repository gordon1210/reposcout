"""Tests for billing: totals, discounts, parts pricing, tariffs, invoices, payments, credits and statements."""

from fractions import Fraction

from .billing import TARIFFS, step_field
from .checks_kit import DESCRIBE_IT, WITH_SETUP, Spec, register
from .checks_workorders import scene_spec

WORLD = "tests/support/world.ts"
BILLING = "src/billing"
LOGENTRY = "src/timesheets/entries.ts"

# Completes a general work order on a fresh technician and drafts, issues or inspects its invoice.
BILL = r'''
const hhmm = (minutes: number) => `${String(Math.floor(minutes / 60)).padStart(2, '0')}:${String(minutes % 60).padStart(2, '0')}`;

interface Work {
  labor?: number;
  travel?: { km: number; minutes: number };
  parts?: Array<{ sku: string; quantity: number }>;
  on?: string;
}

function done(scene: Scene, work: Work = {}) {
  scene.app.clock.set(at(work.on ?? MONDAY, '12:00'));
  const tech = work.parts ? scene.tech : (seedTechnician(scene.app, { skills: ['plumbing'] }).id as string);
  const job = open(scene);
  «assignJob»(scene.ctx, 'dispatch', job.id, tech);
  «startJob»(scene.ctx, tech, job.id);
  if (work.labor) {
    logEntry(scene.ctx, { technicianId: tech, jobId: job.id, kind: 'labor', start: at(MONDAY, '09:00'), end: at(MONDAY, hhmm(540 + work.labor)) });
  }
  if (work.travel) {
    logEntry(scene.ctx, { technicianId: tech, jobId: job.id, kind: 'travel', start: at(MONDAY, '13:00'), end: at(MONDAY, hhmm(780 + work.travel.minutes)), km: work.travel.km });
  }
  for (const item of Object.keys(«requireJob»(scene.ctx, job.id).checklist)) «recordChecklistItem»(scene.ctx, tech, job.id, item, true);
  «completeJob»(scene.ctx, tech, job.id, { resolution: 'Replaced the faulty component and tested', partsUsed: work.parts });
  return job;
}

const drafted = (scene: Scene, work: Work = {}) => draftInvoiceForJob(scene.ctx, 'billing', done(scene, work).id);
const issued = (scene: Scene, work: Work = {}) => issueInvoice(scene.ctx, 'billing', drafted(scene, work).id);
const kinds = (invoice: { lines: Array<{ kind: string }> }) => invoice.lines.map((line) => line.kind);
const region = (scene: Scene, billingRegion: string) => {
  call(scene.app, 'PATCH', `/customers/${scene.customer.id}`, 'admin', { billingRegion });
};
'''

BILL_JOBS = ("completeJob",)
BILL_IMPORTS = {
    WORLD: ["MONDAY", "at", "call", "seedTechnician"],
    LOGENTRY: ["logEntry"],
    f"{BILLING}/invoices.ts": ["draftInvoiceForJob", "issueInvoice"],
}


def bill_spec(groups, body, jobs=(), **extra):
    def merge(ctx):
        merged = {target: list(names) for target, names in BILL_IMPORTS.items()}
        for target, names in (groups(ctx) if callable(groups) else groups).items():
            merged[target] = sorted(set(merged.get(target, [])) | set(names))
        return merged

    return scene_spec(merge, BILL + body, jobs=tuple(set(BILL_JOBS) | set(jobs)), **extra)


TAX = r'''
const line = (amountCents: number, taxable = true, kind = 'labor') => ({
  kind: kind as any,
  description: 'Line',
  quantity: 1,
  unitCents: amountCents,
  amountCents,
  taxable,
});

describe('computeTotals', () => {
  it('is zero without lines', () => {
    assert.deepEqual(computeTotals([], 1900, false), { subtotalCents: 0, taxCents: 0, totalCents: 0 });
  });

  it('taxes taxable lines only', () => {
    assert.deepEqual(computeTotals([line(10000), line(5000, false, 'rounding')], 1900, false), {
      subtotalCents: 15000,
      taxCents: 1900,
      totalCents: 16900,
    });
  });

  it('charges no tax to exempt customers', () => {
    assert.deepEqual(computeTotals([line(10000)], 1900, true), { subtotalCents: 10000, taxCents: 0, totalCents: 10000 });
  });

  it('lets discounts reduce the taxable base', () => {
    assert.equal(computeTotals([line(10000), line(-2000, true, 'discount')], 1900, false).taxCents, 1520);
  });

  it('never refunds tax on a negative taxable base', () => {
    const totals = computeTotals([line(-1000, true, 'credit')], 1900, false);
    assert.equal(totals.taxCents, 0);
    assert.equal(totals.totalCents, -1000);
  });

  it('computes tax once on the sum, not per line', () => {
    assert.equal(computeTotals([line(333), line(333), line(333)], 1900, false).taxCents, 190);
  });

  const cases: Array<[number, number, number]> = [
    [10000, 1900, 1900],
    [20000, 810, 1620],
    [999, 1900, 190],
    [12345, 810, 1000],
    [10000, 0, 0],
    [4000, 2500, 1000],
  ];
  for (const [amount, rate, tax] of cases) {
    it(`taxes ${amount} at ${rate} basis points`, () => {
      assert.equal(computeTotals([line(amount)], rate, false).taxCents, tax);
    });
  }
});
'''

DISCOUNTS = r'''
const line = (kind: string, amountCents: number) => ({
  kind: kind as any,
  description: 'Line',
  quantity: 1,
  unitCents: amountCents,
  amountCents,
  taxable: true,
});

const contract = (plan: string, laborDiscountBasisPoints: number) => ({ plan, laborDiscountBasisPoints }) as any;

describe('contractDiscountLine', () => {
  it('discounts standard and after-hours labour only', () => {
    const discount = contractDiscountLine(
      [line('callout', 4500), line('labor', 7200), line('after-hours', 3600), line('part', 2000), line('travel', 1000)],
      contract('priority', 500),
    );
    assert.equal(discount?.amountCents, -540);
    assert.equal(discount?.unitCents, -540);
    assert.equal(discount?.kind, 'discount');
    assert.equal(discount?.taxable, true);
    assert.equal(discount?.description, 'priority contract labour discount');
  });

  it('applies the rate of the plan', () => {
    assert.equal(contractDiscountLine([line('labor', 10000)], contract('premium', 1000))?.amountCents, -1000);
    assert.equal(contractDiscountLine([line('labor', 10000)], contract('basic', 250))?.amountCents, -250);
  });

  it('is absent without a contract', () => {
    assert.equal(contractDiscountLine([line('labor', 7200)], undefined), undefined);
  });

  it('is absent when the plan has no labour discount', () => {
    assert.equal(contractDiscountLine([line('labor', 7200)], contract('basic', 0)), undefined);
  });

  it('is absent without labour', () => {
    assert.equal(contractDiscountLine([line('callout', 4500), line('part', 2000)], contract('premium', 1000)), undefined);
    assert.equal(contractDiscountLine([], contract('premium', 1000)), undefined);
  });
});

describe('volumeDiscountBasisPoints', () => {
  const cases: Array<[number, number]> = [
    [-5000, 0],
    [0, 0],
    [99_999, 0],
    [100_000, 0],
    [199_999, 0],
    [200_000, 50],
    [299_999, 50],
    [400_000, 100],
    [1_000_000, 250],
    [1_500_000, 350],
    [3_999_999, 950],
    [4_000_000, 1000],
    [10_000_000, 1000],
    [90_000_000, 1000],
  ];
  for (const [spend, expected] of cases) {
    it(`grants ${expected} basis points for ${spend} cents of annual spend`, () => {
      assert.equal(volumeDiscountBasisPoints(spend), expected);
    });
  }

  it('grows in steps of 50 basis points', () => {
    for (let spend = 0; spend <= 4_000_000; spend += 100_000) {
      assert.equal(volumeDiscountBasisPoints(spend) % 50, 0, String(spend));
    }
  });

  it('never decreases with more spend', () => {
    let previous = 0;
    for (let spend = 0; spend <= 5_000_000; spend += 50_000) {
      const current = volumeDiscountBasisPoints(spend);
      assert.ok(current >= previous, String(spend));
      previous = current;
    }
  });
});
'''

PARTS = r'''
const PUMP = { id: 'PUMP-CIRC', name: 'Circulation pump', category: 'heating', unitCostCents: 10000, listPriceCents: 18000 };

describe('partUnitPrice', () => {
  const eastern = tariffFor('metro-east');
  const part = (unitCostCents: number, listPriceCents: number) => ({ unitCostCents, listPriceCents }) as any;

  it('adds the regional markup to the cost', () => {
    assert.equal(partUnitPrice(part(1000, 1000), eastern), 1250);
    assert.equal(partUnitPrice(part(2000, 100), eastern), 2500);
  });

  it('rounds the price to five cents', () => {
    assert.equal(partUnitPrice(part(1253, 100), eastern), 1565);
    assert.equal(partUnitPrice(part(1004, 100), eastern), 1255);
  });

  it('never goes below the list price', () => {
    assert.equal(partUnitPrice(part(1000, 2000), eastern), 2000);
    assert.equal(partUnitPrice(part(1000, 1250), eastern), 1250);
  });

  it('follows the markup of the tariff', () => {
    assert.equal(partUnitPrice(part(5000, 100), tariffFor('lakeside')), 5900);
    assert.equal(partUnitPrice(part(5000, 100), tariffFor('capital')), 6500);
    assert.equal(partUnitPrice(part(5000, 100), tariffFor('northern-hills')), 6000);
  });
});

describe('part invoice lines', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
    addPart(scene.ctx, 'warehouse', PUMP);
    receiveStock(scene.ctx, 'warehouse', 'VAN-000001', 'PUMP-CIRC', 5, 10000);
  });

  it('lists used parts at the marked-up price', () => {
    const invoice = drafted(scene, { labor: 60, parts: [{ sku: 'PUMP-CIRC', quantity: 2 }] });
    const part = invoice.lines.find((entry) => entry.kind === 'part');
    assert.equal(part?.description, 'Circulation pump');
    assert.equal(part?.quantity, 2);
    assert.equal(part?.unitCents, 18000);
    assert.equal(part?.amountCents, 36000);
    assert.equal(part?.taxable, true);
  });

  it('adds parts to the subtotal and the tax base', () => {
    const invoice = drafted(scene, { labor: 60, parts: [{ sku: 'PUMP-CIRC', quantity: 1 }] });
    assert.equal(invoice.subtotalCents, 4500 + 7200 + 18000);
    assert.equal(invoice.taxCents, 5643);
    assert.equal(invoice.totalCents, 35343);
  });

  it('prices from the cost when the markup exceeds the list price', () => {
    const cheap = { id: 'GSK-KIT', name: 'Gasket kit', category: 'heating', unitCostCents: 300, listPriceCents: 300 };
    addPart(scene.ctx, 'warehouse', cheap);
    receiveStock(scene.ctx, 'warehouse', 'VAN-000001', 'GSK-KIT', 5, 300);
    const part = drafted(scene, { labor: 60, parts: [{ sku: 'GSK-KIT', quantity: 3 }] }).lines.find((entry) => entry.kind === 'part');
    assert.equal(part?.unitCents, 375);
    assert.equal(part?.amountCents, 1125);
  });

  it('has no part lines for work without parts', () => {
    assert.ok(!kinds(drafted(scene, { labor: 60 })).includes('part'));
  });
});
'''

LABOR = r'''
describe('labour billing in metro-east', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  const cases: Array<[number, number, number]> = [
    [10, 60, 7200],
    [45, 60, 7200],
    [60, 60, 7200],
    [61, 75, 9000],
    [75, 75, 9000],
    [100, 105, 12600],
    [120, 120, 14400],
    [121, 135, 16200],
  ];
  for (const [worked, billed, amount] of cases) {
    it(`bills ${worked} worked minutes as ${billed} minutes`, () => {
      const labor = drafted(scene, { labor: worked }).lines.find((entry) => entry.kind === 'labor');
      assert.equal(labor?.amountCents, amount);
      assert.equal(labor?.unitCents, 7200);
      assert.equal(labor?.quantity, billed / 60);
      assert.ok(labor?.description.includes(`(${billed} min)`));
    });
  }

  it('bills the callout fee and nothing else for work without time entries', () => {
    const invoice = drafted(scene);
    assert.deepEqual(kinds(invoice), ['callout']);
    assert.equal(invoice.subtotalCents, 4500);
  });

  it('adds up several labour entries before rounding', () => {
    scene.app.clock.set(at(MONDAY, '12:00'));
    const tech = seedTechnician(scene.app, { skills: ['plumbing'] }).id as string;
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, tech);
    «startJob»(scene.ctx, tech, job.id);
    logEntry(scene.ctx, { technicianId: tech, jobId: job.id, kind: 'labor', start: at(MONDAY, '09:00'), end: at(MONDAY, '09:40') });
    logEntry(scene.ctx, { technicianId: tech, jobId: job.id, kind: 'labor', start: at(MONDAY, '10:00'), end: at(MONDAY, '10:40') });
    for (const item of Object.keys(«requireJob»(scene.ctx, job.id).checklist)) «recordChecklistItem»(scene.ctx, tech, job.id, item, true);
    «completeJob»(scene.ctx, tech, job.id, { resolution: 'Replaced the faulty component and tested' });
    const labor = draftInvoiceForJob(scene.ctx, 'billing', job.id).lines.find((entry) => entry.kind === 'labor');
    assert.equal(labor?.quantity, 1.5);
    assert.equal(labor?.amountCents, 10800);
  });

  it('ignores rejected time entries', () => {
    scene.app.clock.set(at(MONDAY, '12:00'));
    const tech = seedTechnician(scene.app, { skills: ['plumbing'] }).id as string;
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, tech);
    «startJob»(scene.ctx, tech, job.id);
    const rejected = logEntry(scene.ctx, { technicianId: tech, jobId: job.id, kind: 'labor', start: at(MONDAY, '09:00'), end: at(MONDAY, '11:00') });
    rejectEntry(scene.ctx, 'dispatch', rejected.id, 'Entered on the wrong order');
    for (const item of Object.keys(«requireJob»(scene.ctx, job.id).checklist)) «recordChecklistItem»(scene.ctx, tech, job.id, item, true);
    «completeJob»(scene.ctx, tech, job.id, { resolution: 'Replaced the faulty component and tested' });
    assert.deepEqual(kinds(draftInvoiceForJob(scene.ctx, 'billing', job.id)), ['callout']);
  });

  it('does not bill travel or admin entries as labour', () => {
    const invoice = drafted(scene, { travel: { km: 10, minutes: 30 } });
    assert.ok(!kinds(invoice).includes('labor'));
    assert.ok(kinds(invoice).includes('travel'));
  });
});

describe('labour billing in northern-hills', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
    region(scene, 'northern-hills');
  });

  const cases: Array<[number, number]> = [
    [25, 6400],
    [60, 6400],
    [61, 9600],
    [90, 9600],
    [100, 12800],
  ];
  for (const [worked, amount] of cases) {
    it(`bills ${worked} worked minutes in half-hour blocks`, () => {
      assert.equal(drafted(scene, { labor: worked }).lines.find((entry) => entry.kind === 'labor')?.amountCents, amount);
    });
  }
});

describe('labour billing in highlands', () => {
  it('charges a minimum of ninety minutes', () => {
    const scene = start();
    region(scene, 'highlands');
    const labor = drafted(scene, { labor: 20 }).lines.find((entry) => entry.kind === 'labor');
    assert.equal(labor?.quantity, 1.5);
    assert.equal(labor?.amountCents, 9300);
  });
});

describe('labour billing in coastal', () => {
  it('charges a minimum of forty-five minutes', () => {
    const scene = start();
    region(scene, 'coastal');
    const labor = drafted(scene, { labor: 20 }).lines.find((entry) => entry.kind === 'labor');
    assert.equal(labor?.quantity, 0.75);
    assert.equal(labor?.amountCents, 5100);
  });
});

describe('after-hours labour in metro-east', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  function labour(date: string, from: string, to: string) {
    scene.app.clock.set(at(date, '20:00'));
    const tech = seedTechnician(scene.app, { skills: ['plumbing'] }).id as string;
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, tech);
    «startJob»(scene.ctx, tech, job.id);
    logEntry(scene.ctx, { technicianId: tech, jobId: job.id, kind: 'labor', start: at(date, from), end: at(date, to) });
    for (const item of Object.keys(«requireJob»(scene.ctx, job.id).checklist)) «recordChecklistItem»(scene.ctx, tech, job.id, item, true);
    «completeJob»(scene.ctx, tech, job.id, { resolution: 'Replaced the faulty component and tested' });
    return draftInvoiceForJob(scene.ctx, 'billing', job.id);
  }

  it('bills labour that starts before the working day at the after-hours rate', () => {
    const invoice = labour(MONDAY, '«earlyFrom»', '«earlyTo»');
    assert.deepEqual(kinds(invoice), ['callout', 'after-hours']);
    const line = invoice.lines.find((entry) => entry.kind === 'after-hours');
    assert.equal(line?.quantity, 1);
    assert.equal(line?.unitCents, 10800);
    assert.equal(line?.amountCents, 10800);
  });

  it('bills labour that starts at the end of the working day at the after-hours rate', () => {
    const invoice = labour(MONDAY, '17:00', '18:00');
    assert.deepEqual(kinds(invoice), ['callout', 'after-hours']);
    assert.equal(invoice.lines.find((entry) => entry.kind === 'after-hours')?.amountCents, 10800);
  });

  it('bills weekend labour at the after-hours rate', () => {
    const invoice = labour('2026-03-07', '10:00', '11:00');
    assert.deepEqual(kinds(invoice), ['callout', 'after-hours']);
  });

  it('bills holiday labour at the after-hours rate', () => {
    const invoice = labour('2026-04-06', '10:00', '11:00');
    assert.deepEqual(kinds(invoice), ['callout', 'after-hours']);
  });

  it('bills labour from the start of the working day at the standard rate', () => {
    const invoice = labour(MONDAY, '08:00', '09:00');
    assert.deepEqual(kinds(invoice), ['callout', 'labor']);
    assert.equal(invoice.lines.find((entry) => entry.kind === 'labor')?.amountCents, 7200);
  });

  it('tops up the minimum on the standard line when after-hours labour is short', () => {
    const invoice = labour(MONDAY, '17:00', '17:30');
    assert.deepEqual(kinds(invoice), ['callout', 'labor', 'after-hours']);
    assert.equal(invoice.lines.find((entry) => entry.kind === 'labor')?.quantity, 0.5);
    assert.equal(invoice.lines.find((entry) => entry.kind === 'after-hours')?.quantity, 0.5);
  });
});
'''

INVOICES = r'''
describe('draftInvoiceForJob', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('builds callout, labour, tax and totals', () => {
    const invoice = drafted(scene, { labor: 60 });
    assert.equal(invoice.status, 'draft');
    assert.equal(invoice.id, 'INVD-000001');
    assert.equal(invoice.currency, 'EUR');
    assert.deepEqual(kinds(invoice), ['callout', 'labor']);
    assert.equal(invoice.subtotalCents, 11700);
    assert.equal(invoice.taxCents, 2223);
    assert.equal(invoice.totalCents, 13923);
    assert.equal(invoice.paidCents, 0);
    assert.equal(invoice.number, undefined);
    assert.deepEqual(invoice.jobIds.length, 1);
  });

  it('refuses work orders that are not completed', () => {
    const job = open(scene);
    assert.throws(
      () => draftInvoiceForJob(scene.ctx, 'billing', job.id),
      (error: any) => error.code === 'not-completed' && error.status === 409,
    );
  });

  it('refuses a second invoice for the same work order', () => {
    const job = done(scene, { labor: 60 });
    draftInvoiceForJob(scene.ctx, 'billing', job.id);
    assert.throws(() => draftInvoiceForJob(scene.ctx, 'billing', job.id), (error: any) => error.code === 'already-invoiced');
  });

  it('allows a new invoice after the first one was voided', () => {
    const job = done(scene, { labor: 60 });
    const first = draftInvoiceForJob(scene.ctx, 'billing', job.id);
    voidInvoice(scene.ctx, 'billing', first.id, 'Drafted by mistake');
    assert.notEqual(draftInvoiceForJob(scene.ctx, 'billing', job.id).id, first.id);
  });

  it('charges no tax to exempt customers', () => {
    customers(scene.ctx).update(scene.customer.id, { taxExempt: true });
    const invoice = drafted(scene, { labor: 60 });
    assert.equal(invoice.taxCents, 0);
    assert.equal(invoice.totalCents, 11700);
  });

  it('waives the callout for premium contracts and discounts labour', () => {
    seedContract(scene.app, scene.customer.id, [scene.site.id], 'premium', ['general']);
    for (let visit = 0; visit < 4; visit += 1) done(scene);
    const invoice = drafted(scene, { labor: 60 });
    assert.ok(!kinds(invoice).includes('callout'));
    assert.deepEqual(invoice.lines.find((entry) => entry.kind === 'discount')?.amountCents, -720);
    assert.equal(invoice.subtotalCents, 6480);
  });

  it('discounts labour for priority contracts but keeps the callout', () => {
    seedContract(scene.app, scene.customer.id, [scene.site.id], 'priority', ['general']);
    for (let visit = 0; visit < 2; visit += 1) done(scene);
    const invoice = drafted(scene, { labor: 60 });
    assert.ok(kinds(invoice).includes('callout'));
    assert.deepEqual(invoice.lines.find((entry) => entry.kind === 'discount')?.amountCents, -360);
  });

  it('covers an included contract visit with an allowance instead of a callout', () => {
    seedContract(scene.app, scene.customer.id, [scene.site.id], 'basic', ['general']);
    const invoice = drafted(scene, { labor: 60 });
    assert.deepEqual(kinds(invoice), ['labor', 'discount']);
    assert.equal(invoice.lines[1].amountCents, -7200);
    assert.equal(invoice.totalCents, 0);
    assert.equal(issueInvoice(scene.ctx, 'billing', invoice.id).status, 'paid');
  });

  it('audits the draft', () => {
    const invoice = drafted(scene, { labor: 60 });
    const entry = auditTrail(scene.ctx, invoice.id)[0];
    assert.equal(entry.action, 'invoice.drafted');
    assert.equal(entry.actorId, 'billing');
  });

  it('numbers drafts sequentially', () => {
    assert.deepEqual([drafted(scene).id, drafted(scene).id], ['INVD-000001', 'INVD-000002']);
  });
});

describe('issueInvoice', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('numbers the invoice and sets the due date', () => {
    const invoice = issued(scene, { labor: 60 });
    assert.equal(invoice.status, 'issued');
    assert.match(invoice.number ?? '', /^INV-\d{6}$/);
    assert.equal(invoice.issuedOn, MONDAY);
    assert.equal(invoice.dueOn, '2026-04-01');
  });

  it('follows the configured prefix and payment terms', () => {
    const custom = start({ config: { billing: { invoicePrefix: 'FL', paymentTermsDays: 14 } } });
    const invoice = issued(custom, { labor: 60 });
    assert.match(invoice.number ?? '', /^FL-\d{6}$/);
    assert.equal(invoice.dueOn, '2026-03-16');
  });

  it('only issues drafts', () => {
    const invoice = issued(scene, { labor: 60 });
    assert.throws(() => issueInvoice(scene.ctx, 'billing', invoice.id), (error: any) => error.code === 'not-draft');
  });

  it('refuses invoices below the configured minimum', () => {
    const strict = start({ config: { billing: { minimumInvoiceCents: 50000 } } });
    const invoice = drafted(strict, { labor: 60 });
    assert.throws(() => issueInvoice(strict.ctx, 'billing', invoice.id), (error: any) => error.code === 'below-minimum');
  });

  it('refuses to exceed the credit limit of the customer', () => {
    customers(scene.ctx).update(scene.customer.id, { creditLimitCents: 20000 });
    issued(scene, { labor: 60 });
    const second = drafted(scene, { labor: 60 });
    assert.throws(
      () => issueInvoice(scene.ctx, 'billing', second.id),
      (error: any) => error.code === 'credit-limit' && error.details.exposure === 13923 && error.details.limit === 20000,
    );
  });

  it('counts only unpaid amounts towards the credit limit', () => {
    customers(scene.ctx).update(scene.customer.id, { creditLimitCents: 20000 });
    const first = issued(scene, { labor: 60 });
    recordPayment(scene.ctx, 'billing', first.id, 13923, 'transfer');
    assert.equal(issueInvoice(scene.ctx, 'billing', drafted(scene, { labor: 60 }).id).status, 'issued');
  });

  it('audits the number', () => {
    const invoice = issued(scene, { labor: 60 });
    assert.deepEqual(auditTrail(scene.ctx, invoice.id).at(-1)?.details, { number: invoice.number });
  });
});

describe('voidInvoice', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('voids drafts and issued invoices with the reason', () => {
    const draft = voidInvoice(scene.ctx, 'billing', drafted(scene).id, 'Raised in error');
    assert.equal(draft.status, 'void');
    assert.equal(draft.voidReason, 'Raised in error');
    assert.equal(voidInvoice(scene.ctx, 'billing', issued(scene, { labor: 60 }).id, 'Customer cancelled').status, 'void');
  });

  it('cannot void twice', () => {
    const invoice = voidInvoice(scene.ctx, 'billing', drafted(scene).id, 'Raised in error');
    assert.throws(() => voidInvoice(scene.ctx, 'billing', invoice.id, 'Again'), (error: any) => error.code === 'already-void');
  });

  it('cannot void invoices with payments', () => {
    const invoice = issued(scene, { labor: 60 });
    recordPayment(scene.ctx, 'billing', invoice.id, 1000, 'cash');
    assert.throws(() => voidInvoice(scene.ctx, 'billing', invoice.id, 'Too late'), (error: any) => error.code === 'has-payments');
  });

  it('rejects unknown invoices', () => {
    assert.throws(() => voidInvoice(scene.ctx, 'billing', 'INVD-999999', 'Missing'), (error: any) => error.status === 404);
  });
});
'''

PAYMENTS = r'''
describe('recordPayment', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('keeps the invoice issued after a partial payment', () => {
    const invoice = issued(scene, { labor: 60 });
    const payment = recordPayment(scene.ctx, 'billing', invoice.id, 5000, 'card', 'AUTH-1');
    assert.equal(payment.id, 'PAY-000001');
    assert.equal(payment.method, 'card');
    assert.equal(payment.reference, 'AUTH-1');
    assert.equal(payment.receivedOn, MONDAY);
    const current = invoices(scene.ctx).require(invoice.id, 'invoice');
    assert.equal(current.paidCents, 5000);
    assert.equal(current.status, 'issued');
  });

  it('marks the invoice paid with the final payment', () => {
    const invoice = issued(scene, { labor: 60 });
    recordPayment(scene.ctx, 'billing', invoice.id, 5000, 'card');
    recordPayment(scene.ctx, 'billing', invoice.id, 8923, 'transfer');
    const current = invoices(scene.ctx).require(invoice.id, 'invoice');
    assert.equal(current.paidCents, 13923);
    assert.equal(current.status, 'paid');
  });

  it('refuses overpayments and reports the outstanding amount', () => {
    const invoice = issued(scene, { labor: 60 });
    assert.throws(
      () => recordPayment(scene.ctx, 'billing', invoice.id, 13924, 'cash'),
      (error: any) => error.code === 'overpayment' && error.details.outstanding === 13923,
    );
    assert.equal(invoices(scene.ctx).require(invoice.id, 'invoice').paidCents, 0);
  });

  it('refuses payments on drafts, paid and void invoices', () => {
    const draft = drafted(scene, { labor: 60 });
    assert.throws(() => recordPayment(scene.ctx, 'billing', draft.id, 100, 'cash'), (error: any) => error.code === 'not-payable');
    const paid = issued(scene, { labor: 60 });
    recordPayment(scene.ctx, 'billing', paid.id, 13923, 'cash');
    assert.throws(() => recordPayment(scene.ctx, 'billing', paid.id, 100, 'cash'), (error: any) => error.code === 'not-payable');
  });

  it('refuses non-positive and fractional amounts', () => {
    const invoice = issued(scene, { labor: 60 });
    for (const amount of [0, -100, 10.5]) {
      assert.throws(() => recordPayment(scene.ctx, 'billing', invoice.id, amount, 'cash'), (error: any) => error.code === 'invalid-amount');
    }
  });

  it('accounts for credit notes', () => {
    const invoice = issued(scene, { labor: 60 });
    issueCreditNote(scene.ctx, 'billing', invoice.id, 3923, 'Goodwill reduction');
    assert.throws(() => recordPayment(scene.ctx, 'billing', invoice.id, 10001, 'cash'), (error: any) => error.code === 'overpayment');
    recordPayment(scene.ctx, 'billing', invoice.id, 10000, 'cash');
    assert.equal(invoices(scene.ctx).require(invoice.id, 'invoice').status, 'paid');
  });

  it('audits the payment', () => {
    const invoice = issued(scene, { labor: 60 });
    const payment = recordPayment(scene.ctx, 'billing', invoice.id, 5000, 'cash');
    assert.deepEqual(auditTrail(scene.ctx, payment.id)[0].details, { invoiceId: invoice.id, amountCents: 5000 });
  });
});

describe('lateFee', () => {
  let scene: Scene;
  let invoice: Awaited<ReturnType<typeof issued>>;

  beforeEach(() => {
    scene = start();
    invoice = issued(scene, { labor: 60 });
  });

  const cases: Array<[string, number]> = [
    ['2026-03-02', 0],
    ['2026-04-01', 0],
    ['2026-04-02', 200],
    ['2026-05-01', 200],
    ['2026-05-02', 400],
    ['2026-05-31', 400],
    ['2026-06-01', 600],
  ];
  for (const [today, fee] of cases) {
    it(`charges ${fee} cents on ${today}`, () => {
      assert.equal(lateFee(scene.ctx, invoice, today), fee);
    });
  }

  it('is based on the outstanding amount', () => {
    recordPayment(scene.ctx, 'billing', invoice.id, 13000, 'cash');
    assert.equal(lateFee(scene.ctx, invoices(scene.ctx).require(invoice.id, 'invoice'), '2026-06-01'), 0);
  });

  it('is zero for paid and void invoices', () => {
    recordPayment(scene.ctx, 'billing', invoice.id, 13923, 'cash');
    assert.equal(lateFee(scene.ctx, invoices(scene.ctx).require(invoice.id, 'invoice'), '2026-06-01'), 0);
    const voided = voidInvoice(scene.ctx, 'billing', drafted(scene, { labor: 60 }).id, 'Raised in error');
    assert.equal(lateFee(scene.ctx, voided, '2026-06-01'), 0);
  });

  it('is zero for drafts without a due date', () => {
    assert.equal(lateFee(scene.ctx, drafted(scene, { labor: 60 }), '2026-06-01'), 0);
  });
});

describe('overdueInvoices', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('lists overdue invoices, most overdue first', () => {
    const later = issued(scene, { labor: 60, on: '2026-03-16' });
    const earlier = issued(scene, { labor: 60 });
    const overdue = overdueInvoices(scene.ctx, '2026-05-01');
    assert.deepEqual(overdue.map((entry) => entry.invoiceId), [earlier.id, later.id]);
    assert.deepEqual(overdue.map((entry) => entry.daysOverdue), [30, 16]);
    assert.equal(overdue[0].outstandingCents, 13923);
    assert.equal(overdue[0].lateFeeCents, 200);
  });

  it('leaves out invoices that are not yet due, paid or drafts', () => {
    const open = issued(scene, { labor: 60 });
    const paid = issued(scene, { labor: 60 });
    recordPayment(scene.ctx, 'billing', paid.id, 13923, 'cash');
    drafted(scene, { labor: 60 });
    assert.deepEqual(overdueInvoices(scene.ctx, '2026-04-01'), []);
    assert.deepEqual(overdueInvoices(scene.ctx, '2026-04-02').map((entry) => entry.invoiceId), [open.id]);
  });

  it('breaks ties by invoice id', () => {
    const first = issued(scene, { labor: 60 });
    const second = issued(scene, { labor: 60 });
    assert.deepEqual(overdueInvoices(scene.ctx, '2026-05-01').map((entry) => entry.invoiceId), [first.id, second.id]);
  });
});
'''

CREDITS = r'''
describe('issueCreditNote', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('creates a numbered note for the customer', () => {
    const invoice = issued(scene, { labor: 60 });
    const note = issueCreditNote(scene.ctx, 'billing', invoice.id, 2000, 'Part of the visit was not needed');
    assert.equal(note.id, 'CRN-000001');
    assert.match(note.number, /^CN-\d{6}$/);
    assert.equal(note.customerId, scene.customer.id);
    assert.equal(note.issuedOn, MONDAY);
    assert.equal(creditedCents(scene.ctx, invoice.id), 2000);
  });

  it('follows the configured prefix', () => {
    const custom = start({ config: { billing: { creditNotePrefix: 'CR' } } });
    const invoice = issued(custom, { labor: 60 });
    assert.match(issueCreditNote(custom.ctx, 'billing', invoice.id, 100, 'Rounding goodwill').number, /^CR-\d{6}$/);
  });

  it('adds up several notes', () => {
    const invoice = issued(scene, { labor: 60 });
    issueCreditNote(scene.ctx, 'billing', invoice.id, 2000, 'First reduction');
    issueCreditNote(scene.ctx, 'billing', invoice.id, 3000, 'Second reduction');
    assert.equal(creditedCents(scene.ctx, invoice.id), 5000);
    assert.equal(creditNotes(scene.ctx).count(() => true), 2);
  });

  it('cannot exceed the invoice total across notes', () => {
    const invoice = issued(scene, { labor: 60 });
    issueCreditNote(scene.ctx, 'billing', invoice.id, 13000, 'Large reduction');
    assert.throws(
      () => issueCreditNote(scene.ctx, 'billing', invoice.id, 924, 'One too many'),
      (error: any) => error.code === 'credit-exceeds-invoice' && error.details.credited === 13000 && error.details.total === 13923,
    );
    assert.equal(issueCreditNote(scene.ctx, 'billing', invoice.id, 923, 'Exactly the rest').amountCents, 923);
  });

  it('marks fully credited invoices as paid', () => {
    const invoice = issued(scene, { labor: 60 });
    issueCreditNote(scene.ctx, 'billing', invoice.id, 13923, 'Cancelled visit');
    assert.equal(invoices(scene.ctx).require(invoice.id, 'invoice').status, 'paid');
  });

  it('counts payments and credits together', () => {
    const invoice = issued(scene, { labor: 60 });
    recordPayment(scene.ctx, 'billing', invoice.id, 10000, 'card');
    issueCreditNote(scene.ctx, 'billing', invoice.id, 3923, 'Remaining balance waived');
    assert.equal(invoices(scene.ctx).require(invoice.id, 'invoice').status, 'paid');
  });

  it('requires an issued or paid invoice', () => {
    const draft = drafted(scene, { labor: 60 });
    assert.throws(() => issueCreditNote(scene.ctx, 'billing', draft.id, 100, 'Too early'), (error: any) => error.code === 'not-issued');
    const paid = issued(scene, { labor: 60 });
    recordPayment(scene.ctx, 'billing', paid.id, 13923, 'cash');
    assert.equal(issueCreditNote(scene.ctx, 'billing', paid.id, 100, 'Courtesy refund').amountCents, 100);
  });

  it('refuses non-positive and fractional amounts', () => {
    const invoice = issued(scene, { labor: 60 });
    for (const amount of [0, -5, 12.5]) {
      assert.throws(() => issueCreditNote(scene.ctx, 'billing', invoice.id, amount, 'Bad amount'), (error: any) => error.code === 'invalid-amount');
    }
  });

  it('audits the note', () => {
    const invoice = issued(scene, { labor: 60 });
    const note = issueCreditNote(scene.ctx, 'billing', invoice.id, 2000, 'Visit shortened');
    assert.deepEqual(auditTrail(scene.ctx, note.id)[0].details, { invoiceId: invoice.id, amountCents: 2000 });
  });
});
'''

STATEMENTS = r'''
describe('agingBucket', () => {
  const cases: Array<[number, string]> = [
    [-10, 'current'],
    [0, 'current'],
    [1, '1-30'],
    [30, '1-30'],
    [31, '31-60'],
    [60, '31-60'],
    [61, '61-90'],
    [90, '61-90'],
    [91, '90+'],
    [400, '90+'],
  ];
  for (const [days, bucket] of cases) {
    it(`puts ${days} days overdue in ${bucket}`, () => {
      assert.equal(agingBucket(days), bucket);
    });
  }
});

describe('customerStatement', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('is empty for a customer without invoices', () => {
    assert.deepEqual(customerStatement(scene.ctx, scene.customer.id, MONDAY), {
      customerId: scene.customer.id,
      asOf: MONDAY,
      lines: [],
      buckets: { current: 0, '1-30': 0, '31-60': 0, '61-90': 0, '90+': 0 },
      totalOutstandingCents: 0,
    });
  });

  it('lists issued invoices with their outstanding amount', () => {
    const invoice = issued(scene, { labor: 60 });
    recordPayment(scene.ctx, 'billing', invoice.id, 3923, 'cash');
    const statement = customerStatement(scene.ctx, scene.customer.id, MONDAY);
    assert.equal(statement.lines.length, 1);
    assert.equal(statement.lines[0].invoiceId, invoice.id);
    assert.equal(statement.lines[0].totalCents, 13923);
    assert.equal(statement.lines[0].outstandingCents, 10000);
    assert.equal(statement.lines[0].bucket, 'current');
    assert.equal(statement.totalOutstandingCents, 10000);
  });

  it('buckets invoices by days overdue', () => {
    issued(scene, { labor: 60 });
    issued(scene, { labor: 60, on: '2026-03-16' });
    const statement = customerStatement(scene.ctx, scene.customer.id, '2026-04-20');
    assert.deepEqual(statement.lines.map((entry) => entry.bucket), ['1-30', '1-30']);
    assert.equal(statement.buckets['1-30'], 27846);
    const older = customerStatement(scene.ctx, scene.customer.id, '2026-06-20');
    assert.deepEqual(older.lines.map((entry) => entry.bucket), ['61-90', '61-90']);
    assert.equal(older.totalOutstandingCents, 27846);
    assert.deepEqual(customerStatement(scene.ctx, scene.customer.id, '2026-09-01').lines.map((entry) => entry.bucket), ['90+', '90+']);
  });

  it('keeps invoices that are not yet due as current', () => {
    issued(scene, { labor: 60 });
    assert.equal(customerStatement(scene.ctx, scene.customer.id, '2026-04-01').buckets.current, 13923);
  });

  it('leaves out drafts, paid and void invoices', () => {
    drafted(scene, { labor: 60 });
    const paid = issued(scene, { labor: 60 });
    recordPayment(scene.ctx, 'billing', paid.id, 13923, 'cash');
    voidInvoice(scene.ctx, 'billing', issued(scene, { labor: 60 }).id, 'Customer cancelled');
    assert.deepEqual(customerStatement(scene.ctx, scene.customer.id, MONDAY).lines, []);
  });

  it('reduces the outstanding amount by credit notes', () => {
    const invoice = issued(scene, { labor: 60 });
    issueCreditNote(scene.ctx, 'billing', invoice.id, 3923, 'Goodwill reduction');
    assert.equal(customerStatement(scene.ctx, scene.customer.id, MONDAY).totalOutstandingCents, 10000);
  });

  it('only covers the requested customer', () => {
    issued(scene, { labor: 60 });
    const other = seedCustomer(scene.app, { name: 'Another Customer' });
    assert.deepEqual(customerStatement(scene.ctx, other.customer.id, MONDAY).lines, []);
  });
});
'''

API = r'''
let scene: Scene;

beforeEach(() => {
  scene = start();
});

describe('POST /jobs/:id/invoice', () => {
  it('drafts an invoice for billing staff', () => {
    const job = done(scene, { labor: 60 });
    const body = expectStatus(scene.app, 201, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
    assert.equal(body.status, 'draft');
    assert.equal(body.display.total, 'EUR 139.23');
    assert.equal(body.display.tax, 'EUR 22.23');
    assert.equal(body.outstandingCents, 13923);
  });

  it('is limited to billing roles', () => {
    const job = done(scene, { labor: 60 });
    for (const actor of ['dispatch', 'warehouse', scene.tech]) expectStatus(scene.app, 403, 'POST', `${JOBS}/${job.id}/invoice`, actor);
    expectStatus(scene.app, 201, 'POST', `${JOBS}/${job.id}/invoice`, 'admin');
  });

  it('reports conflicts and unknown work orders', () => {
    const job = done(scene, { labor: 60 });
    expectStatus(scene.app, 201, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
    assert.equal(expectStatus(scene.app, 409, 'POST', `${JOBS}/${job.id}/invoice`, 'billing').error.code, 'already-invoiced');
    expectStatus(scene.app, 404, 'POST', `${JOBS}/WO-999999/invoice`, 'billing');
  });
});

describe('invoice lifecycle routes', () => {
  it('issues, pays and credits an invoice', () => {
    const job = done(scene, { labor: 60 });
    const draft = call(scene.app, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
    const issuedBody = expectStatus(scene.app, 200, 'POST', `/invoices/${draft.id}/issue`, 'billing');
    assert.equal(issuedBody.status, 'issued');
    const payment = expectStatus(scene.app, 201, 'POST', `/invoices/${draft.id}/payments`, 'billing', { amountCents: 5000, method: 'transfer', reference: 'BANK-1' });
    assert.equal(payment.reference, 'BANK-1');
    const note = expectStatus(scene.app, 201, 'POST', `/invoices/${draft.id}/credit-notes`, 'billing', { amountCents: 923, reason: 'Goodwill reduction' });
    assert.equal(note.amountCents, 923);
    const current = call(scene.app, 'GET', `/invoices/${draft.id}`, 'billing');
    assert.equal(current.paidCents, 5000);
    assert.equal(current.outstandingCents, 8000);
    assert.equal(current.display.outstanding, 'EUR 80.00');
  });

  it('voids with a reason', () => {
    const job = done(scene, { labor: 60 });
    const draft = call(scene.app, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
    expectStatus(scene.app, 422, 'POST', `/invoices/${draft.id}/void`, 'billing', { reason: 'x' });
    expectStatus(scene.app, 422, 'POST', `/invoices/${draft.id}/void`, 'billing', {});
    assert.equal(expectStatus(scene.app, 200, 'POST', `/invoices/${draft.id}/void`, 'billing', { reason: 'Raised in error' }).status, 'void');
  });

  it('validates payment and credit payloads', () => {
    const job = done(scene, { labor: 60 });
    const draft = call(scene.app, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
    call(scene.app, 'POST', `/invoices/${draft.id}/issue`, 'billing');
    for (const payload of [{ amountCents: 0, method: 'cash' }, { amountCents: 100, method: 'crypto' }, { amountCents: 10.5, method: 'cash' }, { method: 'cash' }]) {
      expectStatus(scene.app, 422, 'POST', `/invoices/${draft.id}/payments`, 'billing', payload);
    }
    expectStatus(scene.app, 422, 'POST', `/invoices/${draft.id}/credit-notes`, 'billing', { amountCents: 100, reason: 'x' });
    assert.equal(expectStatus(scene.app, 409, 'POST', `/invoices/${draft.id}/payments`, 'billing', { amountCents: 99999, method: 'cash' }).error.code, 'overpayment');
  });

  it('keeps writes to billing staff', () => {
    const job = done(scene, { labor: 60 });
    const draft = call(scene.app, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
    expectStatus(scene.app, 403, 'POST', `/invoices/${draft.id}/issue`, 'dispatch');
    expectStatus(scene.app, 403, 'POST', `/invoices/${draft.id}/void`, scene.tech, { reason: 'Not allowed' });
    expectStatus(scene.app, 403, 'POST', `/invoices/${draft.id}/payments`, 'warehouse', { amountCents: 100, method: 'cash' });
  });
});

describe('invoice read routes', () => {
  it('shows invoices to billing and hides them from field roles', () => {
    const invoice = call(scene.app, 'POST', `${JOBS}/${done(scene, { labor: 60 }).id}/invoice`, 'billing');
    expectStatus(scene.app, 200, 'GET', `/invoices/${invoice.id}`, 'billing');
    expectStatus(scene.app, 403, 'GET', `/invoices/${invoice.id}`, 'dispatch');
    expectStatus(scene.app, 403, 'GET', `/invoices/${invoice.id}`, scene.tech);
    expectStatus(scene.app, 404, 'GET', '/invoices/INVD-999999', 'billing');
  });

  it('lets portal users see issued invoices of their own account only', () => {
    const portal = call(scene.app, 'POST', `/customers/${scene.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' });
    const stranger = seedCustomer(scene.app, { name: 'Another Customer' });
    const strangerPortal = call(scene.app, 'POST', `/customers/${stranger.customer.id}/portal-users`, 'admin', { name: 'Sam Rivera' });
    const draft = call(scene.app, 'POST', `${JOBS}/${done(scene, { labor: 60 }).id}/invoice`, 'billing');
    expectStatus(scene.app, 404, 'GET', `/invoices/${draft.id}`, portal.id);
    call(scene.app, 'POST', `/invoices/${draft.id}/issue`, 'billing');
    expectStatus(scene.app, 200, 'GET', `/invoices/${draft.id}`, portal.id);
    expectStatus(scene.app, 403, 'GET', `/invoices/${draft.id}`, strangerPortal.id);
  });

  it('lists customer invoices without drafts for portal users', () => {
    const portal = call(scene.app, 'POST', `/customers/${scene.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' });
    const issuedInvoice = issued(scene, { labor: 60 });
    drafted(scene, { labor: 60 });
    assert.equal(call(scene.app, 'GET', `/customers/${scene.customer.id}/invoices`, 'billing').items.length, 2);
    const own = call(scene.app, 'GET', `/customers/${scene.customer.id}/invoices`, portal.id).items;
    assert.deepEqual(own.map((item: any) => item.id), [issuedInvoice.id]);
    expectStatus(scene.app, 403, 'GET', `/customers/${seedCustomer(scene.app, { name: 'Other' }).customer.id}/invoices`, portal.id);
  });
});

describe('statement, overdue and late fee routes', () => {
  it('returns the statement as of a date', () => {
    const invoice = issued(scene, { labor: 60 });
    const statement = call(scene.app, 'GET', `/customers/${scene.customer.id}/statement?asOf=2026-04-20`, 'billing');
    assert.equal(statement.lines[0].invoiceId, invoice.id);
    assert.equal(statement.buckets['1-30'], 13923);
    assert.equal(call(scene.app, 'GET', `/customers/${scene.customer.id}/statement`, 'billing').asOf, MONDAY);
    expectStatus(scene.app, 422, 'GET', `/customers/${scene.customer.id}/statement?asOf=yesterday`, 'billing');
  });

  it('lists overdue invoices on a date', () => {
    const invoice = issued(scene, { labor: 60 });
    assert.deepEqual(call(scene.app, 'GET', '/invoices/overdue?on=2026-04-01', 'billing').items, []);
    const [entry] = call(scene.app, 'GET', '/invoices/overdue?on=2026-05-01', 'billing').items;
    assert.equal(entry.invoiceId, invoice.id);
    assert.equal(entry.daysOverdue, 30);
  });

  it('computes the late fee on a date', () => {
    const invoice = issued(scene, { labor: 60 });
    assert.equal(call(scene.app, 'GET', `/invoices/${invoice.id}/late-fee?on=2026-05-02`, 'billing').lateFeeCents, 400);
    assert.equal(call(scene.app, 'GET', `/invoices/${invoice.id}/late-fee`, 'billing').lateFeeCents, 0);
  });

  it('is limited to billing roles', () => {
    expectStatus(scene.app, 403, 'GET', `/customers/${scene.customer.id}/statement`, 'dispatch');
    expectStatus(scene.app, 403, 'GET', '/invoices/overdue', scene.tech);
  });
});

describe('GET /tariffs/:region', () => {
  it('returns the tariff of a region', () => {
    const tariff = call(scene.app, 'GET', '/tariffs/lakeside', 'billing');
    assert.equal(tariff.id, 'T-LS-2026');
    assert.equal(tariff.currency, 'CHF');
    assert.equal(tariff.cashRoundingCents, 5);
  });

  it('rejects unknown regions and unauthorised roles', () => {
    expectStatus(scene.app, 404, 'GET', '/tariffs/atlantis', 'billing');
    expectStatus(scene.app, 403, 'GET', '/tariffs/lakeside', 'dispatch');
  });
});
'''


def frac_round(value):
    """Rounds half away from zero, like the application's money arithmetic."""
    return int((value + Fraction(1, 2)) // 1) if value >= 0 else -int((-value + Fraction(1, 2)) // 1)


def tariff_expectation(spec):
    for km in range(25, 60):
        found = tariff_outcome(spec, km)
        if found:
            return found
    raise AssertionError(spec)


def tariff_outcome(spec, km):
    (_, _, _, _, standard, _, increment, minimum, callout, _, km_rate, free_km, travel_time, _, tax, cash) = spec
    labor_minutes = max(-(-100 // increment) * increment, minimum)
    labor = frac_round(Fraction(standard * labor_minutes, 60))
    billed_km = max(0, km - free_km)
    travel = billed_km * km_rate
    travel_time_amount = frac_round(Fraction(standard * 30, 60)) if travel_time else 0
    subtotal = callout + labor + travel + travel_time_amount
    raw_tax = Fraction(subtotal * tax, 10000)
    if raw_tax.denominator == 2:
        return None
    tax_cents = frac_round(raw_tax)
    total = subtotal + tax_cents
    rounded = frac_round(Fraction(total, cash)) * cash
    kinds = ["callout", "labor"] + (["travel"] if billed_km else []) + (["travel-time"] if travel_time else [])
    if rounded != total:
        kinds.append("rounding")
    return {
        "labor_minutes": labor_minutes, "labor": labor, "travel": travel, "travel_time": travel_time_amount,
        "subtotal": subtotal, "tax": tax_cents, "total": rounded, "rounding": rounded - total, "kinds": kinds,
        "km": km,
    }


TARIFF = r'''
describe('«region» tariff', () => {
  const tariff = tariffFor('«region»');

  it('defines the rates of the region', () => {
    assert.equal(tariff.id, '«tid»');
    assert.equal(tariff.region, '«region»');
    assert.equal(tariff.currency, '«currency»');
    assert.deepEqual(tariff.laborRates, { apprentice: «apprentice», standard: «standard», specialist: «specialist» });
    assert.equal(tariff.«step», «increment»);
    assert.equal(tariff.minimumLaborMinutes, «minimum»);
    assert.equal(tariff.calloutFeeCents, «callout»);
    assert.equal(tariff.taxRateBp, «tax»);
    assert.equal(tariff.cashRoundingCents, «cash»);
  });

  it('is registered under its region', () => {
    assert.ok(tariffRegions().includes('«region»'));
  });

  it('prices skilled labour above apprentice labour', () => {
    assert.ok(tariff.laborRates.apprentice < tariff.laborRates.standard);
    assert.ok(tariff.laborRates.standard < tariff.laborRates.specialist);
  });

  it('invoices a visit with labour and travel', () => {
    const scene = start();
    region(scene, '«region»');
    const invoice = drafted(scene, { labor: 100, travel: { km: «km», minutes: 30 } });
    assert.deepEqual(kinds(invoice), «kinds»);
    assert.equal(invoice.currency, '«currency»');
    assert.equal(invoice.lines.find((entry) => entry.kind === 'labor')?.amountCents, «labor»);
    assert.equal(invoice.lines.find((entry) => entry.kind === 'travel')?.amountCents ?? 0, «travel»);
    assert.equal(invoice.lines.find((entry) => entry.kind === 'travel-time')?.amountCents ?? 0, «travel_time»);
    assert.equal(invoice.taxCents, «taxcents»);
    assert.equal(invoice.totalCents, «total»);
    assert.equal(invoice.lines.find((entry) => entry.kind === 'rounding')?.amountCents ?? 0, «rounding»);
  });

  it('bills «labor_minutes» minutes for 100 worked minutes', () => {
    const scene = start();
    region(scene, '«region»');
    assert.ok(drafted(scene, { labor: 100 }).lines.find((entry) => entry.kind === 'labor')?.description.includes('(«labor_minutes» min)'));
  });
});
'''


def build_tariff_files():
    files = {}
    for spec in TARIFFS:
        region = spec[0]
        files[f"tests/billing/tariff-{region}.test.ts"] = (spec, tariff_expectation(spec))
    return files


def tariff_specs():
    result = {}
    for path, (spec, expected) in build_tariff_files().items():

        def subs(ctx, spec=spec, expected=expected):
            return {
                "region": spec[0], "tid": spec[1], "currency": spec[2], "apprentice": spec[3], "standard": spec[4],
                "specialist": spec[5], "step": step_field(ctx), "increment": spec[6], "minimum": spec[7],
                "callout": spec[8], "tax": spec[14], "cash": spec[15], "labor_minutes": expected["labor_minutes"],
                "kinds": "[" + ", ".join(f"'{kind}'" for kind in expected["kinds"]) + "]",
                "labor": expected["labor"], "travel": expected["travel"], "travel_time": expected["travel_time"],
                "taxcents": expected["tax"], "km": expected["km"], "total": expected["total"], "rounding": expected["rounding"],
            }

        result[path] = bill_spec(
            {f"{BILLING}/tariffs/index.ts": ["tariffFor", "tariffRegions"]}, TARIFF, subs=subs)
    return result


FILES = {
    "tests/billing/tax.test.ts": Spec({f"{BILLING}/tax.ts": ["computeTotals"]}, TAX),
    "tests/billing/discounts.test.ts": Spec(
        {f"{BILLING}/discounts.ts": ["contractDiscountLine", "volumeDiscountBasisPoints"]}, DISCOUNTS),
    "tests/billing/parts.test.ts": bill_spec(
        {f"{BILLING}/parts.ts": ["partUnitPrice"], f"{BILLING}/tariffs/index.ts": ["tariffFor"],
         "src/inventory/catalog.ts": ["addPart"], "src/inventory/stock.ts": ["receiveStock"]}, PARTS),
    "tests/billing/labor.test.ts": bill_spec(
        {LOGENTRY: ["logEntry"], "src/timesheets/approvals.ts": ["rejectEntry"]}, LABOR, jobs=("completeJob",),
        subs=lambda ctx: {"earlyFrom": "06:30", "earlyTo": "07:30"} if ctx.early
        else {"earlyFrom": "07:30", "earlyTo": "08:30"}),
    "tests/billing/invoices.test.ts": bill_spec(
        {BILLING + "/invoices.ts": ["voidInvoice"], BILLING + "/payments.ts": ["recordPayment"],
         "src/adapters/repositories/customers.ts": ["customers"], "src/adapters/store/audit.ts": ["auditTrail"],
         WORLD: ["seedContract"]}, INVOICES),
    "tests/billing/payments.test.ts": bill_spec(
        {BILLING + "/credits.ts": ["issueCreditNote"],
         BILLING + "/payments.ts": ["lateFee", "overdueInvoices", "recordPayment"],
         BILLING + "/invoices.ts": ["voidInvoice"], "src/adapters/repositories/invoices.ts": ["invoices"],
         "src/adapters/store/audit.ts": ["auditTrail"]}, PAYMENTS),
    "tests/billing/credits.test.ts": bill_spec(
        {BILLING + "/credits.ts": ["issueCreditNote"], BILLING + "/payments.ts": ["creditedCents", "recordPayment"],
         "src/adapters/repositories/invoices.ts": ["creditNotes", "invoices"],
         "src/adapters/store/audit.ts": ["auditTrail"]}, CREDITS),
    "tests/billing/statements.test.ts": bill_spec(
        {BILLING + "/credits.ts": ["issueCreditNote"], BILLING + "/invoices.ts": ["voidInvoice"],
         BILLING + "/payments.ts": ["recordPayment"],
         BILLING + "/statements.ts": ["agingBucket", "customerStatement"], WORLD: ["seedCustomer"]}, STATEMENTS),
    "tests/api/billing.test.ts": bill_spec({WORLD: ["JOBS", "expectStatus", "seedCustomer"]}, API),
}


def generate(ctx):
    register(ctx, {**FILES, **tariff_specs()})
