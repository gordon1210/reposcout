"""Unit tests for configuration, authorization, the in-memory store and application wiring."""

from .checks_kit import APP, WITH_SETUP, Spec, register

DEFAULTS = r'''
describe('DEFAULT_CONFIG', () => {
  it('describes the standard organisation', () => {
    assert.equal(DEFAULT_CONFIG.organization.name, 'Fieldline Services');
    assert.equal(DEFAULT_CONFIG.organization.currency, 'EUR');
    assert.equal(DEFAULT_CONFIG.organization.defaultRegion, 'metro-east');
  });

  it('is a valid configuration', () => {
    assert.deepEqual(configProblems(DEFAULT_CONFIG), []);
  });

  it('schedules a working week with half-hour slots', () => {
    const { scheduling } = DEFAULT_CONFIG;
    assert.equal(scheduling.dayStartMinute, «start»);
    assert.equal(scheduling.dayEndMinute, 1020);
    assert.deepEqual(scheduling.workingDays, [1, 2, 3, 4, 5]);
    assert.equal(scheduling.slotMinutes, 30);
    assert.equal(scheduling.windowMinutes % scheduling.slotMinutes, 0);
  });

  it('weights the dispatch criteria to a hundred', () => {
    const weights = Object.values(DEFAULT_CONFIG.dispatch.weights);
    assert.equal(weights.reduce((sum, weight) => sum + weight, 0), 100);
    assert.equal(DEFAULT_CONFIG.dispatch.weights.skill, 40);
  });

  it('keeps overtime thresholds inside the maximum shift', () => {
    const { timesheets } = DEFAULT_CONFIG;
    assert.ok(timesheets.overtimeAfterMinutes <= timesheets.maxShiftMinutes);
    assert.equal(timesheets.weeklyOvertimeAfterMinutes, 2400);
  });

  it('lists public holidays as sorted calendar dates', () => {
    const holidays = DEFAULT_CONFIG.holidays;
    assert.deepEqual([...holidays].sort(), holidays);
    assert.ok(holidays.every((day) => /^\d{4}-\d{2}-\d{2}$/.test(day)));
    assert.ok(holidays.includes('2026-12-25'));
  });

  it('ships with automatic invoicing switched off', () => {
    assert.equal(DEFAULT_CONFIG.features.autoInvoiceOnCompletion, false);
    assert.equal(DEFAULT_CONFIG.features.partsAutoResume, true);
  });
});
'''

MERGE = r'''
describe('mergeConfig', () => {
  it('returns a copy when there is nothing to override', () => {
    const merged = mergeConfig(DEFAULT_CONFIG, undefined);
    assert.deepEqual(merged, DEFAULT_CONFIG);
    assert.notEqual(merged, DEFAULT_CONFIG);
    assert.notEqual(merged.scheduling, DEFAULT_CONFIG.scheduling);
  });

  it('merges nested objects key by key', () => {
    const merged = mergeConfig(DEFAULT_CONFIG, { billing: { invoicePrefix: 'FLD' } });
    assert.equal(merged.billing.invoicePrefix, 'FLD');
    assert.equal(merged.billing.creditNotePrefix, 'CN');
    assert.equal(merged.billing.paymentTermsDays, 30);
  });

  it('merges two levels deep', () => {
    const merged = mergeConfig(DEFAULT_CONFIG, { dispatch: { weights: { skill: 50, distance: 20 } } });
    assert.deepEqual(merged.dispatch.weights, { skill: 50, distance: 20, load: 20, continuity: 10 });
  });

  it('replaces arrays instead of merging them', () => {
    const merged = mergeConfig(DEFAULT_CONFIG, { scheduling: { workingDays: [2, 4] }, holidays: ['2026-07-14'] });
    assert.deepEqual(merged.scheduling.workingDays, [2, 4]);
    assert.deepEqual(merged.holidays, ['2026-07-14']);
  });

  it('ignores keys that are explicitly undefined', () => {
    const merged = mergeConfig(DEFAULT_CONFIG, { sla: { warningRatio: undefined, creditCapBasisPoints: 500 } });
    assert.equal(merged.sla.warningRatio, 0.8);
    assert.equal(merged.sla.creditCapBasisPoints, 500);
  });

  it('rejects unknown keys at any depth', () => {
    assert.throws(() => mergeConfig(DEFAULT_CONFIG, { telemetry: true } as any), /Unknown configuration key: telemetry/);
    assert.throws(() => mergeConfig(DEFAULT_CONFIG, { sla: { strictness: 3 } } as any), /Unknown configuration key: strictness/);
  });

  it('never mutates the base or aliases override values', () => {
    const days = [1, 3];
    const merged = mergeConfig(DEFAULT_CONFIG, { scheduling: { workingDays: days } });
    days.push(5);
    assert.deepEqual(merged.scheduling.workingDays, [1, 3]);
    assert.deepEqual(DEFAULT_CONFIG.scheduling.workingDays, [1, 2, 3, 4, 5]);
  });
});
'''

