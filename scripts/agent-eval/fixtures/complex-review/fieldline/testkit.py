"""Test runner and shared test support for the generated application."""

from .core import imports, render

RUNNER = '''
// Runs every tests/**/*.test.ts file in one process with node:test and prints a compact report.
import { readdirSync } from 'node:fs';
import { join, relative } from 'node:path';
import { run } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('.', import.meta.url));
const filter = process.argv[2];

function collect(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) return collect(path);
      return entry.name.endsWith('.test.ts') ? [path] : [];
    })
    .sort();
}

const files = collect(root).filter((file) => !filter || relative(root, file).includes(filter));
if (files.length === 0) {
  console.error(`no test files match ${filter ?? 'tests/**/*.test.ts'}`);
  process.exit(1);
}

let passed = 0;
const failures = [];
const stream = run({ files, isolation: 'none', concurrency: 1 });
stream.on('test:pass', (event) => {
  if (event.details?.type !== 'suite') passed += 1;
});
stream.on('test:fail', (event) => {
  if (event.details?.type === 'suite') return;
  const cause = event.details?.error?.cause ?? event.details?.error;
  failures.push(`${event.file ? relative(root, event.file) : '?'} > ${event.name}\\n    ${String(cause?.message ?? cause).split('\\n').join('\\n    ')}`);
});
stream.on('test:diagnostic', (event) => {
  if (/^(tests|suites|pass|fail|cancelled|skipped|todo|duration_ms) /.test(event.message)) return;
  console.log(event.message);
});
for await (const _ of stream) {
  // Events are handled by the listeners above.
}
for (const failure of failures) console.error(`FAIL ${failure}`);
console.log(`${files.length} files, ${passed} passed, ${failures.length} failed`);
process.exitCode = failures.length > 0 ? 1 : 0;
'''


def world_module(ctx):
    jp = "/workorders" if ctx.split else "/jobs"
    return render(ctx, '''
import assert from 'node:assert/strict';
«imports»

export const MONDAY = '2026-03-02';
export const at = (date: string, clock: string): string => `${date}T${clock}:00Z`;

export interface Reply {
  status: number;
  body: any;
}

/** Sends a request and fails with the response body when the status differs from `expected`. */
export function expectStatus(app: App, expected: number, method: Method, url: string, actor: string, body?: unknown): any {
  const response = app.request(method, url, { actor, body });
  if (response.status !== expected) {
    assert.fail(`${method} ${url} as ${actor}: expected ${expected}, got ${response.status} ${JSON.stringify(response.body)}`);
  }
  return response.body;
}

export const call = (app: App, method: Method, url: string, actor: string, body?: unknown): any => {
  const response = app.request(method, url, { actor, body });
  if (response.status >= 300) {
    assert.fail(`${method} ${url} as ${actor}: ${response.status} ${JSON.stringify(response.body)}`);
  }
  return response.body;
};

export const JOBS = '«jp»';

export interface CustomerSeed {
  name?: string;
  region?: string;
  segment?: string;
  taxExempt?: boolean;
  location?: { lat: number; lon: number };
}

export function seedCustomer(app: App, seed: CustomerSeed = {}) {
  const region = seed.region ?? 'metro-east';
  const customer = call(app, 'POST', '/customers', 'admin', {
    name: seed.name ?? 'Harbour View Apartments',
    segment: seed.segment ?? 'commercial',
    billingRegion: region,
    email: 'facilities@harbourview.example',
    taxExempt: seed.taxExempt ?? false,
  });
  const site = call(app, 'POST', `/customers/${customer.id}/sites`, 'admin', {
    label: 'Main building',
    address: {
      line1: '12 Quay Street',
      city: 'Eastport',
      postcode: 'EP1 4QS',
      region,
      location: seed.location ?? { lat: 52.52, lon: 13.4 },
    },
  });
  return { customer, site };
}

export function seedContract(app: App, customerId: string, siteIds: string[], plan: string, coveredWorkTypes: string[], startsOn = '2026-01-01') {
  const draft = call(app, 'POST', '/contracts', 'admin', { customerId, siteIds, plan, startsOn, coveredWorkTypes });
  return call(app, 'POST', `/contracts/${draft.id}/activate`, 'admin');
}

export interface TechnicianSeed {
  name?: string;
  email?: string;
  region?: string;
  skills?: string[];
  home?: { lat: number; lon: number };
}

let technicianCount = 0;

export function seedTechnician(app: App, seed: TechnicianSeed = {}) {
  technicianCount += 1;
  return call(app, 'POST', '/technicians', 'admin', {
    name: seed.name ?? `Technician ${technicianCount}`,
    email: seed.email ?? `tech${technicianCount}@fieldline.example`,
    region: seed.region ?? 'metro-east',
    home: seed.home ?? { lat: 52.5, lon: 13.38 },
    hourlyCostCents: 3200,
    certifications: (seed.skills ?? []).map((skill) => ({ skill })),
  });
}

export function openWorkOrder(app: App, customerId: string, siteId: string, workType = 'general', extra: Record<string, unknown> = {}) {
  return call(app, 'POST', JOBS, 'dispatch', { customerId, siteId, workType, summary: 'Investigate reported fault', ...extra });
}

let assetCount = 0;

export function seedAsset(app: App, siteId: string, kind = 'boiler', extra: Record<string, unknown> = {}) {
  assetCount += 1;
  return call(app, 'POST', '/assets', 'admin', {
    siteId,
    kind,
    manufacturer: 'Acme Thermal',
    model: `Model ${assetCount}`,
    serial: `SN-${kind.toUpperCase()}-${assetCount.toString().padStart(4, '0')}`,
    installedOn: '2024-06-01',
    ...extra,
  });
}

/** Ticks every checklist item of a work order as the given actor. */
export function completeChecklist(app: App, jobId: string, actor: string): void {
  const job = call(app, 'GET', `${JOBS}/${jobId}`, actor);
  for (const item of Object.keys(job.checklist)) call(app, 'PUT', `${JOBS}/${jobId}/checklist`, actor, { item, done: true });
}

export interface World {
  app: App;
  customer: any;
  site: any;
  technician: any;
}

/** A metro-east customer with one site and one general technician, at Monday 08:00. */
export function createWorld(options: AppOptions = {}, customer: CustomerSeed = {}): World {
  const app = createApp({ now: at(MONDAY, '08:00'), ...options });
  const { customer: created, site } = seedCustomer(app, customer);
  const technician = seedTechnician(app, { skills: ['gas-safe', 'plumbing', 'electrical'] });
  return { app, customer: created, site, technician };
}
''', jp=jp, imports=imports(ctx, "tests/support/world.ts", {
        "src/app.ts": ["createApp", "type App", "type AppOptions"],
        "src/api/http.ts": ["type Method"],
    }))


def generate(ctx):
    ctx.add("tests/run.mjs", RUNNER)
    ctx.add("tests/support/world.ts", world_module(ctx))
