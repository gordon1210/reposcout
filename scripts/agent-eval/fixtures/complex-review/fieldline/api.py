"""HTTP-style API: request model, router, serializers and the route modules per area."""

import textwrap

from .core import imports, render


def http_module(ctx):
    return render(ctx, '''
«imports»

export type Method = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';

export const METHODS: readonly Method[] = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE'];

export interface ApiRequest {
  method: Method;
  path: string;
  params: Record<string, string>;
  query: Record<string, string>;
  body: unknown;
  actor: Actor;
}

export interface ApiResponse {
  status: number;
  body: unknown;
  headers?: Record<string, string>;
}

export type Handler = (ctx: AppContext, request: ApiRequest) => ApiResponse;

export const ok = (body: unknown): ApiResponse => ({ status: 200, body });
export const created = (body: unknown): ApiResponse => ({ status: 201, body });
export const accepted = (body: unknown): ApiResponse => ({ status: 202, body });
export const noContent = (): ApiResponse => ({ status: 204, body: null });

export function param(request: ApiRequest, name: string): string {
  const value = request.params[name];
  if (value === undefined) throw new Error(`Route has no parameter ${name}`);
  return decodeURIComponent(value);
}

export function optionalQuery(request: ApiRequest, name: string): string | undefined {
  const value = request.query[name]?.trim();
  return value ? value : undefined;
}

export function requiredQuery(request: ApiRequest, name: string): string {
  const value = optionalQuery(request, name);
  if (value === undefined) throw new ValidationError([{ path: name, message: 'is required' }]);
  return value;
}

export function queryDate(request: ApiRequest, name: string, fallback?: CalendarDate): CalendarDate {
  const value = optionalQuery(request, name) ?? fallback;
  if (value === undefined) throw new ValidationError([{ path: name, message: 'is required' }]);
  if (!isCalendarDate(value)) throw new ValidationError([{ path: name, message: 'must be a date (YYYY-MM-DD)' }]);
  return value;
}

export function queryList(request: ApiRequest, name: string): string[] | undefined {
  const value = optionalQuery(request, name);
  if (value === undefined) return undefined;
  return value.split(',').map((item) => item.trim()).filter(Boolean);
}

export function queryInteger(
  request: ApiRequest,
  name: string,
  fallback: number,
  bounds: { min?: number; max?: number } = {},
): number {
  const raw = optionalQuery(request, name);
  if (raw === undefined) return fallback;
  const value = Number(raw);
  if (!Number.isInteger(value)) throw new ValidationError([{ path: name, message: 'must be a whole number' }]);
  if (bounds.min !== undefined && value < bounds.min) {
    throw new ValidationError([{ path: name, message: `must be at least ${bounds.min}` }]);
  }
  if (bounds.max !== undefined && value > bounds.max) {
    throw new ValidationError([{ path: name, message: `must be at most ${bounds.max}` }]);
  }
  return value;
}
''', imports=imports(ctx, "src/api/http.ts", {
        "src/context.ts": ["type AppContext"],
        "src/auth/roles.ts": ["type Actor"],
        "src/shared/errors.ts": ["ValidationError"],
        "src/shared/time.ts": ["isCalendarDate", "type CalendarDate"],
    }))


def router_module(ctx):
    groups = {"src/api/http.ts": ["type Handler", "type Method"]}
    alias = ""
    canonical = "path"
    definition = ""
    mount_sig = ""
    mount_impl = ""
    if ctx.split:
        alias = '''
/**
 * Paths published before work orders replaced jobs. Requests to them are served by the
 * current routes; only a whole leading `/jobs` segment is rewritten.
 */
const LEGACY_PREFIXES: ReadonlyArray<readonly [RegExp, string]> = [[/^\\/jobs(?=\\/|$)/, '/workorders']];

export function canonicalPath(path: string): string {
  for (const [pattern, replacement] of LEGACY_PREFIXES) {
    if (pattern.test(path)) return path.replace(pattern, replacement);
  }
  return path;
}
'''
        canonical = "canonicalPath(path)"
    if ctx.refactor:
        groups["src/auth/permissions.ts"] = ["authorize", "type Permission"]
        definition = '''
/** One route as data; the router checks `permission` before the handler runs. */
export interface RouteDefinition {
  method: Method;
  path: string;
  permission?: Permission;
  handle: Handler;
}
'''
        mount_sig = "\n  mount(definitions: readonly RouteDefinition[]): void;"
        mount_impl = '''
    mount(definitions) {
      for (const definition of definitions) {
        const { permission, handle } = definition;
        add(
          definition.method,
          definition.path,
          permission === undefined
            ? handle
            : (ctx, request) => {
                authorize(request.actor, permission);
                return handle(ctx, request);
              },
        );
      }
    },'''
    return render(ctx, '''
«imports»
«definition»
export interface RouteMatch {
  pattern: string;
  handler: Handler;
  params: Record<string, string>;
}

export interface Router {
  add(method: Method, pattern: string, handler: Handler): void;
  get(pattern: string, handler: Handler): void;
  post(pattern: string, handler: Handler): void;
  put(pattern: string, handler: Handler): void;
  patch(pattern: string, handler: Handler): void;
  delete(pattern: string, handler: Handler): void;«mount_sig»
  match(method: Method, path: string): RouteMatch | undefined;
  /** Methods registered for a path, used to tell 404 from 405. */
  methodsFor(path: string): Method[];
  table(): Array<{ method: Method; pattern: string }>;
}

interface Route {
  method: Method;
  pattern: string;
  segments: string[];
  handler: Handler;
}

const segmentsOf = (path: string): string[] => path.split('/').filter((segment) => segment.length > 0);
«alias»
function bind(route: Route, segments: readonly string[]): Record<string, string> | undefined {
  if (route.segments.length !== segments.length) return undefined;
  const params: Record<string, string> = {};
  for (let index = 0; index < segments.length; index += 1) {
    const expected = route.segments[index];
    const actual = segments[index];
    if (expected.startsWith(':')) params[expected.slice(1)] = actual;
    else if (expected !== actual) return undefined;
  }
  return params;
}

/** Literal segments win over parameters, so `/invoices/overdue` is not read as an invoice id. */
function specificity(route: Route): number {
  return route.segments.reduce((score, segment) => score * 2 + (segment.startsWith(':') ? 0 : 1), 1);
}

export function createRouter(): Router {
  const routes: Route[] = [];
  const add = (method: Method, pattern: string, handler: Handler): void => {
    if (routes.some((route) => route.method === method && route.pattern === pattern)) {
      throw new Error(`Route ${method} ${pattern} is registered twice`);
    }
    routes.push({ method, pattern, segments: segmentsOf(pattern), handler });
    routes.sort((left, right) => specificity(right) - specificity(left));
  };
  return {
    add,
    get: (pattern, handler) => add('GET', pattern, handler),
    post: (pattern, handler) => add('POST', pattern, handler),
    put: (pattern, handler) => add('PUT', pattern, handler),
    patch: (pattern, handler) => add('PATCH', pattern, handler),
    delete: (pattern, handler) => add('DELETE', pattern, handler),«mount_impl»
    match(method, path) {
      const segments = segmentsOf(«canonical»);
      for (const route of routes) {
        if (route.method !== method) continue;
        const params = bind(route, segments);
        if (params) return { pattern: route.pattern, handler: route.handler, params };
      }
      return undefined;
    },
    methodsFor(path) {
      const segments = segmentsOf(«canonical»);
      return [...new Set(routes.filter((route) => bind(route, segments)).map((route) => route.method))];
    },
    table() {
      return routes
        .map(({ method, pattern }) => ({ method, pattern }))
        .sort((left, right) => left.pattern.localeCompare(right.pattern) || left.method.localeCompare(right.method));
    },
  };
}
''', definition=definition, mount_sig=mount_sig, mount_impl=mount_impl, alias=alias, canonical=canonical,
                  imports=imports(ctx, "src/api/router.ts", groups))


def errors_module(ctx):
    return render(ctx, '''
«imports»

export interface ErrorBody {
  error: {
    code: string;
    message: string;
    details?: Record<string, unknown>;
  };
}

/** Domain errors keep their status and code; anything else is logged and reported as 500. */
export function errorResponse(log: Logger, error: unknown): ApiResponse {
  if (isDomainError(error)) {
    const body: ErrorBody = { error: { code: error.code, message: error.message } };
    if (Object.keys(error.details).length > 0) body.error.details = error.details;
    return { status: error.status, body };
  }
  const message = error instanceof Error ? error.message : String(error);
  log.error('unhandled error', { message });
  return { status: 500, body: { error: { code: 'internal', message: 'Internal error' } } };
}

export function routeNotFound(method: string, path: string): ApiResponse {
  return { status: 404, body: { error: { code: 'route-not-found', message: `No route for ${method} ${path}` } } };
}

export function methodNotAllowed(allowed: readonly string[]): ApiResponse {
  return {
    status: 405,
    body: { error: { code: 'method-not-allowed', message: `Allowed: ${allowed.join(', ')}` } },
    headers: { allow: allowed.join(', ') },
  };
}

export function unauthenticated(): ApiResponse {
  return { status: 401, body: { error: { code: 'unauthenticated', message: 'Unknown or missing actor' } } };
}
''', imports=imports(ctx, "src/api/errors.ts", {
        "src/shared/errors.ts": ["isDomainError"],
        "src/shared/log.ts": ["type Logger"],
        "src/api/http.ts": ["type ApiResponse"],
    }))


