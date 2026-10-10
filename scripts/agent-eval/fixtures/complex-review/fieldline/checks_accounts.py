"""Tests for customers, contracts and assets: services, validation and their API routes."""

from .checks_kit import APP, WITH_SETUP, Spec, register

WORLD = "tests/support/world.ts"
VALIDATE = "src/shared/validate.ts"
CUSTOMERS = "src/customers/service.ts"
CUSTOMER_INPUT = "src/customers/validation.ts"

ACCOUNT_NUMBERS = r'''
describe('check letters', () => {
  const cases: Array<[string, string]> = [
    ['000000', 'A'],
    ['000001', 'H'],
    ['000002', 'Q'],
    ['000003', 'X'],
    ['004211', 'N'],
    ['123456', 'R'],
    ['999999', 'G'],
  ];
  for (const [digits, letter] of cases) {
    it(`derives ${letter} from ${digits}`, () => {
      assert.equal(checkLetter(digits), letter);
    });
  }

  it('weights alternating positions so transposed digits change the letter', () => {
    assert.equal(checkLetter('000012'), 'T');
    assert.equal(checkLetter('000021'), 'P');
  });
});

describe('formatAccountNumber', () => {
  it('prefixes the segment initial and pads the sequence', () => {
    assert.equal(formatAccountNumber('commercial', 1), 'C000001H');
    assert.equal(formatAccountNumber('residential', 2), 'R000002Q');
    assert.equal(formatAccountNumber('public', 3), 'P000003X');
  });

  it('keeps six digits for large sequences', () => {
    assert.equal(formatAccountNumber('commercial', 123456), 'C123456R');
    assert.equal(formatAccountNumber('residential', 999999), 'R999999G');
  });

  it('refuses sequences that no longer fit', () => {
    assert.throws(() => formatAccountNumber('commercial', 1_000_000), RangeError);
  });
});

describe('isValidAccountNumber', () => {
  it('accepts well-formed numbers in any case with surrounding spaces', () => {
    assert.equal(isValidAccountNumber('C000001H'), true);
    assert.equal(isValidAccountNumber(' c000001h '), true);
    assert.equal(isValidAccountNumber('P000003X'), true);
  });

  it('rejects a wrong check letter', () => {
    assert.equal(isValidAccountNumber('C000001J'), false);
    assert.equal(isValidAccountNumber('C000021T'), false);
  });

  it('rejects unknown segment letters and malformed shapes', () => {
    for (const value of ['X000001H', 'C00001H', 'C0000011H', 'C000001', '000001H', '']) {
      assert.equal(isValidAccountNumber(value), false, value);
    }
  });

  it('validates every generated number', () => {
    for (let sequence = 1; sequence <= 60; sequence += 1) {
      assert.equal(isValidAccountNumber(formatAccountNumber('public', sequence)), true, String(sequence));
    }
  });
});

describe('nextAccountNumber', () => {
  it('draws from one sequence across segments', () => {
    const { ctx } = createApp();
    assert.equal(nextAccountNumber(ctx, 'commercial'), 'C000001H');
    assert.equal(nextAccountNumber(ctx, 'residential'), 'R000002Q');
    assert.equal(nextAccountNumber(ctx, 'public'), 'P000003X');
  });

  it('does not share its sequence with other identifier families', () => {
    const { ctx } = createApp();
    ctx.ids.next('CUS');
    ctx.ids.next('CUS');
    assert.equal(nextAccountNumber(ctx, 'commercial'), 'C000001H');
  });
});
'''

CUSTOMER_VALIDATION = r'''
function issuesOf(check: Check<unknown>, input: unknown): string[] {
  try {
    parseInput(check, input);
  } catch (error) {
    if (error instanceof ValidationError) return error.issues.map((issue) => `${issue.path || 'input'}: ${issue.message}`);
    throw error;
  }
  return [];
}

describe('customerInput', () => {
  it('accepts a minimal customer and applies defaults', () => {
    const parsed = parseInput(customerInput, { name: 'Oakfield School', segment: 'public', billingRegion: 'valley' });
    assert.deepEqual(parsed, { name: 'Oakfield School', segment: 'public', billingRegion: 'valley', taxExempt: false });
  });

  it('trims text and keeps optional contact details', () => {
    const parsed = parseInput(customerInput, {
      name: '  Oak Lane Dental  ',
      segment: 'commercial',
      billingRegion: 'metro-east',
      email: 'Reception@OakLane.example',
      phone: '+49 30 123456',
      creditLimitCents: 0,
    });
    assert.equal(parsed.name, 'Oak Lane Dental');
    assert.equal(parsed.email, 'Reception@OakLane.example');
    assert.equal(parsed.phone, '+49 30 123456');
    assert.equal(parsed.creditLimitCents, 0);
  });

  it('treats null optional fields as absent', () => {
    const parsed = parseInput(customerInput, {
      name: 'Birchwood Hall',
      segment: 'residential',
      billingRegion: 'coastal',
      email: null,
      creditLimitCents: null,
    });
    assert.equal('email' in parsed, false);
    assert.equal('creditLimitCents' in parsed, false);
  });

  it('reports every problem with its field path in declaration order', () => {
    assert.deepEqual(
      issuesOf(customerInput, {
        name: 'A',
        segment: 'industrial',
        billingRegion: '  ',
        email: 'nope',
        phone: 'abc',
        creditLimitCents: 1.5,
        vip: true,
      }),
      [
        'name: must have at least 2 characters',
        'segment: must be one of residential, commercial, public',
        'billingRegion: is required',
        'email: has an invalid format',
        'phone: has an invalid format',
        'creditLimitCents: must be a whole number',
        'vip: is not a recognized field',
      ],
    );
  });

  it('rejects negative credit limits and non-boolean tax flags', () => {
    assert.deepEqual(
      issuesOf(customerInput, { name: 'Pine Court', segment: 'commercial', billingRegion: 'valley', taxExempt: 'yes', creditLimitCents: -1 }),
      ['taxExempt: must be true or false', 'creditLimitCents: must be at least 0'],
    );
  });

  it('rejects inputs that are not objects', () => {
    assert.deepEqual(issuesOf(customerInput, 'Pine Court'), ['input: must be an object']);
    assert.deepEqual(issuesOf(customerInput, [1, 2]), ['input: must be an object']);
  });

  it('treats a missing body as an empty object', () => {
    assert.deepEqual(issuesOf(customerInput, undefined), [
      'name: must be a string',
      'segment: must be one of residential, commercial, public',
      'billingRegion: must be a string',
    ]);
  });
});

describe('customerPatch', () => {
  it('accepts an empty patch and partial updates', () => {
    assert.deepEqual(parseInput(customerPatch, {}), {});
    assert.deepEqual(parseInput(customerPatch, { phone: '020 7946 0000' }), { phone: '020 7946 0000' });
  });

  it('does not allow changing the segment', () => {
    assert.deepEqual(issuesOf(customerPatch, { segment: 'public' }), ['segment: is not a recognized field']);
  });
});

describe('siteInput', () => {
  const address = { line1: '5 Mill Lane', city: 'Eastport', postcode: 'EP3 9XA', region: 'metro-east', location: { lat: 52.5, lon: 13.4 } };

  it('defaults hazards to an empty list', () => {
    const parsed = parseInput(siteInput, { label: 'Depot', address });
    assert.deepEqual(parsed.hazards, []);
    assert.equal(parsed.address.line1, '5 Mill Lane');
  });

  it('points at nested fields in issues', () => {
    assert.deepEqual(
      issuesOf(siteInput, { label: 'Depot', address: { ...address, city: '', postcode: 'AB', location: { lat: 91, lon: 13 } } }),
      [
        'address.city: is required',
        'address.postcode: must have at least 3 characters',
        'address.location.lat: must be at most 90',
      ],
    );
  });

  it('limits labels and hazard lists', () => {
    assert.deepEqual(issuesOf(siteInput, { label: 'L'.repeat(81), address }), ['label: must have at most 80 characters']);
    const hazards = Array.from({ length: 21 }, (_, index) => `hazard ${index}`);
    assert.deepEqual(issuesOf(siteInput, { label: 'Depot', address, hazards }), ['hazards: must contain at most 20 items']);
  });

  it('flags the offending hazard entry', () => {
    assert.deepEqual(issuesOf(siteInput, { label: 'Depot', address, hazards: ['asbestos', ''] }), ['hazards[1]: is required']);
  });
});
'''