LOAD = r'''
describe('resolveConfig', () => {
  it('falls back to the defaults', () => {
    assert.deepEqual(resolveConfig(), DEFAULT_CONFIG);
  });

  it('applies overrides on top of the defaults', () => {
    const config = resolveConfig({ organization: { currency: 'GBP' }, timesheets: { mileageRateCents: 45 } });
    assert.equal(config.organization.currency, 'GBP');
    assert.equal(config.organization.name, 'Fieldline Services');
    assert.equal(config.timesheets.mileageRateCents, 45);
  });

  it('freezes the resolved configuration, nested values included', () => {
    const config = resolveConfig();
    assert.ok(Object.isFrozen(config));
    assert.ok(Object.isFrozen(config.scheduling));
    assert.ok(Object.isFrozen(config.holidays));
    assert.throws(() => {
      (config.scheduling as any).slotMinutes = 10;
    }, TypeError);
    assert.throws(() => {
      (config.holidays as string[]).push('2026-08-15');
    }, TypeError);
  });

  it('keeps separate resolutions independent', () => {
    const first = resolveConfig({ billing: { invoicePrefix: 'AAA' } });
    const second = resolveConfig();
    assert.equal(first.billing.invoicePrefix, 'AAA');
    assert.equal(second.billing.invoicePrefix, 'INV');
  });

  it('reports every problem in one ConfigurationError', () => {
    try {
      resolveConfig({ dispatch: { weights: { skill: 90 } }, organization: { currency: 'euro' } });
      assert.fail('expected a configuration error');
    } catch (error) {
      assert.ok(error instanceof ConfigurationError);
      assert.equal(error.name, 'ConfigurationError');
      assert.equal(error.problems.length, 2);
      assert.ok(error.problems.includes('dispatch.weights must add up to 100'));
      assert.ok(error.problems.includes('organization.currency must be an ISO code'));
      assert.match(error.message, /^Invalid configuration: .*; .*$/);
    }
  });

  it('can be merged again after resolution', () => {
    const config = resolveConfig();
    const next = mergeConfig(config, { billing: { paymentTermsDays: 14 } });
    next.billing.lateFeeBasisPoints = 200;
    assert.equal(next.billing.paymentTermsDays, 14);
    assert.equal(config.billing.lateFeeBasisPoints, 150);
  });
});
'''

SCHEMA = r'''
function problemsFor(override: DeepPartial<AppConfig>): string[] {
  return configProblems(mergeConfig(DEFAULT_CONFIG, override));
}

describe('configProblems', () => {
  const cases: Array<[string, DeepPartial<AppConfig>, string]> = [
    ['a day start after the day end', { scheduling: { dayStartMinute: 900, dayEndMinute: 600 } }, 'scheduling.dayStartMinute must be before dayEndMinute'],
    ['a day of zero length', { scheduling: { dayStartMinute: 1000, dayEndMinute: 1000 } }, 'scheduling.dayStartMinute must be before dayEndMinute'],
    ['a day bound beyond midnight', { scheduling: { dayEndMinute: 1500 } }, 'scheduling day bounds must be minutes of the day'],
    ['a fractional day bound', { scheduling: { dayStartMinute: 480.5 } }, 'scheduling day bounds must be minutes of the day'],
    ['an invalid weekday', { scheduling: { workingDays: [1, 7] } }, 'scheduling.workingDays must contain weekday numbers 0-6'],
    ['slots that do not divide the day', { scheduling: { slotMinutes: 7 } }, 'scheduling.slotMinutes must divide the day evenly'],
    ['a window that is not a slot multiple', { scheduling: { windowMinutes: 100 } }, 'scheduling.windowMinutes must be a multiple of slotMinutes'],
    ['a window shorter than a slot', { scheduling: { slotMinutes: 60, windowMinutes: 30 } }, 'scheduling.windowMinutes must be a multiple of slotMinutes'],
    ['a zero clock rounding step', { timesheets: { clockRoundingMinutes: 0 } }, 'timesheet rounding steps must be positive'],
    ['overtime beyond the maximum shift', { timesheets: { overtimeAfterMinutes: 800, maxShiftMinutes: 700 } }, 'timesheets.overtimeAfterMinutes cannot exceed maxShiftMinutes'],
    ['weights that do not add up', { dispatch: { weights: { load: 30 } } }, 'dispatch.weights must add up to 100'],
    ['a warning ratio of one', { sla: { warningRatio: 1 } }, 'sla.warningRatio must be between 0 and 1'],
    ['a warning ratio of zero', { sla: { warningRatio: 0 } }, 'sla.warningRatio must be between 0 and 1'],
    ['a lowercase currency', { organization: { currency: 'eur' } }, 'organization.currency must be an ISO code'],
  ];

  for (const [name, override, message] of cases) {
    it(`flags ${name}`, () => {
      const problems = problemsFor(override);
      assert.ok(problems.includes(message), `expected "${message}" in ${JSON.stringify(problems)}`);
    });
  }

  it('accepts a custom but consistent configuration', () => {
    const problems = problemsFor({
      scheduling: { slotMinutes: 15, windowMinutes: 90, workingDays: [1, 2, 3, 4, 5, 6] },
      dispatch: { weights: { skill: 25, distance: 25, load: 25, continuity: 25 } },
      sla: { warningRatio: 0.5 },
    });
    assert.deepEqual(problems, []);
  });
});
'''

ROLES = r'''
describe('roles', () => {
  it('treats admin, dispatcher, billing and warehouse as staff', () => {
    const staff: Role[] = ['admin', 'dispatcher', 'billing', 'warehouse'];
    for (const role of staff) assert.equal(isStaff({ id: 'x', name: 'X', role }), true, role);
  });

  it('keeps technicians and customers outside the staff group', () => {
    assert.equal(isStaff({ id: 'T1', name: 'Tech', role: 'technician', subjectId: 'TEC-000001' }), false);
    assert.equal(isStaff({ id: 'U1', name: 'Portal', role: 'customer', subjectId: 'CUS-000001' }), false);
  });

  it('lists the staff roles without duplicates', () => {
    assert.equal(new Set(STAFF_ROLES).size, STAFF_ROLES.length);
    assert.equal(STAFF_ROLES.length, 4);
  });

  it('defines an immutable system actor with administrative rights', () => {
    assert.equal(SYSTEM_ACTOR.id, 'system');
    assert.equal(SYSTEM_ACTOR.role, 'admin');
    assert.ok(Object.isFrozen(SYSTEM_ACTOR));
    assert.ok(isStaff(SYSTEM_ACTOR));
  });
});
'''