def scope_module(ctx):
    return render(ctx, '''
«imports»

/** Customers see their own work orders, technicians the ones assigned to them, staff all. */
export function canSeeJob(actor: Actor, job: «Job»): boolean {
  switch (actor.role) {
    case 'customer':
      return job.customerId === actor.subjectId;
    case 'technician':
      return job.technicianId !== undefined && job.technicianId === actor.subjectId;
    default:
      return true;
  }
}

/** Work orders outside the actor's scope are reported as missing rather than forbidden. */
export function visibleJob(ctx: AppContext, actor: Actor, jobId: string): «Job» {
  const job = «requireJob»(ctx, jobId);
  if (!canSeeJob(actor, job)) throw new NotFoundError('work order', jobId);
  return job;
}

export function assertAssignedTechnician(actor: Actor, job: «Job»): void {
  if (actor.role === 'technician' && job.technicianId !== actor.subjectId) {
    throw new ForbiddenError(`work on ${job.number}`, actor.id);
  }
}

/** Customer actors are always limited to their own account, whatever they asked for. */
export function customerFilter(actor: Actor, requested: string | undefined): string | undefined {
  return actor.role === 'customer' ? actor.subjectId : requested;
}

export function technicianFilter(actor: Actor, requested: string | undefined): string | undefined {
  return actor.role === 'technician' ? actor.subjectId : requested;
}
''', imports=imports(ctx, "src/api/scope.ts", {
        "src/context.ts": ["type AppContext"],
        "src/auth/roles.ts": ["type Actor"],
        "src/shared/errors.ts": ["ForbiddenError", "NotFoundError"],
    }, jobs=("type Job", "requireJob")))


def serializers_module(ctx):
    return render(ctx, '''
«imports»

export function customerView(customer: Customer) {
  return {
    ...customer,
    creditLimit: formatCents(customer.creditLimitCents, tariffFor(customer.billingRegion).currency),
  };
}

export function jobSummary(job: «Job») {
  return {
    id: job.id,
    number: job.number,
    status: job.status,
    priority: job.priority,
    workType: job.workType,
    summary: job.summary,
    customerId: job.customerId,
    siteId: job.siteId,
    technicianId: job.technicianId,
    requestedAt: job.requestedAt,
    scheduledStart: job.scheduledStart,
  };
}

export function jobDetail(ctx: AppContext, job: «Job», audience: 'staff' | 'customer') {
  const handler = ctx.workTypes.resolve(job.workType);
  const activeHold = job.holds.find((hold) => hold.releasedAt === undefined);
  return {
    ...jobSummary(job),
    workTypeLabel: handler.label,
    description: job.description,
    source: job.source,
    contractId: job.contractId,
    assetId: job.assetId,
    scheduledEnd: job.scheduledEnd,
    startedAt: job.startedAt,
    hold: activeHold ? { reason: activeHold.reason, since: activeHold.since } : undefined,
    checklist: job.checklist,
    completion: job.completion,
    tags: audience === 'staff' ? job.tags : undefined,
    reopenCount: job.reopenCount,
    notes: «listJobNotes»(ctx, job.id, audience).map((note) => ({ at: note.at, text: note.text, authorId: note.authorId })),
  };
}

export function invoiceView(invoice: Invoice, creditedCents: Cents = 0) {
  const outstanding = outstandingCents(invoice, creditedCents);
  return {
    ...invoice,
    outstandingCents: outstanding,
    display: {
      subtotal: formatCents(invoice.subtotalCents, invoice.currency),
      tax: formatCents(invoice.taxCents, invoice.currency),
      total: formatCents(invoice.totalCents, invoice.currency),
      outstanding: formatCents(outstanding, invoice.currency),
    },
  };
}

export function appointmentView(appointment: Appointment) {
  return {
    ...appointment,
    window: describeWindow(appointment),
  };
}
''', imports=imports(ctx, "src/api/serializers.ts", {
        "src/context.ts": ["type AppContext"],
        "src/billing/model.ts": ["outstandingCents", "type Invoice"],
        "src/billing/tariffs/index.ts": ["tariffFor"],
        "src/customers/model.ts": ["type Customer"],
        "src/scheduling/model.ts": ["type Appointment"],
        "src/scheduling/windows.ts": ["describeWindow"],
        "src/shared/money.ts": ["formatCents", "type Cents"],
    }, jobs=("type Job", "listJobNotes")))


class Route:
    def __init__(self, method, path, permission, body):
        self.method = method
        self.path = path
        self.permission = permission
        self.body = textwrap.dedent(body).strip("\n")


def _indent(text, spaces):
    pad = " " * spaces
    return "\n".join(pad + line if line else "" for line in text.split("\n"))


def route_module(ctx, here, area, routes, groups, preamble="", jobs=(), **subs):
    """Render an area's routes as router registrations, or as a definition table in the refactor."""
    groups = {target: list(names) for target, names in groups.items()}
    if ctx.refactor:
        groups.setdefault("src/api/router.ts", []).append("type RouteDefinition")
        entries = []
        for route in routes:
            fields = [f"method: '{route.method}'", f"path: '{route.path}'"]
            if route.permission:
                fields.append(f"permission: '{route.permission}'")
            head = "".join(f"    {field},\n" for field in fields)
            entries.append(f"  {{\n{head}    handle(ctx, request) {{\n{_indent(route.body, 6)}\n    }},\n  }},")
        body = f"export const {area}Routes: readonly RouteDefinition[] = [\n" + "\n".join(entries) + "\n];"
    else:
        groups.setdefault("src/api/router.ts", []).append("type Router")
        if any(route.permission for route in routes):
            groups.setdefault("src/auth/permissions.ts", []).append("authorize")
        registrations = []
        for route in routes:
            lines = route.body
            if route.permission:
                lines = f"authorize(request.actor, '{route.permission}');\n{lines}"
            method = route.method.lower()
            registrations.append(f"  router.{method}('{route.path}', (ctx, request) => {{\n{_indent(lines, 4)}\n  }});")
        name = area[0].upper() + area[1:]
        body = f"export function register{name}Routes(router: Router): void {{\n" + "\n\n".join(registrations) + "\n}"
    template = "«imports»\n\n" + (textwrap.dedent(preamble).strip("\n") + "\n\n" if preamble else "") + body + "\n"
    return render(ctx, template, imports=imports(ctx, here, groups, jobs=jobs), **subs)


def j(ctx):
    return "/workorders" if ctx.split else "/jobs"


def customer_routes(ctx):
    if ctx.refactor:
        create, patch, site = "request.body", "request.body", "request.body"
        validation = []
    else:
        create = "parseInput(customerInput, request.body)"
        patch = "parseInput(customerPatch, request.body)"
        site = "parseInput(siteInput, request.body)"
        validation = ["customerInput", "customerPatch", "siteInput"]
    routes = [
        Route("POST", "/customers", "customers:write", f'''
            const customer = createCustomer(ctx, request.actor.id, {create});
            return created(customerView(customer));
        '''),
        Route("GET", "/customers", "customers:read", '''
            const region = optionalQuery(request, 'region');
            const text = optionalQuery(request, 'q')?.toLowerCase();
            const matches = customers(ctx)
              .find((customer) => (region === undefined || customer.billingRegion === region) && (!text || customer.name.toLowerCase().includes(text)))
              .sort((left, right) => left.name.localeCompare(right.name));
            const page = paginate(matches, readPageRequest(request.query));
            return ok({ ...page, items: page.items.map(customerView) });
        '''),
        Route("GET", "/customers/:id", None, '''
            const customerId = param(request, 'id');
            if (request.actor.role === 'customer') assertCustomerScope(request.actor, customerId);
            else authorize(request.actor, 'customers:read');
            const customer = customers(ctx).require(customerId, 'customer');
            return ok({ ...customerView(customer), sites: sitesForCustomer(ctx, customer.id) });
        '''),
        Route("PATCH", "/customers/:id", "customers:write", f'''
            return ok(customerView(updateCustomer(ctx, request.actor.id, param(request, 'id'), {patch})));
        '''),
        Route("POST", "/customers/:id/suspend", "customers:write", '''
            const { reason } = parseInput(reasonInput, request.body);
            return ok(customerView(suspendCustomer(ctx, request.actor.id, param(request, 'id'), reason)));
        '''),
        Route("POST", "/customers/:id/reactivate", "customers:write", '''
            return ok(customerView(reactivateCustomer(ctx, request.actor.id, param(request, 'id'))));
        '''),
        Route("POST", "/customers/:id/close", "customers:write", '''
            const customerId = param(request, 'id');
            const openWork = «listJobs»(ctx, { customerId, status: [...OPEN_STATUSES] }).length;
            return ok(customerView(closeCustomer(ctx, request.actor.id, customerId, openWork)));
        '''),
        Route("POST", "/customers/:id/sites", "customers:write", f'''
            return created(addSite(ctx, request.actor.id, param(request, 'id'), {site}));
        '''),
        Route("GET", "/customers/:id/sites", None, '''
            const customerId = param(request, 'id');
            if (request.actor.role === 'customer') assertCustomerScope(request.actor, customerId);
            else authorize(request.actor, 'customers:read');
            return ok({ items: sitesForCustomer(ctx, customerId) });
        '''),
        Route("PUT", "/sites/:id/access", "customers:write", '''
            const data = parseInput(accessInput, request.body);
            return ok(updateSiteAccess(ctx, request.actor.id, param(request, 'id'), data.accessNotes, data.hazards));
        '''),
        Route("POST", "/customers/:id/portal-users", "customers:write", '''
            const customer = customers(ctx).require(param(request, 'id'), 'customer');
            const { name } = parseInput(portalUserInput, request.body);
            const actor = registerActor(ctx, { id: ctx.ids.next('USR'), name, role: 'customer', subjectId: customer.id });
            return created(actor);
        '''),
    ]
    groups = {
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/adapters/repositories/sites.ts": ["sitesForCustomer"],
        "src/api/http.ts": ["created", "ok", "optionalQuery", "param"],
        "src/api/serializers.ts": ["customerView"],
        "src/auth/actors.ts": ["registerActor"],
        "src/auth/permissions.ts": ["assertCustomerScope", "authorize"],
        "src/customers/service.ts": ["addSite", "closeCustomer", "createCustomer", "reactivateCustomer",
                                     "suspendCustomer", "updateCustomer", "updateSiteAccess"],
        "src/shared/paging.ts": ["paginate", "readPageRequest"],
        "src/shared/validate.ts": ["array", "object", "optional", "parseInput", "string", "withDefault"],
    }
    if validation:
        groups["src/customers/validation.ts"] = validation
    preamble = '''
        const reasonInput = object({ reason: string({ min: 3, max: 200 }) });

        const accessInput = object({
          accessNotes: optional(string({ max: 500 })),
          hazards: withDefault(array(string({ min: 1, max: 60 }), { max: 20 }), []),
        });

        const portalUserInput = object({ name: string({ min: 2, max: 80 }) });
    '''
    return route_module(ctx, "src/api/routes/customers.ts", "customer", routes, groups, preamble,
                        jobs=("listJobs", "OPEN_STATUSES"))


