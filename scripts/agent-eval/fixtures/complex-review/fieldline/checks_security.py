"""Cross-cutting API tests: authentication, permissions, record scoping, system routes and error handling."""

from .checks_kit import DESCRIBE_IT, WITH_SETUP, Spec, register

WORLD = "tests/support/world.ts"
HTTP = "src/api/http.ts"

AUTHENTICATION = r'''
let world: World;
let portal: string;

beforeEach(() => {
  world = createWorld();
  portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
});

const SAMPLE: Array<[Method, string]> = [
  ['GET', '/health'],
  ['GET', '/customers'],
  ['POST', '/customers'],
  ['GET', JOBS],
  ['GET', '/reports/backlog'],
  ['GET', '/notifications'],
  ['POST', '/quotes'],
  ['GET', '/webhooks/topics'],
  ['GET', '/stock/WH-CENTRAL'],
  ['GET', '/technicians'],
];

describe('authentication', () => {
  it('rejects actors that are not registered', () => {
    for (const [method, url] of SAMPLE) {
      const body = expectStatus(world.app, 401, method, url, 'intruder', {});
      assert.deepEqual(body, { error: { code: 'unauthenticated', message: 'Unknown or missing actor' } });
    }
  });

  it('rejects requests without an actor', () => {
    for (const [method, url] of SAMPLE) assert.equal(world.app.request(method, url).status, 401, `${method} ${url}`);
  });

  it('matches actor identifiers exactly', () => {
    for (const actor of ['Admin', 'ADMIN', ' admin', 'admin ', '']) {
      assert.equal(world.app.request('GET', '/health', { actor }).status, 401, JSON.stringify(actor));
    }
  });

  it('checks the actor before the payload', () => {
    const body = expectStatus(world.app, 401, 'POST', '/customers', 'intruder', { name: '' });
    assert.equal(body.error.code, 'unauthenticated');
  });

  it('accepts every kind of registered actor', () => {
    for (const actor of ['admin', 'dispatch', 'billing', 'warehouse', world.technician.id, portal]) {
      assert.equal(expectStatus(world.app, 200, 'GET', '/health', actor).status, 'ok');
    }
  });

  it('knows technicians and portal users from the moment they are created', () => {
    const technician = seedTechnician(world.app, { name: 'Casey Lin' });
    const user = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Sam Rivera' });
    for (const actor of [technician.id, user.id]) expectStatus(world.app, 200, 'GET', '/health', actor);
    assert.notEqual(user.id, portal);
  });

  it('does not treat a customer id as an actor', () => {
    assert.equal(world.app.request('GET', '/health', { actor: world.customer.id }).status, 401);
  });

  it('ignores the query string when routing', () => {
    expectStatus(world.app, 200, 'GET', '/health?verbose=1&x=', 'admin');
  });
});

describe('routing errors', () => {
  it('reports unknown paths', () => {
    const body = expectStatus(world.app, 404, 'GET', '/no/such/route', 'admin');
    assert.deepEqual(body, { error: { code: 'route-not-found', message: 'No route for GET /no/such/route' } });
  });

  it('reports unknown sub-resources of known records', () => {
    assert.equal(expectStatus(world.app, 404, 'GET', `${JOBS}/WO-000001/frobnicate`, 'admin').error.code, 'route-not-found');
    assert.equal(expectStatus(world.app, 404, 'POST', '/customers/CUS-000001/sites/extra', 'admin', {}).error.code, 'route-not-found');
  });

  it('distinguishes a missing record from a missing route', () => {
    const body = expectStatus(world.app, 404, 'GET', `${JOBS}/WO-999999`, 'admin');
    assert.equal(body.error.code, 'not-found');
  });

  it('answers a wrong method with the allowed ones', () => {
    const response = world.app.request('DELETE', '/customers', { actor: 'admin' });
    assert.equal(response.status, 405);
    assert.equal((response.body as any).error.code, 'method-not-allowed');
    assert.deepEqual(response.headers?.allow.split(', ').sort(), ['GET', 'POST']);
    assert.equal((response.body as any).error.message, `Allowed: ${response.headers?.allow}`);
  });

  it('lists a single allowed method', () => {
    for (const [method, url, allowed] of [
      ['POST', '/health', 'GET'],
      ['GET', '/customers/CUS-000001/close', 'POST'],
      ['PUT', '/reports/kpis', 'GET'],
      ['GET', '/webhooks', 'POST'],
    ] as Array<[Method, string, string]>) {
      const response = world.app.request(method, url, { actor: 'admin' });
      assert.equal(response.status, 405, `${method} ${url}`);
      assert.equal(response.headers?.allow, allowed);
    }
  });

  it('sees the same methods on parameterised paths', () => {
    const response = world.app.request('POST', '/customers/CUS-000001', { actor: 'admin' });
    assert.equal(response.status, 405);
    assert.deepEqual(response.headers?.allow.split(', ').sort(), ['GET', 'PATCH']);
  });
});
'''