PERMISSIONS = r'''
const ALL: Permission[] = [
  'customers:read', 'customers:write', 'contracts:write', 'jobs:read', 'jobs:open', 'jobs:schedule',
  'jobs:work', 'jobs:cancel', 'technicians:write', 'inventory:read', 'inventory:write', 'timesheets:write',
  'timesheets:approve', 'billing:read', 'billing:write', 'reports:read', 'quotes:write', 'notifications:read',
  'system:admin',
];

const GRANTED: Record<Role, Permission[]> = {
  admin: ALL,
  dispatcher: [
    'customers:read', 'jobs:read', 'jobs:open', 'jobs:schedule', 'jobs:cancel', 'technicians:write',
    'inventory:read', 'timesheets:approve', 'reports:read', 'quotes:write', 'notifications:read',
  ],
  technician: ['jobs:read', 'jobs:work', 'inventory:read', 'timesheets:write'],
  billing: ['customers:read', 'jobs:read', 'billing:read', 'billing:write', 'reports:read', 'quotes:write'],
  warehouse: ['inventory:read', 'inventory:write', 'jobs:read'],
  customer: ['jobs:read', 'jobs:open', 'billing:read', 'notifications:read'],
};

const actorFor = (role: Role): Actor => ({ id: `${role}-1`, name: `Test ${role}`, role, subjectId: 'SUBJECT-1' });

describe('can', () => {
  for (const role of Object.keys(GRANTED) as Role[]) {
    it(`grants ${role} exactly its permissions`, () => {
      const allowed = ALL.filter((permission) => can(actorFor(role), permission));
      assert.deepEqual(allowed, ALL.filter((permission) => GRANTED[role].includes(permission)));
    });
  }

  it('reserves system administration for admins', () => {
    const holders = (Object.keys(GRANTED) as Role[]).filter((role) => can(actorFor(role), 'system:admin'));
    assert.deepEqual(holders, ['admin']);
  });

  it('lets only technicians record work and timesheets besides admins', () => {
    const workers = (Object.keys(GRANTED) as Role[]).filter((role) => can(actorFor(role), 'jobs:work'));
    assert.deepEqual(workers.sort(), ['admin', 'technician']);
  });
});

describe('authorize', () => {
  it('passes silently when the permission is granted', () => {
    assert.doesNotThrow(() => authorize(actorFor('billing'), 'billing:write'));
  });

  it('throws a 403 ForbiddenError naming the action and the actor', () => {
    try {
      authorize(actorFor('dispatcher'), 'billing:write');
      assert.fail('expected a ForbiddenError');
    } catch (error) {
      assert.ok(error instanceof ForbiddenError);
      assert.equal(error.status, 403);
      assert.equal(error.code, 'forbidden');
      assert.equal(error.message, 'dispatcher-1 may not billing:write');
      assert.deepEqual(error.details, { action: 'billing:write', actorId: 'dispatcher-1' });
    }
  });
});

describe('customer scope', () => {
  const portal: Actor = { id: 'USR-000001', name: 'Portal', role: 'customer', subjectId: 'CUS-000002' };

  it('shows customers only their own account', () => {
    assert.equal(canSeeCustomer(portal, 'CUS-000002'), true);
    assert.equal(canSeeCustomer(portal, 'CUS-000003'), false);
  });

  it('hides customer accounts from technicians and shows them to office staff', () => {
    assert.equal(canSeeCustomer(actorFor('technician'), 'CUS-000002'), false);
    for (const role of ['admin', 'dispatcher', 'billing', 'warehouse'] as Role[]) {
      assert.equal(canSeeCustomer(actorFor(role), 'CUS-000002'), true, role);
    }
  });

  it('asserts the scope only against customer actors', () => {
    assert.doesNotThrow(() => assertCustomerScope(portal, 'CUS-000002'));
    assert.doesNotThrow(() => assertCustomerScope(actorFor('dispatcher'), 'CUS-000009'));
    assert.throws(() => assertCustomerScope(portal, 'CUS-000009'), (error: any) => {
      return error instanceof ForbiddenError && error.message === 'USR-000001 may not access customer CUS-000009';
    });
  });
});
'''

