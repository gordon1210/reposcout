"""Paged list endpoints: a shared list helper, paged route responses, a paged statement, tests and docs."""

VARIANT = "holdout-c-e"

LISTING = r'''
import type { Page } from '../shared/paging.ts';
import { paginate, readPageRequest } from '../shared/paging.ts';
import type { PageRequest } from '../shared/paging.ts';
import type { ApiRequest } from './http.ts';

/** The page window a list request asks for: `page` and `pageSize` from the query string. */
export function listRequest(request: ApiRequest): PageRequest {
  return readPageRequest(request.query);
}

/**
 * Cuts an already filtered and ordered list down to the requested page. `view` maps the items of
 * that page only, so expensive serialisation never runs for the rest of the list.
 */
export function listResponse<T>(request: ApiRequest, items: readonly T[], view?: (item: T) => unknown): Page<unknown> {
  const page = paginate(items, listRequest(request));
  return { ...page, items: view === undefined ? page.items : page.items.map(view) };
}
'''

LISTING_TEST = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { listRequest, listResponse } from '../../src/api/listing.ts';
import type { ApiRequest } from '../../src/api/http.ts';
import { DEFAULT_PAGE_SIZE } from '../../src/shared/paging.ts';

const requestWith = (query: Record<string, string>): ApiRequest => ({
  method: 'GET',
  path: '/things',
  params: {},
  query,
  body: undefined,
  actor: { id: 'admin', name: 'Admin', role: 'admin' },
});

const numbers = (count: number) => Array.from({ length: count }, (_, index) => index + 1);

describe('listRequest', () => {
  it('starts on the first page with the default size', () => {
    assert.deepEqual(listRequest(requestWith({})), { page: 1, pageSize: DEFAULT_PAGE_SIZE });
  });

  it('reads page and pageSize from the query', () => {
    assert.deepEqual(listRequest(requestWith({ page: '3', pageSize: '10' })), { page: 3, pageSize: 10 });
  });

  it('rejects values outside the allowed range', () => {
    for (const query of [{ page: '0' }, { pageSize: '0' }, { pageSize: '201' }, { page: 'x' }]) {
      assert.throws(() => listRequest(requestWith(query)), (error: any) => error.code === 'invalid-input');
    }
  });
});

describe('listResponse', () => {
  it('returns every item of a short list on one page', () => {
    const page = listResponse(requestWith({}), numbers(7));
    assert.deepEqual(page.items, numbers(7));
    assert.deepEqual([page.page, page.pageSize, page.total, page.pages], [1, 25, 7, 1]);
  });

  it('cuts a long list at the default page size', () => {
    const page = listResponse(requestWith({}), numbers(60));
    assert.equal(page.items.length, 25);
    assert.deepEqual([page.total, page.pages], [60, 3]);
  });

  it('maps only the items of the requested page', () => {
    const seen: number[] = [];
    const page = listResponse(requestWith({ pageSize: '3', page: '2' }), numbers(8), (item) => {
      seen.push(item);
      return item * 10;
    });
    assert.deepEqual(seen, [4, 5, 6]);
    assert.deepEqual(page.items, [40, 50, 60]);
    assert.equal(page.pages, 3);
  });

  it('answers an empty page past the end', () => {
    const page = listResponse(requestWith({ page: '9', pageSize: '5' }), numbers(8));
    assert.deepEqual(page.items, []);
    assert.deepEqual([page.page, page.total, page.pages], [9, 8, 2]);
  });

  it('reports one empty page for an empty list', () => {
    const page = listResponse(requestWith({}), []);
    assert.deepEqual([page.items.length, page.total, page.pages], [0, 0, 1]);
  });
});
'''

STATEMENT = r'''
import { invoicesForCustomer } from '../adapters/repositories/invoices.ts';
import type { AppContext } from '../context.ts';
import type { Cents } from '../shared/money.ts';
import { DEFAULT_PAGE_SIZE, paginate } from '../shared/paging.ts';
import type { PageRequest } from '../shared/paging.ts';
import { daysBetween } from '../shared/time.ts';
import type { CalendarDate } from '../shared/time.ts';
import { outstandingCents } from './model.ts';
import { creditedCents } from './payments.ts';

export type AgingBucket = 'current' | '1-30' | '31-60' | '61-90' | '90+';

