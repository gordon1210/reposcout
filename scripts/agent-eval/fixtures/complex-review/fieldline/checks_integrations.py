"""Tests for integrations: export formats, webhooks, the accounting journal and the integration routes."""

from .checks_kit import DESCRIBE_IT, WITH_SETUP, Spec, register
from .checks_workorders import scene_spec

WORLD = "tests/support/world.ts"
INTEGRATIONS = "src/integrations"

FORMATS = r'''
const ROWS = [
  { account: '1200', reference: 'INV-2026-000001', debitCents: 13900, creditCents: 0 },
  { account: '4000', reference: 'INV-2026-000001', debitCents: 0, creditCents: 11680 },
];

describe('createExportRegistry', () => {
  it('offers the four formats', () => {
    assert.deepEqual(createExportRegistry().keys(), ['csv', 'json', 'ndjson', 'tsv']);
  });

  it('matches format names regardless of case', () => {
    const registry = createExportRegistry();
    assert.equal(registry.lookup('CSV'), registry.lookup('csv'));
    assert.equal(registry.has('NdJson'), true);
    assert.equal(registry.lookup('xml'), undefined);
  });

  it('declares a content type for each format', () => {
    const types = Object.fromEntries(createExportRegistry().entries().map(([key, format]) => [key, format.contentType]));
    assert.deepEqual(types, {
      csv: 'text/csv',
      json: 'application/json',
      ndjson: 'application/x-ndjson',
      tsv: 'text/tab-separated-values',
    });
  });

  it('is created fresh each time', () => {
    const first = createExportRegistry();
    const second = createExportRegistry();
    first.register('xml', { key: 'xml', contentType: 'application/xml', render: () => '<rows/>' });
    assert.equal(first.has('xml'), true);
    assert.equal(second.has('xml'), false);
  });

  it('refuses a second csv registration', () => {
    const registry = createExportRegistry();
    assert.throws(() => registry.register('CSV', registry.resolve('csv')), /duplicate registration for csv/);
  });
});

describe('csv', () => {
  const render = (rows: readonly Record<string, unknown>[]) => createExportRegistry().resolve('csv').render(rows);

  it('writes a header and a line per row', () => {
    assert.equal(render(ROWS), 'account,reference,debitCents,creditCents\n1200,INV-2026-000001,13900,0\n4000,INV-2026-000001,0,11680\n');
  });

  it('quotes cells that need it', () => {
    assert.equal(render([{ name: 'Harbour View, East' }]), 'name\n"Harbour View, East"\n');
  });
});

describe('json', () => {
  const render = (rows: readonly Record<string, unknown>[]) => createExportRegistry().resolve('json').render(rows);

  it('pretty-prints the rows with a final line feed', () => {
    const text = render(ROWS.slice(0, 1));
    assert.ok(text.endsWith('}\n]\n'));
    assert.deepEqual(JSON.parse(text), ROWS.slice(0, 1));
    assert.match(text, /^\[\n  \{\n    "account": "1200",/);
  });

  it('writes an empty array for no rows', () => {
    assert.equal(render([]), '[]\n');
  });
});

describe('ndjson', () => {
  const render = (rows: readonly Record<string, unknown>[]) => createExportRegistry().resolve('ndjson').render(rows);

  it('writes one compact object per line', () => {
    const lines = render(ROWS).split('\n');
    assert.equal(lines.length, 3);
    assert.deepEqual(lines.slice(0, 2).map((line) => JSON.parse(line)), ROWS);
    assert.equal(lines[2], '');
  });

  it('writes nothing at all for no rows', () => {
    assert.equal(render([]), '');
  });
});

describe('tsv', () => {
  const render = (rows: readonly Record<string, unknown>[]) => createExportRegistry().resolve('tsv').render(rows);

  it('separates cells with tabs', () => {
    assert.equal(render(ROWS), 'account\treference\tdebitCents\tcreditCents\n1200\tINV-2026-000001\t13900\t0\n4000\tINV-2026-000001\t0\t11680\n');
  });

  it('replaces tabs and line breaks inside cells', () => {
    assert.equal(render([{ note: 'first\tsecond\nthird' }]), 'note\nfirst second third\n');
  });

  it('writes empty cells for missing values', () => {
    assert.equal(render([{ a: 1, b: null }, { a: undefined }]), 'a\tb\n1\t\n\t\n');
  });

  it('does not quote anything', () => {
    assert.equal(render([{ name: 'He said "hi", twice' }]), 'name\nHe said "hi", twice\n');
  });

  it('uses the columns of the first row', () => {
    assert.equal(render([{ a: 1 }, { a: 2, b: 3 }]), 'a\n1\n2\n');
  });

  it('writes just a line feed for no rows', () => {
    assert.equal(render([]), '\n');
  });
});
'''