ACTORS = r'''
describe('actor directory', () => {
  it('seeds the four office roles', () => {
    const { ctx } = createApp();
    assert.equal(findActor(ctx, 'admin')?.role, 'admin');
    assert.equal(findActor(ctx, 'dispatch')?.role, 'dispatcher');
    assert.equal(findActor(ctx, 'billing')?.role, 'billing');
    assert.equal(findActor(ctx, 'warehouse')?.role, 'warehouse');
  });

  it('returns undefined for unknown ids', () => {
    const { ctx } = createApp();
    assert.equal(findActor(ctx, 'ghost'), undefined);
  });

  it('registers actors and finds them by role', () => {
    const { ctx } = createApp();
    registerActor(ctx, { id: 'TEC-000007', name: 'Sam Doyle', role: 'technician', subjectId: 'TEC-000007' });
    registerActor(ctx, { id: 'TEC-000008', name: 'Lee Park', role: 'technician', subjectId: 'TEC-000008' });
    assert.deepEqual(actorsWithRole(ctx, 'technician').map((actor) => actor.id), ['TEC-000007', 'TEC-000008']);
    assert.deepEqual(actorsWithRole(ctx, 'admin').map((actor) => actor.id), ['admin']);
    assert.deepEqual(actorsWithRole(ctx, 'customer'), []);
  });

  it('rejects duplicate ids with a 409', () => {
    const { ctx } = createApp();
    assert.throws(
      () => registerActor(ctx, { id: 'admin', name: 'Second Admin', role: 'admin' }),
      (error: any) => error.code === 'duplicate-actor' && error.status === 409,
    );
  });

  it('stores a frozen copy of the registered actor', () => {
    const { ctx } = createApp();
    const source = { id: 'USR-000001', name: 'Portal User', role: 'customer' as const, subjectId: 'CUS-000001' };
    registerActor(ctx, source);
    source.name = 'Changed afterwards';
    const stored = findActor(ctx, 'USR-000001');
    assert.equal(stored?.name, 'Portal User');
    assert.ok(Object.isFrozen(stored));
  });

  it('keeps directories of separate applications apart', () => {
    const first = createApp();
    const second = createApp();
    registerActor(first.ctx, { id: 'only-here', name: 'Visitor', role: 'billing' });
    assert.ok(findActor(first.ctx, 'only-here'));
    assert.equal(findActor(second.ctx, 'only-here'), undefined);
  });
});
'''

MEMORY = r'''
interface Row {
  id: string;
  label: string;
  qty: number;
}

const row = (id: string, label: string, qty = 1): Row => ({ id, label, qty });

describe('memory store collections', () => {
  let store: Store;

  beforeEach(() => {
    store = createMemoryStore();
  });

  it('inserts and reads back records', () => {
    const parts = store.collection<Row>('parts');
    parts.insert(row('P-1', 'Pressure valve', 4));
    assert.deepEqual(parts.get('P-1'), row('P-1', 'Pressure valve', 4));
    assert.equal(parts.get('P-2'), undefined);
  });

  it('copies records on the way in and out', () => {
    const parts = store.collection<Row>('parts');
    const original = row('P-1', 'Pressure valve');
    parts.insert(original);
    original.label = 'Changed outside';
    const fetched = parts.get('P-1')!;
    fetched.qty = 99;
    assert.equal(parts.get('P-1')?.label, 'Pressure valve');
    assert.equal(parts.get('P-1')?.qty, 1);
  });

  it('refuses duplicate ids', () => {
    const parts = store.collection<Row>('parts');
    parts.insert(row('P-1', 'Pressure valve'));
    assert.throws(() => parts.insert(row('P-1', 'Another')), /parts: duplicate id P-1/);
  });

  it('names the singular entity in not-found errors', () => {
    const customers = store.collection<Row>('customers');
    assert.throws(() => customers.require('CUS-404'), (error: any) => {
      return error.code === 'not-found' && error.status === 404 && error.message === 'customer CUS-404 was not found';
    });
    assert.throws(() => customers.require('CUS-404', 'account'), /account CUS-404 was not found/);
  });

  it('merges partial updates and accepts updater functions', () => {
    const parts = store.collection<Row>('parts');
    parts.insert(row('P-1', 'Pressure valve', 2));
    assert.deepEqual(parts.update('P-1', { qty: 7 }), row('P-1', 'Pressure valve', 7));
    const doubled = parts.update('P-1', (current) => ({ ...current, qty: current.qty * 2 }));
    assert.equal(doubled.qty, 14);
    assert.equal(parts.require('P-1').qty, 14);
  });

  it('does not let an update change the id or hit a missing record', () => {
    const parts = store.collection<Row>('parts');
    parts.insert(row('P-1', 'Pressure valve'));
    assert.throws(() => parts.update('P-1', { id: 'P-9' }), /an update cannot change the id/);
    assert.throws(() => parts.update('P-7', { qty: 1 }), /part P-7 was not found/);
  });

  it('removes records and reports whether anything was removed', () => {
    const parts = store.collection<Row>('parts');
    parts.insert(row('P-1', 'Pressure valve'));
    assert.equal(parts.remove('P-1'), true);
    assert.equal(parts.remove('P-1'), false);
    assert.equal(parts.count(), 0);
  });

  it('queries by predicate in insertion order', () => {
    const parts = store.collection<Row>('parts');
    parts.insert(row('P-3', 'Gasket', 10));
    parts.insert(row('P-1', 'Valve', 0));
    parts.insert(row('P-2', 'Filter', 5));
    assert.deepEqual(parts.all().map((entry) => entry.id), ['P-3', 'P-1', 'P-2']);
    assert.deepEqual(parts.find((entry) => entry.qty > 0).map((entry) => entry.id), ['P-3', 'P-2']);
    assert.equal(parts.first((entry) => entry.qty === 0)?.label, 'Valve');
    assert.equal(parts.first((entry) => entry.qty > 100), undefined);
    assert.equal(parts.count((entry) => entry.qty >= 5), 2);
  });

  it('keeps collections separate and exposes a snapshot of those in use', () => {
    store.collection<Row>('parts').insert(row('P-1', 'Valve'));
    store.collection<Row>('stock').insert(row('S-1', 'Bin A'));
    assert.equal(store.collection<Row>('stock').get('P-1'), undefined);
    const snapshot = store.snapshot();
    assert.deepEqual(Object.keys(snapshot).sort(), ['parts', 'stock']);
    assert.equal(snapshot.parts[0].id, 'P-1');
  });
});
'''