CUSTOMER_SERVICE = r'''
function register(app: App, overrides: Record<string, unknown> = {}) {
  const input = parseInput(customerInput, { name: 'Brightwater Lodge', segment: 'commercial', billingRegion: 'metro-east', ...overrides });
  return createCustomer(app.ctx, 'admin', input);
}

describe('createCustomer', () => {
  let app: App;

  beforeEach(() => {
    app = createApp({ now: '2026-03-02T09:30:00Z' });
  });

  it('creates an active customer with identifiers and a timestamp', () => {
    const customer = register(app);
    assert.equal(customer.id, 'CUS-000001');
    assert.equal(customer.accountNumber, 'C000001H');
    assert.equal(customer.status, 'active');
    assert.equal(customer.createdAt, '2026-03-02T09:30:00Z');
    assert.equal(register(app, { name: 'Second Customer' }).id, 'CUS-000002');
  });

  it('lowercases email addresses', () => {
    assert.equal(register(app, { email: 'Facilities@Brightwater.Example' }).email, 'facilities@brightwater.example');
  });

  it('applies the default credit limit of the segment', () => {
    assert.equal(register(app, { segment: 'residential' }).creditLimitCents, 150_000);
    assert.equal(register(app, { segment: 'commercial' }).creditLimitCents, 2_500_000);
    assert.equal(register(app, { segment: 'public' }).creditLimitCents, 5_000_000);
  });

  it('keeps an explicit credit limit, including zero', () => {
    assert.equal(register(app, { creditLimitCents: 40_000 }).creditLimitCents, 40_000);
    assert.equal(register(app, { creditLimitCents: 0 }).creditLimitCents, 0);
  });

  it('exempts public bodies from tax regardless of the flag', () => {
    assert.equal(register(app, { segment: 'public', taxExempt: false }).taxExempt, true);
    assert.equal(register(app, { segment: 'commercial', taxExempt: false }).taxExempt, false);
    assert.equal(register(app, { segment: 'commercial', taxExempt: true }).taxExempt, true);
  });

  it('assigns account numbers in creation order', () => {
    const numbers = [register(app), register(app, { segment: 'residential' }), register(app, { segment: 'public' })].map(
      (customer) => customer.accountNumber,
    );
    assert.deepEqual(numbers, ['C000001H', 'R000002Q', 'P000003X']);
  });
});

describe('updateCustomer', () => {
  let app: App;

  beforeEach(() => {
    app = createApp();
  });

  it('changes only the supplied fields', () => {
    const customer = register(app, { email: 'ops@brightwater.example', phone: '+49 30 111222' });
    const updated = updateCustomer(app.ctx, 'admin', customer.id, parseInput(customerPatch, { phone: '+49 30 333444', creditLimitCents: 90_000 }));
    assert.equal(updated.phone, '+49 30 333444');
    assert.equal(updated.creditLimitCents, 90_000);
    assert.equal(updated.email, 'ops@brightwater.example');
    assert.equal(updated.name, 'Brightwater Lodge');
  });

  it('lowercases a replaced email and may move the billing region', () => {
    const customer = register(app);
    const updated = updateCustomer(app.ctx, 'admin', customer.id, parseInput(customerPatch, { email: 'NEW@Brightwater.example', billingRegion: 'coastal' }));
    assert.equal(updated.email, 'new@brightwater.example');
    assert.equal(updated.billingRegion, 'coastal');
  });

  it('audits the names of the changed fields', () => {
    const customer = register(app);
    updateCustomer(app.ctx, 'dispatch', customer.id, parseInput(customerPatch, { name: 'Brightwater Lodge Ltd', phone: '+49 30 555666' }));
    const entry = auditTrail(app.ctx, customer.id).find((item) => item.action === 'customer.updated');
    assert.equal(entry?.actorId, 'dispatch');
    assert.deepEqual(entry?.details, { fields: ['name', 'phone'] });
  });

  it('refuses to change closed customers', () => {
    const customer = register(app);
    closeCustomer(app.ctx, 'admin', customer.id, 0);
    assert.throws(
      () => updateCustomer(app.ctx, 'admin', customer.id, parseInput(customerPatch, { name: 'Renamed' })),
      (error: any) => error.status === 409 && error.code === 'customer-closed',
    );
  });

  it('reports unknown customers as not found', () => {
    assert.throws(
      () => updateCustomer(app.ctx, 'admin', 'CUS-000404', parseInput(customerPatch, { name: 'Renamed' })),
      /customer CUS-000404 was not found/,
    );
  });
});
'''

CUSTOMER_SITES = r'''
const address = (overrides: Record<string, unknown> = {}) => ({
  line1: '12 Quay Street',
  city: 'Eastport',
  postcode: 'EP1 4QS',
  region: 'metro-east',
  location: { lat: 52.52, lon: 13.4 },
  ...overrides,
});

const siteData = (label: string, overrides: Record<string, unknown> = {}, extra: Record<string, unknown> = {}) =>
  parseInput(siteInput, { label, address: address(overrides), ...extra });

describe('customer sites', () => {
  let app: App;
  let customer: Customer;

  beforeEach(() => {
    app = createApp();
    customer = createCustomer(app.ctx, 'admin', parseInput(customerInput, {
      name: 'Harbour View Apartments',
      segment: 'commercial',
      billingRegion: 'metro-east',
    }));
  });

  it('adds a site and normalises its postcode', () => {
    const site = addSite(app.ctx, 'admin', customer.id, siteData('Main building', { postcode: 'ep1 4qs' }));
    assert.equal(site.id, 'SITE-000001');
    assert.equal(site.customerId, customer.id);
    assert.equal(site.address.postcode, 'EP14QS');
    assert.deepEqual(site.hazards, []);
  });

  it('lowercases and de-duplicates hazards in insertion order', () => {
    const site = addSite(app.ctx, 'admin', customer.id, siteData('Plant room', {}, { hazards: ['Asbestos', 'asbestos', 'Confined-Space'] }));
    assert.deepEqual(site.hazards, ['asbestos', 'confined-space']);
  });

  it('rejects the same address twice, ignoring case and postcode spacing', () => {
    const first = addSite(app.ctx, 'admin', customer.id, siteData('Main building'));
    assert.throws(
      () => addSite(app.ctx, 'admin', customer.id, siteData('Duplicate', { line1: '12 QUAY STREET', postcode: 'ep14qs' })),
      (error: any) => error.code === 'duplicate-site' && error.details.siteId === first.id,
    );
  });

  it('allows other buildings on the same postcode', () => {
    addSite(app.ctx, 'admin', customer.id, siteData('Main building'));
    const annex = addSite(app.ctx, 'admin', customer.id, siteData('Annex', { line1: '14 Quay Street' }));
    assert.equal(annex.id, 'SITE-000002');
  });

  it('allows the same address for different customers', () => {
    const other = createCustomer(app.ctx, 'admin', parseInput(customerInput, { name: 'Quay Traders', segment: 'commercial', billingRegion: 'metro-east' }));
    addSite(app.ctx, 'admin', customer.id, siteData('Main building'));
    assert.doesNotThrow(() => addSite(app.ctx, 'admin', other.id, siteData('Shop')));
  });

  it('refuses sites for closed customers and unknown customers', () => {
    closeCustomer(app.ctx, 'admin', customer.id, 0);
    assert.throws(() => addSite(app.ctx, 'admin', customer.id, siteData('Late addition')), /closed/);
    assert.throws(() => addSite(app.ctx, 'admin', 'CUS-000099', siteData('Orphan')), /customer CUS-000099 was not found/);
  });

  it('updates access notes and normalises hazards', () => {
    const site = addSite(app.ctx, 'admin', customer.id, siteData('Main building'));
    const updated = updateSiteAccess(app.ctx, 'dispatch', site.id, 'Key safe by the loading bay', ['  Dog ', '', 'DOG', 'Stairs']);
    assert.equal(updated.accessNotes, 'Key safe by the loading bay');
    assert.deepEqual(updated.hazards, ['dog', 'stairs']);
    const entry = auditTrail(app.ctx, site.id).find((item) => item.action === 'site.access-updated');
    assert.deepEqual(entry?.details, { hazards: 2 });
  });

  it('caps the number of listed hazards at twenty', () => {
    const site = addSite(app.ctx, 'admin', customer.id, siteData('Main building'));
    const many = Array.from({ length: 21 }, (_, index) => `hazard-${index}`);
    assert.throws(() => updateSiteAccess(app.ctx, 'admin', site.id, undefined, many), (error: any) => error.code === 'too-many-hazards');
    assert.equal(updateSiteAccess(app.ctx, 'admin', site.id, undefined, many.slice(0, 20)).hazards.length, 20);
  });
});
'''