export function agingBucket(daysOverdue: number): AgingBucket {
  if (daysOverdue <= 0) return 'current';
  if (daysOverdue <= 30) return '1-30';
  if (daysOverdue <= 60) return '31-60';
  if (daysOverdue <= 90) return '61-90';
  return '90+';
}

export interface StatementLine {
  invoiceId: string;
  number?: string;
  issuedOn?: CalendarDate;
  dueOn?: CalendarDate;
  totalCents: Cents;
  outstandingCents: Cents;
  bucket: AgingBucket;
}

export interface StatementPaging {
  page: number;
  pageSize: number;
  total: number;
  pages: number;
}

export interface Statement {
  customerId: string;
  asOf: CalendarDate;
  lines: StatementLine[];
  paging: StatementPaging;
  buckets: Record<AgingBucket, Cents>;
  totalOutstandingCents: Cents;
}

function outstandingLines(ctx: AppContext, customerId: string, asOf: CalendarDate): StatementLine[] {
  return invoicesForCustomer(ctx, customerId)
    .filter((invoice) => invoice.status === 'issued')
    .map((invoice) => {
      const outstanding = outstandingCents(invoice, creditedCents(ctx, invoice.id));
      const overdue = invoice.dueOn === undefined ? 0 : daysBetween(invoice.dueOn, asOf);
      return {
        invoiceId: invoice.id,
        number: invoice.number,
        issuedOn: invoice.issuedOn,
        dueOn: invoice.dueOn,
        totalCents: invoice.totalCents,
        outstandingCents: outstanding,
        bucket: agingBucket(overdue),
      };
    })
    .filter((line) => line.outstandingCents > 0);
}

export function customerStatement(
  ctx: AppContext,
  customerId: string,
  asOf: CalendarDate,
  request: PageRequest = { page: 1, pageSize: DEFAULT_PAGE_SIZE },
): Statement {
  const { items: lines, ...paging } = paginate(outstandingLines(ctx, customerId, asOf), request);
  const buckets: Record<AgingBucket, Cents> = { current: 0, '1-30': 0, '31-60': 0, '61-90': 0, '90+': 0 };
  for (const line of lines) buckets[line.bucket] += line.outstandingCents;
  return {
    customerId,
    asOf,
    lines,
    paging,
    buckets,
    totalOutstandingCents: lines.reduce((sum, line) => sum + line.outstandingCents, 0),
  };
}
'''

STATEMENT_TEST = r'''
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { call, createWorld } from '../support/world.ts';
import { issuedInvoices } from '../support/billing.ts';

describe('statement paging', () => {
  it('lists all outstanding invoices on the first page', () => {
    const world = createWorld();
    const invoices = issuedInvoices(world, 4);
    const statement = call(world.app, 'GET', `/customers/${world.customer.id}/statement`, 'billing');
    assert.deepEqual(statement.lines.map((line: any) => line.invoiceId), invoices.map((invoice) => invoice.id));
    assert.deepEqual(statement.paging, { page: 1, pageSize: 25, total: 4, pages: 1 });
    assert.equal(statement.totalOutstandingCents, invoices.reduce((sum, invoice) => sum + invoice.totalCents, 0));
    assert.equal(statement.buckets.current, statement.totalOutstandingCents);
  });

  it('cuts the lines at the requested page size', () => {
    const world = createWorld();
    const invoices = issuedInvoices(world, 5);
    const second = call(world.app, 'GET', `/customers/${world.customer.id}/statement?pageSize=2&page=2`, 'billing');
    assert.deepEqual(second.lines.map((line: any) => line.invoiceId), [invoices[2].id, invoices[3].id]);
    assert.deepEqual(second.paging, { page: 2, pageSize: 2, total: 5, pages: 3 });
  });

  it('answers an empty page past the last page', () => {
    const world = createWorld();
    issuedInvoices(world, 3);
    const beyond = call(world.app, 'GET', `/customers/${world.customer.id}/statement?pageSize=2&page=7`, 'billing');
    assert.deepEqual(beyond.lines, []);
    assert.deepEqual(beyond.paging, { page: 7, pageSize: 2, total: 3, pages: 2 });
  });

  it('keeps the order of the invoices across pages', () => {
    const world = createWorld();
    const invoices = issuedInvoices(world, 5);
    const seen: string[] = [];
    for (const page of [1, 2, 3]) {
      const body = call(world.app, 'GET', `/customers/${world.customer.id}/statement?pageSize=2&page=${page}`, 'billing');
      seen.push(...body.lines.map((line: any) => line.invoiceId));
    }
    assert.deepEqual(seen, invoices.map((invoice) => invoice.id));
  });

  it('rejects page sizes outside the allowed range', () => {
    const world = createWorld();
    for (const query of ['pageSize=0', 'pageSize=201', 'page=0']) {
      const response = world.app.request('GET', `/customers/${world.customer.id}/statement?${query}`, { actor: 'billing' });
      assert.equal(response.status, 422, query);
    }
  });

  it('keeps customers on their own statement', () => {
    const world = createWorld();
    issuedInvoices(world, 2);
    const portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
    assert.equal(call(world.app, 'GET', `/customers/${world.customer.id}/statement?pageSize=1`, portal).paging.total, 2);
    assert.equal(world.app.request('GET', '/customers/CUS-999999/statement', { actor: portal }).status, 403);
  });
});
'''

BILLING_SUPPORT = r'''
import type { App } from '../../src/app.ts';
import { call, completeChecklist, JOBS, openWorkOrder } from './world.ts';
import type { World } from './world.ts';