AUDIT = r'''
describe('audit trail', () => {
  let app: App;

  beforeEach(() => {
    app = createApp({ now: '2026-03-02T09:00:00Z' });
  });

  it('stamps entries with the clock and a sequential id', () => {
    const entry = recordAudit(app.ctx, 'admin', 'customer.created', 'CUS-000001', { segment: 'commercial' });
    assert.equal(entry.id, 'AUD-000001');
    assert.equal(entry.at, '2026-03-02T09:00:00Z');
    assert.deepEqual(entry.details, { segment: 'commercial' });
    app.clock.advance(15);
    assert.equal(recordAudit(app.ctx, 'admin', 'customer.updated', 'CUS-000001').at, '2026-03-02T09:15:00Z');
  });

  it('collects the trail of one subject in id order', () => {
    recordAudit(app.ctx, 'admin', 'asset.registered', 'AST-000001');
    recordAudit(app.ctx, 'dispatch', 'contract.created', 'CON-000001');
    recordAudit(app.ctx, 'admin', 'asset.retired', 'AST-000001');
    const trail = auditTrail(app.ctx, 'AST-000001');
    assert.deepEqual(trail.map((entry) => entry.action), ['asset.registered', 'asset.retired']);
    assert.deepEqual(auditTrail(app.ctx, 'AST-999999'), []);
  });

  it('filters actions by actor and start time', () => {
    recordAudit(app.ctx, 'dispatch', 'one', 'X-1');
    app.clock.advance(60);
    recordAudit(app.ctx, 'dispatch', 'two', 'X-2');
    recordAudit(app.ctx, 'admin', 'three', 'X-3');
    assert.deepEqual(actionsBy(app.ctx, 'dispatch').map((entry) => entry.action), ['one', 'two']);
    assert.deepEqual(actionsBy(app.ctx, 'dispatch', '2026-03-02T10:00:00Z').map((entry) => entry.action), ['two']);
    assert.deepEqual(actionsBy(app.ctx, 'billing'), []);
  });

  it('is written by domain services', () => {
    const input = parseInput(customerInput, { name: 'Marlow Dental', segment: 'commercial', billingRegion: 'metro-east' });
    const customer = createCustomer(app.ctx, 'dispatch', input);
    const [entry] = auditTrail(app.ctx, customer.id);
    assert.equal(entry.actorId, 'dispatch');
    assert.equal(entry.action, 'customer.created');
    assert.deepEqual(entry.details, { segment: 'commercial' });
  });
});
'''

REPO_CUSTOMERS = r'''
function open(app: App, name: string, region: string, segment = 'commercial') {
  const input = parseInput(customerInput, { name, segment, billingRegion: region });
  return createCustomer(app.ctx, 'admin', input);
}

describe('customer repository', () => {
  it('finds accounts by number regardless of case and padding', () => {
    const app = createApp();
    const customer = open(app, 'Alder Court Management', 'metro-east');
    assert.equal(findCustomerByAccount(app.ctx, customer.accountNumber)?.id, customer.id);
    assert.equal(findCustomerByAccount(app.ctx, `  ${customer.accountNumber.toLowerCase()} `)?.id, customer.id);
    assert.equal(findCustomerByAccount(app.ctx, 'C999999Z'), undefined);
  });

  it('lists active customers alphabetically', () => {
    const app = createApp();
    const zeta = open(app, 'Zeta Foods', 'metro-east');
    open(app, 'Acorn Nursery', 'metro-west');
    const middle = open(app, 'Meridian Clinic', 'metro-east');
    suspendCustomer(app.ctx, 'admin', middle.id, 'Unpaid invoices');
    assert.deepEqual(activeCustomers(app.ctx).map((customer) => customer.name), ['Acorn Nursery', zeta.name]);
  });

  it('filters by billing region', () => {
    const app = createApp();
    open(app, 'North Quay Lofts', 'coastal');
    open(app, 'Harbour Bakery', 'coastal');
    open(app, 'Hilltop Farm Shop', 'highlands');
    assert.equal(customersInRegion(app.ctx, 'coastal').length, 2);
    assert.equal(customersInRegion(app.ctx, 'highlands')[0].name, 'Hilltop Farm Shop');
    assert.deepEqual(customersInRegion(app.ctx, 'valley'), []);
  });
});
'''

REPO_SITES = r'''
const address = (line1: string, region = 'metro-east') => ({
  line1,
  city: 'Eastport',
  postcode: 'EP2 6TT',
  region,
  location: { lat: 52.5, lon: 13.4 },
});

function siteFor(app: App, customerId: string, label: string, line1: string, region?: string) {
  const input = parseInput(siteInput, { label, address: address(line1, region) });
  return addSite(app.ctx, 'admin', customerId, input);
}

describe('site repository', () => {
  let app: App;
  let customer: Customer;

  beforeEach(() => {
    app = createApp();
    customer = createCustomer(app.ctx, 'admin', parseInput(customerInput, {
      name: 'Pennine Storage',
      segment: 'commercial',
      billingRegion: 'metro-east',
    }));
  });

  it('lists the sites of a customer sorted by label', () => {
    siteFor(app, customer.id, 'Warehouse B', '4 Dock Road');
    siteFor(app, customer.id, 'Annex', '6 Dock Road');
    siteFor(app, customer.id, 'Main yard', '8 Dock Road');
    assert.deepEqual(sitesForCustomer(app.ctx, customer.id).map((site) => site.label), ['Annex', 'Main yard', 'Warehouse B']);
  });

  it('only returns sites of the requested customer', () => {
    const other = createCustomer(app.ctx, 'admin', parseInput(customerInput, {
      name: 'Quill & Co',
      segment: 'commercial',
      billingRegion: 'metro-east',
    }));
    siteFor(app, customer.id, 'Depot', '1 Canal Way');
    siteFor(app, other.id, 'Office', '2 Canal Way');
    assert.equal(sitesForCustomer(app.ctx, other.id).length, 1);
    assert.equal(sitesForCustomer(app.ctx, other.id)[0].label, 'Office');
  });

  it('requires the site to belong to the customer', () => {
    const site = siteFor(app, customer.id, 'Depot', '1 Canal Way');
    assert.equal(requireSiteOf(app.ctx, customer.id, site.id).label, 'Depot');
    assert.throws(() => requireSiteOf(app.ctx, 'CUS-000099', site.id), (error: any) => error.status === 404);
    assert.throws(() => requireSiteOf(app.ctx, customer.id, 'SITE-000099'), /site SITE-000099 was not found/);
  });

  it('groups sites by address region', () => {
    siteFor(app, customer.id, 'Pier', '9 Pier Street', 'coastal');
    siteFor(app, customer.id, 'Hill', '3 Ridge Lane', 'highlands');
    assert.deepEqual(sitesInRegion(app.ctx, 'coastal').map((site) => site.label), ['Pier']);
    assert.deepEqual(sitesInRegion(app.ctx, 'valley'), []);
  });
});
'''

