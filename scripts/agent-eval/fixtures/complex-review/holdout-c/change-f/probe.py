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

const app = createApp({ now: at(MONDAY, '08:00') });
const customer = ok(app, 'POST', '/customers', 'admin', {
  name: 'Harbour View Apartments', segment: 'commercial', billingRegion: 'metro-east',
  email: 'facilities@harbourview.example', taxExempt: false,
});
const site = ok(app, 'POST', `/customers/${customer.id}/sites`, 'admin', {
  label: 'Main building',
  address: { line1: '12 Quay Street', city: 'Eastport', postcode: 'EP1 4QS', region: 'metro-east', location: { lat: 52.52, lon: 13.4 } },
});
const technician = ok(app, 'POST', '/technicians', 'admin', {
  name: 'Robin Hale', email: 'robin@fieldline.example', region: 'metro-east', home: { lat: 52.5, lon: 13.38 },
  hourlyCostCents: 3200, certifications: [{ skill: 'gas-safe' }],
});
const boiler = ok(app, 'POST', '/assets', 'admin', {
  siteId: site.id, kind: 'boiler', manufacturer: 'Acme Thermal', model: 'B-100', serial: 'SN-B-1', installedOn: '2024-06-01',
});
const draft = ok(app, 'POST', '/contracts', 'admin', {
  customerId: customer.id, siteIds: [site.id], plan: 'premium', startsOn: '2026-01-01', coveredWorkTypes: ['boiler-service', 'general'],
});
const contract = ok(app, 'POST', `/contracts/${draft.id}/activate`, 'admin');

const result = {};
const slaOf = (jobId) => {
  const response = send(app, 'GET', `/jobs/${jobId}/sla`, 'dispatch');
  return response.status === 200
    ? [response.status, response.body.policyId, response.body.status, response.body.elapsedMinutes, response.body.remainingMinutes]
    : [response.status, response.body.error?.code];
};

// Work orders opened through the active routes on a contract site run the contract clock.
const open = (workType, extra = {}) => ok(app, 'POST', '/jobs', 'dispatch', {
  customerId: customer.id, siteId: site.id, workType, summary: 'Investigate reported fault', ...extra,
});
const service = open('boiler-service', { assetId: boiler.id });
const general = open('general');
const emergency = open('emergency-callout');
result.opened = [service, general, emergency].map((job) => [job.workType, job.priority, job.contractId === contract.id, job.tags]);
result.clocks = [service, general, emergency].map((job) => slaOf(job.id));
result.sources = [service, general].map((job) => ok(app, 'GET', `/jobs/${job.id}`, 'dispatch').source);

// A follow-up of completed contract work inherits the contract and runs its own clock.
ok(app, 'POST', `/jobs/${service.id}/assign`, 'dispatch', { technicianId: technician.id });
ok(app, 'POST', `/jobs/${service.id}/start`, technician.id);
const current = ok(app, 'GET', `/jobs/${service.id}`, technician.id);
for (const item of Object.keys(current.checklist)) ok(app, 'PUT', `/jobs/${service.id}/checklist`, technician.id, { item, done: true });
ok(app, 'POST', `/jobs/${service.id}/complete`, technician.id, {
  resolution: 'Service completed, combustion within limits', signatureName: 'Avery Collins',
});
result.serviceClock = slaOf(service.id);
const followUp = ok(app, 'POST', `/jobs/${service.id}/follow-ups`, 'dispatch', { summary: 'Replace the expansion vessel' });
const followUpDetail = ok(app, 'GET', `/jobs/${followUp.id}`, 'dispatch');
result.followUp = [followUpDetail.source, followUpDetail.contractId === contract.id, followUpDetail.tags, followUpDetail.number !== service.number];
result.followUpClock = slaOf(followUp.id);

// Customers raise work through the portal and are served under the same contract.
const portal = ok(app, 'POST', `/customers/${customer.id}/portal-users`, 'admin', { name: 'Avery Collins' });
const raised = ok(app, 'POST', '/jobs', portal.id, { customerId: customer.id, siteId: site.id, workType: 'general', summary: 'Radiator cold' });
result.portalJob = [slaOf(raised.id), ok(app, 'GET', `/jobs/${raised.id}`, 'dispatch').source];

// The permission model for dispatch and billing around the new routes stays as it was.
result.access = ['/dispatch/board', `/contracts/${contract.id}`, '/reports/backlog', '/sla/compliance?from=2026-03-01&to=2026-03-31'].map((url) => [
  url.split('?')[0].replace(contract.id, ':id'), send(app, 'GET', url, 'dispatch').status, send(app, 'GET', url, 'billing').status, send(app, 'GET', url, portal.id).status,
]);

console.log(JSON.stringify(result));
'''

result = subprocess.run(["node", "--input-type=module", "-e", SCRIPT, sys.argv[1]],
                        text=True, capture_output=True, check=True, timeout=8)
sys.stdout.write(result.stdout)