/** Completes one general work order for the world's customer and returns its drafted invoice. */
export function draftedInvoice(world: World) {
  const { app, customer, site, technician } = world;
  const job = openWorkOrder(app, customer.id, site.id);
  call(app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: technician.id });
  call(app, 'POST', `${JOBS}/${job.id}/start`, technician.id);
  completeChecklist(app, job.id, technician.id);
  call(app, 'POST', `${JOBS}/${job.id}/complete`, technician.id, {
    resolution: 'Replaced the faulty component and tested',
    signatureName: 'Avery Collins',
  });
  return call(app, 'POST', `${JOBS}/${job.id}/invoice`, 'billing');
}

/** Issues `count` invoices, one per completed work order, and returns them in issue order. */
export function issuedInvoices(world: World, count: number) {
  const issued = [];
  for (let index = 0; index < count; index += 1) {
    issued.push(call(world.app, 'POST', `/invoices/${draftedInvoice(world).id}/issue`, 'billing'));
  }
  return issued;
}

export const invoiceIds = (app: App, customerId: string, actor = 'billing'): string[] =>
  call(app, 'GET', `/customers/${customerId}/invoices?pageSize=200`, actor).items.map((item: any) => item.id);
'''

PAGING_TEST = r'''
import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';
import { §WORLD§ } from '../../support/world.ts';
import type { World } from '../../support/world.ts';
§IMPORTS§