REPO_CONTRACTS = r'''
function contract(id: string, overrides: Partial<Contract> = {}): Contract {
  return {
    id,
    customerId: 'CUS-000001',
    siteIds: ['SITE-000001'],
    plan: 'basic',
    status: 'active',
    startsOn: '2026-01-01',
    slaPolicyId: 'standard',
    coveredWorkTypes: ['general'],
    laborDiscountBasisPoints: 0,
    includedVisitsPerYear: 1,
    renewal: 'manual',
    ...overrides,
  };
}

describe('contract repository', () => {
  let app: App;

  beforeEach(() => {
    app = createApp();
  });

  it('selects the active contract covering a site on a date', () => {
    contracts(app.ctx).insert(contract('CON-000001', { endsOn: '2026-12-31' }));
    assert.equal(contractForSite(app.ctx, 'SITE-000001', '2026-06-15')?.id, 'CON-000001');
    assert.equal(contractForSite(app.ctx, 'SITE-000001', '2026-12-31')?.id, 'CON-000001');
    assert.equal(contractForSite(app.ctx, 'SITE-000001', '2027-01-01'), undefined);
    assert.equal(contractForSite(app.ctx, 'SITE-000001', '2025-12-31'), undefined);
  });

  it('ignores drafts, ended contracts and other sites', () => {
    contracts(app.ctx).insert(contract('CON-000001', { status: 'draft' }));
    contracts(app.ctx).insert(contract('CON-000002', { status: 'terminated' }));
    contracts(app.ctx).insert(contract('CON-000003', { siteIds: ['SITE-000002'] }));
    assert.equal(contractForSite(app.ctx, 'SITE-000001', '2026-06-15'), undefined);
    assert.equal(contractForSite(app.ctx, 'SITE-000002', '2026-06-15')?.id, 'CON-000003');
  });

  it('prefers the contract with the later start when renewals overlap', () => {
    contracts(app.ctx).insert(contract('CON-000001', { startsOn: '2026-01-01', endsOn: '2026-12-31' }));
    contracts(app.ctx).insert(contract('CON-000002', { startsOn: '2026-06-01', plan: 'premium' }));
    assert.equal(contractForSite(app.ctx, 'SITE-000001', '2026-05-31')?.id, 'CON-000001');
    assert.equal(contractForSite(app.ctx, 'SITE-000001', '2026-06-01')?.id, 'CON-000002');
  });

  it('lists contracts per customer', () => {
    contracts(app.ctx).insert(contract('CON-000001'));
    contracts(app.ctx).insert(contract('CON-000002', { customerId: 'CUS-000002' }));
    assert.deepEqual(contractsForCustomer(app.ctx, 'CUS-000002').map((entry) => entry.id), ['CON-000002']);
  });

  it('finds active contracts that end before a date', () => {
    contracts(app.ctx).insert(contract('CON-000001', { endsOn: '2026-03-31' }));
    contracts(app.ctx).insert(contract('CON-000002', { endsOn: '2026-09-30' }));
    contracts(app.ctx).insert(contract('CON-000003'));
    contracts(app.ctx).insert(contract('CON-000004', { endsOn: '2026-02-01', status: 'expired' }));
    assert.deepEqual(expiringContracts(app.ctx, '2026-04-01').map((entry) => entry.id), ['CON-000001']);
    assert.deepEqual(expiringContracts(app.ctx, '2026-12-01').map((entry) => entry.id), ['CON-000001', 'CON-000002']);
  });
});
'''