PERMISSIONS = r'''
let world: World;
let actors: Record<string, string>;

const MISSING = 'MISSING-1';
type Row = [Method, string, ((world: World) => unknown)?];

const own = (extra: Record<string, unknown> = {}) => (target: World) => ({ technicianId: target.technician.id, kind: 'admin', ...extra });
const GRANTS: Record<string, { roles: string[]; routes: Row[] }> = {
  'customers:read': {
    roles: ['admin', 'dispatcher', 'billing'],
    routes: [['GET', '/customers'], ['GET', `/contracts/${MISSING}`], ['GET', `/customers/${MISSING}/contracts`], ['GET', `/sites/${MISSING}/coverage`]],
  },
  'customers:write': {
    roles: ['admin'],
    routes: [
      ['POST', '/customers'], ['PATCH', `/customers/${MISSING}`], ['POST', `/customers/${MISSING}/suspend`],
      ['POST', `/customers/${MISSING}/reactivate`], ['POST', `/customers/${MISSING}/close`], ['POST', `/customers/${MISSING}/sites`],
      ['PUT', `/sites/${MISSING}/access`], ['POST', `/customers/${MISSING}/portal-users`], ['POST', '/assets'],
      ['POST', `/assets/${MISSING}/retire`],
    ],
  },
  'contracts:write': {
    roles: ['admin'],
    routes: [['POST', '/contracts'], ['POST', `/contracts/${MISSING}/activate`], ['POST', `/contracts/${MISSING}/terminate`]],
  },
  'technicians:write': {
    roles: ['admin', 'dispatcher'],
    routes: [
      ['POST', '/technicians'], ['PUT', `/technicians/${MISSING}/certifications`], ['POST', `/technicians/${MISSING}/absences`],
      ['POST', `/technicians/${MISSING}/deactivate`], ['GET', '/technicians/certifications/expiring'],
    ],
  },
  'jobs:open': { roles: ['admin', 'dispatcher', 'customer'], routes: [['POST', JOBS]] },
  'jobs:schedule': {
    roles: ['admin', 'dispatcher'],
    routes: [
      ['GET', '/technicians'], ['GET', '/assets/due'], ['POST', `${JOBS}/${MISSING}/schedule`], ['POST', `${JOBS}/${MISSING}/assign`],
      ['POST', `${JOBS}/${MISSING}/close`], ['POST', `${JOBS}/${MISSING}/reopen`], ['POST', `${JOBS}/${MISSING}/follow-ups`],
      ['POST', '/appointments'], ['POST', `/appointments/${MISSING}/cancel`], ['POST', `/appointments/${MISSING}/reschedule`],
      ['GET', `/technicians/${MISSING}/slots`], ['GET', `/technicians/${MISSING}/load`], ['GET', `${JOBS}/${MISSING}/recommendations`],
      ['GET', '/dispatch/board'], ['GET', '/dispatch/capacity'], ['GET', '/dispatch/rules'], ['POST', `${JOBS}/${MISSING}/reservations`],
    ],
  },
  'jobs:cancel': { roles: ['admin', 'dispatcher'], routes: [['POST', `${JOBS}/${MISSING}/cancel`]] },
  'jobs:work': {
    roles: ['admin', 'technician'],
    routes: [
      ['POST', `${JOBS}/${MISSING}/start`], ['POST', `${JOBS}/${MISSING}/hold`], ['POST', `${JOBS}/${MISSING}/resume`],
      ['POST', `${JOBS}/${MISSING}/complete`], ['PUT', `${JOBS}/${MISSING}/checklist`], ['POST', `/appointments/${MISSING}/no-access`],
    ],
  },
  'jobs:read': {
    roles: ['admin', 'dispatcher', 'billing', 'warehouse', 'technician', 'customer'],
    routes: [
      ['GET', JOBS], ['GET', '/work-types'], ['GET', `${JOBS}/${MISSING}`], ['GET', `${JOBS}/${MISSING}/timeline`],
      ['GET', `${JOBS}/${MISSING}/sla`], ['GET', `${JOBS}/${MISSING}/reservations`], ['GET', `${JOBS}/${MISSING}/notifications`],
      ['POST', `${JOBS}/${MISSING}/notes`], ['POST', `/appointments/${MISSING}/confirm`],
    ],
  },
  'inventory:read': {
    roles: ['admin', 'dispatcher', 'technician', 'warehouse'],
    routes: [['GET', '/parts'], ['GET', '/stock/WH-CENTRAL'], ['GET', '/stock/WH-CENTRAL/reorder'], ['GET', '/inventory/valuation']],
  },
  'inventory:write': {
    roles: ['admin', 'warehouse'],
    routes: [
      ['POST', '/parts'], ['PATCH', `/parts/${MISSING}/price`], ['POST', `/parts/${MISSING}/discontinue`], ['POST', '/stock/receipts'],
      ['POST', '/stock/transfers'], ['POST', '/stock/counts'],
    ],
  },
  'timesheets:write': {
    roles: ['admin', 'technician'],
    routes: [['POST', '/time-entries', own()], ['POST', '/clock/in', own()], ['POST', '/clock/out', () => ({ entryId: MISSING })]],
  },
  'timesheets:approve': {
    roles: ['admin', 'dispatcher'],
    routes: [
      ['POST', `/time-entries/${MISSING}/correct`], ['POST', '/time-entries/approve'], ['POST', `/time-entries/${MISSING}/reject`],
      ['GET', '/time-entries/pending'], ['GET', `/technicians/${MISSING}/mileage`],
    ],
  },
  'billing:read': {
    roles: ['admin', 'billing', 'customer'],
    routes: [['GET', '/invoices/overdue'], ['GET', `/invoices/${MISSING}`], ['GET', `/invoices/${MISSING}/late-fee`], ['GET', '/tariffs/metro-east']],
  },
  'billing:write': {
    roles: ['admin', 'billing'],
    routes: [
      ['POST', `${JOBS}/${MISSING}/invoice`], ['POST', `/invoices/${MISSING}/issue`], ['POST', `/invoices/${MISSING}/void`],
      ['POST', `/invoices/${MISSING}/payments`], ['POST', `/invoices/${MISSING}/credit-notes`],
    ],
  },
  'reports:read': {
    roles: ['admin', 'dispatcher', 'billing'],
    routes: [
      ['GET', '/reports/kpis'], ['GET', '/reports/utilisation'], ['GET', '/reports/backlog'], ['GET', '/reports/revenue'],
      ['GET', '/reports/activity'], ['GET', '/sla/compliance'],
    ],
  },
  'quotes:write': {
    roles: ['admin', 'dispatcher', 'billing'],
    routes: [['POST', '/quotes'], ['GET', `/quotes/${MISSING}`], ['POST', `/quotes/${MISSING}/send`], ['GET', `/customers/${MISSING}/quotes`]],
  },
  'notifications:read': { roles: ['admin', 'dispatcher', 'customer'], routes: [['GET', '/notifications']] },
  'system:admin': {
    roles: ['admin'],
    routes: [
      ['GET', '/config'], ['GET', `/audit/${MISSING}`], ['POST', '/contracts/roll'], ['POST', '/quotes/expire'], ['POST', '/sla/scan'],
      ['GET', '/notifications/templates'], ['POST', '/notifications/flush'], ['POST', '/webhooks'], ['GET', '/webhooks/topics'],
      ['GET', '/webhooks/deliveries'],
    ],
  },
};

const ROLES = ['admin', 'dispatcher', 'billing', 'warehouse', 'technician', 'customer'];

beforeEach(() => {
  world = createWorld();
  const portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
  actors = { admin: 'admin', dispatcher: 'dispatch', billing: 'billing', warehouse: 'warehouse', technician: world.technician.id, customer: portal };
});

const send = (role: string, [method, url, body]: Row) => world.app.request(method, url, { actor: actors[role], body: body?.(world) });

for (const [permission, grant] of Object.entries(GRANTS)) {
  describe(permission, () => {
    it('is refused to the other roles', () => {
      for (const role of ROLES.filter((candidate) => !grant.roles.includes(candidate))) {
        for (const row of grant.routes) {
          const response = send(role, row);
          const label = `${role} ${row[0]} ${row[1]}`;
          assert.equal(response.status, 403, label);
          assert.deepEqual((response.body as any).error.details, { action: permission, actorId: actors[role] }, label);
        }
      }
    });

    it('is passed to the roles that hold it', () => {
      for (const role of grant.roles) {
        for (const row of grant.routes) {
          const { status } = send(role, row);
          assert.ok(status !== 401 && status !== 403 && status < 500, `${role} ${row[0]} ${row[1]} answered ${status}`);
        }
      }
    });
  });
}
'''