CUSTOMER_LIFECYCLE = r'''
describe('customer lifecycle', () => {
  let app: App;
  let customer: Customer;

  beforeEach(() => {
    app = createApp();
    customer = createCustomer(app.ctx, 'admin', parseInput(customerInput, { name: 'Delta Works', segment: 'commercial', billingRegion: 'metro-east' }));
  });

  it('suspends an active customer with a reason', () => {
    const suspended = suspendCustomer(app.ctx, 'billing', customer.id, 'Invoices overdue');
    assert.equal(suspended.status, 'suspended');
    assert.equal(suspended.suspendedReason, 'Invoices overdue');
  });

  it('cannot suspend twice', () => {
    suspendCustomer(app.ctx, 'billing', customer.id, 'Invoices overdue');
    assert.throws(
      () => suspendCustomer(app.ctx, 'billing', customer.id, 'Again'),
      (error: any) => error.code === 'customer-not-active' && /status suspended/.test(error.message),
    );
  });

  it('reactivates a suspended customer and clears the reason', () => {
    suspendCustomer(app.ctx, 'billing', customer.id, 'Invoices overdue');
    const active = reactivateCustomer(app.ctx, 'billing', customer.id);
    assert.equal(active.status, 'active');
    assert.equal('suspendedReason' in active, false);
  });

  it('only reactivates suspended customers', () => {
    assert.throws(() => reactivateCustomer(app.ctx, 'billing', customer.id), (error: any) => error.code === 'customer-not-suspended');
  });

  it('closes only when no work is open', () => {
    assert.throws(
      () => closeCustomer(app.ctx, 'admin', customer.id, 2),
      (error: any) => error.code === 'customer-has-open-work' && error.details.openWork === 2,
    );
    assert.equal(closeCustomer(app.ctx, 'admin', customer.id, 0).status, 'closed');
  });

  it('records each lifecycle step in the audit trail', () => {
    suspendCustomer(app.ctx, 'billing', customer.id, 'Invoices overdue');
    reactivateCustomer(app.ctx, 'admin', customer.id);
    closeCustomer(app.ctx, 'admin', customer.id, 0);
    assert.deepEqual(
      auditTrail(app.ctx, customer.id).map((entry) => entry.action),
      ['customer.created', 'customer.suspended', 'customer.reactivated', 'customer.closed'],
    );
  });

  it('decides whether new work is accepted per status', () => {
    const base = { ...customer };
    assert.equal(acceptsNewWork({ ...base, status: 'active' }, false), true);
    assert.equal(acceptsNewWork({ ...base, status: 'suspended' }, false), false);
    assert.equal(acceptsNewWork({ ...base, status: 'suspended' }, true), true);
    assert.equal(acceptsNewWork({ ...base, status: 'closed' }, true), false);
  });

  it('lets suspended customers raise emergencies only', () => {
    const site = addSite(app.ctx, 'admin', customer.id, parseInput(siteInput, {
      label: 'Yard',
      address: { line1: '1 Delta Road', city: 'Eastport', postcode: 'EP5 1DD', region: 'metro-east', location: { lat: 52.5, lon: 13.4 } },
    }));
    suspendCustomer(app.ctx, 'billing', customer.id, 'Invoices overdue');
    const request = { customerId: customer.id, siteId: site.id, workType: 'general', summary: 'Door will not close' };
    assert.throws(() => «openJob»(app.ctx, 'dispatch', request), (error: any) => error.code === 'customer-not-accepting-work');
    const urgent = «openJob»(app.ctx, 'dispatch', { ...request, summary: 'Flooding in the plant room', priority: 'emergency' });
    assert.equal(urgent.priority, 'emergency');
  });
});
'''

CONTRACT_SERVICE = r'''
interface Fixture {
  app: App;
  customer: Customer;
  sites: Site[];
}

function fixture(): Fixture {
  const app = createApp({ now: '2026-03-02T09:00:00Z' });
  const customer = createCustomer(app.ctx, 'admin', parseInput(customerInput, { name: 'Ridge Estates', segment: 'commercial', billingRegion: 'metro-east' }));
  const sites = ['1 Ridge Road', '2 Ridge Road', '3 Ridge Road'].map((line1, index) =>
    addSite(app.ctx, 'admin', customer.id, parseInput(siteInput, {
      label: `Block ${index + 1}`,
      address: { line1, city: 'Eastport', postcode: 'EP7 2RR', region: 'metro-east', location: { lat: 52.5, lon: 13.4 } },
    })),
  );
  return { app, customer, sites };
}

const draft = (f: Fixture, overrides: Record<string, unknown> = {}) =>
  createContract(f.app.ctx, 'admin', {
    customerId: f.customer.id,
    siteIds: [f.sites[0].id],
    plan: 'basic',
    startsOn: '2026-01-01',
    coveredWorkTypes: ['general'],
    ...overrides,
  });

describe('createContract', () => {
  it('creates a draft with the terms of the plan', () => {
    const f = fixture();
    const contract = draft(f, { plan: 'priority', renewal: 'auto' });
    assert.equal(contract.id, 'CON-000001');
    assert.equal(contract.status, 'draft');
    assert.equal(contract.slaPolicyId, 'priority');
    assert.equal(contract.laborDiscountBasisPoints, 500);
    assert.equal(contract.includedVisitsPerYear, 2);
    assert.equal(contract.renewal, 'auto');
  });

  it('defaults to manual renewal', () => {
    assert.equal(draft(fixture()).renewal, 'manual');
  });

  it('removes duplicate sites and work types', () => {
    const f = fixture();
    const contract = draft(f, {
      siteIds: [f.sites[0].id, f.sites[1].id, f.sites[0].id],
      coveredWorkTypes: ['general', 'boiler-service', 'general'],
    });
    assert.deepEqual(contract.siteIds, [f.sites[0].id, f.sites[1].id]);
    assert.deepEqual(contract.coveredWorkTypes, ['general', 'boiler-service']);
  });

  it('validates the payload', () => {
    const f = fixture();
    assert.throws(() => draft(f, { plan: 'gold' }), (error: any) => error.code === 'invalid-input' && /plan: must be one of basic, priority, premium/.test(error.message));
    assert.throws(() => draft(f, { siteIds: [] }), /siteIds: must contain at least 1 item/);
    assert.throws(() => draft(f, { startsOn: '01/01/2026' }), /startsOn: must be a date/);
  });

  it('rejects unknown work types', () => {
    assert.throws(() => draft(fixture(), { coveredWorkTypes: ['general', 'teleportation'] }), (error: any) => {
      return error.code === 'unknown-work-type' && error.status === 422;
    });
  });

  it('needs the end date to follow the start date', () => {
    const f = fixture();
    assert.throws(() => draft(f, { endsOn: '2026-01-01' }), (error: any) => error.code === 'invalid-term');
    assert.throws(() => draft(f, { endsOn: '2025-12-31' }), (error: any) => error.code === 'invalid-term');
    assert.equal(draft(f, { endsOn: '2026-01-02' }).endsOn, '2026-01-02');
  });

  it('requires an active customer who owns the sites', () => {
    const f = fixture();
    const other = createCustomer(f.app.ctx, 'admin', parseInput(customerInput, { name: 'Other Estates', segment: 'commercial', billingRegion: 'metro-east' }));
    assert.throws(() => draft(f, { customerId: other.id }), /site SITE-000001 was not found/);
    suspendCustomer(f.app.ctx, 'admin', f.customer.id, 'Invoices overdue');
    assert.throws(() => draft(f), (error: any) => error.code === 'customer-not-active');
  });
});

describe('activateContract', () => {
  it('activates a draft', () => {
    const f = fixture();
    const active = activateContract(f.app.ctx, 'admin', draft(f).id);
    assert.equal(active.status, 'active');
  });

  it('cannot activate twice', () => {
    const f = fixture();
    const contract = draft(f);
    activateContract(f.app.ctx, 'admin', contract.id);
    assert.throws(() => activateContract(f.app.ctx, 'admin', contract.id), (error: any) => error.code === 'contract-not-draft');
  });

  it('refuses a second active contract for the same site', () => {
    const f = fixture();
    const first = activateContract(f.app.ctx, 'admin', draft(f).id);
    const second = draft(f, { plan: 'premium', siteIds: [f.sites[0].id, f.sites[1].id] });
    assert.throws(
      () => activateContract(f.app.ctx, 'admin', second.id),
      (error: any) => error.code === 'overlapping-contract' && error.details.contractId === first.id,
    );
  });

  it('allows contracts for different sites side by side', () => {
    const f = fixture();
    activateContract(f.app.ctx, 'admin', draft(f).id);
    const other = draft(f, { siteIds: [f.sites[2].id] });
    assert.equal(activateContract(f.app.ctx, 'admin', other.id).status, 'active');
  });

  it('allows a replacement once the earlier contract has ended', () => {
    const f = fixture();
    activateContract(f.app.ctx, 'admin', draft(f, { endsOn: '2026-02-28' }).id);
    const replacement = draft(f, { startsOn: '2026-03-01', plan: 'priority' });
    assert.equal(activateContract(f.app.ctx, 'admin', replacement.id).status, 'active');
  });
});

describe('terminateContract', () => {
  it('ends an open-ended contract today', () => {
    const f = fixture();
    const active = activateContract(f.app.ctx, 'admin', draft(f).id);
    const ended = terminateContract(f.app.ctx, 'admin', active.id, 'Customer relocated');
    assert.equal(ended.status, 'terminated');
    assert.equal(ended.endsOn, '2026-03-02');
    assert.equal(ended.terminatedReason, 'Customer relocated');
  });

  it('brings a later end date forward but keeps an earlier one', () => {
    const f = fixture();
    const later = activateContract(f.app.ctx, 'admin', draft(f, { endsOn: '2026-12-31' }).id);
    assert.equal(terminateContract(f.app.ctx, 'admin', later.id, 'Moved out').endsOn, '2026-03-02');
    const earlier = activateContract(f.app.ctx, 'admin', draft(f, { siteIds: [f.sites[1].id], endsOn: '2026-02-20' }).id);
    assert.equal(terminateContract(f.app.ctx, 'admin', earlier.id, 'Paperwork cleanup').endsOn, '2026-02-20');
  });

  it('cannot terminate an ended contract', () => {
    const f = fixture();
    const active = activateContract(f.app.ctx, 'admin', draft(f).id);
    terminateContract(f.app.ctx, 'admin', active.id, 'Moved out');
    assert.throws(() => terminateContract(f.app.ctx, 'admin', active.id, 'Again'), (error: any) => error.code === 'contract-ended');
  });
});

describe('rollContracts', () => {
  it('expires lapsed manual contracts and renews automatic ones', () => {
    const f = fixture();
    const manual = activateContract(f.app.ctx, 'admin', draft(f, { startsOn: '2025-03-01', endsOn: '2026-02-28' }).id);
    const auto = activateContract(f.app.ctx, 'admin', draft(f, { siteIds: [f.sites[1].id], startsOn: '2025-03-01', endsOn: '2026-02-28', renewal: 'auto' }).id);
    const current = activateContract(f.app.ctx, 'admin', draft(f, { siteIds: [f.sites[2].id], startsOn: '2026-01-01', endsOn: '2026-12-31' }).id);
    const result = rollContracts(f.app.ctx, '2026-03-01');
    assert.deepEqual(result, { expired: [manual.id], renewed: [auto.id] });
    const renewed = contracts(f.app.ctx).require(auto.id);
    assert.equal(renewed.startsOn, '2026-03-01');
    assert.equal(renewed.endsOn, '2027-02-28');
    assert.equal(contracts(f.app.ctx).require(manual.id).status, 'expired');
    assert.equal(contracts(f.app.ctx).require(current.id).status, 'active');
  });

  it('does nothing before any end date has passed', () => {
    const f = fixture();
    activateContract(f.app.ctx, 'admin', draft(f, { endsOn: '2026-06-30' }).id);
    assert.deepEqual(rollContracts(f.app.ctx, '2026-06-30'), { expired: [], renewed: [] });
  });
});
'''