REQUEST = r'''
describe('request dispatch', () => {
  let app: App;

  beforeEach(() => {
    app = createApp();
  });

  it('answers the health check for any known actor', () => {
    const response = app.request('GET', '/health', { actor: 'billing' });
    assert.equal(response.status, 200);
    assert.deepEqual(response.body, { status: 'ok', time: DEFAULT_START, organization: 'Fieldline Services' });
  });

  it('rejects unknown and missing actors with 401', () => {
    assert.equal(app.request('GET', '/health', { actor: 'stranger' }).status, 401);
    const anonymous = app.request('GET', '/health');
    assert.equal(anonymous.status, 401);
    assert.equal((anonymous.body as any).error.code, 'unauthenticated');
  });

  it('reports unknown paths as 404 even without an actor', () => {
    const response = app.request('GET', '/does/not/exist');
    assert.equal(response.status, 404);
    assert.equal((response.body as any).error.code, 'route-not-found');
    assert.match((response.body as any).error.message, /GET \/does\/not\/exist/);
  });

  it('answers a wrong method with 405 and an allow header', () => {
    const response = app.request('DELETE', '/customers', { actor: 'admin' });
    assert.equal(response.status, 405);
    const allowed = (response.headers?.allow ?? '').split(', ').sort();
    assert.deepEqual(allowed, ['GET', 'POST']);
    assert.equal((response.body as any).error.code, 'method-not-allowed');
  });

  it('parses the query string into the request', () => {
    const response = app.request('GET', '/customers?q=none&pageSize=5', { actor: 'admin' });
    assert.equal(response.status, 200);
    assert.deepEqual((response.body as any).items, []);
  });

  it('serialises responses to plain JSON', () => {
    const created = app.request('POST', '/customers', {
      actor: 'admin',
      body: { name: 'Linden Surgery', segment: 'commercial', billingRegion: 'metro-east' },
    });
    assert.equal(created.status, 201);
    const body = created.body as any;
    assert.equal('email' in body, false);
    assert.equal(body.status, 'active');
    assert.equal(body.createdAt, DEFAULT_START);
  });

  it('does not let handlers alias the request body', () => {
    const body = { name: 'Fenwick Pharmacy', segment: 'commercial', billingRegion: 'metro-east' };
    app.request('POST', '/customers', { actor: 'admin', body });
    assert.deepEqual(body, { name: 'Fenwick Pharmacy', segment: 'commercial', billingRegion: 'metro-east' });
  });

  it('maps unexpected failures to a generic 500 and logs them', () => {
    app.ctx.workTypes.entries = () => {
      throw new TypeError('boom');
    };
    const failed = app.request('GET', '/work-types', { actor: 'admin' });
    assert.equal(failed.status, 500);
    assert.deepEqual(failed.body, { error: { code: 'internal', message: 'Internal error' } });
    assert.deepEqual(app.ctx.log.records('error').map((record) => record.fields.message), ['boom']);
  });

  it('lists the route table sorted by pattern and method', () => {
    const table = app.routes();
    assert.ok(table.length > 60);
    const keys = table.map((entry) => `${entry.pattern} ${entry.method}`);
    assert.deepEqual([...keys].sort((left, right) => {
      const [lp, lm] = left.split(' ');
      const [rp, rm] = right.split(' ');
      return lp.localeCompare(rp) || lm.localeCompare(rm);
    }), keys);
    assert.equal(new Set(keys).size, keys.length);
    assert.ok(table.some((entry) => entry.method === 'GET' && entry.pattern === '/health'));
  });
});
'''

OPTIONS = r'''
describe('createApp', () => {
  it('starts the manual clock at the requested time', () => {
    const app = createApp({ now: '2026-07-01T06:45:00Z' });
    assert.equal(app.clock.now(), '2026-07-01T06:45:00Z');
    assert.equal(app.ctx.clock.now(), '2026-07-01T06:45:00Z');
    assert.equal(app.clock.advance(90), '2026-07-01T08:15:00Z');
  });

  it('defaults to the standard start', () => {
    assert.equal(createApp().clock.now(), DEFAULT_START);
    assert.equal(DEFAULT_START, '2026-03-02T08:00:00Z');
  });

  it('rejects malformed start times', () => {
    assert.throws(() => createApp({ now: 'yesterday' }), RangeError);
  });

  it('resolves configuration overrides', () => {
    const app = createApp({ config: { billing: { invoicePrefix: 'FLD' }, organization: { name: 'Fieldline North' } } });
    assert.equal(app.ctx.config.billing.invoicePrefix, 'FLD');
    assert.equal(app.ctx.config.organization.name, 'Fieldline North');
    const health = app.request('GET', '/health', { actor: 'admin' });
    assert.equal((health.body as any).organization, 'Fieldline North');
  });

  it('fails fast on an invalid configuration', () => {
    assert.throws(() => createApp({ config: { sla: { warningRatio: 2 } } }), ConfigurationError);
  });

  it('keeps applications isolated from each other', () => {
    const left = createApp();
    const right = createApp();
    left.request('POST', '/customers', {
      actor: 'admin',
      body: { name: 'Isolated Holdings', segment: 'commercial', billingRegion: 'metro-east' },
    });
    assert.equal(left.ctx.store.collection('customers').count(), 1);
    assert.equal(right.ctx.store.collection('customers').count(), 0);
    assert.equal(right.ctx.ids.peek('CUS'), 'CUS-000001');
  });

  it('wires every registry', () => {
    const { ctx } = createApp();
    assert.ok(ctx.workTypes.has('general'));
    assert.ok(ctx.templates.keys().length > 0);
    assert.deepEqual(ctx.scoringRules.keys(), ['continuity', 'distance', 'load', 'skill-match']);
    assert.ok(ctx.exportFormats.keys().length > 0);
  });

  it('registers event subscriptions for the cross-cutting concerns', () => {
    const { ctx } = createApp();
    assert.ok(ctx.bus.subscribers(«JobEvents».completed) >= 3);
    assert.ok(ctx.bus.subscribers(«JobEvents».opened) >= 2);
    assert.equal(ctx.bus.subscribers('never.published'), 0);
  });
});
'''