CUSTOMER_SCOPE = r'''
let world: World;
let other: { customer: any; site: any };
let mine: string;
let theirs: string;
let ownJob: any;
let foreignJob: any;

beforeEach(() => {
  world = createWorld();
  other = seedCustomer(world.app, { name: 'Brightwater Dental' });
  mine = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
  theirs = call(world.app, 'POST', `/customers/${other.customer.id}/portal-users`, 'admin', { name: 'Sam Rivera' }).id;
  ownJob = openWorkOrder(world.app, world.customer.id, world.site.id);
  foreignJob = openWorkOrder(world.app, other.customer.id, other.site.id);
});

function invoiceFor(customerId: string, siteId: string, issue = true) {
  const job = openWorkOrder(world.app, customerId, siteId);
  call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
  call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
  completeChecklist(world.app, job.id, world.technician.id);
  call(world.app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Replaced the pressure valve' });
  const draft = call(world.app, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
  return issue ? call(world.app, 'POST', `/invoices/${draft.id}/issue`, 'billing') : draft;
}

describe('work orders', () => {
  it('lists only the own work orders, whatever the filter says', () => {
    for (const query of ['', `?customerId=${other.customer.id}`, `?customerId=${world.customer.id}`]) {
      const { items } = expectStatus(world.app, 200, 'GET', `${JOBS}${query}`, mine);
      assert.deepEqual(items.map((item: any) => item.id), [ownJob.id], query);
    }
    assert.equal(expectStatus(world.app, 200, 'GET', JOBS, 'admin').total, 2);
  });

  it('shows the own work order and hides foreign ones as missing', () => {
    expectStatus(world.app, 200, 'GET', `${JOBS}/${ownJob.id}`, mine);
    const hidden = expectStatus(world.app, 404, 'GET', `${JOBS}/${foreignJob.id}`, mine);
    const absent = expectStatus(world.app, 404, 'GET', `${JOBS}/WO-909090`, mine);
    assert.equal(hidden.error.code, absent.error.code);
    assert.deepEqual(hidden.error.details, { entity: 'work order', id: foreignJob.id });
  });

  it('hides every sub-resource of foreign work orders', () => {
    for (const suffix of ['timeline', 'sla', 'reservations', 'notifications']) {
      assert.equal(expectStatus(world.app, 404, 'GET', `${JOBS}/${foreignJob.id}/${suffix}`, mine).error.code, 'not-found', suffix);
    }
    expectStatus(world.app, 404, 'POST', `${JOBS}/${foreignJob.id}/notes`, mine, { text: 'Please hurry' });
  });

  it('only opens work orders for the own account', () => {
    const body = expectStatus(world.app, 403, 'POST', JOBS, mine, {
      customerId: other.customer.id,
      siteId: other.site.id,
      workType: 'general',
      summary: 'Door will not close',
    });
    assert.equal(body.error.code, 'forbidden');
    const opened = expectStatus(world.app, 201, 'POST', JOBS, mine, {
      customerId: world.customer.id,
      siteId: world.site.id,
      workType: 'general',
      summary: 'Door will not close',
    });
    assert.equal(opened.source, 'portal');
  });

  it('shows notes shared with the customer but not internal ones', () => {
    call(world.app, 'POST', `${JOBS}/${ownJob.id}/notes`, 'dispatch', { text: 'Key is under the mat' });
    call(world.app, 'POST', `${JOBS}/${ownJob.id}/notes`, 'dispatch', { text: 'Technician will call ahead', visibility: 'customer' });
    call(world.app, 'POST', `${JOBS}/${ownJob.id}/notes`, mine, { text: 'Gate code is 4711', visibility: 'internal' });
    const customerView = call(world.app, 'GET', `${JOBS}/${ownJob.id}`, mine);
    assert.deepEqual(customerView.notes.map((note: any) => note.text), ['Technician will call ahead', 'Gate code is 4711']);
    assert.equal(customerView.tags, undefined);
    assert.equal(call(world.app, 'GET', `${JOBS}/${ownJob.id}`, 'dispatch').notes.length, 3);
  });
});

describe('customer records', () => {
  it('shows the own account and refuses the others', () => {
    expectStatus(world.app, 200, 'GET', `/customers/${world.customer.id}`, mine);
    expectStatus(world.app, 200, 'GET', `/customers/${world.customer.id}/sites`, mine);
    assert.equal(expectStatus(world.app, 403, 'GET', `/customers/${other.customer.id}`, mine).error.code, 'forbidden');
    expectStatus(world.app, 403, 'GET', `/customers/${other.customer.id}/sites`, mine);
    expectStatus(world.app, 403, 'GET', `/customers/${world.customer.id}`, theirs);
  });

  it('keeps portal users away from the customer directory', () => {
    expectStatus(world.app, 403, 'GET', '/customers', mine);
    expectStatus(world.app, 403, 'GET', `/customers/${world.customer.id}/contracts`, mine);
    expectStatus(world.app, 403, 'PATCH', `/customers/${world.customer.id}`, mine, { name: 'Renamed' });
  });
});

describe('billing', () => {
  it('limits statements and invoice lists to the own account', () => {
    for (const path of ['statement', 'invoices']) {
      expectStatus(world.app, 200, 'GET', `/customers/${world.customer.id}/${path}`, mine);
      expectStatus(world.app, 403, 'GET', `/customers/${other.customer.id}/${path}`, mine);
    }
  });

  it('limits invoices to the own account and hides drafts', () => {
    const issued = invoiceFor(world.customer.id, world.site.id);
    const foreign = invoiceFor(other.customer.id, other.site.id);
    const draft = invoiceFor(world.customer.id, world.site.id, false);
    expectStatus(world.app, 200, 'GET', `/invoices/${issued.id}`, mine);
    expectStatus(world.app, 403, 'GET', `/invoices/${foreign.id}`, mine);
    assert.equal(expectStatus(world.app, 404, 'GET', `/invoices/${draft.id}`, mine).error.code, 'not-found');
    expectStatus(world.app, 200, 'GET', `/invoices/${draft.id}`, 'billing');
    const listed = call(world.app, 'GET', `/customers/${world.customer.id}/invoices`, mine).items;
    assert.deepEqual(listed.map((invoice: any) => invoice.id), [issued.id]);
  });

  it('limits late fees to the own account', () => {
    const issued = invoiceFor(world.customer.id, world.site.id);
    const foreign = invoiceFor(other.customer.id, other.site.id);
    world.app.clock.set(at('2026-05-04', '10:00'));
    const fee = expectStatus(world.app, 200, 'GET', `/invoices/${issued.id}/late-fee`, mine);
    assert.equal(typeof fee.lateFeeCents, 'number');
    assert.ok(fee.lateFeeCents > 0);
    assert.equal(expectStatus(world.app, 403, 'GET', `/invoices/${foreign.id}/late-fee`, mine).error.code, 'forbidden');
    assert.ok(expectStatus(world.app, 200, 'GET', `/invoices/${foreign.id}/late-fee`, 'billing').lateFeeCents > 0);
  });

  it('lists only the own overdue invoices', () => {
    const issued = invoiceFor(world.customer.id, world.site.id);
    const foreign = invoiceFor(other.customer.id, other.site.id);
    world.app.clock.set(at('2026-05-04', '10:00'));
    const everything = call(world.app, 'GET', '/invoices/overdue', 'billing').items;
    assert.deepEqual(everything.map((entry: any) => entry.customerId).sort(), [world.customer.id, other.customer.id].sort());
    const own = call(world.app, 'GET', '/invoices/overdue', mine).items;
    assert.deepEqual(own.map((entry: any) => entry.customerId), [world.customer.id]);
    assert.deepEqual(own.map((entry: any) => entry.invoiceId), [issued.id]);
    assert.deepEqual(call(world.app, 'GET', '/invoices/overdue', theirs).items.map((entry: any) => entry.invoiceId), [foreign.id]);
  });
});

describe('quotes', () => {
  const quoteFor = (customer: any, site: any) => {
    const draft = call(world.app, 'POST', '/quotes', 'dispatch', { customerId: customer.id, siteId: site.id, workType: 'general' });
    return call(world.app, 'POST', `/quotes/${draft.id}/send`, 'dispatch');
  };

  it('lets customers answer only their own quotes', () => {
    const foreign = quoteFor(other.customer, other.site);
    assert.equal(expectStatus(world.app, 403, 'POST', `/quotes/${foreign.id}/accept`, mine).error.code, 'forbidden');
    expectStatus(world.app, 403, 'POST', `/quotes/${foreign.id}/decline`, mine, { reason: 'Too expensive for us' });
    assert.equal(call(world.app, 'GET', `/quotes/${foreign.id}`, 'dispatch').status, 'sent');
    const own = quoteFor(world.customer, world.site);
    const accepted = expectStatus(world.app, 200, 'POST', `/quotes/${own.id}/accept`, mine);
    assert.equal(accepted.quote.status, 'accepted');
    expectStatus(world.app, 200, 'GET', `${JOBS}/${accepted.jobId}`, mine);
    expectStatus(world.app, 404, 'GET', `${JOBS}/${accepted.jobId}`, theirs);
  });

  it('keeps quote drafting and lookups with staff', () => {
    const quote = quoteFor(world.customer, world.site);
    expectStatus(world.app, 403, 'GET', `/quotes/${quote.id}`, mine);
    expectStatus(world.app, 403, 'GET', `/customers/${world.customer.id}/quotes`, mine);
    expectStatus(world.app, 403, 'POST', '/quotes', mine, { customerId: world.customer.id, siteId: world.site.id, workType: 'general' });
  });
});
'''

