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
  hourlyCostCents: 3200, certifications: [],
});

function issuedInvoice() {
  const job = ok(app, 'POST', '/jobs', 'dispatch', {
    customerId: customer.id, siteId: site.id, workType: 'general', summary: 'Replace the faulty component',
  });
  ok(app, 'POST', `/jobs/${job.id}/assign`, 'dispatch', { technicianId: technician.id });
  ok(app, 'POST', `/jobs/${job.id}/start`, technician.id);
  const current = ok(app, 'GET', `/jobs/${job.id}`, technician.id);
  for (const item of Object.keys(current.checklist)) ok(app, 'PUT', `/jobs/${job.id}/checklist`, technician.id, { item, done: true });
  ok(app, 'POST', `/jobs/${job.id}/complete`, technician.id, { resolution: 'Replaced the faulty component and tested', signatureName: 'Avery Collins' });
  const invoice = ok(app, 'POST', `/jobs/${job.id}/invoice`, 'billing');
  return ok(app, 'POST', `/invoices/${invoice.id}/issue`, 'billing');
}

const invoices = [];
for (let index = 0; index < 30; index += 1) invoices.push(issuedInvoice());

const statement = (query = '') => ok(app, 'GET', `/customers/${customer.id}/statement${query}`, 'billing');
const summary = (asOf, query = '') => {
  const value = statement(`?asOf=${asOf}${query}`);
  return { totalOutstandingCents: value.totalOutstandingCents, buckets: value.buckets };
};

const result = {};
result.invoiceTotalCents = invoices.reduce((sum, invoice) => sum + invoice.totalCents, 0);
result.invoicesIssued = invoices.length;
result.beforeDue = summary('2026-03-20');
result.shortlyOverdue = summary('2026-04-10');
result.overdue = summary('2026-05-15');

// A payment on the first invoice reduces what is outstanding.
const payment = ok(app, 'POST', `/invoices/${invoices[0].id}/payments`, 'billing', { amountCents: 5000, method: 'transfer' });
result.paymentStatus = payment.id ? 'recorded' : 'missing';
result.afterPayment = summary('2026-05-15');

// A list that was already paged keeps its size.
result.jobs = ok(app, 'GET', '/jobs', 'dispatch').items.length;

console.log(JSON.stringify(result));
'''

result = subprocess.run(["node", "--input-type=module", "-e", SCRIPT, sys.argv[1]],
                        text=True, capture_output=True, check=True, timeout=8)
sys.stdout.write(result.stdout)