CALENDAR = r'''
describe('businessCalendar', () => {
  it('mirrors the scheduling configuration', () => {
    const { ctx } = createApp();
    const calendar = businessCalendar(ctx);
    assert.equal(calendar.dayStartMinute, ctx.config.scheduling.dayStartMinute);
    assert.equal(calendar.dayEndMinute, 1020);
    assert.deepEqual(calendar.workingDays, [1, 2, 3, 4, 5]);
    assert.deepEqual(calendar.holidays, ctx.config.holidays);
  });

  it('follows configured working days and holidays', () => {
    const { ctx } = createApp({ config: { scheduling: { workingDays: [1, 2, 3, 4, 5, 6] }, holidays: ['2026-03-09'] } });
    const calendar = businessCalendar(ctx);
    assert.deepEqual(calendar.workingDays, [1, 2, 3, 4, 5, 6]);
    assert.deepEqual(calendar.holidays, ['2026-03-09']);
  });

  it('feeds the shared calendar helpers', () => {
    const { ctx } = createApp({ config: { holidays: ['2026-03-04'] } });
    const calendar = businessCalendar(ctx);
    assert.equal(isWorkingDay(calendar, '2026-03-03'), true);
    assert.equal(isWorkingDay(calendar, '2026-03-04'), false);
    assert.equal(isWorkingDay(calendar, '2026-03-07'), false);
  });
});
'''


FILES = {
    "tests/config/defaults.test.ts": Spec(
        {"src/config/defaults.ts": ["DEFAULT_CONFIG"], "src/config/schema.ts": ["configProblems"]}, DEFAULTS,
        subs=lambda ctx: {"start": 420 if ctx.early else 480}),
    "tests/config/merge.test.ts": Spec(
        {"src/config/defaults.ts": ["DEFAULT_CONFIG"], "src/config/load.ts": ["mergeConfig"]}, MERGE),
    "tests/config/load.test.ts": Spec(
        {"src/config/defaults.ts": ["DEFAULT_CONFIG"],
         "src/config/load.ts": ["ConfigurationError", "mergeConfig", "resolveConfig"]}, LOAD),
    "tests/config/schema.test.ts": Spec(
        {"src/config/defaults.ts": ["DEFAULT_CONFIG"], "src/config/load.ts": ["mergeConfig"],
         "src/config/schema.ts": ["configProblems", "type AppConfig", "type DeepPartial"]}, SCHEMA),
    "tests/auth/roles.test.ts": Spec(
        {"src/auth/roles.ts": ["isStaff", "STAFF_ROLES", "SYSTEM_ACTOR", "type Role"]}, ROLES),
    "tests/auth/permissions.test.ts": Spec(
        {"src/auth/permissions.ts": ["assertCustomerScope", "authorize", "can", "canSeeCustomer", "type Permission"],
         "src/auth/roles.ts": ["type Actor", "type Role"], "src/shared/errors.ts": ["ForbiddenError"]}, PERMISSIONS),
    "tests/auth/actors.test.ts": Spec(
        {**APP, "src/auth/actors.ts": ["actorsWithRole", "findActor", "registerActor"]}, ACTORS),
    "tests/adapters/memory-store.test.ts": Spec(
        {"src/adapters/store/memory.ts": ["createMemoryStore", "type Store"]}, MEMORY, node=WITH_SETUP),
    "tests/adapters/audit.test.ts": Spec(
        {**APP, "src/adapters/store/audit.ts": ["actionsBy", "auditTrail", "recordAudit"],
         "src/app.ts": ["createApp", "type App"],
         "src/customers/service.ts": ["createCustomer"],
         "src/customers/validation.ts": ["customerInput"], "src/shared/validate.ts": ["parseInput"]},
        AUDIT, node=WITH_SETUP),
    "tests/adapters/customer-repository.test.ts": Spec(
        {"src/adapters/repositories/customers.ts": ["activeCustomers", "customersInRegion", "findCustomerByAccount"],
         "src/app.ts": ["createApp", "type App"],
         "src/customers/service.ts": ["createCustomer", "suspendCustomer"],
         "src/customers/validation.ts": ["customerInput"], "src/shared/validate.ts": ["parseInput"]}, REPO_CUSTOMERS),
    "tests/adapters/site-repository.test.ts": Spec(
        {"src/adapters/repositories/sites.ts": ["requireSiteOf", "sitesForCustomer", "sitesInRegion"],
         "src/app.ts": ["createApp", "type App"], "src/customers/model.ts": ["type Customer"],
         "src/customers/service.ts": ["addSite", "createCustomer"],
         "src/customers/validation.ts": ["customerInput", "siteInput"], "src/shared/validate.ts": ["parseInput"]},
        REPO_SITES, node=WITH_SETUP),
    "tests/adapters/contract-repository.test.ts": Spec(
        {"src/adapters/repositories/contracts.ts": ["contractForSite", "contracts", "contractsForCustomer",
                                                    "expiringContracts"],
         "src/app.ts": ["createApp", "type App"], "src/contracts/model.ts": ["type Contract"]},
        REPO_CONTRACTS, node=WITH_SETUP),
    "tests/app/request.test.ts": Spec(
        {"src/app.ts": ["createApp", "DEFAULT_START", "type App"]}, REQUEST, node=WITH_SETUP),
    "tests/app/options.test.ts": Spec(
        {"src/app.ts": ["createApp", "DEFAULT_START"], "src/config/load.ts": ["ConfigurationError"]}, OPTIONS,
        jobs=["JobEvents"]),
    "tests/app/calendar.test.ts": Spec(
        {"src/app.ts": ["createApp"], "src/context.ts": ["businessCalendar"],
         "src/shared/calendar.ts": ["isWorkingDay"]}, CALENDAR),
}


def generate(ctx):
    register(ctx, FILES)