WEBHOOKS = r'''
const ENDPOINT = 'https://hooks.harbourview.example/fieldline';
let app: ReturnType<typeof createApp>;
let ctx: AppContext;

beforeEach(() => {
  app = createApp({ now: at(MONDAY, '09:00') });
  ctx = app.ctx;
});

const hooks = () => ctx.store.collection<Webhook>('webhooks');
const deliveries = () => ctx.store.collection<Delivery>('deliveries').find(() => true);
const breach = (customerId: string, jobId = 'WO-000001') => ctx.bus.emit({ type: SlaEvents.breached, at: at(MONDAY, '09:30'), jobId, customerId });
const booking = (customerId: string) => ctx.bus.emit({ type: AppointmentEvents.booked, at: at(MONDAY, '09:40'), customerId, appointmentId: 'APT-000001' });

describe('registerWebhook', () => {
  it('stores an active webhook with a generated secret', () => {
    const hook = registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached']);
    assert.equal(hook.id, 'HOOK-000001');
    assert.equal(hook.url, ENDPOINT);
    assert.deepEqual(hook.topics, ['sla.breached']);
    assert.equal(hook.active, true);
    assert.equal(hook.failures, 0);
    assert.equal(hook.customerId, undefined);
    assert.match(hook.secret, /^[0-9a-f]{32}$/);
    assert.deepEqual(hooks().require(hook.id, 'webhook'), hook);
  });

  it('sorts topics and removes duplicates', () => {
    const hook = registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached', 'appointment.booked', 'sla.breached']);
    assert.deepEqual(hook.topics, ['appointment.booked', 'sla.breached']);
  });

  it('gives every webhook its own secret', () => {
    const first = registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached']);
    const second = registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached']);
    assert.notEqual(first.secret, second.secret);
    assert.equal(second.id, 'HOOK-000002');
  });

  it('can be limited to one customer', () => {
    assert.equal(registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached'], 'CUS-000004').customerId, 'CUS-000004');
  });

  it('requires https', () => {
    for (const url of ['http://hooks.example/fieldline', 'ftp://hooks.example', 'hooks.example/fieldline', 'https://', 'https://a b.example']) {
      assert.throws(
        () => registerWebhook(ctx, 'admin', url, ['sla.breached']),
        (error: any) => error instanceof DomainError && error.code === 'invalid-url',
        url,
      );
    }
    assert.equal(hooks().count(), 0);
  });

  it('refuses unknown topics and lists them', () => {
    assert.throws(
      () => registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached', 'sla.exploded', 'invoice.paid']),
      (error: any) => error.code === 'unknown-topic' && error.message === 'Unknown topics: sla.exploded, invoice.paid',
    );
    assert.equal(hooks().count(), 0);
  });

  it('accepts every published topic', () => {
    const topics = [...new Set(Object.values(EXTERNAL_TOPICS))];
    assert.deepEqual(registerWebhook(ctx, 'admin', ENDPOINT, topics).topics, [...topics].sort());
  });

  it('audits the registration without the secret', () => {
    const hook = registerWebhook(ctx, 'admin', ENDPOINT, ['appointment.booked']);
    const [entry] = auditTrail(ctx, hook.id);
    assert.equal(entry.action, 'webhook.registered');
    assert.equal(entry.actorId, 'admin');
    assert.deepEqual(entry.details, { topics: ['appointment.booked'] });
  });
});

describe('sign', () => {
  it('is the hex HMAC-SHA256 of the payload', () => {
    const expected = createHmac('sha256', 'shared-secret').update('{"topic":"x"}').digest('hex');
    assert.equal(sign('shared-secret', '{"topic":"x"}'), expected);
  });

  it('depends on both secret and payload', () => {
    const base = sign('one', 'payload');
    assert.notEqual(base, sign('two', 'payload'));
    assert.notEqual(base, sign('one', 'payload '));
    assert.equal(base, sign('one', 'payload'));
  });
});

describe('deliveries', () => {
  it('queues a signed delivery for each subscribed webhook', () => {
    const hook = registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached']);
    breach('CUS-000001');
    const [delivery] = deliveries();
    assert.equal(delivery.id, 'DLV-000001');
    assert.equal(delivery.webhookId, hook.id);
    assert.equal(delivery.topic, 'sla.breached');
    assert.equal(delivery.status, 'pending');
    assert.equal(delivery.attempts, 0);
    assert.equal(delivery.createdAt, at(MONDAY, '09:00'));
    assert.equal(delivery.signature, sign(hook.secret, delivery.payload));
  });

  it('carries the topic and the event data without its internal type', () => {
    registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached']);
    breach('CUS-000001', 'WO-000007');
    assert.deepEqual(JSON.parse(deliveries()[0].payload), {
      topic: 'sla.breached',
      data: { at: at(MONDAY, '09:30'), jobId: 'WO-000007', customerId: 'CUS-000001' },
    });
  });

  it('skips webhooks that did not subscribe to the topic', () => {
    registerWebhook(ctx, 'admin', ENDPOINT, ['appointment.booked']);
    breach('CUS-000001');
    assert.deepEqual(deliveries(), []);
  });

  it('fans out to every subscriber', () => {
    registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached']);
    registerWebhook(ctx, 'admin', 'https://erp.harbourview.example/inbound', ['sla.breached', 'appointment.booked']);
    registerWebhook(ctx, 'admin', 'https://other.example/inbound', ['appointment.booked']);
    breach('CUS-000001');
    assert.deepEqual(deliveries().map((delivery) => delivery.webhookId), ['HOOK-000001', 'HOOK-000002']);
    booking('CUS-000001');
    assert.equal(deliveries().length, 4);
  });

  it('restricts customer webhooks to that customer\'s events', () => {
    const own = registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached'], 'CUS-000001');
    registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached'], 'CUS-000002');
    breach('CUS-000001');
    assert.deepEqual(deliveries().map((delivery) => delivery.webhookId), [own.id]);
  });

  it('delivers events of every customer to unrestricted webhooks', () => {
    registerWebhook(ctx, 'admin', ENDPOINT, ['appointment.booked']);
    booking('CUS-000001');
    booking('CUS-000002');
    assert.equal(deliveries().length, 2);
  });

  it('skips inactive webhooks', () => {
    const hook = registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached']);
    hooks().update(hook.id, { active: false });
    breach('CUS-000001');
    assert.deepEqual(deliveries(), []);
  });

  it('numbers deliveries in order', () => {
    registerWebhook(ctx, 'admin', ENDPOINT, ['sla.breached']);
    breach('CUS-000001');
    breach('CUS-000001');
    assert.deepEqual(deliveries().map((delivery) => delivery.id), ['DLV-000001', 'DLV-000002']);
  });
});
'''

