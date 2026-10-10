"""End-to-end tests that drive the application through its request API."""

from .core import imports, render


def header(ctx, here, groups=None, jobs=()):
    groups = dict(groups or {})
    groups.setdefault("tests/support/world.ts", [])
    return imports(ctx, here, groups, jobs=jobs)


def workorder_lifecycle(ctx):
    here = "tests/api/workorder-lifecycle.test.ts"
    return render(ctx, '''
import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';
«imports»

describe('work order lifecycle over the API', () => {
  let world: World;

  beforeEach(() => {
    world = createWorld();
  });

  it('opens a work order with the work type defaults', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'leak-investigation');
    assert.equal(job.status, 'new');
    assert.equal(job.priority, 'high');
    assert.equal(job.workTypeLabel, 'Leak investigation');
    assert.match(job.number, /^FL-2026-\\d{5}$/);
    assert.deepEqual(Object.values(job.checklist), Object.values(job.checklist).map(() => false));
  });

  it('rejects unknown work types', () => {
    const body = expectStatus(world.app, 422, 'POST', JOBS, 'dispatch', {
      customerId: world.customer.id,
      siteId: world.site.id,
      workType: 'teleportation',
      summary: 'Beam me up',
    });
    assert.equal(body.error.code, 'unknown-work-type');
  });

  it('schedules, assigns, starts and completes a visit', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    const scheduled = call(app, 'POST', `${JOBS}/${job.id}/schedule`, 'dispatch', { start: at(MONDAY, '10:00') });
    assert.equal(scheduled.status, 'scheduled');
    assert.equal(scheduled.scheduledStart, at(MONDAY, '10:00'));
    call(app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    app.clock.set(at(MONDAY, '10:05'));
    const started = call(app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    assert.equal(started.status, 'in_progress');
    completeChecklist(app, job.id, world.technician.id);
    app.clock.set(at(MONDAY, '11:00'));
    const done = call(app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, {
      resolution: 'Replaced worn hinge and adjusted door closer',
    });
    assert.equal(done.status, 'completed');
    assert.equal(done.completion.completedAt, at(MONDAY, '11:00'));
  });

  it('refuses completion while checklist items are open', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    call(app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    const body = expectStatus(app, 409, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, {
      resolution: 'All finished here',
    });
    assert.equal(body.error.code, 'checklist-incomplete');
  });

  it('keeps technicians to their own work orders', () => {
    const { app } = world;
    const other = seedTechnician(app, { name: 'Morgan Reyes' });
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    call(app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    expectStatus(app, 404, 'POST', `${JOBS}/${job.id}/start`, other.id);
    expectStatus(app, 404, 'GET', `${JOBS}/${job.id}`, other.id);
  });

  it('records holds in the timeline and resumes to the previous status', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    call(app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    app.clock.advance(30);
    const held = call(app, 'POST', `${JOBS}/${job.id}/hold`, world.technician.id, { reason: 'weather', note: 'Storm warning' });
    assert.equal(held.status, 'on_hold');
    assert.equal(held.hold.reason, 'weather');
    app.clock.advance(90);
    const resumed = call(app, 'POST', `${JOBS}/${job.id}/resume`, world.technician.id, {});
    assert.equal(resumed.status, 'in_progress');
    const timeline = call(app, 'GET', `${JOBS}/${job.id}/timeline`, 'dispatch');
    assert.deepEqual(
      timeline.items.filter((item: any) => item.kind !== 'status').map((item: any) => item.label),
      ['on hold (weather)', 'hold released'],
    );
  });

  it('cancels open work and refuses further transitions', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    const cancelled = call(app, 'POST', `${JOBS}/${job.id}/cancel`, 'dispatch', { reason: 'Customer fixed it' });
    assert.equal(cancelled.status, 'cancelled');
    const body = expectStatus(app, 409, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    assert.equal(body.error.code, 'invalid-transition');
  });

  it('lists work orders by status with paging', () => {
    const { app } = world;
    for (let index = 0; index < 3; index += 1) openWorkOrder(app, world.customer.id, world.site.id);
    const cancelled = openWorkOrder(app, world.customer.id, world.site.id);
    call(app, 'POST', `${JOBS}/${cancelled.id}/cancel`, 'dispatch', { reason: 'Duplicate request' });
    const page = call(app, 'GET', `${JOBS}?status=new&pageSize=2`, 'dispatch');
    assert.equal(page.total, 3);
    assert.equal(page.items.length, 2);
    assert.equal(page.pages, 2);
    expectStatus(app, 422, 'GET', `${JOBS}?status=lost`, 'dispatch');
  });

  it('shares only customer-visible notes with the customer', () => {
    const { app } = world;
    const portal = call(app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' });
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    call(app, 'POST', `${JOBS}/${job.id}/notes`, 'dispatch', { text: 'Gate code 4471' });
    call(app, 'POST', `${JOBS}/${job.id}/notes`, 'dispatch', { text: 'Technician booked for Monday', visibility: 'customer' });
    const seen = call(app, 'GET', `${JOBS}/${job.id}`, portal.id);
    assert.deepEqual(seen.notes.map((note: any) => note.text), ['Technician booked for Monday']);
    assert.equal(seen.tags, undefined);
  });
});
''', imports=header(ctx, here, {"tests/support/world.ts": [
        "at", "call", "completeChecklist", "createWorld", "expectStatus", "JOBS", "MONDAY", "openWorkOrder",
        "seedTechnician", "type World"]}))