TECHNICIAN_SCOPE = r'''
let world: World;
let colleague: any;
let mine: any;
let theirs: any;
let unassigned: any;

beforeEach(() => {
  world = createWorld();
  colleague = seedTechnician(world.app, { name: 'Casey Lin', skills: ['gas-safe', 'plumbing', 'electrical'] });
  const open = () => openWorkOrder(world.app, world.customer.id, world.site.id);
  mine = open();
  theirs = open();
  unassigned = open();
  call(world.app, 'POST', `${JOBS}/${mine.id}/assign`, 'dispatch', { technicianId: world.technician.id });
  call(world.app, 'POST', `${JOBS}/${theirs.id}/assign`, 'dispatch', { technicianId: colleague.id });
});

const me = () => world.technician.id as string;

describe('work orders', () => {
  it('lists only the assigned work orders, whatever the filter says', () => {
    for (const query of ['', `?technicianId=${colleague.id}`]) {
      const { items } = expectStatus(world.app, 200, 'GET', `${JOBS}${query}`, me());
      assert.deepEqual(items.map((item: any) => item.id), [mine.id], query);
    }
    assert.equal(expectStatus(world.app, 200, 'GET', JOBS, colleague.id).total, 1);
  });

  it('hides work orders of colleagues and unassigned work', () => {
    expectStatus(world.app, 200, 'GET', `${JOBS}/${mine.id}`, me());
    for (const hidden of [theirs, unassigned]) {
      assert.deepEqual(expectStatus(world.app, 404, 'GET', `${JOBS}/${hidden.id}`, me()).error.details, { entity: 'work order', id: hidden.id });
      expectStatus(world.app, 404, 'GET', `${JOBS}/${hidden.id}/timeline`, me());
    }
  });

  it('cannot work on work orders of colleagues', () => {
    const attempts: Array<[Method, string, unknown]> = [
      ['POST', 'start', undefined],
      ['POST', 'hold', { reason: 'awaiting_parts' }],
      ['PUT', 'checklist', { item: 'any', done: true }],
      ['POST', 'complete', { resolution: 'Done already' }],
    ];
    for (const [method, action, body] of attempts) {
      assert.equal(expectStatus(world.app, 404, method, `${JOBS}/${theirs.id}/${action}`, me(), body).error.code, 'not-found', action);
    }
    assert.equal(call(world.app, 'GET', `${JOBS}/${theirs.id}`, 'dispatch').status, 'assigned');
  });

  it('works on assigned work orders', () => {
    assert.equal(expectStatus(world.app, 200, 'POST', `${JOBS}/${mine.id}/start`, me()).status, 'in_progress');
    assert.equal(expectStatus(world.app, 200, 'POST', `${JOBS}/${mine.id}/hold`, me(), { reason: 'awaiting_parts' }).status, 'on_hold');
    assert.equal(expectStatus(world.app, 200, 'POST', `${JOBS}/${mine.id}/resume`, me(), {}).status, 'in_progress');
  });

  it('loses sight of work orders that are reassigned', () => {
    call(world.app, 'POST', `${JOBS}/${mine.id}/assign`, 'dispatch', { technicianId: colleague.id });
    expectStatus(world.app, 404, 'GET', `${JOBS}/${mine.id}`, me());
    expectStatus(world.app, 200, 'GET', `${JOBS}/${mine.id}`, colleague.id);
  });
});

describe('technician records', () => {
  it('shows a route only to its owner and to staff', () => {
    const url = (id: string) => `/technicians/${id}/route?date=${MONDAY}`;
    expectStatus(world.app, 200, 'GET', url(me()), me());
    assert.equal(expectStatus(world.app, 403, 'GET', url(colleague.id), me()).error.code, 'forbidden');
    for (const actor of ['admin', 'dispatch']) expectStatus(world.app, 200, 'GET', url(colleague.id), actor);
  });

  it('shows timesheets to the owner and approvers only', () => {
    const url = (id: string) => `/technicians/${id}/timesheet?week=${MONDAY}`;
    expectStatus(world.app, 200, 'GET', url(me()), me());
    expectStatus(world.app, 403, 'GET', url(colleague.id), me());
    for (const actor of ['admin', 'dispatch']) expectStatus(world.app, 200, 'GET', url(me()), actor);
    for (const actor of ['billing', 'warehouse']) expectStatus(world.app, 403, 'GET', url(me()), actor);
  });

  it('keeps mileage claims with approvers', () => {
    const url = `/technicians/${me()}/mileage?from=${MONDAY}&to=2026-03-09`;
    expectStatus(world.app, 403, 'GET', url, me());
    expectStatus(world.app, 200, 'GET', url, 'dispatch');
  });
});

describe('time recording', () => {
  const entry = (technicianId: string) => ({ technicianId, kind: 'labor', jobId: mine.id, start: at(MONDAY, '09:00'), end: at(MONDAY, '10:00') });

  it('books time for oneself only', () => {
    expectStatus(world.app, 201, 'POST', '/time-entries', me(), entry(me()));
    assert.equal(expectStatus(world.app, 403, 'POST', '/time-entries', me(), entry(colleague.id)).error.code, 'forbidden');
  });

  it('lets administrators book time for anyone', () => {
    expectStatus(world.app, 201, 'POST', '/time-entries', 'admin', entry(colleague.id));
  });

  it('clocks in for oneself only', () => {
    expectStatus(world.app, 403, 'POST', '/clock/in', me(), { technicianId: colleague.id, kind: 'admin' });
    const started = expectStatus(world.app, 201, 'POST', '/clock/in', me(), { technicianId: me(), kind: 'admin' });
    assert.equal(started.technicianId, me());
  });

  it('clocks out only entries of its own', () => {
    const started = call(world.app, 'POST', '/clock/in', colleague.id, { technicianId: colleague.id, kind: 'admin' });
    assert.equal(expectStatus(world.app, 403, 'POST', '/clock/out', me(), { entryId: started.id }).error.code, 'forbidden');
    world.app.clock.set(at(MONDAY, '09:30'));
    expectStatus(world.app, 200, 'POST', '/clock/out', colleague.id, { entryId: started.id });
  });

  it('cannot approve time', () => {
    const logged = call(world.app, 'POST', '/time-entries', me(), entry(me()));
    expectStatus(world.app, 403, 'POST', '/time-entries/approve', me(), { entryIds: [logged.id] });
    expectStatus(world.app, 200, 'POST', '/time-entries/approve', 'dispatch', { entryIds: [logged.id] });
  });
});

describe('everything else', () => {
  it('keeps customers, invoices and quotes out of reach', () => {
    expectStatus(world.app, 403, 'GET', `/customers/${world.customer.id}`, me());
    expectStatus(world.app, 403, 'GET', `/customers/${world.customer.id}/sites`, me());
    expectStatus(world.app, 403, 'GET', '/invoices/overdue', me());
    expectStatus(world.app, 403, 'GET', '/quotes/QUO-000001', me());
  });

  it('cannot see appointments of colleagues', () => {
    const booked = call(world.app, 'POST', '/appointments', 'dispatch', { jobId: theirs.id, technicianId: colleague.id, start: at(MONDAY, '10:00') });
    expectStatus(world.app, 404, 'POST', `/appointments/${booked.id}/no-access`, me(), { reason: 'Nobody home' });
    expectStatus(world.app, 404, 'POST', `/appointments/${booked.id}/confirm`, me());
    expectStatus(world.app, 200, 'POST', `/appointments/${booked.id}/confirm`, colleague.id);
  });
});
'''