describe('§TITLE§', () => {
  let world: World;
  let url: string;
  let count: number;

  beforeEach(() => {
    world = createWorld();
§SEED§
  });

  const withQuery = (query: string) => `${url}${url.includes('?') ? '&' : '?'}${query}`;
  const fetch = (query = 'page=1', actor = '§ACTOR§') => call(world.app, 'GET', withQuery(query), actor);
  const keys = (page: any): string[] => page.items.map((item: any) => item.§KEY§);

  it('wraps the items in a page of the default size', () => {
    const page = fetch();
    assert.equal(page.page, 1);
    assert.equal(page.pageSize, 25);
    assert.equal(page.total, count);
    assert.equal(page.pages, 1);
    assert.equal(page.items.length, count);
  });

  it('returns the requested slice', () => {
    const all = keys(fetch('pageSize=200'));
    const second = fetch('pageSize=2&page=2');
    assert.deepEqual(keys(second), all.slice(2, 4));
    assert.equal(second.total, count);
    assert.equal(second.pages, Math.ceil(count / 2));
  });

  it('covers every item exactly once across the pages', () => {
    const all = keys(fetch('pageSize=200'));
    const seen: string[] = [];
    for (let page = 1; page <= Math.ceil(count / 2); page += 1) seen.push(...keys(fetch(`pageSize=2&page=${page}`)));
    assert.deepEqual(seen, all);
    assert.equal(new Set(seen).size, count);
  });

  it('answers an empty page past the last page', () => {
    const beyond = fetch('pageSize=2&page=99');
    assert.deepEqual(beyond.items, []);
    assert.equal(beyond.total, count);
    assert.equal(beyond.page, 99);
  });

  it('rejects page sizes and pages outside the allowed range', () => {
    for (const query of ['pageSize=0', 'pageSize=201', 'page=0', 'page=abc']) {
      assert.equal(expectStatus(world.app, 422, 'GET', withQuery(query), '§ACTOR§').error.code, 'invalid-input', query);
    }
  });
§EXTRA§
});
'''

# One entry per paged route: the title, the seed code (sets `url` and `count`), the identifying item key,
# the calling actor, extra imports and further tests.
CASES = {
    "tests/api/paging/customer-invoices.test.ts": {
        "title": "GET /customers/:id/invoices paging",
        "imports": "import { draftedInvoice, issuedInvoices } from '../../support/billing.ts';\n",
        "seed": """    issuedInvoices(world, 5);
    url = `/customers/${world.customer.id}/invoices`;
    count = 5;""",
        "key": "id",
        "actor": "billing",
        "extra": """
  it('counts only the invoices the caller may see', () => {
    draftedInvoice(world);
    const portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
    assert.equal(fetch('page=1', 'billing').total, count + 1);
    const own = fetch('pageSize=2', portal);
    assert.equal(own.total, count);
    assert.equal(own.pages, 3);
    assert.ok(own.items.every((item: any) => item.status !== 'draft'));
  });

  it('serialises the credited amount of each invoice', () => {
    const [first] = fetch('pageSize=1').items;
    call(world.app, 'POST', `/invoices/${first.id}/credit-notes`, 'billing', { amountCents: 500, reason: 'Goodwill reduction' });
    const [again] = fetch('pageSize=1').items;
    assert.equal(again.id, first.id);
    assert.notDeepEqual(again, first);
  });
""",
    },
    "tests/api/paging/overdue-invoices.test.ts": {
        "title": "GET /invoices/overdue paging",
        "imports": "import { issuedInvoices } from '../../support/billing.ts';\n",
        "seed": """    issuedInvoices(world, 5);
    url = '/invoices/overdue?on=2026-06-01';
    count = 5;""",
        "key": "invoiceId",
        "actor": "billing",
        "extra": """
  it('lists nothing before the first due date', () => {
    const early = call(world.app, 'GET', '/invoices/overdue?on=2026-03-10&pageSize=2', 'billing');
    assert.deepEqual([early.items.length, early.total, early.pages], [0, 0, 1]);
  });
""",
    },
    "tests/api/paging/notifications.test.ts": {
        "title": "GET /notifications paging",
        "world": ["openWorkOrder"],
        "imports": "",
        "seed": """    for (let index = 0; index < 5; index += 1) openWorkOrder(world.app, world.customer.id, world.site.id, 'general', { priority: 'emergency' });
    url = '/notifications?recipient=dispatch';
    count = 5;""",
        "key": "id",
        "actor": "admin",
        "extra": """
  it('pages the notifications of the requested recipient only', () => {
    const other = call(world.app, 'GET', '/notifications?recipient=nobody&pageSize=2', 'admin');
    assert.deepEqual([other.items.length, other.total], [0, 0]);
    assert.equal(fetch('pageSize=2').total, count);
  });
""",
    },
    "tests/api/paging/job-notifications.test.ts": {
        "title": "GET /jobs/:id/notifications paging",
        "world": ["JOBS", "openWorkOrder"],
        "imports": "",
        "seed": """    const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general', { priority: 'emergency' });
    call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    url = `${JOBS}/${job.id}/notifications`;
    count = 3;""",
        "key": "id",
        "actor": "dispatch",
        "extra": """
  it('lists the notifications of the work order in creation order', () => {
    const ids = keys(fetch('pageSize=200'));
    assert.deepEqual(ids, [...ids].sort());
  });
""",
    },
    "tests/api/paging/customer-contracts.test.ts": {
        "title": "GET /customers/:id/contracts paging",
        "imports": "",
        "seed": """    for (let index = 0; index < 5; index += 1) {
      call(world.app, 'POST', '/contracts', 'admin', {
        customerId: world.customer.id,
        siteIds: [world.site.id],
        plan: 'basic',
        startsOn: '2026-01-01',
        coveredWorkTypes: ['general'],
      });
    }
    url = `/customers/${world.customer.id}/contracts`;
    count = 5;""",
        "key": "id",
        "actor": "dispatch",
        "extra": """
  it('is limited to the staff who may read customers', () => {
    expectStatus(world.app, 403, 'GET', url, 'warehouse');
    expectStatus(world.app, 403, 'GET', withQuery('pageSize=2'), world.technician.id);
  });
""",
    },
    "tests/api/paging/site-assets.test.ts": {
        "title": "GET /sites/:id/assets paging",
        "world": ["seedAsset"],
        "imports": "",
        "seed": """    for (const kind of ['boiler', 'chiller', 'generator', 'elevator', 'fire-panel']) seedAsset(world.app, world.site.id, kind);
    url = `/sites/${world.site.id}/assets`;
    count = 5;""",
        "key": "id",
        "actor": "dispatch",
        "extra": """
  it('adds the warranty state and next service date to every asset of the page', () => {
    for (const item of fetch('pageSize=2').items) {
      assert.equal(item.warranty, 'none');
      assert.match(item.nextServiceOn, /^\\d{4}-\\d{2}-\\d{2}$/);
    }
  });
""",
    },
    "tests/api/paging/due-assets.test.ts": {
        "title": "GET /assets/due paging",
        "world": ["seedAsset"],
        "imports": "",
        "seed": """    for (const kind of ['boiler', 'chiller', 'generator', 'elevator', 'fire-panel']) seedAsset(world.app, world.site.id, kind);
    url = '/assets/due?on=2027-06-01';
    count = 5;""",
        "key": "id",
        "actor": "dispatch",
        "extra": """
  it('lists the most overdue asset first on the first page', () => {
    const [first] = fetch('pageSize=1').items;
    assert.equal(first.kind, 'elevator');
  });

  it('leaves out assets that are not due yet', () => {
    const early = call(world.app, 'GET', '/assets/due?on=2024-06-20&within=0&pageSize=2', 'dispatch');
    assert.deepEqual([early.items.length, early.total], [0, 0]);
  });
""",
    },
    "tests/api/paging/customer-quotes.test.ts": {
        "title": "GET /customers/:id/quotes paging",
        "imports": "",
        "seed": """    for (let index = 0; index < 5; index += 1) {
      call(world.app, 'POST', '/quotes', 'dispatch', { customerId: world.customer.id, siteId: world.site.id, workType: 'general' });
    }
    url = `/customers/${world.customer.id}/quotes`;
    count = 5;""",
        "key": "id",
        "actor": "dispatch",
        "extra": """
  it('lists open quotes only', () => {
    const [first] = fetch('pageSize=1').items;
    call(world.app, 'POST', `/quotes/${first.id}/send`, 'dispatch');
    call(world.app, 'POST', `/quotes/${first.id}/decline`, 'dispatch', { reason: 'Customer chose another supplier' });
    assert.equal(fetch('pageSize=2').total, count - 1);
  });
""",
    },
    "tests/api/paging/parts.test.ts": {
        "title": "GET /parts paging",
        "imports": "",
        "seed": """    for (const sku of ['PUMP-CIRC', 'VLV-15MM', 'FLT-OIL', 'GSK-20', 'THM-WIFI']) {
      call(world.app, 'POST', '/parts', 'warehouse', {
        id: sku,
        name: `Part ${sku}`,
        category: sku.startsWith('VLV') ? 'plumbing' : 'heating',
        unitCostCents: 1000,
        listPriceCents: 1800,
      });
    }
    url = '/parts';
    count = 5;""",
        "key": "id",
        "actor": "warehouse",
        "extra": """
  it('keeps the category filter in front of the paging', () => {
    const heating = fetch('category=heating&pageSize=2');
    assert.equal(heating.total, 4);
    assert.equal(heating.pages, 2);
    assert.ok(heating.items.every((item: any) => item.category === 'heating'));
  });

  it('orders the parts by sku', () => {
    const ids = keys(fetch('pageSize=200'));
    assert.deepEqual(ids, [...ids].sort());
  });
""",
    },
    "tests/api/paging/customer-sites.test.ts": {
        "title": "GET /customers/:id/sites paging",
        "imports": "",
        "seed": """    for (let index = 2; index <= 5; index += 1) {
      call(world.app, 'POST', `/customers/${world.customer.id}/sites`, 'admin', {
        label: `Annex ${index}`,
        address: { line1: `${index} Quay Street`, city: 'Eastport', postcode: 'EP1 4QS', region: 'metro-east', location: { lat: 52.52, lon: 13.4 } },
      });
    }
    url = `/customers/${world.customer.id}/sites`;
    count = 5;""",
        "key": "id",
        "actor": "dispatch",
        "extra": """
  it('lets the customer page its own sites', () => {
    const portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
    const own = fetch('pageSize=2', portal);
    assert.deepEqual([own.items.length, own.total, own.pages], [2, 5, 3]);
  });
""",
    },
    "tests/api/paging/technicians.test.ts": {
        "title": "GET /technicians paging",
        "world": ["seedTechnician"],
        "imports": "",
        "seed": """    for (let index = 0; index < 4; index += 1) seedTechnician(world.app, { region: index % 2 === 0 ? 'metro-east' : 'metro-west' });
    url = '/technicians';
    count = 5;""",
        "key": "id",
        "actor": "dispatch",
        "extra": """
  it('keeps the region filter in front of the paging', () => {
    const west = fetch('region=metro-west&pageSize=1');
    assert.equal(west.total, 2);
    assert.equal(west.pages, 2);
    assert.ok(west.items.every((item: any) => item.region === 'metro-west'));
  });
""",
    },
    "tests/api/paging/audit-trail.test.ts": {
        "title": "GET /audit/:subject paging",
        "imports": "",
        "seed": """    for (let index = 0; index < 5; index += 1) {
      call(world.app, 'PATCH', `/customers/${world.customer.id}`, 'admin', { name: `Harbour View Apartments ${index}` });
    }
    url = `/audit/${world.customer.id}`;
    count = call(world.app, 'GET', `${url}?pageSize=200`, 'admin').total;""",
        "key": "id",
        "actor": "admin",
        "extra": """
  it('holds one entry for the creation and one for every change', () => {
    assert.equal(count, 6);
    assert.equal(fetch('pageSize=1').items[0].action, 'customer.created');
  });
""",
    },
}

ARCHITECTURE_DOCS = r'''
## Lists