def contract_routes(ctx):
    routes = [
        Route("POST", "/contracts", "contracts:write", '''
            return created(createContract(ctx, request.actor.id, request.body));
        '''),
        Route("GET", "/contracts/:id", "customers:read", '''
            const contract = contracts(ctx).require(param(request, 'id'), 'contract');
            const today = dateOf(ctx.clock.now());
            const usage = contract.siteIds.map((siteId) => ({ siteId, includedVisitsUsed: includedVisitsUsed(ctx, contract, siteId, today) }));
            return ok({ ...contract, usage });
        '''),
        Route("POST", "/contracts/:id/activate", "contracts:write", '''
            return ok(activateContract(ctx, request.actor.id, param(request, 'id')));
        '''),
        Route("POST", "/contracts/:id/terminate", "contracts:write", '''
            const { reason } = parseInput(object({ reason: string({ min: 3, max: 200 }) }), request.body);
            return ok(terminateContract(ctx, request.actor.id, param(request, 'id'), reason));
        '''),
        Route("GET", "/customers/:id/contracts", "customers:read", '''
            return ok({ items: contractsForCustomer(ctx, param(request, 'id')) });
        '''),
        Route("POST", "/contracts/roll", "system:admin", '''
            return ok(rollContracts(ctx, queryDate(request, 'on', dateOf(ctx.clock.now()))));
        '''),
        Route("GET", "/sites/:id/coverage", "customers:read", '''
            const workType = requiredQuery(request, 'workType');
            const coverage = coverageFor(ctx, param(request, 'id'), workType, queryDate(request, 'on', dateOf(ctx.clock.now())));
            return ok({ covered: coverage !== undefined, coverage });
        '''),
    ]
    return route_module(ctx, "src/api/routes/contracts.ts", "contract", routes, {
        "src/adapters/repositories/contracts.ts": ["contracts", "contractsForCustomer"],
        "src/api/http.ts": ["created", "ok", "param", "queryDate", "requiredQuery"],
        "src/contracts/entitlements.ts": ["coverageFor", "includedVisitsUsed"],
        "src/contracts/service.ts": ["activateContract", "createContract", "rollContracts", "terminateContract"],
        "src/shared/time.ts": ["dateOf"],
        "src/shared/validate.ts": ["object", "parseInput", "string"],
    })


def asset_routes(ctx):
    routes = [
        Route("POST", "/assets", "customers:write", '''
            return created(registerAsset(ctx, request.actor.id, request.body));
        '''),
        Route("GET", "/sites/:id/assets", "jobs:read", '''
            const today = dateOf(ctx.clock.now());
            const items = assetsAtSite(ctx, param(request, 'id')).map((asset) => ({
              ...asset,
              warranty: warrantyStatus(asset, today),
              nextServiceOn: nextServiceDate(asset),
            }));
            return ok({ items });
        '''),
        Route("POST", "/assets/:id/retire", "customers:write", '''
            const on = queryDate(request, 'on', dateOf(ctx.clock.now()));
            return ok(retireAsset(ctx, request.actor.id, param(request, 'id'), on));
        '''),
        Route("GET", "/assets/due", "jobs:schedule", '''
            const on = queryDate(request, 'on', dateOf(ctx.clock.now()));
            const within = queryInteger(request, 'within', 30, { min: 0, max: 365 });
            return ok({ items: assetsDueForService(ctx, on, within) });
        '''),
        Route("GET", "/assets/by-serial/:serial", "jobs:read", '''
            const asset = findAssetBySerial(ctx, param(request, 'serial'));
            if (!asset) throw new NotFoundError('asset', param(request, 'serial'));
            return ok(asset);
        '''),
    ]
    return route_module(ctx, "src/api/routes/assets.ts", "asset", routes, {
        "src/adapters/repositories/assets.ts": ["assetsAtSite", "findAssetBySerial"],
        "src/api/http.ts": ["created", "ok", "param", "queryDate", "queryInteger"],
        "src/assets/service.ts": ["assetsDueForService", "nextServiceDate", "registerAsset", "retireAsset",
                                  "warrantyStatus"],
        "src/shared/errors.ts": ["NotFoundError"],
        "src/shared/time.ts": ["dateOf"],
    })