SYSTEM = r'''
let world: World;
let portal: string;

beforeEach(() => {
  world = createWorld();
  portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
});

const everyone = () => ['admin', 'dispatch', 'billing', 'warehouse', world.technician.id as string, portal];

describe('GET /health', () => {
  it('answers every authenticated actor', () => {
    for (const actor of everyone()) {
      assert.deepEqual(expectStatus(world.app, 200, 'GET', '/health', actor), {
        status: 'ok',
        time: at(MONDAY, '08:00'),
        organization: world.app.ctx.config.organization.name,
      });
    }
  });

  it('follows the clock', () => {
    world.app.clock.set(at('2026-03-05', '15:30'));
    assert.equal(call(world.app, 'GET', '/health', 'admin').time, at('2026-03-05', '15:30'));
  });
});

describe('GET /config', () => {
  it('shows the effective configuration to administrators', () => {
    const config = expectStatus(world.app, 200, 'GET', '/config', 'admin');
    assert.deepEqual(config.scheduling.workingDays, world.app.ctx.config.scheduling.workingDays);
    assert.equal(config.billing.invoicePrefix, world.app.ctx.config.billing.invoicePrefix);
    assert.deepEqual(config.holidays, world.app.ctx.config.holidays);
  });

  it('reflects configuration overrides', () => {
    const custom = createWorld({ config: { organization: { name: 'Harbour Service Desk' } } });
    assert.equal(call(custom.app, 'GET', '/config', 'admin').organization.name, 'Harbour Service Desk');
    assert.equal(call(custom.app, 'GET', '/health', 'admin').organization, 'Harbour Service Desk');
  });

  it('is reserved for administrators', () => {
    for (const actor of everyone().slice(1)) assert.equal(expectStatus(world.app, 403, 'GET', '/config', actor).error.code, 'forbidden');
  });

  it('hands out a copy', () => {
    const config = call(world.app, 'GET', '/config', 'admin');
    config.billing.invoicePrefix = 'XXX';
    assert.notEqual(call(world.app, 'GET', '/config', 'admin').billing.invoicePrefix, 'XXX');
  });
});

describe('GET /work-types', () => {
  it('lists the registered work types for everyone with read access', () => {
    const keys = world.app.ctx.workTypes.keys();
    for (const actor of everyone()) {
      const { items } = expectStatus(world.app, 200, 'GET', '/work-types', actor);
      assert.deepEqual(items.map((item: any) => item.key), keys);
    }
  });

  it('describes each work type', () => {
    const { items } = call(world.app, 'GET', '/work-types', 'admin');
    const general = items.find((item: any) => item.key === 'general');
    assert.equal(typeof general.label, 'string');
    assert.ok(Array.isArray(general.skills));
    assert.equal(general.requiresAsset, false);
    assert.ok(items.every((item: any) => item.defaultDurationMinutes > 0 && typeof item.defaultPriority === 'string'));
  });
});

describe('GET /audit/:subject', () => {
  it('shows what happened to a record', () => {
    const trail = expectStatus(world.app, 200, 'GET', `/audit/${world.customer.id}`, 'admin').items;
    assert.equal(trail.length, 1);
    assert.equal(trail[0].action, 'customer.created');
    assert.equal(trail[0].actorId, 'admin');
    assert.equal(trail[0].subject, world.customer.id);
  });

  it('is empty for unknown subjects', () => {
    assert.deepEqual(call(world.app, 'GET', '/audit/NOBODY', 'admin').items, []);
  });

  it('is reserved for administrators', () => {
    for (const actor of everyone().slice(1)) expectStatus(world.app, 403, 'GET', `/audit/${world.customer.id}`, actor);
  });
});

describe('route table', () => {
  const routes = () => world.app.routes();

  it('lists every route once, ordered by pattern and method', () => {
    const keys = routes().map((route) => `${route.method} ${route.pattern}`);
    assert.equal(new Set(keys).size, keys.length);
    const sorted = [...routes()].sort((left, right) => left.pattern.localeCompare(right.pattern) || left.method.localeCompare(right.method));
    assert.deepEqual(routes(), sorted);
  });

  it('contains only absolute patterns with known methods', () => {
    for (const route of routes()) {
      assert.ok(route.pattern.startsWith('/'), route.pattern);
      assert.ok(['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].includes(route.method), route.method);
    }
  });

  it('answers every GET route without server errors', () => {
    for (const route of routes().filter((candidate) => candidate.method === 'GET')) {
      const url = route.pattern.replace(/:[a-zA-Z]+/g, 'MISSING-1');
      const response = world.app.request('GET', url, { actor: 'admin' });
      assert.ok(response.status < 500 && response.status !== 405, `GET ${url} answered ${response.status}`);
      if (response.status === 404) assert.notEqual((response.body as any).error.code, 'route-not-found', url);
    }
  });

  it('never routes a literal segment into a parameter', () => {
    for (const [url, pattern] of [
      ['/invoices/overdue', '/invoices/overdue'],
      ['/assets/due', '/assets/due'],
      ['/time-entries/pending', '/time-entries/pending'],
      ['/technicians/certifications/expiring', '/technicians/certifications/expiring'],
    ]) {
      assert.ok(routes().some((route) => route.method === 'GET' && route.pattern === pattern), pattern);
      assert.notEqual(world.app.request('GET', url, { actor: 'admin' }).status, 404, url);
    }
  });
});
'''