def service_levels(ctx):
    here = "tests/api/service-levels.test.ts"
    return render(ctx, '''
import assert from 'node:assert/strict';
import { beforeEach, describe, it } from 'node:test';
«imports»

describe('service level clocks', () => {
  let world: World;

  beforeEach(() => {
    world = createWorld();
  });

  function contracted(plan: string) {
    seedContract(world.app, world.customer.id, [world.site.id], plan, ['general', 'leak-investigation']);
    const job = openWorkOrder(world.app, world.customer.id, world.site.id);
    call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    return job;
  }

  it('starts a clock only for contracted work', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id);
    expectStatus(world.app, 404, 'GET', `${JOBS}/${job.id}/sla`, 'dispatch');
    const covered = contracted('premium');
    const sla = call(world.app, 'GET', `${JOBS}/${covered.id}/sla`, 'dispatch');
    assert.equal(sla.policyId, 'premium');
    assert.equal(sla.status, 'running');
  });

  it('meets the premium target for a same-morning fix', () => {
    const { app } = world;
    const job = contracted('premium');
    app.clock.set(at(MONDAY, '09:00'));
    call(app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    completeChecklist(app, job.id, world.technician.id);
    app.clock.set(at(MONDAY, '11:30'));
    call(app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Reset pump controller and tested' });
    const sla = call(app, 'GET', `${JOBS}/${job.id}/sla`, 'dispatch');
    assert.equal(sla.status, 'met');
    assert.equal(sla.elapsedMinutes, 210);
  });

  it('breaches when resolution exceeds the target and grants a credit', () => {
    const { app } = world;
    const job = contracted('premium');
    call(app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    completeChecklist(app, job.id, world.technician.id);
    app.clock.set(at(MONDAY, '18:10'));
    call(app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Rebuilt manifold after long search' });
    const sla = call(app, 'GET', `${JOBS}/${job.id}/sla`, 'dispatch');
    assert.equal(sla.status, 'breached');
    assert.equal(sla.breachMinutes, 135);
    assert.equal(sla.creditBasisPoints, 600);
  });

  it('keeps counting while work waits for parts', () => {
    const { app } = world;
    const job = contracted('premium');
    call(app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    app.clock.set(at(MONDAY, '09:00'));
    call(app, 'POST', `${JOBS}/${job.id}/hold`, world.technician.id, { reason: 'awaiting_parts' });
    app.clock.set(at(MONDAY, '12:00'));
    const sla = call(app, 'GET', `${JOBS}/${job.id}/sla`, 'dispatch');
    assert.equal(sla.status, 'running');
    assert.equal(sla.elapsedMinutes, 240);
  });

  it('pauses after a recorded no-access visit and resumes on rebooking', () => {
    const { app } = world;
    const job = contracted('priority');
    const appointment = call(app, 'POST', '/appointments', 'dispatch', {
      jobId: job.id,
      technicianId: world.technician.id,
      start: at(MONDAY, '10:00'),
    });
    app.clock.set(at(MONDAY, '10:20'));
    call(app, 'POST', `/appointments/${appointment.id}/no-access`, world.technician.id, { reason: 'Nobody home' });
    app.clock.set(at(MONDAY, '14:20'));
    const paused = call(app, 'GET', `${JOBS}/${job.id}/sla`, 'dispatch');
    assert.equal(paused.status, 'paused');
    assert.equal(paused.elapsedMinutes, 140);
  });
});
''', imports=header(ctx, here, {"tests/support/world.ts": [
        "at", "call", "completeChecklist", "createWorld", "expectStatus", "JOBS", "MONDAY", "openWorkOrder",
        "seedContract", "type World"]}))


TESTS = {
    "tests/api/workorder-lifecycle.test.ts": workorder_lifecycle,
    "tests/api/service-levels.test.ts": service_levels,
}


def generate(ctx):
    for path, builder in TESTS.items():
        ctx.add(path, builder(ctx))
