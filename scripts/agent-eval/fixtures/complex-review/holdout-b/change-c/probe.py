"""Private independent public-workflow observations; never installed in review trees."""
import subprocess
import sys

SCRIPT = r'''
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { createApp } = await import(pathToFileURL(join(process.argv[1], 'src/app.ts')).href);

const MONDAY = '2026-03-02';
const TUESDAY = '2026-03-03';
const at = (date, clock) => `${date}T${clock}:00Z`;

function send(app, method, url, actor, body) {
  const response = app.request(method, url, { actor, body });
  if (response.status >= 300) throw new Error(`${method} ${url}: ${response.status} ${JSON.stringify(response.body)}`);
  return response.body;
}

function world() {
  const app = createApp({ now: at(MONDAY, '08:00') });
  const customer = send(app, 'POST', '/customers', 'admin', {
    name: 'Harbour View Apartments', segment: 'commercial', billingRegion: 'metro-east',
    email: 'facilities@harbourview.example', taxExempt: false,
  });
  const site = send(app, 'POST', `/customers/${customer.id}/sites`, 'admin', {
    label: 'Main building',
    address: { line1: '12 Quay Street', city: 'Eastport', postcode: 'EP1 4QS', region: 'metro-east', location: { lat: 52.52, lon: 13.4 } },
  });
  const technician = send(app, 'POST', '/technicians', 'admin', {
    name: 'Robin Hale', email: 'robin@fieldline.example', region: 'metro-east', home: { lat: 52.5, lon: 13.38 },
    hourlyCostCents: 3200, certifications: [{ skill: 'plumbing' }],
  });
  const job = () => send(app, 'POST', '/jobs', 'dispatch', {
    customerId: customer.id, siteId: site.id, workType: 'general', summary: 'Investigate reported fault',
  });
  return { app, technician, job };
}

const startsOn = (slots) => slots.map((slot) => slot.start.slice(11, 16));

// A day's slots are read, then the technician goes absent, then the day is read and booked again.
function absenceAfterRead() {
  const { app, technician, job } = world();
  const target = job();
  const slotsUrl = `/technicians/${technician.id}/slots?date=${TUESDAY}&jobId=${target.id}&duration=60`;
  const before = send(app, 'GET', slotsUrl, 'dispatch').slots;
  const loadBefore = send(app, 'GET', `/technicians/${technician.id}/load?date=${TUESDAY}`, 'dispatch');
  send(app, 'POST', `/technicians/${technician.id}/absences`, 'admin', { from: TUESDAY, to: TUESDAY, kind: 'sick' });
  const after = send(app, 'GET', slotsUrl, 'dispatch').slots;
  const loadAfter = send(app, 'GET', `/technicians/${technician.id}/load?date=${TUESDAY}`, 'dispatch');
  const booking = app.request('POST', '/appointments', {
    actor: 'dispatch',
    body: { jobId: target.id, technicianId: technician.id, start: at(TUESDAY, '09:00'), durationMinutes: 60 },
  });
  return {
    slotsBefore: before.length,
    slotsAfterAbsence: startsOn(after),
    availableMinutesBefore: loadBefore.availableMinutes,
    availableMinutesAfter: loadAfter.availableMinutes,
    bookingStatus: booking.status,
  };
}

// Bookings made after a read are visible to the next read.
function bookingAfterRead() {
  const { app, technician, job } = world();
  const first = job();
  const second = job();
  const slotsUrl = `/technicians/${technician.id}/slots?date=${TUESDAY}&jobId=${second.id}&duration=60`;
  const before = startsOn(send(app, 'GET', slotsUrl, 'dispatch').slots);
  send(app, 'POST', '/appointments', 'dispatch', {
    jobId: first.id, technicianId: technician.id, start: at(TUESDAY, '09:00'), durationMinutes: 60,
  });
  const after = startsOn(send(app, 'GET', slotsUrl, 'dispatch').slots);
  return { offeredBefore: before.includes('09:00'), offeredAfter: after.includes('09:00'), after: after.slice(0, 4) };
}

console.log(JSON.stringify({ absence: absenceAfterRead(), booking: bookingAfterRead() }));
'''

result = subprocess.run(["node", "--input-type=module", "-e", SCRIPT, sys.argv[1]],
                        text=True, capture_output=True, check=True, timeout=8)
sys.stdout.write(result.stdout)