CONTRACT_PLANS = r'''
describe('plan terms', () => {
  const plans: Array<[ContractPlan, string, number, number, boolean]> = [
    ['basic', 'standard', 0, 1, false],
    ['priority', 'priority', 500, 2, false],
    ['premium', 'premium', 1000, 4, true],
  ];

  for (const [plan, policy, discount, visits, waived] of plans) {
    it(`defines the ${plan} plan`, () => {
      const terms = PLAN_TERMS[plan];
      assert.equal(terms.slaPolicyId, policy);
      assert.equal(terms.laborDiscountBasisPoints, discount);
      assert.equal(terms.includedVisitsPerYear, visits);
      assert.equal(terms.waiveCalloutFee, waived);
    });
  }

  it('improves every benefit from basic to premium', () => {
    const order: ContractPlan[] = ['basic', 'priority', 'premium'];
    for (let index = 1; index < order.length; index += 1) {
      const lower = PLAN_TERMS[order[index - 1]];
      const higher = PLAN_TERMS[order[index]];
      assert.ok(higher.laborDiscountBasisPoints > lower.laborDiscountBasisPoints);
      assert.ok(higher.includedVisitsPerYear > lower.includedVisitsPerYear);
    }
  });

  it('only waives the callout fee on the top plan', () => {
    const waiving = (Object.keys(PLAN_TERMS) as ContractPlan[]).filter((plan) => PLAN_TERMS[plan].waiveCalloutFee);
    assert.deepEqual(waiving, ['premium']);
  });
});

describe('contract years', () => {
  const contract = (startsOn: string): Contract => ({
    id: 'CON-000001',
    customerId: 'CUS-000001',
    siteIds: ['SITE-000001'],
    plan: 'basic',
    status: 'active',
    startsOn,
    slaPolicyId: 'standard',
    coveredWorkTypes: ['general'],
    laborDiscountBasisPoints: 0,
    includedVisitsPerYear: 1,
    renewal: 'manual',
  });

  const cases: Array<[string, string, string]> = [
    ['2025-06-15', '2026-03-02', '2025-06-15'],
    ['2025-06-15', '2026-06-15', '2026-06-15'],
    ['2025-06-15', '2026-12-31', '2026-06-15'],
    ['2024-01-01', '2026-03-02', '2026-01-01'],
    ['2026-06-15', '2026-03-02', '2026-06-15'],
    ['2026-03-02', '2026-03-02', '2026-03-02'],
  ];
  for (const [startsOn, on, expected] of cases) {
    it(`starts the year on ${expected} for a ${startsOn} contract on ${on}`, () => {
      assert.equal(contractYearStart(contract(startsOn), on), expected);
    });
  }
});
'''