def job_routes(ctx):
    J = j(ctx)
    routes = [
        Route("POST", J, "jobs:open", '''
            const data = parseInput(openInput, request.body);
            if (request.actor.role === 'customer') assertCustomerScope(request.actor, data.customerId);
            const source = request.actor.role === 'customer' ? 'portal' : 'phone';
            const job = «openJob»(ctx, request.actor.id, { ...data, source });
            return created(jobDetail(ctx, job, request.actor.role === 'customer' ? 'customer' : 'staff'));
        '''),
        Route("GET", J, "jobs:read", '''
            const status = queryList(request, 'status');
            const unknown = (status ?? []).filter((value) => !STATUSES.includes(value));
            if (unknown.length > 0) throw new ValidationError([{ path: 'status', message: `unknown status ${unknown.join(', ')}` }]);
            const matches = «listJobs»(ctx, {
              status: status as «JobStatus»[] | undefined,
              customerId: customerFilter(request.actor, optionalQuery(request, 'customerId')),
              technicianId: technicianFilter(request.actor, optionalQuery(request, 'technicianId')),
              workType: optionalQuery(request, 'workType'),
              text: optionalQuery(request, 'q'),
            });
            const page = paginate(matches, readPageRequest(request.query));
            return ok({ ...page, items: page.items.map(jobSummary) });
        '''),
        Route("GET", f"{J}/:id", "jobs:read", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            return ok(jobDetail(ctx, job, request.actor.role === 'customer' ? 'customer' : 'staff'));
        '''),
        Route("POST", f"{J}/:id/schedule", "jobs:schedule", '''
            const data = parseInput(scheduleInput, request.body);
            const job = «requireJob»(ctx, param(request, 'id'));
            const duration = data.durationMinutes ?? ctx.workTypes.resolve(job.workType).defaultDurationMinutes;
            return ok(jobSummary(«scheduleJob»(ctx, request.actor.id, job.id, data.start, duration)));
        '''),
        Route("POST", f"{J}/:id/assign", "jobs:schedule", '''
            const { technicianId } = parseInput(assignInput, request.body);
            return ok(jobSummary(«assignJob»(ctx, request.actor.id, param(request, 'id'), technicianId)));
        '''),
        Route("POST", f"{J}/:id/start", "jobs:work", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            assertAssignedTechnician(request.actor, job);
            return ok(jobSummary(«startJob»(ctx, request.actor.id, job.id)));
        '''),
        Route("POST", f"{J}/:id/hold", "jobs:work", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            assertAssignedTechnician(request.actor, job);
            const data = parseInput(holdInput, request.body);
            return ok(jobDetail(ctx, «holdJob»(ctx, request.actor.id, job.id, data.reason, data.note), 'staff'));
        '''),
        Route("POST", f"{J}/:id/resume", "jobs:work", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            assertAssignedTechnician(request.actor, job);
            const { note } = parseInput(noteInput, request.body);
            return ok(jobDetail(ctx, «resumeJob»(ctx, request.actor.id, job.id, note), 'staff'));
        '''),
        Route("PUT", f"{J}/:id/checklist", "jobs:work", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            assertAssignedTechnician(request.actor, job);
            const { item, done } = parseInput(checklistInput, request.body);
            return ok(«recordChecklistItem»(ctx, request.actor.id, job.id, item, done).checklist);
        '''),
        Route("POST", f"{J}/:id/complete", "jobs:work", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            assertAssignedTechnician(request.actor, job);
            const report = parseInput(completionInput, request.body);
            return ok(jobDetail(ctx, «completeJob»(ctx, request.actor.id, job.id, report), 'staff'));
        '''),
        Route("POST", f"{J}/:id/cancel", "jobs:cancel", '''
            const { reason } = parseInput(reasonInput, request.body);
            return ok(jobSummary(«cancelJob»(ctx, request.actor.id, param(request, 'id'), reason)));
        '''),
        Route("POST", f"{J}/:id/close", "jobs:schedule", '''
            return ok(jobSummary(«closeJob»(ctx, request.actor.id, param(request, 'id'))));
        '''),
        Route("POST", f"{J}/:id/reopen", "jobs:schedule", '''
            const { reason } = parseInput(reasonInput, request.body);
            return ok(jobSummary(«reopenJob»(ctx, request.actor.id, param(request, 'id'), reason)));
        '''),
        Route("POST", f"{J}/:id/follow-ups", "jobs:schedule", '''
            const { summary } = parseInput(followUpInput, request.body);
            return created(jobSummary(«createFollowUp»(ctx, request.actor.id, param(request, 'id'), summary)));
        '''),
        Route("GET", f"{J}/:id/timeline", "jobs:read", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            return ok({ items: «jobTimeline»(ctx, job.id) });
        '''),
        Route("POST", f"{J}/:id/notes", "jobs:read", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            const data = parseInput(noteBodyInput, request.body);
            const visibility = request.actor.role === 'customer' ? 'customer' : data.visibility;
            return created(«addJobNote»(ctx, job.id, request.actor.id, data.text, visibility));
        '''),
    ]
    preamble = '''
        const STATUSES: readonly string[] = ['new', 'scheduled', 'assigned', 'in_progress', 'on_hold', 'completed', 'closed', 'cancelled'];

        const openInput = object({
          customerId: string({ min: 1 }),
          siteId: string({ min: 1 }),
          assetId: optional(string({ min: 1 })),
          workType: string({ min: 1, max: 60 }),
          summary: string({ min: 3, max: 200 }),
          description: optional(string({ max: 4000 })),
          priority: optional(oneOf(«JOB_PRIORITIES»)),
          tags: optional(array(string({ min: 1, max: 30 }), { max: 10 })),
        });

        const scheduleInput = object({
          start: timestamp(),
          durationMinutes: optional(integer({ min: 15, max: 720 })),
        });

        const assignInput = object({ technicianId: string({ min: 1 }) });
        const holdInput = object({ reason: oneOf(HOLD_REASONS), note: optional(string({ max: 500 })) });
        const noteInput = object({ note: optional(string({ max: 500 })) });
        const reasonInput = object({ reason: string({ min: 3, max: 500 }) });
        const followUpInput = object({ summary: string({ min: 3, max: 200 }) });
        const checklistInput = object({ item: string({ min: 1, max: 120 }), done: boolean() });

        const completionInput = object({
          resolution: string({ min: 3, max: 2000 }),
          partsUsed: optional(array(object({ sku: string({ min: 1 }), quantity: integer({ min: 1, max: 500 }) }), { max: 50 })),
          signatureName: optional(string({ min: 2, max: 80 })),
          followUpRequired: optional(boolean()),
          meterReading: optional(number({ min: 0 })),
        });

        const noteBodyInput = object({
          text: string({ min: 1, max: 4000 }),
          visibility: withDefault(oneOf(['internal', 'customer'] as const), 'internal'),
        });
    '''
    area = "workOrder" if ctx.split else "job"
    here = "src/api/routes/workorders.ts" if ctx.split else "src/api/routes/jobs.ts"
    return route_module(ctx, here, area, routes, {
        "src/api/http.ts": ["created", "ok", "optionalQuery", "param", "queryList"],
        "src/api/scope.ts": ["assertAssignedTechnician", "customerFilter", "technicianFilter", "visibleJob"],
        "src/api/serializers.ts": ["jobDetail", "jobSummary"],
        "src/auth/permissions.ts": ["assertCustomerScope"],
        "src/shared/errors.ts": ["ValidationError"],
        "src/shared/paging.ts": ["paginate", "readPageRequest"],
        "src/shared/validate.ts": ["array", "boolean", "integer", "number", "object", "oneOf", "optional",
                                   "parseInput", "string", "timestamp", "withDefault"],
    }, preamble, jobs=("type JobStatus", "JOB_PRIORITIES", "HOLD_REASONS", "openJob", "listJobs", "requireJob",
                       "scheduleJob", "assignJob", "startJob", "holdJob", "resumeJob", "recordChecklistItem",
                       "completeJob", "cancelJob", "closeJob", "reopenJob", "createFollowUp", "jobTimeline",
                       "addJobNote"))


