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

function contracted(w) {
  const draft = call(w.app, 'POST', '/contracts', 'admin', {
    customerId: w.customer.id, siteIds: [w.site.id], plan: 'premium', startsOn: '2026-01-01', coveredWorkTypes: ['general'],
  });
  call(w.app, 'POST', `/contracts/${draft.id}/activate`, 'admin');
}

// A premium-contract work order started at 08:00, held at 09:00 for `reason`, observed at 12:00, resumed then and observed at 13:00.
function held(reason) {
  const w = world('metro-east');
  const { app, technician } = w;
  contracted(w);
  const job = call(app, 'POST', '/jobs', 'dispatch', {
    customerId: w.customer.id, siteId: w.site.id, workType: 'general', summary: 'Investigate reported fault',
  });
  call(app, 'POST', `/jobs/${job.id}/assign`, 'dispatch', { technicianId: technician.id });
  call(app, 'POST', `/jobs/${job.id}/start`, technician.id);
  app.clock.set(at(MONDAY, '09:00'));
  call(app, 'POST', `/jobs/${job.id}/hold`, technician.id, { reason });
  app.clock.set(at(MONDAY, '12:00'));
  const during = call(app, 'GET', `/jobs/${job.id}/sla`, 'dispatch');
  call(app, 'POST', `/jobs/${job.id}/resume`, technician.id);
  app.clock.set(at(MONDAY, '13:00'));
  const after = call(app, 'GET', `/jobs/${job.id}/sla`, 'dispatch');
  return { during: [during.status, during.elapsedMinutes], after: [after.status, after.elapsedMinutes] };
}

const result = {};
for (const reason of ['customer_unavailable', 'access_denied', 'awaiting_approval', 'awaiting_parts', 'weather']) result[reason] = held(reason);
result.webhookTopics = call(world('metro-east').app, 'GET', '/webhooks/topics', 'admin');
console.log(JSON.stringify(result));
'''

result = subprocess.run(["node", "--input-type=module", "-e", SCRIPT, sys.argv[1]],
                        text=True, capture_output=True, check=True, timeout=8)
sys.stdout.write(result.stdout)