ACCOUNTING = r'''
let scene: Scene;

beforeEach(() => {
  scene = start();
});

const line = (kind: InvoiceLine['kind'], amountCents: number): InvoiceLine => ({
  kind,
  description: `${kind} line`,
  quantity: 1,
  unitCents: amountCents,
  amountCents,
  taxable: true,
});

function invoiceOf(lines: InvoiceLine[], taxCents = 0, extra: Partial<Invoice> = {}): Invoice {
  const subtotalCents = lines.reduce((total, entry) => total + entry.amountCents, 0);
  return {
    id: 'INV-000001',
    number: 'INV-2026-000001',
    customerId: 'CUS-000001',
    jobIds: ['WO-000001'],
    status: 'issued',
    currency: 'EUR',
    lines,
    subtotalCents,
    taxCents,
    totalCents: subtotalCents + taxCents,
    paidCents: 0,
    createdAt: at(MONDAY, '10:00'),
    issuedOn: MONDAY,
    dueOn: '2026-04-01',
    ...extra,
  };
}

const summary = (entries: JournalLine[]) => entries.map((entry) => [entry.account, entry.debitCents, entry.creditCents]);
const balance = (entries: JournalLine[]) => entries.reduce((total, entry) => total + entry.debitCents - entry.creditCents, 0);

describe('journalFor', () => {
  it('is empty until an invoice is issued', () => {
    assert.deepEqual(journalFor(invoiceOf([line('callout', 4500)], 855, { number: undefined, issuedOn: undefined, status: 'draft' })), []);
    assert.deepEqual(journalFor(invoiceOf([line('callout', 4500)], 0, { number: undefined })), []);
  });

  it('books receivables against revenue and tax', () => {
    const entries = journalFor(invoiceOf([line('callout', 4500), line('labor', 7200)], 2223));
    assert.deepEqual(summary(entries), [['1200', 13923, 0], ['4000', 0, 11700], ['2200', 0, 2223]]);
    assert.ok(entries.every((entry) => entry.date === MONDAY && entry.reference === 'INV-2026-000001'));
  });

  it('separates parts and travel revenue', () => {
    const entries = journalFor(invoiceOf([line('labor', 6000), line('part', 2500), line('travel', 900), line('travel-time', 1100), line('part', 500)], 0));
    assert.deepEqual(summary(entries), [['1200', 11000, 0], ['4000', 0, 6000], ['4100', 0, 3000], ['4200', 0, 2000]]);
  });

  it('nets discounts, credits and rounding on one account', () => {
    const entries = journalFor(invoiceOf([line('labor', 10000), line('discount', -1000), line('credit', -250), line('rounding', 2)], 0));
    assert.deepEqual(summary(entries), [['1200', 8752, 0], ['4000', 0, 10000], ['4900', 1248, 0]]);
  });

  it('books a positive net on the discount account as credit', () => {
    const entries = journalFor(invoiceOf([line('labor', 1000), line('rounding', 3)], 0));
    assert.deepEqual(summary(entries), [['1200', 1003, 0], ['4000', 0, 1000], ['4900', 0, 3]]);
  });

  it('leaves out accounts that net to zero', () => {
    const entries = journalFor(invoiceOf([line('labor', 1000), line('discount', -5), line('rounding', 5)], 0));
    assert.deepEqual(summary(entries), [['1200', 1000, 0], ['4000', 0, 1000]]);
  });

  it('omits the tax line for tax-free invoices', () => {
    assert.ok(journalFor(invoiceOf([line('labor', 1000)], 0)).every((entry) => entry.account !== '2200'));
  });

  it('balances every combination of lines', () => {
    const kinds: Array<InvoiceLine['kind']> = ['callout', 'labor', 'travel', 'travel-time', 'part', 'discount', 'credit', 'rounding'];
    for (const [index, kind] of kinds.entries()) {
      const lines = [line('labor', 5000), line(kind, kind === 'discount' || kind === 'credit' ? -700 : 300 + index)];
      assert.equal(balance(journalFor(invoiceOf(lines, 950))), 0, kind);
    }
  });
});

describe('journalExport', () => {
  function issued(date: string, region = 'metro-east') {
    scene.app.clock.set(at(date, '10:00'));
    const owner = region === 'metro-east' ? { customer: scene.customer, site: scene.site } : seedCustomer(scene.app, { region });
    const job = open(scene, 'general', { customerId: owner.customer.id, siteId: owner.site.id });
    work(scene, job.id);
    const draft = draftInvoiceForJob(scene.ctx, 'billing', job.id);
    return issueInvoice(scene.ctx, 'billing', draft.id);
  }

  it('exports the journal of issued invoices as CSV', () => {
    const invoice = issued('2026-03-03');
    const exported = journalExport(scene.ctx, '2026-03-01', '2026-04-01', 'csv');
    assert.equal(exported.contentType, 'text/csv');
    const rows = journalFor(invoice);
    assert.equal(exported.body.split('\n').length, rows.length + 2);
    assert.ok(exported.body.startsWith('date,account,reference,debitCents,creditCents\n'));
    assert.ok(exported.body.includes(`2026-03-03,1200,${invoice.number},${invoice.totalCents},0\n`));
    assert.equal(balance(rows), 0);
  });

  it('orders the journal by invoice number', () => {
    const first = issued('2026-03-05');
    const second = issued('2026-03-04');
    const references = journalExport(scene.ctx, '2026-03-01', '2026-04-01', 'ndjson').body.trim().split('\n').map((entry) => JSON.parse(entry).reference);
    assert.deepEqual([...new Set(references)], [first.number, second.number]);
  });

  it('selects invoices by issue date, end exclusive', () => {
    issued('2026-03-10');
    const inRange = (from: string, to: string) => journalExport(scene.ctx, from, to, 'ndjson').body !== '';
    assert.equal(inRange('2026-03-10', '2026-03-11'), true);
    assert.equal(inRange('2026-03-09', '2026-03-10'), false);
    assert.equal(inRange('2026-03-11', '2026-03-12'), false);
  });

  it('leaves out void invoices', () => {
    const kept = issued('2026-03-03');
    const dropped = issued('2026-03-04');
    voidInvoice(scene.ctx, 'billing', dropped.id, 'Duplicate of an earlier invoice');
    const body = journalExport(scene.ctx, '2026-03-01', '2026-04-01', 'csv').body;
    assert.ok(body.includes(kept.number!));
    assert.ok(!body.includes(dropped.number!));
  });

  it('leaves out drafts', () => {
    const job = open(scene);
    work(scene, job.id);
    draftInvoiceForJob(scene.ctx, 'billing', job.id);
    assert.equal(journalExport(scene.ctx, '2026-01-01', '2027-01-01', 'csv').body, '\n');
  });

  it('renders every registered format', () => {
    issued('2026-03-03');
    for (const [key, format] of scene.ctx.exportFormats.entries()) {
      const exported = journalExport(scene.ctx, '2026-03-01', '2026-04-01', key.toUpperCase());
      assert.equal(exported.contentType, format.contentType, key);
      assert.ok(exported.body.length > 0, key);
    }
  });

  it('refuses unknown formats', () => {
    assert.throws(
      () => journalExport(scene.ctx, '2026-03-01', '2026-04-01', 'xml'),
      (error: any) => error.code === 'unknown-format' && error.message === 'Unknown export format xml',
    );
  });

  it('covers invoices of several regions', () => {
    const local = issued('2026-03-03');
    const lake = issued('2026-03-04', 'lakeside');
    const body = journalExport(scene.ctx, '2026-03-01', '2026-04-01', 'tsv').body;
    for (const invoice of [local, lake]) assert.ok(body.includes(`\t${invoice.number}\t`));
  });
});
'''