ENTITLEMENTS = r'''
interface Fixture {
  app: App;
  customerId: string;
  siteId: string;
  contract: Contract;
}

function fixture(plan: string, coveredWorkTypes: string[], startsOn = '2026-01-01'): Fixture {
  const app = createApp({ now: '2026-03-02T09:00:00Z' });
  const customer = createCustomer(app.ctx, 'admin', parseInput(customerInput, { name: 'Calder Foods', segment: 'commercial', billingRegion: 'metro-east' }));
  const site = addSite(app.ctx, 'admin', customer.id, parseInput(siteInput, {
    label: 'Packing hall',
    address: { line1: '8 Calder Way', city: 'Eastport', postcode: 'EP8 3CW', region: 'metro-east', location: { lat: 52.5, lon: 13.4 } },
  }));
  const created = createContract(app.ctx, 'admin', { customerId: customer.id, siteIds: [site.id], plan, startsOn, coveredWorkTypes });
  const contract = activateContract(app.ctx, 'admin', created.id);
  return { app, customerId: customer.id, siteId: site.id, contract };
}

const open = (f: Fixture, workType = 'general') =>
  «openJob»(f.app.ctx, 'dispatch', { customerId: f.customerId, siteId: f.siteId, workType, summary: 'Scheduled attendance' });

describe('coverageFor', () => {
  it('describes the contract that covers a site and work type', () => {
    const f = fixture('priority', ['general', 'thermostat-install']);
    const coverage = coverageFor(f.app.ctx, f.siteId, 'thermostat-install', '2026-03-02');
    assert.equal(coverage?.contract.id, f.contract.id);
    assert.equal(coverage?.terms.slaPolicyId, 'priority');
    assert.equal(coverage?.includedVisitAvailable, true);
  });

  it('bills work types outside the contract as ordinary work', () => {
    const f = fixture('premium', ['general']);
    assert.equal(coverageFor(f.app.ctx, f.siteId, 'leak-investigation', '2026-03-02'), undefined);
  });

  it('does not cover dates before the start or after the end', () => {
    const f = fixture('basic', ['general'], '2026-04-01');
    assert.equal(coverageFor(f.app.ctx, f.siteId, 'general', '2026-03-02'), undefined);
    assert.ok(coverageFor(f.app.ctx, f.siteId, 'general', '2026-04-01'));
  });

  it('does not cover sites without a contract', () => {
    const f = fixture('basic', ['general']);
    assert.equal(coverageFor(f.app.ctx, 'SITE-000099', 'general', '2026-03-02'), undefined);
  });
});

describe('included visits', () => {
  it('tags covered work as an included visit until the allowance is used', () => {
    const f = fixture('basic', ['general']);
    const first = open(f);
    const second = open(f);
    assert.ok(first.tags.includes('included-visit'));
    assert.equal(second.tags.includes('included-visit'), false);
    assert.equal(includedVisitsUsed(f.app.ctx, f.contract, f.siteId, '2026-03-02'), 1);
    assert.equal(coverageFor(f.app.ctx, f.siteId, 'general', '2026-03-02')?.includedVisitAvailable, false);
  });

  it('allows more included visits on richer plans', () => {
    const f = fixture('priority', ['general']);
    const jobs = [open(f), open(f), open(f)];
    assert.deepEqual(jobs.map((job) => job.tags.includes('included-visit')), [true, true, false]);
    assert.equal(includedVisitsUsed(f.app.ctx, f.contract, f.siteId, '2026-03-02'), 2);
  });

  it('gives the allowance back when an included visit is cancelled', () => {
    const f = fixture('basic', ['general']);
    const first = open(f);
    «cancelJob»(f.app.ctx, 'dispatch', first.id, 'Customer cancelled the visit');
    assert.equal(includedVisitsUsed(f.app.ctx, f.contract, f.siteId, '2026-03-02'), 0);
    assert.ok(open(f).tags.includes('included-visit'));
  });

  it('links work orders to the covering contract', () => {
    const f = fixture('basic', ['general']);
    assert.equal(open(f).contractId, f.contract.id);
  });

  it('does not tag work types that are never part of a plan', () => {
    const f = fixture('premium', ['leak-investigation']);
    const job = open(f, 'leak-investigation');
    assert.equal(job.contractId, f.contract.id);
    assert.equal(job.tags.includes('included-visit'), false);
  });
});
'''

ASSET_SERVICE = r'''
describe('asset registration', () => {
  let app: App;
  let siteId: string;

  beforeEach(() => {
    app = createApp({ now: '2026-03-02T09:00:00Z' });
    const customer = createCustomer(app.ctx, 'admin', parseInput(customerInput, { name: 'Wren Hotels', segment: 'commercial', billingRegion: 'metro-east' }));
    siteId = addSite(app.ctx, 'admin', customer.id, parseInput(siteInput, {
      label: 'Plant room',
      address: { line1: '3 Wren Court', city: 'Eastport', postcode: 'EP4 1WR', region: 'metro-east', location: { lat: 52.5, lon: 13.4 } },
    })).id;
  });

  const input = (overrides: Record<string, unknown> = {}) => ({
    siteId,
    kind: 'boiler',
    manufacturer: 'Vaillant',
    model: 'ecoTEC plus 832',
    serial: 'vw-2291-ab',
    installedOn: '2023-10-12',
    ...overrides,
  });

  it('registers an asset with an upper-cased serial and the standard interval', () => {
    const asset = registerAsset(app.ctx, 'admin', input());
    assert.equal(asset.id, 'AST-000001');
    assert.equal(asset.serial, 'VW-2291-AB');
    assert.equal(asset.serviceIntervalDays, 365);
    assert.equal(asset.lastServicedOn, undefined);
  });

  it('takes the default interval from the asset kind', () => {
    assert.equal(registerAsset(app.ctx, 'admin', input({ kind: 'chiller', serial: 'CH-1001' })).serviceIntervalDays, 90);
    assert.equal(registerAsset(app.ctx, 'admin', input({ kind: 'elevator', serial: 'EL-1001' })).serviceIntervalDays, 30);
  });

  it('prefers an explicit service interval', () => {
    assert.equal(registerAsset(app.ctx, 'admin', input({ serviceIntervalDays: 200 })).serviceIntervalDays, 200);
  });

  it('refuses duplicate serials whatever their case', () => {
    const first = registerAsset(app.ctx, 'admin', input());
    assert.throws(
      () => registerAsset(app.ctx, 'admin', input({ serial: 'VW-2291-AB', kind: 'heat-pump' })),
      (error: any) => error.code === 'duplicate-serial' && error.details.assetId === first.id,
    );
  });

  it('checks the payload', () => {
    assert.throws(() => registerAsset(app.ctx, 'admin', input({ kind: 'toaster' })), /kind: must be one of boiler/);
    assert.throws(() => registerAsset(app.ctx, 'admin', input({ serial: 'AB' })), /serial: must have at least 3 characters/);
    assert.throws(() => registerAsset(app.ctx, 'admin', input({ serviceIntervalDays: 3 })), /serviceIntervalDays: must be at least 7/);
    assert.throws(() => registerAsset(app.ctx, 'admin', input({ installedOn: '12-10-2023' })), /installedOn: must be a date/);
  });

  it('requires an existing site', () => {
    assert.throws(() => registerAsset(app.ctx, 'admin', input({ siteId: 'SITE-000404' })), /site SITE-000404 was not found/);
  });

  it('rejects a warranty that ends before installation', () => {
    assert.throws(() => registerAsset(app.ctx, 'admin', input({ warrantyUntil: '2023-10-11' })), (error: any) => error.code === 'invalid-warranty');
    assert.equal(registerAsset(app.ctx, 'admin', input({ warrantyUntil: '2023-10-12' })).warrantyUntil, '2023-10-12');
  });

  it('retires an asset once', () => {
    const asset = registerAsset(app.ctx, 'admin', input());
    assert.equal(retireAsset(app.ctx, 'admin', asset.id, '2026-02-01').retiredOn, '2026-02-01');
    assert.throws(() => retireAsset(app.ctx, 'admin', asset.id, '2026-02-02'), (error: any) => error.code === 'asset-retired');
  });

  it('cannot retire an asset before it was installed', () => {
    const asset = registerAsset(app.ctx, 'admin', input());
    assert.throws(() => retireAsset(app.ctx, 'admin', asset.id, '2023-10-11'), (error: any) => error.code === 'invalid-date');
  });

  it('records services and keeps the latest service date', () => {
    const asset = registerAsset(app.ctx, 'admin', input());
    assert.equal(recordService(app.ctx, asset.id, '2025-10-10').lastServicedOn, '2025-10-10');
    assert.equal(recordService(app.ctx, asset.id, '2025-09-01').lastServicedOn, '2025-10-10');
    assert.equal(recordService(app.ctx, asset.id, '2026-02-20').lastServicedOn, '2026-02-20');
  });

  it('never lets the meter reading go backwards', () => {
    const asset = registerAsset(app.ctx, 'admin', input({ kind: 'generator', serial: 'GEN-0001' }));
    assert.equal(recordService(app.ctx, asset.id, '2026-01-10', 410).meterReading, 410);
    assert.equal(recordService(app.ctx, asset.id, '2026-02-10').meterReading, 410);
    assert.throws(() => recordService(app.ctx, asset.id, '2026-02-11', 400), (error: any) => error.code === 'meter-decreased');
    assert.equal(recordService(app.ctx, asset.id, '2026-02-12', 410).meterReading, 410);
  });
});
'''

