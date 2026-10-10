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

// One completed general visit: 100 minutes of labour and one trip of `km`, invoiced as drafted.
function visit(region, km) {
  const { app, customer, site, technician } = world(region);
  const job = call(app, 'POST', '/jobs', 'dispatch', {
    customerId: customer.id, siteId: site.id, workType: 'general', summary: 'Investigate reported fault',
  });
  call(app, 'POST', `/jobs/${job.id}/assign`, 'dispatch', { technicianId: technician.id });
  call(app, 'POST', `/jobs/${job.id}/start`, technician.id);
  app.clock.set(at(MONDAY, '18:00'));
  call(app, 'POST', '/time-entries', 'admin', {
    technicianId: technician.id, jobId: job.id, kind: 'labor', start: at(MONDAY, '09:00'), end: at(MONDAY, '10:40'),
  });
  call(app, 'POST', '/time-entries', 'admin', {
    technicianId: technician.id, jobId: job.id, kind: 'travel', start: at(MONDAY, '13:00'), end: at(MONDAY, '13:20'), km,
  });
  const current = call(app, 'GET', `/jobs/${job.id}`, technician.id);
  for (const item of Object.keys(current.checklist)) {
    call(app, 'PUT', `/jobs/${job.id}/checklist`, technician.id, { item, done: true });
  }
  call(app, 'POST', `/jobs/${job.id}/complete`, technician.id, { resolution: 'Replaced the faulty component and tested' });
  const invoice = call(app, 'POST', `/jobs/${job.id}/invoice`, 'billing');
  const lines = {};
  for (const line of invoice.lines) lines[line.kind] = { quantity: line.quantity, amountCents: line.amountCents };
  return { lines, totalCents: invoice.totalCents };
}

const result = {};
for (const km of [4.4, 7.5, 12.6, 23.5]) result[`metro-east ${km} km`] = visit('metro-east', km);
console.log(JSON.stringify(result));
'''

result = subprocess.run(["node", "--input-type=module", "-e", SCRIPT, sys.argv[1]],
                        text=True, capture_output=True, check=True, timeout=8)
sys.stdout.write(result.stdout)