def sla_routes(ctx):
    J = j(ctx)
    routes = [
        Route("GET", f"{J}/:id/sla", "jobs:read", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            const clock = clockForJob(ctx, job.id);
            if (!clock) throw new NotFoundError('service level clock', job.id);
            return ok(slaStatus(ctx, clock, ctx.clock.now()));
        '''),
        Route("POST", "/sla/scan", "system:admin", '''
            return ok({ warned: scanForWarnings(ctx) });
        '''),
        Route("GET", "/sla/compliance", "reports:read", '''
            const from = queryDate(request, 'from');
            const to = queryDate(request, 'to');
            const rate = complianceRate(ctx, atMinute(from, 0), atMinute(to, 0));
            return ok({ from, to, rate: rate ?? null });
        '''),
    ]
    return route_module(ctx, "src/api/routes/sla.ts", "sla", routes, {
        "src/adapters/repositories/sla-clocks.ts": ["clockForJob"],
        "src/api/http.ts": ["ok", "param", "queryDate"],
        "src/api/scope.ts": ["visibleJob"],
        "src/shared/errors.ts": ["NotFoundError"],
        "src/shared/time.ts": ["atMinute"],
        "src/sla/monitor.ts": ["complianceRate", "scanForWarnings", "slaStatus"],
    })


def scheduling_routes(ctx):
    routes = [
        Route("GET", "/technicians/:id/slots", "jobs:schedule", '''
            const technician = technicians(ctx).require(param(request, 'id'), 'technician');
            const date = queryDate(request, 'date');
            const job = «requireJob»(ctx, requiredQuery(request, 'jobId'));
            const fallback = ctx.workTypes.resolve(job.workType).defaultDurationMinutes;
            const duration = queryInteger(request, 'duration', fallback, { min: 15, max: 720 });
            const slots = availableSlots(
              ctx,
              technician,
              date,
              duration,
              locationOfJob(ctx, job.id),
              appointmentLocations(ctx, technician.id, date),
            );
            return ok({ technicianId: technician.id, date, durationMinutes: duration, slots });
        '''),
        Route("POST", "/appointments", "jobs:schedule", '''
            const data = parseInput(bookingInput, request.body);
            return created(appointmentView(bookAppointment(ctx, request.actor.id, data)));
        '''),
        Route("POST", "/appointments/:id/cancel", "jobs:schedule", '''
            const { reason } = parseInput(reasonInput, request.body);
            return ok(appointmentView(cancelAppointment(ctx, request.actor.id, param(request, 'id'), reason)));
        '''),
        Route("POST", "/appointments/:id/reschedule", "jobs:schedule", '''
            const { start } = parseInput(rescheduleInput, request.body);
            return ok(appointmentView(rescheduleAppointment(ctx, request.actor.id, param(request, 'id'), start)));
        '''),
        Route("POST", "/appointments/:id/confirm", "jobs:read", '''
            const appointment = appointments(ctx).require(param(request, 'id'), 'appointment');
            visibleJob(ctx, request.actor, appointment.jobId);
            return ok(appointmentView(confirmAppointment(ctx, appointment.id)));
        '''),
        Route("POST", "/appointments/:id/no-access", "jobs:work", '''
            const appointment = appointments(ctx).require(param(request, 'id'), 'appointment');
            const job = visibleJob(ctx, request.actor, appointment.jobId);
            assertAssignedTechnician(request.actor, job);
            const { reason } = parseInput(reasonInput, request.body);
            return ok(appointmentView(recordNoAccess(ctx, request.actor.id, appointment.id, reason)));
        '''),
        Route("GET", "/technicians/:id/route", "jobs:read", '''
            const technicianId = param(request, 'id');
            if (request.actor.role === 'technician' && request.actor.subjectId !== technicianId) {
              throw new ForbiddenError(`view the route of ${technicianId}`, request.actor.id);
            }
            const technician = technicians(ctx).require(technicianId, 'technician');
            return ok(dailyRoute(ctx, technician, queryDate(request, 'date', dateOf(ctx.clock.now()))));
        '''),
        Route("GET", "/technicians/:id/load", "jobs:schedule", '''
            const technician = technicians(ctx).require(param(request, 'id'), 'technician');
            return ok(dailyLoad(ctx, technician, queryDate(request, 'date', dateOf(ctx.clock.now()))));
        '''),
    ]
    preamble = '''
        const bookingInput = object({
          jobId: string({ min: 1 }),
          technicianId: string({ min: 1 }),
          start: timestamp(),
          durationMinutes: optional(integer({ min: 15, max: 720 })),
        });

        const rescheduleInput = object({ start: timestamp() });
        const reasonInput = object({ reason: string({ min: 3, max: 500 }) });
    '''
    return route_module(ctx, "src/api/routes/scheduling.ts", "scheduling", routes, {
        "src/adapters/repositories/appointments.ts": ["appointments"],
        "src/adapters/repositories/technicians.ts": ["technicians"],
        "src/api/http.ts": ["created", "ok", "param", "queryDate", "queryInteger", "requiredQuery"],
        "src/api/scope.ts": ["assertAssignedTechnician", "visibleJob"],
        "src/api/serializers.ts": ["appointmentView"],
        "src/scheduling/capacity.ts": ["dailyLoad"],
        "src/scheduling/locations.ts": ["appointmentLocations", "locationOfJob"],
        "src/scheduling/route.ts": ["dailyRoute"],
        "src/scheduling/service.ts": ["bookAppointment", "cancelAppointment", "confirmAppointment", "recordNoAccess",
                                      "rescheduleAppointment"],
        "src/scheduling/slots.ts": ["availableSlots"],
        "src/shared/errors.ts": ["ForbiddenError"],
        "src/shared/time.ts": ["dateOf"],
        "src/shared/validate.ts": ["integer", "object", "optional", "parseInput", "string", "timestamp"],
    }, preamble, jobs=("requireJob",))


def dispatch_routes(ctx):
    J = j(ctx)
    routes = [
        Route("GET", f"{J}/:id/recommendations", "jobs:schedule", '''
            const date = queryDate(request, 'date', dateOf(ctx.clock.now()));
            const limit = queryInteger(request, 'limit', 5, { min: 1, max: 20 });
            return ok({ items: recommendTechnicians(ctx, param(request, 'id'), date, limit) });
        '''),
        Route("GET", "/dispatch/board", "jobs:schedule", '''
            const date = queryDate(request, 'date', dateOf(ctx.clock.now()));
            return ok(dispatchBoard(ctx, date, optionalQuery(request, 'region')));
        '''),
        Route("GET", "/dispatch/capacity", "jobs:schedule", '''
            const date = queryDate(request, 'date', dateOf(ctx.clock.now()));
            return ok({ items: leastLoaded(ctx, requiredQuery(request, 'region'), date) });
        '''),
        Route("GET", "/dispatch/rules", "jobs:schedule", '''
            return ok({
              items: ctx.scoringRules.entries().map(([key, rule]) => ({
                key,
                describe: rule.describe,
                weight: ctx.config.dispatch.weights[rule.weight],
              })),
            });
        '''),
    ]
    return route_module(ctx, "src/api/routes/dispatch.ts", "dispatch", routes, {
        "src/api/http.ts": ["ok", "optionalQuery", "param", "queryDate", "queryInteger", "requiredQuery"],
        "src/dispatch/board.ts": ["dispatchBoard"],
        "src/dispatch/recommend.ts": ["recommendTechnicians"],
        "src/scheduling/capacity.ts": ["leastLoaded"],
        "src/shared/time.ts": ["dateOf"],
    })


def technician_routes(ctx):
    routes = [
        Route("POST", "/technicians", "technicians:write", '''
            const technician = createTechnician(ctx, request.actor.id, request.body);
            registerActor(ctx, { id: technician.id, name: technician.name, role: 'technician', subjectId: technician.id });
            return created(technician);
        '''),
        Route("GET", "/technicians", "jobs:schedule", '''
            const region = optionalQuery(request, 'region');
            const items = region === undefined ? activeTechnicians(ctx) : techniciansInRegion(ctx, region);
            return ok({ items });
        '''),
        Route("PUT", "/technicians/:id/certifications", "technicians:write", '''
            const { certifications } = parseInput(certificationInput, request.body);
            return ok(updateCertifications(ctx, request.actor.id, param(request, 'id'), certifications));
        '''),
        Route("POST", "/technicians/:id/absences", "technicians:write", '''
            const absence = parseInput(absenceInput, request.body);
            return created(recordAbsence(ctx, request.actor.id, param(request, 'id'), absence));
        '''),
        Route("POST", "/technicians/:id/deactivate", "technicians:write", '''
            const technicianId = param(request, 'id');
            const openWork = activeJobsForTechnician(ctx, technicianId).length;
            return ok(deactivateTechnician(ctx, request.actor.id, technicianId, openWork));
        '''),
        Route("GET", "/technicians/certifications/expiring", "technicians:write", '''
            const within = queryInteger(request, 'within', 30, { min: 1, max: 365 });
            return ok({ items: expiringCertifications(ctx, dateOf(ctx.clock.now()), within) });
        '''),
    ]
    preamble = '''
        const certificationInput = object({
          certifications: array(object({ skill: string({ min: 1, max: 40 }), expiresOn: optional(date()) }), { max: 40 }),
        });

        const absenceInput = object({
          from: date(),
          to: date(),
          kind: oneOf(['leave', 'sick', 'training'] as const),
        });
    '''
    return route_module(ctx, "src/api/routes/technicians.ts", "technician", routes, {
        "src/adapters/repositories/jobs.ts": ["activeJobsForTechnician"],
        "src/adapters/repositories/technicians.ts": ["activeTechnicians", "techniciansInRegion"],
        "src/api/http.ts": ["created", "ok", "optionalQuery", "param", "queryInteger"],
        "src/auth/actors.ts": ["registerActor"],
        "src/shared/time.ts": ["dateOf"],
        "src/shared/validate.ts": ["array", "date", "object", "oneOf", "optional", "parseInput", "string"],
        "src/technicians/service.ts": ["createTechnician", "deactivateTechnician", "recordAbsence",
                                       "updateCertifications"],
        "src/technicians/skills.ts": ["expiringCertifications"],
    }, preamble)


def inventory_routes(ctx):
    J = j(ctx)
    routes = [
        Route("POST", "/parts", "inventory:write", '''
            return created(addPart(ctx, request.actor.id, request.body));
        '''),
        Route("GET", "/parts", "inventory:read", '''
            const category = optionalQuery(request, 'category');
            const items = category === undefined ? parts(ctx).all() : partsInCategory(ctx, category);
            return ok({ items: items.sort((left, right) => left.id.localeCompare(right.id)) });
        '''),
        Route("PATCH", "/parts/:sku/price", "inventory:write", '''
            const { listPriceCents } = parseInput(priceInput, request.body);
            return ok(updateListPrice(ctx, request.actor.id, param(request, 'sku'), listPriceCents));
        '''),
        Route("POST", "/parts/:sku/discontinue", "inventory:write", '''
            return ok(discontinuePart(ctx, request.actor.id, param(request, 'sku')));
        '''),
        Route("POST", "/stock/receipts", "inventory:write", '''
            const data = parseInput(receiptInput, request.body);
            return created(receiveStock(ctx, request.actor.id, data.locationId, data.sku, data.quantity, data.unitCostCents, data.reference));
        '''),
        Route("POST", "/stock/transfers", "inventory:write", '''
            const data = parseInput(transferInput, request.body);
            transferStock(ctx, request.actor.id, data.from, data.to, data.sku, data.quantity);
            return ok({ from: levelAt(ctx, data.from, data.sku), to: levelAt(ctx, data.to, data.sku) });
        '''),
        Route("POST", "/stock/counts", "inventory:write", '''
            const data = parseInput(countInput, request.body);
            return ok(adjustCount(ctx, request.actor.id, data.locationId, data.sku, data.counted));
        '''),
        Route("GET", "/stock/:location", "inventory:read", '''
            const locationId = param(request, 'location');
            const items = stockLevels(ctx)
              .find((level) => level.locationId === locationId)
              .map((level) => ({ ...level, available: available(level) }))
              .sort((left, right) => left.sku.localeCompare(right.sku));
            return ok({ locationId, items });
        '''),
        Route("GET", "/stock/:location/reorder", "inventory:read", '''
            return ok({ items: reorderSuggestions(ctx, param(request, 'location')) });
        '''),
        Route("GET", "/inventory/valuation", "inventory:read", '''
            return ok(stockValuation(ctx, optionalQuery(request, 'location')));
        '''),
        Route("POST", f"{J}/:id/reservations", "jobs:schedule", '''
            const { parts: requested } = parseInput(reservationInput, request.body);
            return created({ items: reservePartsForJob(ctx, request.actor.id, param(request, 'id'), requested) });
        '''),
        Route("GET", f"{J}/:id/reservations", "jobs:read", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            return ok({ items: reservationsForJob(ctx, job.id) });
        '''),
    ]
    preamble = '''
        const priceInput = object({ listPriceCents: integer({ min: 0 }) });

        const receiptInput = object({
          locationId: string({ min: 1 }),
          sku: string({ min: 1 }),
          quantity: integer({ min: 1, max: 100_000 }),
          unitCostCents: integer({ min: 0 }),
          reference: optional(string({ max: 60 })),
        });

        const transferInput = object({
          from: string({ min: 1 }),
          to: string({ min: 1 }),
          sku: string({ min: 1 }),
          quantity: integer({ min: 1, max: 100_000 }),
        });

        const countInput = object({ locationId: string({ min: 1 }), sku: string({ min: 1 }), counted: integer({ min: 0 }) });

        const reservationInput = object({
          parts: array(object({ sku: string({ min: 1 }), quantity: integer({ min: 1, max: 500 }) }), { min: 1, max: 50 }),
        });
    '''
    return route_module(ctx, "src/api/routes/inventory.ts", "inventory", routes, {
        "src/adapters/repositories/parts.ts": ["parts", "partsInCategory"],
        "src/adapters/repositories/reservations.ts": ["reservationsForJob"],
        "src/adapters/repositories/stock.ts": ["levelAt", "stockLevels"],
        "src/api/http.ts": ["created", "ok", "optionalQuery", "param"],
        "src/api/scope.ts": ["visibleJob"],
        "src/inventory/catalog.ts": ["addPart", "discontinuePart", "updateListPrice"],
        "src/inventory/model.ts": ["available"],
        "src/inventory/reorder.ts": ["reorderSuggestions"],
        "src/inventory/reservations.ts": ["reservePartsForJob"],
        "src/inventory/stock.ts": ["adjustCount", "receiveStock", "transferStock"],
        "src/inventory/valuation.ts": ["stockValuation"],
        "src/shared/validate.ts": ["array", "integer", "object", "optional", "parseInput", "string"],
    }, preamble)