ASSET_MAINTENANCE = r'''
function asset(overrides: Partial<Asset>): Asset {
  return {
    id: 'AST-000001',
    siteId: 'SITE-000001',
    kind: 'boiler',
    manufacturer: 'Worcester',
    model: 'Greenstar 4000',
    serial: 'WG-0001',
    installedOn: '2025-03-01',
    serviceIntervalDays: 365,
    ...overrides,
  };
}

describe('warrantyStatus', () => {
  const on = '2026-03-02';
  const cases: Array<[string | undefined, string]> = [
    [undefined, 'none'],
    ['2026-03-01', 'expired'],
    ['2026-03-02', 'expiring'],
    ['2026-04-01', 'expiring'],
    ['2026-04-02', 'in-warranty'],
    ['2028-01-01', 'in-warranty'],
  ];
  for (const [warrantyUntil, expected] of cases) {
    it(`reports ${expected} for a warranty ending ${warrantyUntil ?? 'never set'}`, () => {
      assert.equal(warrantyStatus(asset({ warrantyUntil }), on), expected);
    });
  }
});

describe('nextServiceDate', () => {
  it('counts from installation until a service was recorded', () => {
    assert.equal(nextServiceDate(asset({})), '2026-03-01');
  });

  it('counts from the last service afterwards', () => {
    assert.equal(nextServiceDate(asset({ lastServicedOn: '2025-11-20' })), '2026-11-20');
    assert.equal(nextServiceDate(asset({ kind: 'chiller', serviceIntervalDays: 90, lastServicedOn: '2026-01-15' })), '2026-04-15');
  });
});

describe('assetsDueForService', () => {
  let app: App;

  beforeEach(() => {
    app = createApp();
    const customer = createCustomer(app.ctx, 'admin', parseInput(customerInput, { name: 'Elm Park Leisure', segment: 'public', billingRegion: 'valley' }));
    const site = addSite(app.ctx, 'admin', customer.id, parseInput(siteInput, {
      label: 'Leisure centre',
      address: { line1: '1 Elm Park', city: 'Eastport', postcode: 'EP6 7EP', region: 'valley', location: { lat: 52.5, lon: 13.4 } },
    }));
    const base = { siteId: site.id, manufacturer: 'Acme', model: 'X1' };
    registerAsset(app.ctx, 'admin', { ...base, kind: 'boiler', serial: 'B-0001', installedOn: '2025-03-01' });
    registerAsset(app.ctx, 'admin', { ...base, kind: 'chiller', serial: 'C-0001', installedOn: '2025-12-15' });
    registerAsset(app.ctx, 'admin', { ...base, kind: 'elevator', serial: 'E-0001', installedOn: '2026-02-20' });
    registerAsset(app.ctx, 'admin', { ...base, kind: 'solar-inverter', serial: 'S-0001', installedOn: '2025-01-01' });
  });

  const serials = (assets: Asset[]) => assets.map((entry) => entry.serial);

  it('lists only assets whose interval has elapsed when no window is given', () => {
    assert.deepEqual(serials(assetsDueForService(app.ctx, '2026-03-02')), ['B-0001']);
  });

  it('widens the list with a look-ahead window, most overdue first', () => {
    assert.deepEqual(serials(assetsDueForService(app.ctx, '2026-03-02', 13)), ['B-0001', 'C-0001']);
    assert.deepEqual(serials(assetsDueForService(app.ctx, '2026-03-02', 20)), ['B-0001', 'C-0001', 'E-0001']);
  });

  it('drops assets once they are serviced', () => {
    const boiler = findAssetBySerial(app.ctx, 'B-0001')!;
    recordService(app.ctx, boiler.id, '2026-03-01');
    assert.deepEqual(serials(assetsDueForService(app.ctx, '2026-03-02')), []);
  });

  it('ignores retired assets', () => {
    const boiler = findAssetBySerial(app.ctx, 'B-0001')!;
    retireAsset(app.ctx, 'admin', boiler.id, '2026-03-01');
    assert.deepEqual(serials(assetsDueForService(app.ctx, '2026-03-02', 20)), ['C-0001', 'E-0001']);
  });

  it('knows the interval of every asset kind', () => {
    assert.equal(ASSET_KINDS.length, 10);
    for (const kind of ASSET_KINDS) assert.ok(SERVICE_INTERVALS[kind] >= 30, kind);
    assert.equal(SERVICE_INTERVALS['solar-inverter'], 730);
    assert.equal(SERVICE_INTERVALS.generator, 182);
  });
});
'''

API_CUSTOMERS = r'''
describe('customer routes', () => {
  let world: World;

  beforeEach(() => {
    world = createWorld();
  });

  const payload = (overrides: Record<string, unknown> = {}) => ({
    name: 'Aldgate Dental',
    segment: 'commercial',
    billingRegion: 'metro-east',
    ...overrides,
  });

  it('creates customers for administrators only', () => {
    const body = expectStatus(world.app, 201, 'POST', '/customers', 'admin', payload({ email: 'Hello@Aldgate.example' }));
    assert.equal(body.name, 'Aldgate Dental');
    assert.equal(body.email, 'hello@aldgate.example');
    assert.match(body.creditLimit, /^[A-Z]{3} 25,000\.00$/);
    for (const actor of ['dispatch', 'billing', 'warehouse', world.technician.id]) {
      assert.equal(expectStatus(world.app, 403, 'POST', '/customers', actor, payload()).error.code, 'forbidden', actor);
    }
  });

  it('reports every validation problem at once', () => {
    const body = expectStatus(world.app, 422, 'POST', '/customers', 'admin', { name: 'X', segment: 'solo' });
    assert.equal(body.error.code, 'invalid-input');
    const paths = body.error.details.issues.map((issue: any) => issue.path);
    assert.deepEqual(paths, ['name', 'segment', 'billingRegion']);
  });

  it('lists customers alphabetically with paging', () => {
    for (const name of ['Zenith Tools', 'Aldgate Dental', 'Moor Street Cafe']) call(world.app, 'POST', '/customers', 'admin', payload({ name }));
    const page = call(world.app, 'GET', '/customers?pageSize=2&page=1', 'dispatch');
    assert.equal(page.total, 4);
    assert.equal(page.pages, 2);
    assert.deepEqual(page.items.map((item: any) => item.name), ['Aldgate Dental', 'Harbour View Apartments']);
    const rest = call(world.app, 'GET', '/customers?pageSize=2&page=2', 'dispatch');
    assert.deepEqual(rest.items.map((item: any) => item.name), ['Moor Street Cafe', 'Zenith Tools']);
  });

  it('filters by region and name fragment', () => {
    call(world.app, 'POST', '/customers', 'admin', payload({ name: 'Coastal Chandlers', billingRegion: 'coastal' }));
    call(world.app, 'POST', '/customers', 'admin', payload({ name: 'Coastline Garage', billingRegion: 'metro-east' }));
    const coastal = call(world.app, 'GET', '/customers?region=coastal', 'billing');
    assert.deepEqual(coastal.items.map((item: any) => item.name), ['Coastal Chandlers']);
    const matches = call(world.app, 'GET', '/customers?q=COAST', 'billing');
    assert.deepEqual(matches.items.map((item: any) => item.name), ['Coastal Chandlers', 'Coastline Garage']);
  });

  it('returns a customer with its sites', () => {
    const body = call(world.app, 'GET', `/customers/${world.customer.id}`, 'dispatch');
    assert.equal(body.id, world.customer.id);
    assert.deepEqual(body.sites.map((site: any) => site.label), ['Main building']);
  });

  it('lets portal users see only their own account', () => {
    const other = call(world.app, 'POST', '/customers', 'admin', payload());
    const portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' });
    assert.equal(portal.role, 'customer');
    assert.equal(portal.subjectId, world.customer.id);
    assert.equal(call(world.app, 'GET', `/customers/${world.customer.id}`, portal.id).id, world.customer.id);
    assert.equal(expectStatus(world.app, 403, 'GET', `/customers/${other.id}`, portal.id).error.code, 'forbidden');
    assert.equal(expectStatus(world.app, 403, 'GET', '/customers', portal.id).error.code, 'forbidden');
    assert.equal(call(world.app, 'GET', `/customers/${world.customer.id}/sites`, portal.id).items.length, 1);
  });

  it('keeps technicians away from customer records', () => {
    expectStatus(world.app, 403, 'GET', `/customers/${world.customer.id}`, world.technician.id);
    expectStatus(world.app, 403, 'GET', `/customers/${world.customer.id}/sites`, world.technician.id);
  });

  it('answers 404 for unknown customers', () => {
    const body = expectStatus(world.app, 404, 'GET', '/customers/CUS-000404', 'admin');
    assert.equal(body.error.code, 'not-found');
    assert.deepEqual(body.error.details, { entity: 'customer', id: 'CUS-000404' });
  });

  it('patches contact details', () => {
    const body = call(world.app, 'PATCH', `/customers/${world.customer.id}`, 'admin', { phone: '+49 30 998877' });
    assert.equal(body.phone, '+49 30 998877');
    expectStatus(world.app, 422, 'PATCH', `/customers/${world.customer.id}`, 'admin', { segment: 'public' });
  });

  it('suspends, reactivates and closes accounts', () => {
    const { app } = world;
    const url = `/customers/${world.customer.id}`;
    expectStatus(app, 422, 'POST', `${url}/suspend`, 'admin', { reason: 'x' });
    assert.equal(call(app, 'POST', `${url}/suspend`, 'admin', { reason: 'Unpaid invoices' }).status, 'suspended');
    assert.equal(expectStatus(app, 409, 'POST', `${url}/suspend`, 'admin', { reason: 'Unpaid invoices' }).error.code, 'customer-not-active');
    assert.equal(call(app, 'POST', `${url}/reactivate`, 'admin').status, 'active');
    assert.equal(call(app, 'POST', `${url}/close`, 'admin').status, 'closed');
    expectStatus(app, 409, 'PATCH', url, 'admin', { name: 'Too Late Ltd' });
  });

  it('refuses to close a customer with open work', () => {
    const { app } = world;
    openWorkOrder(app, world.customer.id, world.site.id);
    openWorkOrder(app, world.customer.id, world.site.id);
    const body = expectStatus(app, 409, 'POST', `/customers/${world.customer.id}/close`, 'admin');
    assert.equal(body.error.code, 'customer-has-open-work');
    assert.equal(body.error.details.openWork, 2);
  });

  it('adds sites, rejects duplicates and updates access details', () => {
    const { app } = world;
    const site = {
      label: 'Warehouse',
      address: { line1: '90 Dock Road', city: 'Eastport', postcode: 'EP9 1DR', region: 'metro-east', location: { lat: 52.51, lon: 13.41 } },
    };
    const added = expectStatus(app, 201, 'POST', `/customers/${world.customer.id}/sites`, 'admin', site);
    assert.equal(added.address.postcode, 'EP91DR');
    assert.equal(expectStatus(app, 409, 'POST', `/customers/${world.customer.id}/sites`, 'admin', site).error.code, 'duplicate-site');
    const access = call(app, 'PUT', `/sites/${added.id}/access`, 'admin', { accessNotes: 'Ring bell twice', hazards: ['Forklifts', 'forklifts'] });
    assert.equal(access.accessNotes, 'Ring bell twice');
    assert.deepEqual(access.hazards, ['forklifts']);
    const listed = call(app, 'GET', `/customers/${world.customer.id}/sites`, 'dispatch');
    assert.deepEqual(listed.items.map((entry: any) => entry.label), ['Main building', 'Warehouse']);
  });
});
'''

