"""Tests for quotes: pricing, the quote lifecycle and the quote routes."""

from .checks_kit import WITH_SETUP, Spec, register
from .checks_workorders import scene_spec

WORLD = "tests/support/world.ts"
QUOTES = "src/quotes"

PRICING = r'''
let scene: Scene;

beforeEach(() => {
  scene = start();
});

const customerOf = (id = scene.customer.id) => customers(scene.ctx).require(id);
const handler = (key: string) => scene.ctx.workTypes.resolve(key);
const price = (key: string, input: { hazards?: string[]; quantity?: number } = {}, customer = customerOf()) =>
  priceQuote(scene.ctx, customer, handler(key), { hazards: input.hazards ?? [], quantity: input.quantity });

describe('priceQuote labour and callout', () => {
  it('prices a one hour job from the tariff', () => {
    const quote = price('general');
    assert.deepEqual(quote.lines, [
      { description: 'Callout', quantity: 1, unitCents: 4500, amountCents: 4500 },
      { description: 'General maintenance labour, 1 × 60 min', quantity: 1, unitCents: 7200, amountCents: 7200 },
    ]);
    assert.equal(quote.estimatedMinutes, 60);
    assert.equal(quote.crew, 1);
    assert.equal(quote.currency, 'EUR');
  });

  it('adds tax and quotes whole currency units', () => {
    assert.equal(price('general').totalCents, 13900);
  });

  it('rounds the estimated minutes up to the next quarter hour', () => {
    const quote = price('general', { quantity: 3 });
    assert.equal(quote.estimatedMinutes, 105);
    assert.equal(quote.lines[1].quantity, 1.75);
    assert.equal(quote.lines[1].amountCents, 12600);
    assert.equal(quote.totalCents, 20300);
  });

  it('charges every crew member for the full duration', () => {
    const quote = price('general', { hazards: ['confined-space'] });
    assert.equal(quote.crew, 2);
    assert.equal(quote.lines[1].description, 'General maintenance labour, 2 × 60 min');
    assert.equal(quote.lines[1].amountCents, 14400);
    assert.equal(quote.totalCents, 22500);
  });

  it('uses the labour category of the work type', () => {
    const quote = price('duct-cleaning');
    assert.equal(quote.crew, 2);
    assert.equal(quote.estimatedMinutes, 180);
    assert.equal(quote.lines[1].unitCents, 4800);
    assert.equal(quote.lines[1].amountCents, 28800);
    assert.equal(quote.totalCents, 39600);
  });

  it('prices specialist work with the specialist rate', () => {
    const quote = price('heat-pump-install');
    assert.equal(quote.lines[1].unitCents, 9600);
    assert.equal(quote.crew, 2);
  });

  it('leaves tax off for exempt customers', () => {
    const exempt = seedCustomer(scene.app, { name: 'Marlow Hospice', taxExempt: true }).customer;
    const quote = price('general', {}, customerOf(exempt.id));
    assert.equal(quote.totalCents, 11700);
  });

  it('follows the tariff of the billing region', () => {
    const lake = seedCustomer(scene.app, { name: 'Lakeview Marina', region: 'lakeside' }).customer;
    const quote = price('general', {}, customerOf(lake.id));
    assert.equal(quote.currency, 'CHF');
    assert.equal(quote.lines[0].unitCents, 9000);
    assert.equal(quote.lines[1].unitCents, 11200);
    assert.equal(quote.totalCents, 21800);
  });

  it('passes the estimate notes through', () => {
    assert.deepEqual(price('general').notes, []);
    assert.deepEqual(price('emergency-callout').notes, ['make safe first; repairs follow up']);
  });
});

describe('priceQuote parts', () => {
  const KIT = { id: 'GSK-BOILER-KIT', name: 'Boiler service kit', category: 'heating', unitCostCents: 2000, listPriceCents: 2500 };

  it('skips estimated parts that are not in the catalogue', () => {
    const quote = price('boiler-service');
    assert.equal(quote.lines.length, 2);
  });

  it('lists catalogue parts at their invoice price', () => {
    addPart(scene.ctx, 'warehouse', KIT);
    const quote = price('boiler-service');
    assert.equal(quote.lines.length, 3);
    assert.deepEqual(quote.lines[2], { description: 'Boiler service kit', quantity: 1, unitCents: 2500, amountCents: 2500 });
    assert.equal(quote.lines[1].amountCents, 9000);
    assert.equal(quote.totalCents, 19000);
  });

  it('never prices below the list price', () => {
    addPart(scene.ctx, 'warehouse', { ...KIT, unitCostCents: 1000, listPriceCents: 3100 });
    assert.equal(price('boiler-service').lines[2].unitCents, 3100);
  });

  it('lengthens the visit for restricted access', () => {
    const quote = price('boiler-service', { hazards: ['restricted-access'] });
    assert.equal(quote.estimatedMinutes, 90);
    assert.equal(quote.lines[1].quantity, 1.5);
    assert.equal(quote.lines[1].amountCents, 10800);
  });
});
'''