def timesheet_routes(ctx):
    routes = [
        Route("POST", "/time-entries", "timesheets:write", '''
            const input = request.body as Record<string, unknown> | undefined;
            if (request.actor.role === 'technician' && input?.technicianId !== request.actor.subjectId) {
              throw new ForbiddenError('log time for another technician', request.actor.id);
            }
            return created(logEntry(ctx, input));
        '''),
        Route("POST", "/clock/in", "timesheets:write", '''
            const data = parseInput(clockInInput, request.body);
            const technicianId = ownTechnician(request.actor, data.technicianId);
            return created(clockIn(ctx, technicianId, data.kind, data.jobId));
        '''),
        Route("POST", "/clock/out", "timesheets:write", '''
            const data = parseInput(clockOutInput, request.body);
            const entry = timeEntries(ctx).require(data.entryId, 'time entry');
            ownTechnician(request.actor, entry.technicianId);
            return ok(clockOut(ctx, entry.id, data.km, data.note));
        '''),
        Route("POST", "/time-entries/:id/correct", "timesheets:approve", '''
            const data = parseInput(correctionInput, request.body);
            return ok(correctEntry(ctx, param(request, 'id'), data.minutes, data.note));
        '''),
        Route("POST", "/time-entries/approve", "timesheets:approve", '''
            const { entryIds } = parseInput(approvalInput, request.body);
            return ok({ items: approveEntries(ctx, request.actor.id, entryIds) });
        '''),
        Route("POST", "/time-entries/:id/reject", "timesheets:approve", '''
            const { reason } = parseInput(object({ reason: string({ min: 3, max: 500 }) }), request.body);
            return ok(rejectEntry(ctx, request.actor.id, param(request, 'id'), reason));
        '''),
        Route("GET", "/time-entries/pending", "timesheets:approve", '''
            return ok({ items: pendingApprovals(ctx) });
        '''),
        Route("GET", "/technicians/:id/timesheet", None, '''
            const technicianId = param(request, 'id');
            if (request.actor.role === 'technician') ownTechnician(request.actor, technicianId);
            else authorize(request.actor, 'timesheets:approve');
            return ok(weeklySummary(ctx, technicianId, queryDate(request, 'week')));
        '''),
        Route("GET", "/technicians/:id/mileage", "timesheets:approve", '''
            const technicianId = param(request, 'id');
            const from = queryDate(request, 'from');
            const to = queryDate(request, 'to');
            const entries = entriesForTechnician(ctx, technicianId, atMinute(from, 0), atMinute(to, 0));
            return ok(mileageClaim(entries, technicianId, ctx.config.timesheets.mileageRateCents));
        '''),
    ]
    preamble = '''
        const clockInInput = object({
          technicianId: string({ min: 1 }),
          kind: oneOf(['labor', 'travel', 'break', 'admin'] as const),
          jobId: optional(string({ min: 1 })),
        });

        const clockOutInput = object({
          entryId: string({ min: 1 }),
          km: optional(number({ min: 0, max: 2000 })),
          note: optional(string({ max: 500 })),
        });

        const correctionInput = object({ minutes: integer({ min: 1, max: 1440 }), note: string({ min: 3, max: 500 }) });
        const approvalInput = object({ entryIds: array(string({ min: 1 }), { min: 1, max: 200 }) });

        /** Technicians act only for themselves; staff may act for anyone. */
        function ownTechnician(actor: Actor, technicianId: string): string {
          if (actor.role === 'technician' && actor.subjectId !== technicianId) {
            throw new ForbiddenError(`act for technician ${technicianId}`, actor.id);
          }
          return technicianId;
        }
    '''
    groups = {
        "src/adapters/repositories/time-entries.ts": ["entriesForTechnician", "timeEntries"],
        "src/api/http.ts": ["created", "ok", "param", "queryDate"],
        "src/auth/permissions.ts": ["authorize"],
        "src/auth/roles.ts": ["type Actor"],
        "src/shared/errors.ts": ["ForbiddenError"],
        "src/shared/time.ts": ["atMinute"],
        "src/shared/validate.ts": ["array", "integer", "number", "object", "oneOf", "optional", "parseInput",
                                   "string"],
        "src/timesheets/approvals.ts": ["approveEntries", "pendingApprovals", "rejectEntry"],
        "src/timesheets/clock.ts": ["clockIn", "clockOut"],
        "src/timesheets/entries.ts": ["correctEntry", "logEntry"],
        "src/timesheets/mileage.ts": ["mileageClaim"],
        "src/timesheets/summary.ts": ["weeklySummary"],
    }
    return route_module(ctx, "src/api/routes/timesheets.ts", "timesheet", routes, groups, preamble)


def billing_routes(ctx):
    J = j(ctx)
    routes = [
        Route("POST", f"{J}/:id/invoice", "billing:write", '''
            const invoice = draftInvoiceForJob(ctx, request.actor.id, param(request, 'id'));
            return created(invoiceView(invoice));
        '''),
        Route("GET", "/invoices/overdue", "billing:read", '''
            const overdue = overdueInvoices(ctx, queryDate(request, 'on', dateOf(ctx.clock.now())));
            const items = request.actor.role === 'customer'
              ? overdue.filter((invoice) => invoice.customerId === request.actor.subjectId)
              : overdue;
            return ok({ items });
        '''),
        Route("GET", "/invoices/:id", "billing:read", '''
            const invoice = invoices(ctx).require(param(request, 'id'), 'invoice');
            assertCustomerScope(request.actor, invoice.customerId);
            if (request.actor.role === 'customer' && invoice.status === 'draft') throw new NotFoundError('invoice', invoice.id);
            return ok(invoiceView(invoice, creditedCents(ctx, invoice.id)));
        '''),
        Route("POST", "/invoices/:id/issue", "billing:write", '''
            return ok(invoiceView(issueInvoice(ctx, request.actor.id, param(request, 'id'))));
        '''),
        Route("POST", "/invoices/:id/void", "billing:write", '''
            const { reason } = parseInput(reasonInput, request.body);
            return ok(invoiceView(voidInvoice(ctx, request.actor.id, param(request, 'id'), reason)));
        '''),
        Route("POST", "/invoices/:id/payments", "billing:write", '''
            const data = parseInput(paymentInput, request.body);
            const payment = recordPayment(ctx, request.actor.id, param(request, 'id'), data.amountCents, data.method, data.reference);
            return created(payment);
        '''),
        Route("POST", "/invoices/:id/credit-notes", "billing:write", '''
            const data = parseInput(creditInput, request.body);
            return created(issueCreditNote(ctx, request.actor.id, param(request, 'id'), data.amountCents, data.reason));
        '''),
        Route("GET", "/invoices/:id/late-fee", "billing:read", '''
            const invoice = invoices(ctx).require(param(request, 'id'), 'invoice');
            assertCustomerScope(request.actor, invoice.customerId);
            return ok({ lateFeeCents: lateFee(ctx, invoice, queryDate(request, 'on', dateOf(ctx.clock.now()))) });
        '''),
        Route("GET", "/customers/:id/statement", "billing:read", '''
            const customerId = param(request, 'id');
            assertCustomerScope(request.actor, customerId);
            return ok(customerStatement(ctx, customerId, queryDate(request, 'asOf', dateOf(ctx.clock.now()))));
        '''),
        Route("GET", "/customers/:id/invoices", "billing:read", '''
            const customerId = param(request, 'id');
            assertCustomerScope(request.actor, customerId);
            const visible = invoicesForCustomer(ctx, customerId).filter(
              (invoice) => request.actor.role !== 'customer' || invoice.status !== 'draft',
            );
            return ok({ items: visible.map((invoice) => invoiceView(invoice, creditedCents(ctx, invoice.id))) });
        '''),
        Route("GET", "/tariffs/:region", "billing:read", '''
            const region = param(request, 'region');
            if (!tariffRegions().includes(region)) throw new NotFoundError('tariff', region);
            return ok(tariffFor(region));
        '''),
    ]
    preamble = '''
        const reasonInput = object({ reason: string({ min: 3, max: 500 }) });

        const paymentInput = object({
          amountCents: integer({ min: 1 }),
          method: oneOf(['card', 'transfer', 'cash', 'direct-debit'] as const),
          reference: optional(string({ max: 60 })),
        });

        const creditInput = object({ amountCents: integer({ min: 1 }), reason: string({ min: 3, max: 500 }) });
    '''
    return route_module(ctx, "src/api/routes/billing.ts", "billing", routes, {
        "src/adapters/repositories/invoices.ts": ["invoices", "invoicesForCustomer"],
        "src/api/http.ts": ["created", "ok", "param", "queryDate"],
        "src/api/serializers.ts": ["invoiceView"],
        "src/auth/permissions.ts": ["assertCustomerScope"],
        "src/billing/credits.ts": ["issueCreditNote"],
        "src/billing/invoices.ts": ["draftInvoiceForJob", "issueInvoice", "voidInvoice"],
        "src/billing/payments.ts": ["creditedCents", "lateFee", "overdueInvoices", "recordPayment"],
        "src/billing/statements.ts": ["customerStatement"],
        "src/billing/tariffs/index.ts": ["tariffFor", "tariffRegions"],
        "src/shared/errors.ts": ["NotFoundError"],
        "src/shared/time.ts": ["dateOf"],
        "src/shared/validate.ts": ["integer", "object", "oneOf", "optional", "parseInput", "string"],
    }, preamble)