List endpoints answer with a page: `items` plus `page`, `pageSize`, `total` and `pages`. The query
parameters `page` (from 1) and `pageSize` (1 to 200, default 25) select the window. Filtering and
ordering happen before the page is cut, so `total` counts every matching item the caller may see.
A page past the end is empty. Customer statements page their lines in the same way.
'''


def generate(ctx):
    if ctx.variant != VARIANT:
        return
    ctx.add("src/api/listing.ts", LISTING)
    ctx.add("tests/api/listing.test.ts", LISTING_TEST)
    ctx.add("tests/support/billing.ts", BILLING_SUPPORT)
    ctx.add("tests/billing/statement-paging.test.ts", STATEMENT_TEST)
    for path, case in CASES.items():
        ctx.add(path, _paging_test(case))

    ctx.patch("src/shared/paging.ts", "export function readPageRequest(query: Record<string, unknown>, defaultSize = 25): PageRequest {",
              "export const DEFAULT_PAGE_SIZE = 25;\n\nexport function readPageRequest(query: Record<string, unknown>, defaultSize = DEFAULT_PAGE_SIZE): PageRequest {")
    ctx.replace("src/billing/statements.ts", STATEMENT)
    ctx.patch("tests/billing/statements.test.ts", """      lines: [],
      buckets: { current: 0, '1-30': 0, '31-60': 0, '61-90': 0, '90+': 0 },""",
              """      lines: [],
      paging: { page: 1, pageSize: 25, total: 0, pages: 1 },
      buckets: { current: 0, '1-30': 0, '31-60': 0, '61-90': 0, '90+': 0 },""")
    ctx.append("docs/architecture.md", ARCHITECTURE_DOCS)

    _billing_routes(ctx)
    _notification_routes(ctx)
    _simple_routes(ctx)


def _paging_test(case):
    names = sorted({"call", "createWorld", "expectStatus", *case.get("world", ())}, key=str.lower)
    return (PAGING_TEST
            .replace("§WORLD§", ", ".join(names))
            .replace("§IMPORTS§\n", case["imports"])
            .replace("§TITLE§", case["title"])
            .replace("§SEED§", case["seed"])
            .replace("§KEY§", case["key"])
            .replace("§ACTOR§", case["actor"])
            .replace("\n§EXTRA§", case["extra"].rstrip("\n") and "\n" + case["extra"].rstrip("\n")))


def _billing_routes(ctx):
    path = "src/api/routes/billing.ts"
    ctx.patch(path, "import { created, ok, param, queryDate } from '../http.ts';\nimport type { Router } from '../router.ts';",
              "import { created, ok, param, queryDate } from '../http.ts';\nimport { listRequest, listResponse } from '../listing.ts';\nimport type { Router } from '../router.ts';")
    ctx.patch(path, """    return ok({ items });
  });

  router.get('/invoices/:id'""", """    return ok(listResponse(request, items));
  });

  router.get('/invoices/:id'""")
    ctx.patch(path, "return ok(customerStatement(ctx, customerId, queryDate(request, 'asOf', dateOf(ctx.clock.now()))));",
              "return ok(customerStatement(ctx, customerId, queryDate(request, 'asOf', dateOf(ctx.clock.now())), listRequest(request)));")
    ctx.patch(path, "return ok({ items: visible.map((invoice) => invoiceView(invoice, creditedCents(ctx, invoice.id))) });",
              "return ok(listResponse(request, visible, (invoice) => invoiceView(invoice, creditedCents(ctx, invoice.id))));")


def _notification_routes(ctx):
    path = "src/api/routes/notifications.ts"
    ctx.patch(path, "import { ok, optionalQuery, param } from '../http.ts';\nimport type { Router } from '../router.ts';",
              "import { ok, optionalQuery, param } from '../http.ts';\nimport { listResponse } from '../listing.ts';\nimport type { Router } from '../router.ts';")
    ctx.patch(path, "return ok({ items: notificationsFor(ctx, recipientId) });",
              "return ok(listResponse(request, notificationsFor(ctx, recipientId)));")
    ctx.patch(path, "return ok({ items: notificationsForJob(ctx, job.id) });",
              "return ok(listResponse(request, notificationsForJob(ctx, job.id)));")


def _simple_routes(ctx):
    """Routes whose list is the only response and needs a single import and a single call changed."""
    edits = [
        ("src/api/routes/contracts.ts", "import { created, ok, param, queryDate, requiredQuery } from '../http.ts';\n",
         "return ok({ items: contractsForCustomer(ctx, param(request, 'id')) });",
         "return ok(listResponse(request, contractsForCustomer(ctx, param(request, 'id'))));"),
        ("src/api/routes/quotes.ts", None,
         "return ok({ items: openQuotesForCustomer(ctx, param(request, 'id')) });",
         "return ok(listResponse(request, openQuotesForCustomer(ctx, param(request, 'id'))));"),
        ("src/api/routes/customers.ts", "import { created, ok, optionalQuery, param } from '../http.ts';\n",
         "return ok({ items: sitesForCustomer(ctx, customerId) });",
         "return ok(listResponse(request, sitesForCustomer(ctx, customerId)));"),
        ("src/api/routes/inventory.ts", None,
         "return ok({ items: items.sort((left, right) => left.id.localeCompare(right.id)) });",
         "return ok(listResponse(request, items.sort((left, right) => left.id.localeCompare(right.id))));"),
        ("src/api/routes/technicians.ts", None,
         "    return ok({ items });\n  });\n\n  router.put('/technicians/:id/certifications'",
         "    return ok(listResponse(request, items));\n  });\n\n  router.put('/technicians/:id/certifications'"),
        ("src/api/routes/system.ts", None,
         "return ok({ items: auditTrail(ctx, param(request, 'subject')) });",
         "return ok(listResponse(request, auditTrail(ctx, param(request, 'subject'))));"),
        ("src/api/routes/assets.ts", "import { created, ok, param, queryDate, queryInteger } from '../http.ts';\n",
         "    return ok({ items });\n  });\n\n  router.post('/assets/:id/retire'",
         "    return ok(listResponse(request, items));\n  });\n\n  router.post('/assets/:id/retire'"),
        ("src/api/routes/assets.ts", None,
         "return ok({ items: assetsDueForService(ctx, on, within) });",
         "return ok(listResponse(request, assetsDueForService(ctx, on, within)));"),
    ]
    for path, anchor, old, new in edits:
        if "../listing.ts" not in ctx.files[path]:
            anchor = anchor or _http_import(ctx.files[path])
            ctx.patch(path, anchor, anchor + "import { listResponse } from '../listing.ts';\n")
        ctx.patch(path, old, new)


def _http_import(text):
    for line in text.split("\n"):
        if line.startswith("import ") and "from '../http.ts'" in line:
            return line + "\n"
    raise ValueError("no http import")