SERVICE = r'''
let scene: Scene;

beforeEach(() => {
  scene = start();
});

const request = (extra: Record<string, unknown> = {}) => ({
  customerId: scene.customer.id,
  siteId: scene.site.id,
  workType: 'general',
  ...extra,
});

const draft = (extra: Record<string, unknown> = {}) => createQuote(scene.ctx, 'dispatch', request(extra));
const sent = (extra: Record<string, unknown> = {}) => sendQuote(scene.ctx, 'dispatch', draft(extra).id);

function failure(action: () => unknown) {
  try {
    action();
  } catch (error: any) {
    return { code: error.code as string, status: error.status as number };
  }
  return assert.fail('expected the call to fail');
}

describe('createQuote', () => {
  it('stores a priced draft', () => {
    const quote = draft();
    assert.equal(quote.id, 'QUO-000001');
    assert.equal(quote.number, 'Q-2026-000001');
    assert.equal(quote.status, 'draft');
    assert.equal(quote.workType, 'general');
    assert.equal(quote.totalCents, 13900);
    assert.equal(quote.estimatedMinutes, 60);
    assert.equal(quote.createdAt, at(MONDAY, '08:00'));
    assert.deepEqual(quotes(scene.ctx).require(quote.id), quote);
  });

  it('numbers quotes sequentially', () => {
    assert.deepEqual([draft(), draft(), draft()].map((quote) => quote.number), ['Q-2026-000001', 'Q-2026-000002', 'Q-2026-000003']);
  });

  it('is valid for thirty days unless told otherwise', () => {
    assert.equal(draft().validUntil, '2026-04-01');
    assert.equal(draft({ validDays: 7 }).validUntil, '2026-03-09');
    assert.equal(draft({ validDays: 90 }).validUntil, '2026-05-31');
  });

  it('prices the quantity into the estimate', () => {
    const quote = draft({ quantity: 5 });
    assert.equal(quote.quantity, 5);
    assert.equal(quote.estimatedMinutes, 150);
  });

  it('uses the hazards of the site for the crew size', () => {
    call(scene.app, 'PUT', `/sites/${scene.site.id}/access`, 'admin', { hazards: ['working-at-height'] });
    assert.equal(draft().crew, 2);
  });

  it('records an audit entry', () => {
    const quote = draft();
    const [entry] = auditTrail(scene.ctx, quote.id);
    assert.equal(entry.action, 'quote.created');
    assert.equal(entry.actorId, 'dispatch');
    assert.deepEqual(entry.details, { totalCents: 13900 });
  });

  it('rejects unknown work types', () => {
    assert.deepEqual(failure(() => draft({ workType: 'time-travel' })), { code: 'unknown-work-type', status: 422 });
  });

  it('rejects unknown customers and foreign sites', () => {
    const other = seedCustomer(scene.app, { name: 'Brightwater Dental' });
    assert.deepEqual(failure(() => draft({ customerId: 'CUS-999999' })), { code: 'not-found', status: 404 });
    assert.deepEqual(failure(() => draft({ siteId: other.site.id })), { code: 'not-found', status: 404 });
  });

  it('validates the input', () => {
    const invalid: Array<Record<string, unknown>> = [
      { validDays: 0 },
      { validDays: 91 },
      { validDays: 2.5 },
      { quantity: -1 },
      { quantity: 10_001 },
      { workType: '' },
      { siteId: undefined },
    ];
    for (const extra of invalid) assert.deepEqual(failure(() => draft(extra)), { code: 'invalid-input', status: 422 }, JSON.stringify(extra));
    assert.equal(failure(() => createQuote(scene.ctx, 'dispatch', null)).code, 'invalid-input');
  });
});

describe('sendQuote', () => {
  it('marks a draft as sent', () => {
    const quote = sent();
    assert.equal(quote.status, 'sent');
    assert.equal(auditTrail(scene.ctx, quote.id).at(-1)?.action, 'quote.sent');
  });

  it('refuses quotes that are no longer drafts', () => {
    const quote = sent();
    assert.deepEqual(failure(() => sendQuote(scene.ctx, 'dispatch', quote.id)), { code: 'not-draft', status: 409 });
    assert.deepEqual(failure(() => sendQuote(scene.ctx, 'dispatch', 'QUO-404404')), { code: 'not-found', status: 404 });
  });
});

describe('acceptQuote', () => {
  it('opens the quoted work order', () => {
    const quote = sent({ quantity: 2 });
    const { quote: accepted, jobId } = acceptQuote(scene.ctx, 'dispatch', quote.id);
    assert.equal(accepted.status, 'accepted');
    assert.equal(accepted.acceptedJobId, jobId);
    const job = «requireJob»(scene.ctx, jobId);
    assert.equal(job.summary, 'Quoted work Q-2026-000001');
    assert.equal(job.source, 'portal');
    assert.deepEqual(job.tags, ['quoted']);
    assert.equal(job.customerId, scene.customer.id);
    assert.equal(job.siteId, scene.site.id);
    assert.equal(job.workType, 'general');
    assert.equal(job.description, quote.lines.map((line) => line.description).join('; '));
  });

  it('is still possible on the last day of validity', () => {
    const quote = sent({ validDays: 3 });
    scene.app.clock.set(at('2026-03-05', '15:00'));
    assert.equal(acceptQuote(scene.ctx, 'dispatch', quote.id).quote.status, 'accepted');
  });

  it('expires the quote when validity has passed', () => {
    const quote = sent({ validDays: 3 });
    scene.app.clock.set(at('2026-03-06', '09:00'));
    assert.deepEqual(failure(() => acceptQuote(scene.ctx, 'dispatch', quote.id)), { code: 'quote-expired', status: 409 });
    assert.equal(quotes(scene.ctx).require(quote.id).status, 'expired');
    assert.equal(«listJobs»(scene.ctx).length, 0);
  });

  it('requires a sent quote', () => {
    const unsent = draft();
    assert.deepEqual(failure(() => acceptQuote(scene.ctx, 'dispatch', unsent.id)), { code: 'not-sent', status: 409 });
    const quote = sent();
    acceptQuote(scene.ctx, 'dispatch', quote.id);
    assert.deepEqual(failure(() => acceptQuote(scene.ctx, 'dispatch', quote.id)), { code: 'not-sent', status: 409 });
    assert.equal(«listJobs»(scene.ctx).length, 1);
  });

  it('audits the acceptance with the new work order', () => {
    const quote = sent();
    const { jobId } = acceptQuote(scene.ctx, 'billing', quote.id);
    const entry = auditTrail(scene.ctx, quote.id).at(-1);
    assert.equal(entry?.action, 'quote.accepted');
    assert.equal(entry?.actorId, 'billing');
    assert.deepEqual(entry?.details, { jobId });
  });
});

describe('declineQuote', () => {
  it('declines drafts and sent quotes with a reason', () => {
    const first = declineQuote(scene.ctx, 'dispatch', draft().id, 'Found a cheaper provider');
    assert.equal(first.status, 'declined');
    assert.equal(first.declineReason, 'Found a cheaper provider');
    const second = declineQuote(scene.ctx, 'dispatch', sent().id, 'Budget frozen until autumn');
    assert.equal(second.status, 'declined');
    assert.deepEqual(auditTrail(scene.ctx, second.id).at(-1)?.details, { reason: 'Budget frozen until autumn' });
  });

  it('refuses quotes that are already settled', () => {
    const accepted = sent();
    acceptQuote(scene.ctx, 'dispatch', accepted.id);
    assert.deepEqual(failure(() => declineQuote(scene.ctx, 'dispatch', accepted.id, 'Changed our mind')), { code: 'not-open', status: 409 });
    const declined = draft();
    declineQuote(scene.ctx, 'dispatch', declined.id, 'Not needed any more');
    assert.deepEqual(failure(() => declineQuote(scene.ctx, 'dispatch', declined.id, 'Not needed any more')), { code: 'not-open', status: 409 });
  });
});

describe('expireQuotes', () => {
  it('expires open quotes whose validity ended before today', () => {
    const short = sent({ validDays: 3 });
    const long = sent({ validDays: 30 });
    const untouched = draft({ validDays: 3 });
    assert.deepEqual(expireQuotes(scene.ctx, '2026-03-05'), []);
    assert.deepEqual(expireQuotes(scene.ctx, '2026-03-06').sort(), [short.id, untouched.id].sort());
    assert.equal(quotes(scene.ctx).require(short.id).status, 'expired');
    assert.equal(quotes(scene.ctx).require(untouched.id).status, 'expired');
    assert.equal(quotes(scene.ctx).require(long.id).status, 'sent');
  });

  it('leaves settled quotes alone', () => {
    const accepted = sent({ validDays: 3 });
    acceptQuote(scene.ctx, 'dispatch', accepted.id);
    const declined = draft({ validDays: 3 });
    declineQuote(scene.ctx, 'dispatch', declined.id, 'Work was done in-house');
    assert.deepEqual(expireQuotes(scene.ctx, '2026-06-01'), []);
    assert.equal(quotes(scene.ctx).require(accepted.id).status, 'accepted');
    assert.equal(quotes(scene.ctx).require(declined.id).status, 'declined');
  });

  it('feeds the open quote list of a customer', () => {
    const open = sent();
    const gone = draft();
    declineQuote(scene.ctx, 'dispatch', gone.id, 'Not needed any more');
    const expiring = draft({ validDays: 3 });
    assert.deepEqual(openQuotesForCustomer(scene.ctx, scene.customer.id).map((quote) => quote.id).sort(), [expiring.id, open.id].sort());
    expireQuotes(scene.ctx, '2026-03-10');
    assert.deepEqual(openQuotesForCustomer(scene.ctx, scene.customer.id).map((quote) => quote.id), [open.id]);
    assert.deepEqual(openQuotesForCustomer(scene.ctx, 'CUS-999999'), []);
  });
});
'''

