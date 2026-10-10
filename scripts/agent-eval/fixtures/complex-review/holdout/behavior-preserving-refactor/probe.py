"""Private independent public-workflow observations; never installed in review trees."""
import subprocess
import sys

SCRIPT = r'''
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { createApp } = await import(pathToFileURL(join(process.argv[1], 'src/app.ts')).href);

const MONDAY = '2026-03-02';
const at = (date, clock) => `${date}T${clock}:00Z`;

function call(app, method, url, actor, body) {
  const response = app.request(method, url, { actor, body });
  if (response.status >= 300) throw new Error(`${method} ${url}: ${response.status} ${JSON.stringify(response.body)}`);
  return response.body;
}

function world(region) {
  const app = createApp({ now: at(MONDAY, '08:00') });
  const customer = call(app, 'POST', '/customers', 'admin', {
    name: 'Harbour View Apartments', segment: 'commercial', billingRegion: region,
    email: 'facilities@harbourview.example', taxExempt: false,
  });
  const site = call(app, 'POST', `/customers/${customer.id}/sites`, 'admin', {
    label: 'Main building',
    address: { line1: '12 Quay Street', city: 'Eastport', postcode: 'EP1 4QS', region, location: { lat: 52.52, lon: 13.4 } },
  });
  const technician = call(app, 'POST', '/technicians', 'admin', {
    name: 'Robin Hale', email: 'robin@fieldline.example', region, home: { lat: 52.5, lon: 13.38 },
    hourlyCostCents: 3200, certifications: [{ skill: 'plumbing' }],
  });
  return { app, customer, site, technician };
}

const REGIONS = ['capital', 'coastal', 'highlands', 'lakeside', 'metro-east', 'metro-west', 'northern-hills', 'valley'];
const outcome = (response) => response.status < 300 ? response.status : `${response.status}:${response.body?.error?.code}`;

// Status and error code of every route for every kind of actor, with unknown identifiers and an empty body.
function permissions() {
  const w = world('metro-east');
  const portal = call(w.app, 'POST', `/customers/${w.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
  const actors = ['admin', 'dispatch', 'billing', 'warehouse', w.technician.id, portal, 'nobody', undefined];
  const routes = w.app.routes().map(({ method, pattern }) => [method, pattern])
    .sort((left, right) => `${left[1]} ${left[0]}`.localeCompare(`${right[1]} ${right[0]}`));
  const result = {};
  for (const [method, pattern] of routes) {
    const url = pattern.replace(/:[a-z]+/gi, 'MISSING-1');
    result[`${method} ${pattern}`] = actors.map((actor) => outcome(w.app.request(method, url, { actor, body: method === 'GET' ? undefined : {} }))).join(' ');
  }
  return result;
}

const collectionSizes = (app) => Object.fromEntries(
  Object.entries(app.ctx.store.snapshot()).map(([name, records]) => [name, records.length]).sort());

function completeAs(app, technician, job, report) {
  const current = call(app, 'GET', `/jobs/${job.id}`, technician);
  for (const item of Object.keys(current.checklist)) call(app, 'PUT', `/jobs/${job.id}/checklist`, technician, { item, done: true });
  return app.request('POST', `/jobs/${job.id}/complete`, { actor: technician, body: report });
}

// Every registered work type: its public definition, its defaults on a new order and a minimal completion.
function workTypes() {
  const w = world('metro-east');
  const listed = call(w.app, 'GET', '/work-types', 'admin').items;
  const skills = [...new Set(listed.flatMap((item) => item.skills))].sort();
  const technician = call(w.app, 'POST', '/technicians', 'admin', {
    name: 'Sam Rivera', email: 'sam@fieldline.example', region: 'metro-east', home: { lat: 52.5, lon: 13.38 },
    hourlyCostCents: 3200, certifications: skills.map((skill) => ({ skill })),
  }).id;
  const asset = call(w.app, 'POST', '/assets', 'admin', {
    siteId: w.site.id, kind: 'boiler', manufacturer: 'Acme Thermal', model: 'Model 1', serial: 'SN-BOILER-0001', installedOn: '2024-06-01',
  });
  const behaviour = {};
  for (const item of listed) {
    const base = { customerId: w.customer.id, siteId: w.site.id, workType: item.key, summary: 'Scheduled attendance' };
    const bare = w.app.request('POST', '/jobs', { actor: 'dispatch', body: base });
    const job = bare.status < 300 ? bare.body : call(w.app, 'POST', '/jobs', 'dispatch', { ...base, assetId: asset.id });
    call(w.app, 'POST', `/jobs/${job.id}/assign`, 'dispatch', { technicianId: technician });
    call(w.app, 'POST', `/jobs/${job.id}/start`, technician);
    const minimal = completeAs(w.app, technician, job, { resolution: 'Completed the scheduled work in full' });
    behaviour[item.key] = {
      withoutAsset: outcome(bare), priority: job.priority, checklist: Object.keys(job.checklist), minimalCompletion: outcome(minimal),
    };
  }
  return { listed, behaviour, collections: collectionSizes(w.app) };
}

// Each region's public tariff and the draft invoice of the same visit in that region.
function tariffs() {
  const result = {};
  for (const region of REGIONS) {
    const { app, customer, site, technician } = world(region);
    const job = call(app, 'POST', '/jobs', 'dispatch', {
      customerId: customer.id, siteId: site.id, workType: 'general', summary: 'Investigate reported fault',
    });
    call(app, 'POST', `/jobs/${job.id}/assign`, 'dispatch', { technicianId: technician.id });
    call(app, 'POST', `/jobs/${job.id}/start`, technician.id);
    app.clock.set(at(MONDAY, '20:00'));
    for (const [kind, from, to, km] of [['labor', '09:00', '10:40'], ['travel', '13:00', '13:35', 18.5], ['labor', '18:00', '18:20']]) {
      call(app, 'POST', '/time-entries', 'admin', { technicianId: technician.id, jobId: job.id, kind, start: at(MONDAY, from), end: at(MONDAY, to), km });
    }
    completeAs(app, technician.id, job, { resolution: 'Replaced the faulty component and tested' });
    const invoice = call(app, 'POST', `/jobs/${job.id}/invoice`, 'billing');
    result[region] = {
      tariff: call(app, 'GET', `/tariffs/${region}`, 'billing'),
      lines: invoice.lines.map((line) => [line.kind, line.quantity, line.unitCents, line.amountCents]),
      totals: [invoice.subtotalCents, invoice.taxCents, invoice.totalCents],
    };
  }
  return result;
}

// Aging of one issued invoice on statement dates around every bucket boundary.
function statements() {
  const { app, customer, site, technician } = world('metro-east');
  const job = call(app, 'POST', '/jobs', 'dispatch', {
    customerId: customer.id, siteId: site.id, workType: 'general', summary: 'Investigate reported fault',
  });
  call(app, 'POST', `/jobs/${job.id}/assign`, 'dispatch', { technicianId: technician.id });
  call(app, 'POST', `/jobs/${job.id}/start`, technician.id);
  completeAs(app, technician.id, job, { resolution: 'Replaced the faulty component and tested' });
  const draft = call(app, 'POST', `/jobs/${job.id}/invoice`, 'billing');
  const issued = call(app, 'POST', `/invoices/${draft.id}/issue`, 'billing');
  const due = Date.parse(`${issued.dueOn}T00:00:00Z`);
  const result = {};
  for (const days of [-1, 0, 1, 30, 31, 60, 61, 90, 91, 120]) {
    const asOf = new Date(due + days * 86_400_000).toISOString().slice(0, 10);
    const statement = call(app, 'GET', `/customers/${customer.id}/statement?asOf=${asOf}`, 'billing');
    result[days] = statement.aging ?? statement.buckets ?? statement;
  }
  return { aging: result, collections: collectionSizes(app) };
}

// Overlap decisions for time entries around an existing 09:00-10:00 entry, and availability around an absence.
function boundaries() {
  const { app, customer, site, technician } = world('metro-east');
  const job = call(app, 'POST', '/jobs', 'dispatch', {
    customerId: customer.id, siteId: site.id, workType: 'general', summary: 'Investigate reported fault',
  });
  app.clock.set(at('2026-03-06', '20:00'));
  const entry = (from, to) => outcome(app.request('POST', '/time-entries', {
    actor: 'admin', body: { technicianId: technician.id, kind: 'admin', start: at(MONDAY, from), end: at(MONDAY, to) },
  }));
  const entries = [['09:00', '10:00'], ['10:00', '10:30'], ['08:30', '09:00'], ['09:59', '10:15'], ['08:00', '09:01'], ['08:00', '11:00'], ['09:15', '09:45']]
    .map(([from, to]) => `${from}-${to} ${entry(from, to)}`);
  call(app, 'POST', `/technicians/${technician.id}/absences`, 'dispatch', { from: '2026-03-10', to: '2026-03-11', kind: 'leave' });
  const availability = ['2026-03-09', '2026-03-10', '2026-03-11', '2026-03-12'].map((date) => {
    const response = app.request('GET', `/technicians/${technician.id}/slots?date=${date}&jobId=${job.id}`, { actor: 'dispatch' });
    return `${date} ${outcome(response)} ${response.status < 300 ? response.body.slots.length : '-'}`;
  });
  return { entries, availability };
}

const permissionMatrix = permissions();
const catalog = workTypes();
const regions = tariffs();
const aging = statements();
const edges = boundaries();
console.log(JSON.stringify({ permissionMatrix, catalog, regions, aging, edges }));
'''

result = subprocess.run(["node", "--input-type=module", "-e", SCRIPT, sys.argv[1]],
                        text=True, capture_output=True, check=True, timeout=9)
sys.stdout.write(result.stdout)