API = r'''
let world: World;

beforeEach(() => {
  world = createWorld();
});

const ENDPOINT = 'https://hooks.harbourview.example/fieldline';
const stored = () => world.app.ctx.store.collection<Webhook>('webhooks');

describe('POST /webhooks', () => {
  it('registers a webhook and shows only a secret hint', () => {
    const body = expectStatus(world.app, 201, 'POST', '/webhooks', 'admin', { url: ENDPOINT, topics: ['sla.breached', 'appointment.booked'] });
    assert.equal(body.id, 'HOOK-000001');
    assert.deepEqual(body.topics, ['appointment.booked', 'sla.breached']);
    assert.equal(body.active, true);
    assert.equal(body.secret, undefined);
    assert.equal(body.secretHint, `${stored().require(body.id, 'webhook').secret.slice(0, 4)}…`);
  });

  it('accepts a customer restriction', () => {
    const body = call(world.app, 'POST', '/webhooks', 'admin', { url: ENDPOINT, topics: ['appointment.booked'], customerId: world.customer.id });
    assert.equal(body.customerId, world.customer.id);
  });

  it('is reserved for administrators', () => {
    for (const actor of ['dispatch', 'billing', 'warehouse', world.technician.id]) {
      assert.equal(expectStatus(world.app, 403, 'POST', '/webhooks', actor, { url: ENDPOINT, topics: ['sla.breached'] }).error.code, 'forbidden');
    }
    assert.equal(stored().count(), 0);
  });

  it('validates the payload', () => {
    const invalid: unknown[] = [
      {},
      { url: ENDPOINT },
      { topics: ['sla.breached'] },
      { url: 'https://a', topics: ['sla.breached'] },
      { url: ENDPOINT, topics: [] },
      { url: ENDPOINT, topics: [''] },
      { url: ENDPOINT, topics: 'sla.breached' },
    ];
    for (const payload of invalid) assert.equal(expectStatus(world.app, 422, 'POST', '/webhooks', 'admin', payload).error.code, 'invalid-input');
  });

  it('explains domain rejections', () => {
    assert.equal(expectStatus(world.app, 422, 'POST', '/webhooks', 'admin', { url: 'http://hooks.example/inbound', topics: ['sla.breached'] }).error.code, 'invalid-url');
    const body = expectStatus(world.app, 422, 'POST', '/webhooks', 'admin', { url: ENDPOINT, topics: ['sla.breached', 'rain.started'] });
    assert.equal(body.error.code, 'unknown-topic');
    assert.match(body.error.message, /rain\.started/);
  });
});

describe('GET /webhooks/topics', () => {
  it('lists the published topics once and sorted', () => {
    const expected = [...new Set(Object.values(EXTERNAL_TOPICS))].sort();
    assert.deepEqual(call(world.app, 'GET', '/webhooks/topics', 'admin').items, expected);
  });

  it('is reserved for administrators', () => {
    for (const actor of ['dispatch', 'billing']) expectStatus(world.app, 403, 'GET', '/webhooks/topics', actor);
  });
});

describe('GET /webhooks/deliveries', () => {
  beforeEach(() => {
    call(world.app, 'POST', '/webhooks', 'admin', { url: ENDPOINT, topics: ['sla.breached', 'appointment.booked'] });
    world.app.ctx.bus.emit({ type: SlaEvents.breached, at: at(MONDAY, '08:30'), jobId: 'WO-000001', customerId: world.customer.id });
    world.app.ctx.bus.emit({ type: AppointmentEvents.booked, at: at(MONDAY, '08:40'), customerId: world.customer.id });
  });

  it('lists every delivery', () => {
    const { items } = call(world.app, 'GET', '/webhooks/deliveries', 'admin');
    assert.deepEqual(items.map((item: any) => [item.id, item.topic, item.status]), [
      ['DLV-000001', 'sla.breached', 'pending'],
      ['DLV-000002', 'appointment.booked', 'pending'],
    ]);
  });

  it('filters by topic', () => {
    const { items } = call(world.app, 'GET', '/webhooks/deliveries?topic=appointment.booked', 'admin');
    assert.deepEqual(items.map((item: any) => item.id), ['DLV-000002']);
    assert.deepEqual(call(world.app, 'GET', '/webhooks/deliveries?topic=job.cancelled', 'admin').items, []);
  });

  it('is reserved for administrators', () => {
    for (const actor of ['dispatch', 'billing', world.technician.id]) expectStatus(world.app, 403, 'GET', '/webhooks/deliveries', actor);
  });
});

describe('GET /exports/journal', () => {
  const RANGE = 'from=2026-03-01&to=2026-04-01';

  function issueInvoice() {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id);
    call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    completeChecklist(world.app, job.id, world.technician.id);
    call(world.app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Replaced the pressure valve' });
    const draft = call(world.app, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
    return call(world.app, 'POST', `/invoices/${draft.id}/issue`, 'billing');
  }

  it('returns CSV by default', () => {
    const invoice = issueInvoice();
    const response = world.app.request('GET', `/exports/journal?${RANGE}`, { actor: 'billing' });
    assert.equal(response.status, 200);
    assert.equal(response.headers?.['content-type'], 'text/csv');
    assert.ok(String(response.body).includes(`,1200,${invoice.number},${invoice.totalCents},0\n`));
  });

  it('switches format on request', () => {
    issueInvoice();
    const response = world.app.request('GET', `/exports/journal?${RANGE}&format=json`, { actor: 'admin' });
    assert.equal(response.headers?.['content-type'], 'application/json');
    const rows = JSON.parse(String(response.body));
    assert.ok(rows.length >= 3);
    assert.equal(rows.reduce((total: number, row: any) => total + row.debitCents - row.creditCents, 0), 0);
  });

  it('rejects unknown formats', () => {
    assert.equal(expectStatus(world.app, 422, 'GET', `/exports/journal?${RANGE}&format=xlsx`, 'admin').error.code, 'unknown-format');
  });

  it('requires a valid period', () => {
    for (const query of ['', 'from=2026-03-01', 'to=2026-04-01', 'from=march&to=2026-04-01']) {
      expectStatus(world.app, 422, 'GET', `/exports/journal?${query}`, 'admin');
    }
  });

  it('is limited to billing access', () => {
    for (const actor of ['admin', 'billing']) expectStatus(world.app, 200, 'GET', `/exports/journal?${RANGE}`, actor);
    for (const actor of ['dispatch', 'warehouse', world.technician.id]) {
      assert.equal(expectStatus(world.app, 403, 'GET', `/exports/journal?${RANGE}`, actor).error.code, 'forbidden');
    }
  });
});
'''