def quote_routes(ctx):
    routes = [
        Route("POST", "/quotes", "quotes:write", '''
            return created(createQuote(ctx, request.actor.id, request.body));
        '''),
        Route("GET", "/quotes/:id", "quotes:write", '''
            return ok(quotes(ctx).require(param(request, 'id'), 'quote'));
        '''),
        Route("POST", "/quotes/:id/send", "quotes:write", '''
            return ok(sendQuote(ctx, request.actor.id, param(request, 'id')));
        '''),
        Route("POST", "/quotes/:id/accept", None, '''
            const quote = quotes(ctx).require(param(request, 'id'), 'quote');
            if (request.actor.role === 'customer') assertCustomerScope(request.actor, quote.customerId);
            else authorize(request.actor, 'quotes:write');
            return ok(acceptQuote(ctx, request.actor.id, quote.id));
        '''),
        Route("POST", "/quotes/:id/decline", None, '''
            const quote = quotes(ctx).require(param(request, 'id'), 'quote');
            if (request.actor.role === 'customer') assertCustomerScope(request.actor, quote.customerId);
            else authorize(request.actor, 'quotes:write');
            const { reason } = parseInput(object({ reason: string({ min: 3, max: 500 }) }), request.body);
            return ok(declineQuote(ctx, request.actor.id, quote.id, reason));
        '''),
        Route("GET", "/customers/:id/quotes", "quotes:write", '''
            return ok({ items: openQuotesForCustomer(ctx, param(request, 'id')) });
        '''),
        Route("POST", "/quotes/expire", "system:admin", '''
            return ok({ expired: expireQuotes(ctx, dateOf(ctx.clock.now())) });
        '''),
    ]
    return route_module(ctx, "src/api/routes/quotes.ts", "quote", routes, {
        "src/adapters/repositories/quotes.ts": ["openQuotesForCustomer", "quotes"],
        "src/api/http.ts": ["created", "ok", "param"],
        "src/auth/permissions.ts": ["assertCustomerScope", "authorize"],
        "src/quotes/service.ts": ["acceptQuote", "createQuote", "declineQuote", "expireQuotes", "sendQuote"],
        "src/shared/time.ts": ["dateOf"],
        "src/shared/validate.ts": ["object", "parseInput", "string"],
    })


def notification_routes(ctx):
    routes = [
        Route("GET", "/notifications", "notifications:read", '''
            const requested = optionalQuery(request, 'recipient');
            const recipientId = request.actor.role === 'customer' ? request.actor.subjectId ?? request.actor.id : requested ?? request.actor.id;
            return ok({ items: notificationsFor(ctx, recipientId) });
        '''),
        Route("GET", "/notifications/templates", "system:admin", '''
            return ok({ items: ctx.templates.keys() });
        '''),
        Route("PUT", "/preferences/:recipient", "notifications:read", '''
            const recipientId = param(request, 'recipient');
            if (request.actor.role === 'customer' && request.actor.subjectId !== recipientId) {
              throw new ForbiddenError(`change preferences of ${recipientId}`, request.actor.id);
            }
            const data = parseInput(preferenceInput, request.body);
            return ok(setPreference(ctx, recipientId, data.channels, data.optOut));
        '''),
        Route("POST", "/notifications/flush", "system:admin", '''
            return ok({ sent: flushOutbox(ctx) });
        '''),
        Route("GET", f"{j(ctx)}/:id/notifications", "jobs:read", '''
            const job = visibleJob(ctx, request.actor, param(request, 'id'));
            return ok({ items: notificationsForJob(ctx, job.id) });
        '''),
    ]
    preamble = '''
        const CHANNELS = ['email', 'sms', 'portal'] as const;

        const preferenceInput = object({
          channels: array(oneOf(CHANNELS), { min: 1, max: 3 }),
          optOut: withDefault(array(string({ min: 1, max: 60 }), { max: 30 }), []),
        });
    '''
    return route_module(ctx, "src/api/routes/notifications.ts", "notification", routes, {
        "src/adapters/repositories/notifications.ts": ["notificationsFor", "notificationsForJob"],
        "src/api/http.ts": ["ok", "optionalQuery", "param"],
        "src/api/scope.ts": ["visibleJob"],
        "src/notifications/notify.ts": ["flushOutbox"],
        "src/notifications/preferences.ts": ["setPreference"],
        "src/shared/errors.ts": ["ForbiddenError"],
        "src/shared/validate.ts": ["array", "object", "oneOf", "parseInput", "string", "withDefault"],
    }, preamble)


def report_routes(ctx):
    routes = [
        Route("GET", "/reports/kpis", "reports:read", '''
            const from = queryDate(request, 'from');
            const to = queryDate(request, 'to');
            return ok(kpiSnapshot(ctx, { from: atMinute(from, 0), to: atMinute(to, 0) }));
        '''),
        Route("GET", "/reports/utilisation", "reports:read", '''
            return ok({ items: utilisationReport(ctx, queryDate(request, 'from'), queryDate(request, 'to')) });
        '''),
        Route("GET", "/reports/backlog", "reports:read", '''
            return ok({ aging: backlogAging(ctx), byRegion: backlogByRegion(ctx) });
        '''),
        Route("GET", "/reports/revenue", "reports:read", '''
            const rows = revenueByRegion(ctx, queryDate(request, 'from'), queryDate(request, 'to'));
            if (optionalQuery(request, 'format') === 'csv') {
              return { status: 200, body: toCsv(rows.map((row) => ({ ...row }))), headers: { 'content-type': 'text/csv' } };
            }
            return ok({ items: rows });
        '''),
        Route("GET", "/reports/activity", "reports:read", '''
            return ok(recentActivity(ctx, queryInteger(request, 'limit', 200, { min: 1, max: 200 })));
        '''),
    ]
    return route_module(ctx, "src/api/routes/reports.ts", "report", routes, {
        "src/api/http.ts": ["ok", "optionalQuery", "queryDate", "queryInteger"],
        "src/reports/activity.ts": ["recentActivity"],
        "src/reports/backlog.ts": ["backlogAging", "backlogByRegion"],
        "src/reports/csv.ts": ["toCsv"],
        "src/reports/kpis.ts": ["kpiSnapshot"],
        "src/reports/revenue.ts": ["revenueByRegion"],
        "src/reports/utilisation.ts": ["utilisationReport"],
        "src/shared/time.ts": ["atMinute"],
    })