API = r'''
let world: World;
let portal: string;

beforeEach(() => {
  world = createWorld();
  portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
});

const quoteBody = () => ({ customerId: world.customer.id, siteId: world.site.id, workType: 'general' });
const draftQuote = () => call(world.app, 'POST', '/quotes', 'dispatch', quoteBody());
const sentQuote = () => {
  const quote = draftQuote();
  return call(world.app, 'POST', `/quotes/${quote.id}/send`, 'dispatch');
};

describe('POST /quotes', () => {
  it('creates a priced draft', () => {
    const quote = expectStatus(world.app, 201, 'POST', '/quotes', 'dispatch', { ...quoteBody(), quantity: 3, validDays: 14 });
    assert.equal(quote.status, 'draft');
    assert.equal(quote.totalCents, 20300);
    assert.equal(quote.validUntil, '2026-03-16');
    assert.equal(quote.lines.length, 2);
  });

  it('allows the quoting roles only', () => {
    for (const actor of ['admin', 'dispatch', 'billing']) expectStatus(world.app, 201, 'POST', '/quotes', actor, quoteBody());
    for (const actor of ['warehouse', world.technician.id, portal]) {
      assert.equal(expectStatus(world.app, 403, 'POST', '/quotes', actor, quoteBody()).error.code, 'forbidden');
    }
  });

  it('reports invalid input with the offending fields', () => {
    const body = expectStatus(world.app, 422, 'POST', '/quotes', 'dispatch', { customerId: world.customer.id, validDays: 0 });
    const paths = body.error.details.issues.map((issue: any) => issue.path).sort();
    assert.deepEqual(paths, ['siteId', 'validDays', 'workType']);
    assert.equal(expectStatus(world.app, 422, 'POST', '/quotes', 'dispatch', { ...quoteBody(), workType: 'levitation' }).error.code, 'unknown-work-type');
  });
});

describe('GET /quotes/:id', () => {
  it('returns the stored quote to the quoting roles', () => {
    const quote = draftQuote();
    assert.deepEqual(call(world.app, 'GET', `/quotes/${quote.id}`, 'billing'), quote);
    expectStatus(world.app, 403, 'GET', `/quotes/${quote.id}`, 'warehouse');
    expectStatus(world.app, 403, 'GET', `/quotes/${quote.id}`, portal);
  });

  it('reports unknown quotes as missing', () => {
    assert.equal(expectStatus(world.app, 404, 'GET', '/quotes/QUO-777777', 'admin').error.details.entity, 'quote');
  });
});

describe('quote transitions', () => {
  it('sends a draft once', () => {
    const quote = draftQuote();
    assert.equal(expectStatus(world.app, 200, 'POST', `/quotes/${quote.id}/send`, 'dispatch').status, 'sent');
    assert.equal(expectStatus(world.app, 409, 'POST', `/quotes/${quote.id}/send`, 'dispatch').error.code, 'not-draft');
    expectStatus(world.app, 403, 'POST', `/quotes/${quote.id}/send`, world.technician.id);
  });

  it('lets staff accept a sent quote', () => {
    const quote = sentQuote();
    const result = expectStatus(world.app, 200, 'POST', `/quotes/${quote.id}/accept`, 'dispatch');
    assert.equal(result.quote.status, 'accepted');
    const job = call(world.app, 'GET', `${JOBS}/${result.jobId}`, 'dispatch');
    assert.equal(job.summary, `Quoted work ${quote.number}`);
    assert.equal(job.source, 'portal');
  });

  it('lets the customer accept its own quote', () => {
    const quote = sentQuote();
    const result = expectStatus(world.app, 200, 'POST', `/quotes/${quote.id}/accept`, portal);
    assert.equal(call(world.app, 'GET', `${JOBS}/${result.jobId}`, portal).status, 'new');
  });

  it('keeps other customers away from the quote', () => {
    const quote = sentQuote();
    const stranger = seedCustomer(world.app, { name: 'Brightwater Dental' }).customer;
    const outsider = call(world.app, 'POST', `/customers/${stranger.id}/portal-users`, 'admin', { name: 'Sam Rivera' }).id;
    assert.equal(expectStatus(world.app, 403, 'POST', `/quotes/${quote.id}/accept`, outsider).error.code, 'forbidden');
    assert.equal(expectStatus(world.app, 403, 'POST', `/quotes/${quote.id}/decline`, outsider, { reason: 'Not mine' }).error.code, 'forbidden');
    assert.equal(call(world.app, 'GET', `/quotes/${quote.id}`, 'admin').status, 'sent');
  });

  it('refuses acceptance from roles without quoting rights', () => {
    const quote = sentQuote();
    for (const actor of ['warehouse', world.technician.id]) expectStatus(world.app, 403, 'POST', `/quotes/${quote.id}/accept`, actor);
  });

  it('refuses drafts and expired quotes', () => {
    expectStatus(world.app, 409, 'POST', `/quotes/${draftQuote().id}/accept`, 'dispatch');
    const quote = sentQuote();
    world.app.clock.set(at('2026-04-02', '10:00'));
    assert.equal(expectStatus(world.app, 409, 'POST', `/quotes/${quote.id}/accept`, portal).error.code, 'quote-expired');
    assert.equal(call(world.app, 'GET', `/quotes/${quote.id}`, 'admin').status, 'expired');
  });

  it('declines with a reason of at least three characters', () => {
    const quote = sentQuote();
    expectStatus(world.app, 422, 'POST', `/quotes/${quote.id}/decline`, portal, { reason: 'no' });
    expectStatus(world.app, 422, 'POST', `/quotes/${quote.id}/decline`, portal, {});
    const declined = expectStatus(world.app, 200, 'POST', `/quotes/${quote.id}/decline`, portal, { reason: 'Too expensive for us' });
    assert.equal(declined.status, 'declined');
    assert.equal(declined.declineReason, 'Too expensive for us');
    assert.equal(expectStatus(world.app, 409, 'POST', `/quotes/${quote.id}/decline`, 'dispatch', { reason: 'Again' }).error.code, 'not-open');
  });

  it('reports unknown quotes before checking the role', () => {
    expectStatus(world.app, 404, 'POST', '/quotes/QUO-909090/accept', world.technician.id);
    expectStatus(world.app, 404, 'POST', '/quotes/QUO-909090/decline', portal, { reason: 'Whatever' });
  });
});

describe('GET /customers/:id/quotes', () => {
  it('lists the open quotes of a customer', () => {
    const open = sentQuote();
    const declined = draftQuote();
    call(world.app, 'POST', `/quotes/${declined.id}/decline`, 'dispatch', { reason: 'Duplicate quote' });
    const body = expectStatus(world.app, 200, 'GET', `/customers/${world.customer.id}/quotes`, 'billing');
    assert.deepEqual(body.items.map((quote: any) => quote.id), [open.id]);
  });

  it('is limited to quoting roles', () => {
    for (const actor of ['warehouse', world.technician.id, portal]) {
      expectStatus(world.app, 403, 'GET', `/customers/${world.customer.id}/quotes`, actor);
    }
  });
});

describe('POST /quotes/expire', () => {
  it('expires overdue quotes as of the current date', () => {
    const quote = sentQuote();
    assert.deepEqual(call(world.app, 'POST', '/quotes/expire', 'admin'), { expired: [] });
    world.app.clock.set(at('2026-04-02', '09:00'));
    assert.deepEqual(call(world.app, 'POST', '/quotes/expire', 'admin'), { expired: [quote.id] });
    assert.equal(call(world.app, 'GET', `/quotes/${quote.id}`, 'admin').status, 'expired');
  });

  it('is reserved for administrators', () => {
    for (const actor of ['dispatch', 'billing', 'warehouse']) expectStatus(world.app, 403, 'POST', '/quotes/expire', actor);
  });
});
'''

FILES = {
    "tests/quotes/pricing.test.ts": scene_spec(
        {WORLD: ["seedCustomer"], f"{QUOTES}/pricing.ts": ["priceQuote"], "src/adapters/repositories/customers.ts": ["customers"],
         "src/inventory/catalog.ts": ["addPart"]}, PRICING),
    "tests/quotes/service.test.ts": scene_spec(
        {WORLD: ["at", "call", "MONDAY", "seedCustomer"],
         f"{QUOTES}/service.ts": ["acceptQuote", "createQuote", "declineQuote", "expireQuotes", "sendQuote"],
         "src/adapters/repositories/quotes.ts": ["openQuotesForCustomer", "quotes"],
         "src/adapters/store/audit.ts": ["auditTrail"]}, SERVICE, jobs=["listJobs"]),
    "tests/api/quotes.test.ts": Spec(
        {WORLD: ["at", "call", "createWorld", "expectStatus", "JOBS", "seedCustomer", "type World"]}, API,
        node=WITH_SETUP),
}


def generate(ctx):
    register(ctx, FILES)