FILES = {
    "tests/integrations/formats.test.ts": Spec(
        {f"{INTEGRATIONS}/formats.ts": ["createExportRegistry"]}, FORMATS),
    "tests/integrations/webhooks.test.ts": Spec(
        {"src/app.ts": ["createApp"], f"{INTEGRATIONS}/webhooks.ts": ["EXTERNAL_TOPICS", "registerWebhook", "sign"],
         f"{INTEGRATIONS}/types.ts": ["type Delivery", "type Webhook"], WORLD: ["at", "MONDAY"],
         "src/adapters/store/audit.ts": ["auditTrail"], "src/context.ts": ["type AppContext"],
         "src/scheduling/model.ts": ["AppointmentEvents"], "src/shared/errors.ts": ["DomainError"],
         "src/sla/model.ts": ["SlaEvents"], "node:crypto": ["createHmac"]}, WEBHOOKS, node=WITH_SETUP),
    "tests/integrations/accounting.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY", "seedCustomer"],
         f"{INTEGRATIONS}/accounting.ts": ["journalExport", "journalFor", "type JournalLine"],
         "src/billing/invoices.ts": ["draftInvoiceForJob", "issueInvoice", "voidInvoice"],
         "src/billing/model.ts": ["type Invoice", "type InvoiceLine"]}, ACCOUNTING),
    "tests/api/integrations.test.ts": Spec(
        {WORLD: ["at", "call", "completeChecklist", "createWorld", "expectStatus", "JOBS", "MONDAY",
                 "openWorkOrder", "type World"],
         f"{INTEGRATIONS}/types.ts": ["type Webhook"], f"{INTEGRATIONS}/webhooks.ts": ["EXTERNAL_TOPICS"],
         "src/scheduling/model.ts": ["AppointmentEvents"], "src/sla/model.ts": ["SlaEvents"]},
        API, node=WITH_SETUP),
}


def generate(ctx):
    register(ctx, FILES)