ERRORS = r'''
let world: World;

beforeEach(() => {
  world = createWorld();
});

describe('validation errors', () => {
  it('list every offending field', () => {
    const body = expectStatus(world.app, 422, 'POST', '/customers', 'admin', {});
    assert.equal(body.error.code, 'invalid-input');
    const issues: Array<{ path: string; message: string }> = body.error.details.issues;
    assert.ok(issues.length >= 2);
    assert.ok(issues.every((issue) => typeof issue.path === 'string' && typeof issue.message === 'string'));
    assert.equal(body.error.message, issues.map((issue) => `${issue.path || 'input'}: ${issue.message}`).join('; '));
  });

  it('name the parameter in query errors', () => {
    const body = expectStatus(world.app, 422, 'GET', `${JOBS}?status=bogus,new`, 'admin');
    assert.deepEqual(body.error.details.issues, [{ path: 'status', message: 'unknown status bogus' }]);
  });

  it('reject malformed paging', () => {
    for (const query of ['page=0', 'page=abc', 'pageSize=0', 'pageSize=201', 'page=10001', 'pageSize=1.5']) {
      const body = expectStatus(world.app, 422, 'GET', `${JOBS}?${query}`, 'admin');
      assert.equal(body.error.code, 'invalid-input', query);
    }
  });
});

describe('not found errors', () => {
  it('name the entity and the identifier', () => {
    const cases: Array<[string, string]> = [
      ['/customers/MISSING-1', 'customer'],
      [`${JOBS}/MISSING-1`, 'work order'],
      ['/invoices/MISSING-1', 'invoice'],
      ['/quotes/MISSING-1', 'quote'],
      [`/technicians/MISSING-1/load?date=${MONDAY}`, 'technician'],
    ];
    for (const [url, entity] of cases) {
      const body = expectStatus(world.app, 404, 'GET', url, 'admin');
      assert.deepEqual(body.error, { code: 'not-found', message: `${entity} MISSING-1 was not found`, details: { entity, id: 'MISSING-1' } }, url);
    }
  });
});

describe('conflict errors', () => {
  it('carry their own code', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id);
    call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    const body = expectStatus(world.app, 409, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Finished early' });
    assert.equal(body.error.code, 'not-in-progress');
    assert.equal(body.error.details, undefined);
  });

  it('include structured details when there are some', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id);
    call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    const body = expectStatus(world.app, 409, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Finished early' });
    assert.equal(body.error.code, 'checklist-incomplete');
    assert.deepEqual(body.error.details.open, Object.keys(call(world.app, 'GET', `${JOBS}/${job.id}`, 'dispatch').checklist));
  });
});

describe('forbidden errors', () => {
  it('name the missing permission and the actor', () => {
    const body = expectStatus(world.app, 403, 'POST', '/customers', 'dispatch', {});
    assert.deepEqual(body.error, {
      code: 'forbidden',
      message: 'dispatch may not customers:write',
      details: { action: 'customers:write', actorId: 'dispatch' },
    });
  });
});

describe('isolation', () => {
  it('returns copies of stored records', () => {
    const first = call(world.app, 'GET', `/customers/${world.customer.id}`, 'admin');
    first.name = 'Tampered';
    first.sites.length = 0;
    const second = call(world.app, 'GET', `/customers/${world.customer.id}`, 'admin');
    assert.notEqual(second.name, 'Tampered');
    assert.equal(second.sites.length, 1);
  });

  it('does not keep references to request bodies', () => {
    const payload = { customerId: world.customer.id, siteId: world.site.id, workType: 'general', summary: 'Door will not close', tags: ['keys'] };
    const created = call(world.app, 'POST', JOBS, 'dispatch', payload);
    payload.tags.push('mutated');
    payload.summary = 'Changed afterwards';
    const stored = call(world.app, 'GET', `${JOBS}/${created.id}`, 'dispatch');
    assert.equal(stored.summary, 'Door will not close');
    assert.deepEqual(stored.tags, ['keys']);
  });
});

describe('unexpected failures', () => {
  it('are reported without internals and logged', () => {
    world.app.ctx.bus.on(«JobEvents».opened, () => {
      throw new Error('disk quota exceeded at /var/lib/fieldline');
    });
    const response = world.app.request('POST', JOBS, {
      actor: 'dispatch',
      body: { customerId: world.customer.id, siteId: world.site.id, workType: 'general', summary: 'Door will not close' },
    });
    assert.equal(response.status, 500);
    assert.deepEqual(response.body, { error: { code: 'internal', message: 'Internal error' } });
    assert.ok(world.app.ctx.log.records('error').some((record) => JSON.stringify(record).includes('disk quota exceeded')));
  });
});
'''