API_CONTRACTS = r'''
describe('contract routes', () => {
  let world: World;

  beforeEach(() => {
    world = createWorld();
  });

  const draft = (overrides: Record<string, unknown> = {}) => ({
    customerId: world.customer.id,
    siteIds: [world.site.id],
    plan: 'priority',
    startsOn: '2026-01-01',
    coveredWorkTypes: ['general', 'thermostat-install'],
    ...overrides,
  });

  it('creates drafts for administrators only', () => {
    const body = expectStatus(world.app, 201, 'POST', '/contracts', 'admin', draft());
    assert.equal(body.status, 'draft');
    assert.equal(body.laborDiscountBasisPoints, 500);
    for (const actor of ['dispatch', 'billing', world.technician.id]) {
      assert.equal(expectStatus(world.app, 403, 'POST', '/contracts', actor, draft()).error.code, 'forbidden', actor);
    }
  });

  it('rejects unknown plans and unknown fields', () => {
    expectStatus(world.app, 422, 'POST', '/contracts', 'admin', draft({ plan: 'platinum' }));
    expectStatus(world.app, 422, 'POST', '/contracts', 'admin', draft({ discount: 25 }));
  });

  it('activates a contract and exposes included visit usage', () => {
    const { app } = world;
    const created = call(app, 'POST', '/contracts', 'admin', draft());
    assert.equal(call(app, 'POST', `/contracts/${created.id}/activate`, 'admin').status, 'active');
    openWorkOrder(app, world.customer.id, world.site.id, 'general');
    const view = call(app, 'GET', `/contracts/${created.id}`, 'dispatch');
    assert.deepEqual(view.usage, [{ siteId: world.site.id, includedVisitsUsed: 1 }]);
  });

  it('answers 409 for a second activation and for overlapping contracts', () => {
    const { app } = world;
    const first = call(app, 'POST', '/contracts', 'admin', draft());
    call(app, 'POST', `/contracts/${first.id}/activate`, 'admin');
    assert.equal(expectStatus(app, 409, 'POST', `/contracts/${first.id}/activate`, 'admin').error.code, 'contract-not-draft');
    const second = call(app, 'POST', '/contracts', 'admin', draft({ plan: 'premium' }));
    const body = expectStatus(app, 409, 'POST', `/contracts/${second.id}/activate`, 'admin');
    assert.equal(body.error.code, 'overlapping-contract');
    assert.equal(body.error.details.contractId, first.id);
  });

  it('terminates with a reason', () => {
    const { app } = world;
    const contract = seedContract(app, world.customer.id, [world.site.id], 'basic', ['general']);
    expectStatus(app, 422, 'POST', `/contracts/${contract.id}/terminate`, 'admin', { reason: 'no' });
    const ended = call(app, 'POST', `/contracts/${contract.id}/terminate`, 'admin', { reason: 'Customer relocated' });
    assert.equal(ended.status, 'terminated');
    assert.equal(ended.endsOn, '2026-03-02');
  });

  it('lists the contracts of a customer', () => {
    const { app } = world;
    seedContract(app, world.customer.id, [world.site.id], 'basic', ['general']);
    const list = call(app, 'GET', `/customers/${world.customer.id}/contracts`, 'dispatch');
    assert.equal(list.items.length, 1);
    assert.equal(list.items[0].plan, 'basic');
    expectStatus(app, 403, 'GET', `/customers/${world.customer.id}/contracts`, world.technician.id);
  });

  it('reports coverage for a site and work type', () => {
    const { app } = world;
    seedContract(app, world.customer.id, [world.site.id], 'premium', ['general']);
    const covered = call(app, 'GET', `/sites/${world.site.id}/coverage?workType=general`, 'dispatch');
    assert.equal(covered.covered, true);
    assert.equal(covered.coverage.terms.waiveCalloutFee, true);
    const other = call(app, 'GET', `/sites/${world.site.id}/coverage?workType=leak-investigation`, 'dispatch');
    assert.deepEqual(other, { covered: false });
    const early = call(app, 'GET', `/sites/${world.site.id}/coverage?workType=general&on=2025-06-01`, 'dispatch');
    assert.equal(early.covered, false);
  });

  it('requires a work type and a valid date for coverage lookups', () => {
    const { app } = world;
    const missing = expectStatus(app, 422, 'GET', `/sites/${world.site.id}/coverage`, 'dispatch');
    assert.deepEqual(missing.error.details.issues, [{ path: 'workType', message: 'is required' }]);
    const bad = expectStatus(app, 422, 'GET', `/sites/${world.site.id}/coverage?workType=general&on=tomorrow`, 'dispatch');
    assert.equal(bad.error.details.issues[0].path, 'on');
  });

  it('rolls over lapsed contracts for system administrators', () => {
    const { app } = world;
    const created = call(app, 'POST', '/contracts', 'admin', draft({ startsOn: '2025-03-01', endsOn: '2026-02-28' }));
    call(app, 'POST', `/contracts/${created.id}/activate`, 'admin');
    expectStatus(app, 403, 'POST', '/contracts/roll', 'dispatch');
    const result = call(app, 'POST', '/contracts/roll?on=2026-03-01', 'admin');
    assert.deepEqual(result, { expired: [created.id], renewed: [] });
    assert.equal(call(app, 'GET', `/contracts/${created.id}`, 'admin').status, 'expired');
  });

  it('answers 404 for unknown contracts', () => {
    expectStatus(world.app, 404, 'GET', '/contracts/CON-000404', 'admin');
    expectStatus(world.app, 404, 'POST', '/contracts/CON-000404/activate', 'admin');
  });
});
'''

