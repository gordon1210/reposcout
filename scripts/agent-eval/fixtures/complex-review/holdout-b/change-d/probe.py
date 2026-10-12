"""Private independent public-workflow observations; never installed in review trees."""
import subprocess
import sys

SCRIPT = r'''
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { createApp } = await import(pathToFileURL(join(process.argv[1], 'src/app.ts')).href);

const MONDAY = '2026-03-02';
const at = (date, clock) => `${date}T${clock}:00Z`;

function send(app, method, url, actor, body) {
  const response = app.request(method, url, { actor, body });
  return { status: response.status, body: response.body };
}

function ok(app, method, url, actor, body) {
  const response = send(app, method, url, actor, body);
  if (response.status >= 300) throw new Error(`${method} ${url}: ${response.status} ${JSON.stringify(response.body)}`);
  return response.body;
}

function world() {
  const app = createApp({ now: at(MONDAY, '08:00') });
  const customer = ok(app, 'POST', '/customers', 'admin', {
    name: 'Harbour View Apartments', segment: 'commercial', billingRegion: 'metro-east',
    email: 'facilities@harbourview.example', taxExempt: false,
  });
  const site = ok(app, 'POST', `/customers/${customer.id}/sites`, 'admin', {
    label: 'Main building',
    address: { line1: '12 Quay Street', city: 'Eastport', postcode: 'EP1 4QS', region: 'metro-east', location: { lat: 52.52, lon: 13.4 } },
  });
  const technician = (skills) => ok(app, 'POST', '/technicians', 'admin', {
    name: 'Robin Hale', email: `robin${skills.length}@fieldline.example`, region: 'metro-east', home: { lat: 52.5, lon: 13.38 },
    hourlyCostCents: 3200, certifications: skills.map((skill) => ({ skill })),
  });
  return { app, customer, site, technician };
}

const KEYS = [
  'general', 'boiler-service', 'boiler-repair', 'heat-pump-install', 'chiller-maintenance', 'emergency-callout',
  'leak-investigation', 'commissioning',
];
const { app, customer, site, technician } = world();
const result = {};

// Catalogue entries of work types that already exist.
const listed = ok(app, 'GET', '/work-types', 'dispatch').items.filter((item) => KEYS.includes(item.key));
result.catalogue = listed.map((item) => [item.key, item.label, item.skills, item.defaultDurationMinutes, item.defaultPriority, item.requiresAsset]);

// Opening work orders: handler priority, checklist, key case and the general fallback.
const open = (workType, extra = {}) => send(app, 'POST', '/jobs', 'dispatch', {
  customerId: customer.id, siteId: site.id, workType, summary: 'Investigate reported fault', ...extra,
});
const boiler = ok(app, 'POST', '/assets', 'admin', {
  siteId: site.id, kind: 'boiler', manufacturer: 'Acme Thermal', model: 'B-100', serial: 'SN-B-1', installedOn: '2024-06-01',
});
result.opened = {};
for (const key of ['general', 'EMERGENCY-CALLOUT', 'leak-investigation', 'teleportation']) {
  const response = open(key);
  result.opened[key] = [response.status, response.body.workType, response.body.priority, Object.keys(response.body.checklist ?? {})];
}
const serviceJob = open('boiler-service', { assetId: boiler.id });
result.opened['boiler-service'] = [serviceJob.status, serviceJob.body.workType, serviceJob.body.priority, Object.keys(serviceJob.body.checklist)];
const noAsset = open('chiller-maintenance');
result.assetRequired = [noAsset.status, noAsset.body.error?.code];

// Asset kinds: intervals of existing kinds and the rejection of an unknown kind.
const register = (kind, serial) => send(app, 'POST', '/assets', 'admin', {
  siteId: site.id, kind, manufacturer: 'Acme Thermal', model: 'M-1', serial, installedOn: '2024-06-01',
});
result.assets = {};
for (const [kind, serial] of [['boiler', 'SN-1'], ['chiller', 'SN-2'], ['elevator', 'SN-3'], ['solar-inverter', 'SN-4'], ['tumble-dryer', 'SN-5']]) {
  const response = register(kind, serial);
  result.assets[kind] = [response.status, response.body.serviceIntervalDays ?? response.body.error?.code];
}

// Qualification and completion rules of existing handlers.
const assign = (jobId, tech) => send(app, 'POST', `/jobs/${jobId}/assign`, 'dispatch', { technicianId: tech.id });
const unqualified = assign(serviceJob.body.id, technician([]));
result.unqualified = [unqualified.status, unqualified.body.error?.code, unqualified.body.error?.details?.missing];
const gas = technician(['gas-safe', 'x']);
result.assigned = assign(serviceJob.body.id, gas).status;
send(app, 'POST', `/jobs/${serviceJob.body.id}/start`, gas.id);
const current = ok(app, 'GET', `/jobs/${serviceJob.body.id}`, gas.id);
for (const item of Object.keys(current.checklist)) ok(app, 'PUT', `/jobs/${serviceJob.body.id}/checklist`, gas.id, { item, done: true });
const rejected = send(app, 'POST', `/jobs/${serviceJob.body.id}/complete`, gas.id, { resolution: 'Service completed, combustion within limits' });
result.completion = [rejected.status, rejected.body.error?.code, rejected.body.error?.details?.problems];
const accepted = send(app, 'POST', `/jobs/${serviceJob.body.id}/complete`, gas.id, {
  resolution: 'Service completed, combustion within limits', signatureName: 'Avery Collins',
});
result.completed = [accepted.status, accepted.body.status];

console.log(JSON.stringify(result));
'''

result = subprocess.run(["node", "--input-type=module", "-e", SCRIPT, sys.argv[1]],
                        text=True, capture_output=True, check=True, timeout=8)
sys.stdout.write(result.stdout)