def integration_routes(ctx):
    routes = [
        Route("POST", "/webhooks", "system:admin", '''
            const data = parseInput(webhookInput, request.body);
            const webhook = registerWebhook(ctx, request.actor.id, data.url, data.topics, data.customerId);
            return created({ ...webhook, secret: undefined, secretHint: `${webhook.secret.slice(0, 4)}…` });
        '''),
        Route("GET", "/webhooks/topics", "system:admin", '''
            return ok({ items: [...new Set(Object.values(EXTERNAL_TOPICS))].sort() });
        '''),
        Route("GET", "/webhooks/deliveries", "system:admin", '''
            const topic = optionalQuery(request, 'topic');
            const items = ctx.store
              .collection<Delivery>('deliveries')
              .find((delivery) => topic === undefined || delivery.topic === topic);
            return ok({ items });
        '''),
        Route("GET", "/exports/journal", "billing:read", '''
            const exported = journalExport(ctx, queryDate(request, 'from'), queryDate(request, 'to'), optionalQuery(request, 'format') ?? 'csv');
            return { status: 200, body: exported.body, headers: { 'content-type': exported.contentType } };
        '''),
    ]
    preamble = '''
        const webhookInput = object({
          url: string({ min: 10, max: 500 }),
          topics: array(string({ min: 1, max: 60 }), { min: 1, max: 20 }),
          customerId: optional(string({ min: 1 })),
        });
    '''
    return route_module(ctx, "src/api/routes/integrations.ts", "integration", routes, {
        "src/api/http.ts": ["created", "ok", "optionalQuery", "queryDate"],
        "src/integrations/accounting.ts": ["journalExport"],
        "src/integrations/types.ts": ["type Delivery"],
        "src/integrations/webhooks.ts": ["EXTERNAL_TOPICS", "registerWebhook"],
        "src/shared/validate.ts": ["array", "object", "optional", "parseInput", "string"],
    }, preamble)


def system_routes(ctx):
    routes = [
        Route("GET", "/health", None, '''
            return ok({ status: 'ok', time: ctx.clock.now(), organization: ctx.config.organization.name });
        '''),
        Route("GET", "/config", "system:admin", '''
            return ok(ctx.config);
        '''),
        Route("GET", "/work-types", "jobs:read", '''
            const items = ctx.workTypes.entries().map(([key, handler]) => ({
              key,
              label: handler.label,
              skills: handler.skills,
              defaultDurationMinutes: handler.defaultDurationMinutes,
              defaultPriority: handler.defaultPriority,
              requiresAsset: handler.requiresAsset,
            }));
            return ok({ items });
        '''),
        Route("GET", "/audit/:subject", "system:admin", '''
            return ok({ items: auditTrail(ctx, param(request, 'subject')) });
        '''),
    ]
    return route_module(ctx, "src/api/routes/system.ts", "system", routes, {
        "src/adapters/store/audit.ts": ["auditTrail"],
        "src/api/http.ts": ["ok", "param"],
    })


AREAS = ("system", "customer", "contract", "asset", "job", "sla", "scheduling", "dispatch", "technician",
         "inventory", "timesheet", "billing", "quote", "notification", "report", "integration")
AREA_FILES = {"system": "system", "customer": "customers", "contract": "contracts", "asset": "assets",
              "job": "jobs", "sla": "sla", "scheduling": "scheduling", "dispatch": "dispatch",
              "technician": "technicians", "inventory": "inventory", "timesheet": "timesheets",
              "billing": "billing", "quote": "quotes", "notification": "notifications", "report": "reports",
              "integration": "integrations"}


def routes_index(ctx):
    groups = {}
    names = []
    for area in AREAS:
        file = AREA_FILES[area]
        name = area
        if area == "job" and ctx.split:
            file, name = "workorders", "workOrder"
        capital = name[0].upper() + name[1:]
        symbol = f"{name}Routes" if ctx.refactor else f"register{capital}Routes"
        groups[f"src/api/routes/{file}.ts"] = [symbol]
        names.append(symbol)
    if ctx.refactor:
        groups["src/api/router.ts"] = ["type RouteDefinition", "type Router"]
        spread = "".join(f"  ...{name},\n" for name in names)
        body = f'''
export const ROUTES: readonly RouteDefinition[] = [
{spread}];

export function registerRoutes(router: Router): void {{
  router.mount(ROUTES);
}}
'''
    else:
        groups["src/api/router.ts"] = ["type Router"]
        calls = "".join(f"  {name}(router);\n" for name in names)
        body = f'''
export function registerRoutes(router: Router): void {{
{calls}}}
'''
    return render(ctx, "«imports»\n" + body, imports=imports(ctx, "src/api/routes/index.ts", groups))


def app_module(ctx):
    return render(ctx, '''
«imports»

export interface AppOptions {
  now?: Timestamp;
  config?: DeepPartial<AppConfig>;
  logLevel?: LogLevel;
}

export interface RequestOptions {
  actor?: string;
  body?: unknown;
}

export interface App {
  ctx: AppContext;
  clock: ManualClock;
  request(method: Method, url: string, options?: RequestOptions): ApiResponse;
  routes(): Array<{ method: Method; pattern: string }>;
}

export const DEFAULT_START: Timestamp = '2026-03-02T08:00:00Z';

/**
 * Builds an isolated application: configuration, in-memory store, registries, event
 * subscriptions and routes. Responses are plain JSON values, as a network client would see them.
 */
export function createApp(options: AppOptions = {}): App {
  const clock = createManualClock(options.now ?? DEFAULT_START);
  const ctx: AppContext = {
    config: resolveConfig(options.config),
    store: createMemoryStore(),
    bus: createEventBus(),
    clock,
    ids: createIdSource(),
    log: createLogger(options.logLevel ?? 'warn'),
    workTypes: createWorkTypeRegistry(),
    templates: createTemplateRegistry(),
    scoringRules: createScoringRegistry(),
    exportFormats: createExportRegistry(),
  };
  seedStaff(ctx);
  «registerJobSubscriptions»(ctx);
  registerInventorySubscriptions(ctx);
  registerSlaSubscriptions(ctx);
  registerBillingSubscriptions(ctx);
  registerNotificationSubscriptions(ctx);
  registerWebhookSubscriptions(ctx);

  const router = createRouter();
  registerRoutes(router);

  return {
    ctx,
    clock,
    routes: () => router.table(),
    request(method, url, requestOptions = {}) {
      const [path, search = ''] = url.split('?', 2);
      const match = router.match(method, path);
      if (!match) {
        const allowed = router.methodsFor(path);
        return allowed.length > 0 ? methodNotAllowed(allowed) : routeNotFound(method, path);
      }
      const actor = requestOptions.actor === undefined ? undefined : findActor(ctx, requestOptions.actor);
      if (!actor) return unauthenticated();
      const query = Object.fromEntries(new URLSearchParams(search));
      const body = requestOptions.body === undefined ? undefined : structuredClone(requestOptions.body);
      let response: ApiResponse;
      try {
        response = match.handler(ctx, { method, path, params: match.params, query, body, actor });
      } catch (error) {
        response = errorResponse(ctx.log, error);
      }
      return response.body === undefined || response.body === null || typeof response.body === 'string'
        ? response
        : { ...response, body: JSON.parse(JSON.stringify(response.body)) };
    },
  };
}
''', imports=imports(ctx, "src/app.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/store/memory.ts": ["createMemoryStore"],
        "src/api/errors.ts": ["errorResponse", "methodNotAllowed", "routeNotFound", "unauthenticated"],
        "src/api/http.ts": ["type ApiResponse", "type Method"],
        "src/api/router.ts": ["createRouter"],
        "src/api/routes/index.ts": ["registerRoutes"],
        "src/auth/actors.ts": ["findActor", "seedStaff"],
        "src/billing/subscriptions.ts": ["registerBillingSubscriptions"],
        "src/config/load.ts": ["resolveConfig"],
        "src/config/schema.ts": ["type AppConfig", "type DeepPartial"],
        "src/dispatch/rules/index.ts": ["createScoringRegistry"],
        "src/integrations/formats.ts": ["createExportRegistry"],
        "src/integrations/webhooks.ts": ["registerWebhookSubscriptions"],
        "src/inventory/reservations.ts": ["registerInventorySubscriptions"],
        "src/notifications/subscriptions.ts": ["registerNotificationSubscriptions"],
        "src/notifications/templates.ts": ["createTemplateRegistry"],
        "src/shared/clock.ts": ["createManualClock", "type ManualClock"],
        "src/shared/events.ts": ["createEventBus"],
        "src/shared/ids.ts": ["createIdSource"],
        "src/shared/log.ts": ["createLogger", "type LogLevel"],
        "src/shared/time.ts": ["type Timestamp"],
        "src/sla/subscriptions.ts": ["registerSlaSubscriptions"],
        "src/worktypes/registry.ts": ["createWorkTypeRegistry"],
    }, jobs=("registerJobSubscriptions",)))


ROUTE_BUILDERS = {
    "system": system_routes, "customer": customer_routes, "contract": contract_routes, "asset": asset_routes,
    "job": job_routes, "sla": sla_routes, "scheduling": scheduling_routes, "dispatch": dispatch_routes,
    "technician": technician_routes, "inventory": inventory_routes, "timesheet": timesheet_routes,
    "billing": billing_routes, "quote": quote_routes, "notification": notification_routes,
    "report": report_routes, "integration": integration_routes,
}


def generate(ctx):
    ctx.add("src/api/http.ts", http_module(ctx))
    ctx.add("src/api/router.ts", router_module(ctx))
    ctx.add("src/api/errors.ts", errors_module(ctx))
    ctx.add("src/api/scope.ts", scope_module(ctx))
    ctx.add("src/api/serializers.ts", serializers_module(ctx))
    for area in AREAS:
        file = AREA_FILES[area]
        if area == "job" and ctx.split:
            file = "workorders"
        ctx.add(f"src/api/routes/{file}.ts", ROUTE_BUILDERS[area](ctx))
    ctx.add("src/api/routes/index.ts", routes_index(ctx))
    ctx.add("src/app.ts", app_module(ctx))