API_ASSETS = r'''
describe('asset routes', () => {
  let world: World;

  beforeEach(() => {
    world = createWorld();
  });

  const payload = (overrides: Record<string, unknown> = {}) => ({
    siteId: world.site.id,
    kind: 'boiler',
    manufacturer: 'Viessmann',
    model: 'Vitodens 100',
    serial: 'vd-100-5521',
    installedOn: '2025-03-01',
    ...overrides,
  });

  it('registers assets for administrators', () => {
    const body = expectStatus(world.app, 201, 'POST', '/assets', 'admin', payload({ warrantyUntil: '2027-03-01' }));
    assert.equal(body.serial, 'VD-100-5521');
    assert.equal(body.serviceIntervalDays, 365);
    expectStatus(world.app, 403, 'POST', '/assets', 'dispatch', payload({ serial: 'OTHER-1' }));
  });

  it('refuses duplicate serials with 409', () => {
    call(world.app, 'POST', '/assets', 'admin', payload());
    const body = expectStatus(world.app, 409, 'POST', '/assets', 'admin', payload({ serial: 'VD-100-5521' }));
    assert.equal(body.error.code, 'duplicate-serial');
  });

  it('lists the assets at a site with warranty state and next service', () => {
    const { app } = world;
    call(app, 'POST', '/assets', 'admin', payload({ warrantyUntil: '2026-03-20' }));
    call(app, 'POST', '/assets', 'admin', payload({ kind: 'chiller', serial: 'ch-77-01', installedOn: '2026-01-10' }));
    const body = call(app, 'GET', `/sites/${world.site.id}/assets`, world.technician.id);
    assert.deepEqual(body.items.map((item: any) => [item.kind, item.warranty, item.nextServiceOn]), [
      ['boiler', 'expiring', '2026-03-01'],
      ['chiller', 'none', '2026-04-10'],
    ]);
  });

  it('retires an asset on a given date and hides it from the site list', () => {
    const { app } = world;
    const asset = call(app, 'POST', '/assets', 'admin', payload());
    const retired = call(app, 'POST', `/assets/${asset.id}/retire?on=2026-02-15`, 'admin');
    assert.equal(retired.retiredOn, '2026-02-15');
    assert.deepEqual(call(app, 'GET', `/sites/${world.site.id}/assets`, 'dispatch').items, []);
    assert.equal(expectStatus(app, 409, 'POST', `/assets/${asset.id}/retire`, 'admin').error.code, 'asset-retired');
  });

  it('finds assets that are due for service', () => {
    const { app } = world;
    call(app, 'POST', '/assets', 'admin', payload());
    call(app, 'POST', '/assets', 'admin', payload({ kind: 'elevator', serial: 'el-12-01', installedOn: '2026-02-20' }));
    assert.deepEqual(call(app, 'GET', '/assets/due', 'dispatch').items.map((item: any) => item.serial), ['VD-100-5521', 'EL-12-01']);
    assert.deepEqual(call(app, 'GET', '/assets/due?within=0', 'dispatch').items.map((item: any) => item.serial), ['VD-100-5521']);
    assert.deepEqual(call(app, 'GET', '/assets/due?on=2025-01-01&within=0', 'dispatch').items, []);
  });

  it('validates the look-ahead window', () => {
    const { app } = world;
    expectStatus(app, 422, 'GET', '/assets/due?within=abc', 'dispatch');
    expectStatus(app, 422, 'GET', '/assets/due?within=-1', 'dispatch');
    expectStatus(app, 422, 'GET', '/assets/due?within=366', 'dispatch');
    expectStatus(app, 403, 'GET', '/assets/due', world.technician.id);
  });

  it('looks assets up by serial number', () => {
    const { app } = world;
    const asset = call(app, 'POST', '/assets', 'admin', payload());
    assert.equal(call(app, 'GET', '/assets/by-serial/VD-100-5521', 'billing').id, asset.id);
    assert.equal(expectStatus(app, 404, 'GET', '/assets/by-serial/NOPE-1', 'billing').error.code, 'not-found');
  });
});
'''

FILES = {
    "tests/customers/accounts.test.ts": Spec(
        {**APP, "src/customers/accounts.ts": ["checkLetter", "formatAccountNumber", "isValidAccountNumber",
                                               "nextAccountNumber"]}, ACCOUNT_NUMBERS),
    "tests/customers/validation.test.ts": Spec(
        {CUSTOMER_INPUT: ["customerInput", "customerPatch", "siteInput"], VALIDATE: ["parseInput", "type Check"],
         "src/shared/errors.ts": ["ValidationError"]}, CUSTOMER_VALIDATION),
    "tests/customers/service.test.ts": Spec(
        {**APP, "src/app.ts": ["createApp", "type App"], "src/adapters/store/audit.ts": ["auditTrail"],
         CUSTOMERS: ["closeCustomer", "createCustomer", "updateCustomer"],
         CUSTOMER_INPUT: ["customerInput", "customerPatch"], VALIDATE: ["parseInput"]},
        CUSTOMER_SERVICE, node=WITH_SETUP),
    "tests/customers/sites.test.ts": Spec(
        {"src/app.ts": ["createApp", "type App"], "src/adapters/store/audit.ts": ["auditTrail"],
         "src/customers/model.ts": ["type Customer"],
         CUSTOMERS: ["addSite", "closeCustomer", "createCustomer", "updateSiteAccess"],
         CUSTOMER_INPUT: ["customerInput", "siteInput"], VALIDATE: ["parseInput"]},
        CUSTOMER_SITES, node=WITH_SETUP),
    "tests/customers/lifecycle.test.ts": Spec(
        {"src/app.ts": ["createApp", "type App"], "src/adapters/store/audit.ts": ["auditTrail"],
         "src/customers/model.ts": ["type Customer"],
         CUSTOMERS: ["acceptsNewWork", "addSite", "closeCustomer", "createCustomer", "reactivateCustomer",
                     "suspendCustomer"],
         CUSTOMER_INPUT: ["customerInput", "siteInput"], VALIDATE: ["parseInput"]},
        CUSTOMER_LIFECYCLE, jobs=["openJob"], node=WITH_SETUP),
    "tests/contracts/service.test.ts": Spec(
        {"src/app.ts": ["createApp", "type App"], "src/adapters/repositories/contracts.ts": ["contracts"],
         "src/contracts/service.ts": ["activateContract", "createContract", "rollContracts", "terminateContract"],
         "src/customers/model.ts": ["type Customer", "type Site"],
         CUSTOMERS: ["addSite", "createCustomer", "suspendCustomer"],
         CUSTOMER_INPUT: ["customerInput", "siteInput"], VALIDATE: ["parseInput"]}, CONTRACT_SERVICE),
    "tests/contracts/plans.test.ts": Spec(
        {"src/contracts/entitlements.ts": ["contractYearStart"],
         "src/contracts/model.ts": ["PLAN_TERMS", "type Contract", "type ContractPlan"]}, CONTRACT_PLANS),
    "tests/contracts/entitlements.test.ts": Spec(
        {"src/app.ts": ["createApp", "type App"],
         "src/contracts/entitlements.ts": ["coverageFor", "includedVisitsUsed"],
         "src/contracts/model.ts": ["type Contract"],
         "src/contracts/service.ts": ["activateContract", "createContract"],
         CUSTOMERS: ["addSite", "createCustomer"],
         CUSTOMER_INPUT: ["customerInput", "siteInput"], VALIDATE: ["parseInput"]},
        ENTITLEMENTS, jobs=["openJob", "cancelJob"]),
    "tests/assets/registration.test.ts": Spec(
        {"src/app.ts": ["createApp", "type App"],
         "src/assets/service.ts": ["recordService", "registerAsset", "retireAsset"],
         CUSTOMERS: ["addSite", "createCustomer"],
         CUSTOMER_INPUT: ["customerInput", "siteInput"], VALIDATE: ["parseInput"]},
        ASSET_SERVICE, node=WITH_SETUP),
    "tests/assets/maintenance.test.ts": Spec(
        {"src/app.ts": ["createApp", "type App"], "src/adapters/repositories/assets.ts": ["findAssetBySerial"],
         "src/assets/model.ts": ["ASSET_KINDS", "SERVICE_INTERVALS", "type Asset"],
         "src/assets/service.ts": ["assetsDueForService", "nextServiceDate", "recordService", "registerAsset",
                                   "retireAsset", "warrantyStatus"],
         CUSTOMERS: ["addSite", "createCustomer"],
         CUSTOMER_INPUT: ["customerInput", "siteInput"], VALIDATE: ["parseInput"]},
        ASSET_MAINTENANCE, node=WITH_SETUP),
    "tests/api/customers.test.ts": Spec(
        {WORLD: ["call", "createWorld", "expectStatus", "openWorkOrder", "type World"]}, API_CUSTOMERS,
        node=WITH_SETUP),
    "tests/api/contracts.test.ts": Spec(
        {WORLD: ["call", "createWorld", "expectStatus", "openWorkOrder", "seedContract", "type World"]},
        API_CONTRACTS, node=WITH_SETUP),
    "tests/api/assets.test.ts": Spec(
        {WORLD: ["call", "createWorld", "expectStatus", "type World"]}, API_ASSETS, node=WITH_SETUP),
}


def generate(ctx):
    register(ctx, FILES)