FILES = {
    "tests/api/security/authentication.test.ts": Spec(
        {WORLD: ["call", "createWorld", "expectStatus", "JOBS", "seedTechnician", "type World"], HTTP: ["type Method"]},
        AUTHENTICATION, node=WITH_SETUP),
    "tests/api/security/permissions.test.ts": Spec(
        {WORLD: ["call", "createWorld", "JOBS", "type World"], HTTP: ["type Method"]}, PERMISSIONS, node=WITH_SETUP),
    "tests/api/security/customer-scope.test.ts": Spec(
        {WORLD: ["at", "call", "completeChecklist", "createWorld", "expectStatus", "JOBS", "openWorkOrder", "seedCustomer",
                 "type World"]}, CUSTOMER_SCOPE, node=WITH_SETUP),
    "tests/api/security/technician-scope.test.ts": Spec(
        {WORLD: ["at", "call", "createWorld", "expectStatus", "JOBS", "MONDAY", "openWorkOrder", "seedTechnician",
                 "type World"], HTTP: ["type Method"]}, TECHNICIAN_SCOPE, node=WITH_SETUP),
    "tests/api/security/system.test.ts": Spec(
        {WORLD: ["at", "call", "createWorld", "expectStatus", "MONDAY", "type World"]}, SYSTEM, node=WITH_SETUP),
    "tests/api/security/errors.test.ts": Spec(
        {WORLD: ["call", "createWorld", "expectStatus", "JOBS", "MONDAY", "openWorkOrder", "type World"]},
        ERRORS, jobs=("JobEvents",), node=WITH_SETUP),
}


def generate(ctx):
    register(ctx, FILES)
