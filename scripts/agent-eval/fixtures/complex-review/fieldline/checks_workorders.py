"""Tests for the work-order lifecycle: opening, transitions, holds, completion, queries and routes."""

from .checks_kit import WITH_SETUP, Spec, register

WORLD = "tests/support/world.ts"
EVENTS = "src/shared/events.ts"
TECH_SERVICE = "src/technicians/service.ts"

# Shared scene helpers; every template that uses them imports createWorld and the lifecycle calls.
SCENE = r'''
function start(options: AppOptions = {}) {
  const world = createWorld(options);
  return { ...world, ctx: world.app.ctx, tech: world.technician.id as string };
}
type Scene = ReturnType<typeof start>;

const open = (scene: Scene, workType = 'general', extra: Partial<«OpenJobInput»> = {}) =>
  «openJob»(scene.ctx, 'dispatch', {
    customerId: scene.customer.id,
    siteId: scene.site.id,
    workType,
    summary: 'Investigate reported fault',
    ...extra,
  });

function work(scene: Scene, jobId: string, resolution = 'Replaced worn hinge and adjusted closer') {
  const job = «requireJob»(scene.ctx, jobId);
  if (!job.technicianId) «assignJob»(scene.ctx, 'dispatch', jobId, scene.tech);
  if (job.status !== 'in_progress') «startJob»(scene.ctx, scene.tech, jobId);
  for (const item of Object.keys(«requireJob»(scene.ctx, jobId).checklist)) «recordChecklistItem»(scene.ctx, scene.tech, jobId, item, true);
  return «completeJob»(scene.ctx, scene.tech, jobId, { resolution, signatureName: 'Avery Collins' });
}

const eventsOf = (scene: Scene, type: string) => scene.ctx.bus.recent(200).filter((event) => event.type === type);
'''

SCENE_JOBS = ["type OpenJobInput", "openJob", "requireJob", "assignJob", "startJob", "recordChecklistItem",
              "completeJob"]
SCENE_GROUPS = {WORLD: ["createWorld"], "src/app.ts": ["type AppOptions"]}


def scene_spec(groups, body, jobs=(), node=WITH_SETUP, **extra):
    def merge(ctx):
        merged = {WORLD: ["createWorld"], "src/app.ts": ["type AppOptions"]}
        for target, names in (groups(ctx) if callable(groups) else groups).items():
            merged[target] = sorted(set(merged.get(target, [])) | set(names))
        return merged

    return Spec(merge, SCENE + body, tuple(sorted(set(SCENE_JOBS) | set(jobs))), node=node, **extra)


OPENING = r'''
describe('openJob', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('creates a new work order with the handler defaults', () => {
    const job = open(scene, 'leak-investigation');
    assert.equal(job.id, 'WO-000001');
    assert.equal(job.number, 'FL-2026-00001');
    assert.equal(job.status, 'new');
    assert.equal(job.priority, 'high');
    assert.equal(job.workType, 'leak-investigation');
    assert.equal(job.source, 'phone');
    assert.equal(job.requestedAt, at(MONDAY, '08:00'));
    assert.equal(job.reopenCount, 0);
    assert.deepEqual(job.holds, []);
    assert.deepEqual(job.history, [{ at: at(MONDAY, '08:00'), status: 'new', actorId: 'dispatch' }]);
  });

  it('starts with every checklist item of the work type open', () => {
    const job = open(scene, 'drain-clearance');
    assert.deepEqual(job.checklist, { 'flow test': false });
    const repair = open(scene, 'boiler-repair', { assetId: seedAsset(scene.app, scene.site.id, 'boiler').id });
    assert.deepEqual(repair.checklist, { 'fault diagnosed': false, 'repair completed': false, 'flue gas analysis': false });
  });

  it('trims the summary and description', () => {
    const job = open(scene, 'general', { summary: '  Door does not close  ', description: '  Sticks at the top hinge.\n' });
    assert.equal(job.summary, 'Door does not close');
    assert.equal(job.description, 'Sticks at the top hinge.');
  });

  it('defaults the description to an empty string', () => {
    assert.equal(open(scene).description, '');
  });

  it('normalises tags to sorted, unique, lower-case values', () => {
    const job = open(scene, 'general', { tags: ['Urgent', ' keys-at-reception ', 'urgent', '  '] });
    assert.deepEqual(job.tags, ['keys-at-reception', 'urgent']);
  });

  it('honours an explicit priority and source', () => {
    const job = open(scene, 'general', { priority: 'high', source: 'portal' });
    assert.equal(job.priority, 'high');
    assert.equal(job.source, 'portal');
  });

  it('resolves work type keys case-insensitively to the registered key', () => {
    assert.equal(open(scene, ' Leak-Investigation ').workType, 'leak-investigation');
  });

  it('rejects unknown work types', () => {
    assert.throws(
      () => open(scene, 'teleportation'),
      (error: any) => error.code === 'unknown-work-type' && error.status === 422 && /teleportation/.test(error.message),
    );
  });

  it('rejects sites that belong to another customer', () => {
    const other = seedCustomer(scene.app, { name: 'Neighbour Ltd' });
    assert.throws(
      () => «openJob»(scene.ctx, 'dispatch', { customerId: scene.customer.id, siteId: other.site.id, workType: 'general', summary: 'Wrong site' }),
      (error: any) => error.status === 404,
    );
  });

  it('rejects unknown customers', () => {
    assert.throws(
      () => «openJob»(scene.ctx, 'dispatch', { customerId: 'CUS-000404', siteId: scene.site.id, workType: 'general', summary: 'Ghost' }),
      /customer CUS-000404 was not found/,
    );
  });

  it('numbers work orders in order of creation', () => {
    const numbers = [open(scene), open(scene), open(scene)].map((job) => job.number);
    assert.deepEqual(numbers, ['FL-2026-00001', 'FL-2026-00002', 'FL-2026-00003']);
  });
});

describe('openJob with assets', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('requires an asset for work types that service equipment', () => {
    assert.throws(
      () => open(scene, 'boiler-service'),
      (error: any) => error.code === 'asset-required' && /boiler-service/.test(error.message),
    );
  });

  it('accepts an asset installed at the site', () => {
    const asset = seedAsset(scene.app, scene.site.id, 'boiler');
    const job = open(scene, 'boiler-service', { assetId: asset.id });
    assert.equal(job.assetId, asset.id);
  });

  it('rejects assets at a different site', () => {
    const second = call(scene.app, 'POST', `/customers/${scene.customer.id}/sites`, 'admin', {
      label: 'Annex',
      address: { line1: '14 Quay Street', city: 'Eastport', postcode: 'EP1 4QT', region: 'metro-east', location: { lat: 52.52, lon: 13.41 } },
    });
    const asset = seedAsset(scene.app, second.id, 'boiler');
    assert.throws(() => open(scene, 'boiler-service', { assetId: asset.id }), (error: any) => error.code === 'asset-not-at-site');
  });

  it('rejects retired assets', () => {
    const asset = seedAsset(scene.app, scene.site.id, 'boiler');
    call(scene.app, 'POST', `/assets/${asset.id}/retire?on=2026-02-01`, 'admin');
    assert.throws(() => open(scene, 'boiler-service', { assetId: asset.id }), (error: any) => error.code === 'asset-retired');
  });

  it('allows an asset on work types that do not require one', () => {
    const asset = seedAsset(scene.app, scene.site.id, 'chiller');
    assert.equal(open(scene, 'general', { assetId: asset.id }).assetId, asset.id);
  });

  it('rejects unknown assets', () => {
    assert.throws(() => open(scene, 'general', { assetId: 'AST-000404' }), /asset AST-000404 was not found/);
  });
});

describe('openJob with contracts', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('links the covering contract', () => {
    const contract = seedContract(scene.app, scene.customer.id, [scene.site.id], 'basic', ['general']);
    const job = open(scene, 'general');
    assert.equal(job.contractId, contract.id);
    assert.deepEqual(job.tags, ['included-visit']);
  });

  it('keeps work outside the contract unlinked', () => {
    seedContract(scene.app, scene.customer.id, [scene.site.id], 'basic', ['general']);
    const job = open(scene, 'leak-investigation');
    assert.equal(job.contractId, undefined);
    assert.deepEqual(job.tags, []);
  });

  it('raises the default priority on premium contracts', () => {
    seedContract(scene.app, scene.customer.id, [scene.site.id], 'premium', ['general', 'drain-clearance']);
    assert.equal(open(scene, 'general').priority, 'high');
    assert.equal(open(scene, 'drain-clearance').priority, 'high');
  });

  it('keeps the default priority on other plans', () => {
    seedContract(scene.app, scene.customer.id, [scene.site.id], 'priority', ['general']);
    assert.equal(open(scene, 'general').priority, 'normal');
  });

  it('lets an explicit priority win over the plan', () => {
    seedContract(scene.app, scene.customer.id, [scene.site.id], 'premium', ['general']);
    assert.equal(open(scene, 'general', { priority: 'low' }).priority, 'low');
  });
});
'''

TRANSITIONS = r'''
const ALL: «JobStatus»[] = ['new', 'scheduled', 'assigned', 'in_progress', 'on_hold', 'completed', 'closed', 'cancelled'];

const ALLOWED: Record<«JobStatus», «JobStatus»[]> = {
  new: ['scheduled', 'assigned', 'on_hold', 'cancelled'],
  scheduled: ['scheduled', 'assigned', 'on_hold', 'cancelled'],
  assigned: ['scheduled', 'assigned', 'in_progress', 'on_hold', 'cancelled'],
  in_progress: ['on_hold', 'completed', 'cancelled'],
  on_hold: ['new', 'scheduled', 'assigned', 'in_progress', 'cancelled'],
  completed: ['closed', 'new', 'assigned'],
  closed: ['new', 'assigned'],
  cancelled: [],
};

describe('canTransition', () => {
  for (const from of ALL) {
    it(`allows exactly the documented moves out of ${from}`, () => {
      const reachable = ALL.filter((to) => canTransition(from, to));
      assert.deepEqual(reachable, ALL.filter((to) => ALLOWED[from].includes(to)));
    });
  }

  it('treats cancelled as terminal', () => {
    for (const to of ALL) assert.equal(canTransition('cancelled', to), false, to);
  });

  it('never moves a completed work order back into progress', () => {
    assert.equal(canTransition('completed', 'in_progress'), false);
    assert.equal(canTransition('closed', 'in_progress'), false);
  });

  it('only closes completed work', () => {
    const sources = ALL.filter((from) => canTransition(from, 'closed'));
    assert.deepEqual(sources, ['completed']);
  });

  it('only completes work that is in progress', () => {
    const sources = ALL.filter((from) => canTransition(from, 'completed'));
    assert.deepEqual(sources, ['in_progress']);
  });

  it('allows cancelling from every open status', () => {
    for (const status of OPEN_STATUSES) assert.equal(canTransition(status, 'cancelled'), true, status);
    for (const status of ['completed', 'closed'] as const) assert.equal(canTransition(status, 'cancelled'), false, status);
  });
});

describe('open statuses', () => {
  it('lists the statuses that still need attention', () => {
    assert.deepEqual([...OPEN_STATUSES], ['new', 'scheduled', 'assigned', 'in_progress', 'on_hold']);
  });

  for (const status of ALL) {
    it(`classifies ${status}`, () => {
      assert.equal(isOpenStatus(status), ['new', 'scheduled', 'assigned', 'in_progress', 'on_hold'].includes(status));
    });
  }

  it('orders priorities from low to emergency', () => {
    assert.deepEqual([...«JOB_PRIORITIES»], ['low', 'normal', 'high', 'emergency']);
  });
});

describe('invalid transitions through the service', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('refuses to start work that was never assigned', () => {
    const job = open(scene);
    assert.throws(() => «startJob»(scene.ctx, scene.tech, job.id), (error: any) => error.code === 'unassigned');
  });

  it('refuses to start scheduled work before a technician is assigned', () => {
    const job = open(scene);
    «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), 60);
    assert.throws(() => «startJob»(scene.ctx, scene.tech, job.id), (error: any) => error.code === 'unassigned');
  });

  it('reports the rejected move in the error details', () => {
    const job = open(scene);
    «cancelJob»(scene.ctx, 'dispatch', job.id, 'Customer cancelled');
    assert.throws(
      () => «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech),
      (error: any) =>
        error.status === 409 &&
        error.code === 'invalid-transition' &&
        error.details.from === 'cancelled' &&
        error.details.to === 'assigned' &&
        error.message.includes(job.number),
    );
  });

  it('refuses to close work that is still open', () => {
    const job = open(scene);
    assert.throws(() => «closeJob»(scene.ctx, 'dispatch', job.id), (error: any) => error.code === 'not-completed');
  });

  it('leaves the work order untouched after a rejected move', () => {
    const job = open(scene);
    «cancelJob»(scene.ctx, 'dispatch', job.id, 'Customer cancelled');
    const before = «requireJob»(scene.ctx, job.id);
    assert.throws(() => «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), 60));
    assert.deepEqual(«requireJob»(scene.ctx, job.id), before);
  });
});
'''

LIFECYCLE = r'''
describe('scheduleJob', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('moves a new work order to scheduled and computes the end', () => {
    const job = open(scene);
    const scheduled = «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), 90);
    assert.equal(scheduled.status, 'scheduled');
    assert.equal(scheduled.scheduledStart, at(MONDAY, '10:00'));
    assert.equal(scheduled.scheduledEnd, at(MONDAY, '11:30'));
  });

  it('keeps assigned work assigned when the slot changes', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    const moved = «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '13:00'), 60);
    assert.equal(moved.status, 'assigned');
    assert.equal(moved.technicianId, scene.tech);
    assert.equal(moved.scheduledStart, at(MONDAY, '13:00'));
  });

  it('can reschedule repeatedly', () => {
    const job = open(scene);
    «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), 60);
    const again = «scheduleJob»(scene.ctx, 'dispatch', job.id, at('2026-03-03', '09:00'), 120);
    assert.equal(again.scheduledStart, at('2026-03-03', '09:00'));
    assert.equal(again.scheduledEnd, at('2026-03-03', '11:00'));
    assert.equal(again.history.filter((entry) => entry.status === 'scheduled').length, 2);
  });

  it('rejects durations that are not positive whole minutes', () => {
    const job = open(scene);
    for (const minutes of [0, -15, 12.5, Number.NaN]) {
      assert.throws(() => «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), minutes), (error: any) => error.code === 'invalid-duration', String(minutes));
    }
  });

  it('refuses to schedule work that is in progress', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    assert.throws(() => «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), 60), (error: any) => error.code === 'invalid-transition');
  });
});

describe('assignJob', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('assigns a qualified technician', () => {
    const job = open(scene, 'leak-investigation');
    const assigned = «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    assert.equal(assigned.status, 'assigned');
    assert.equal(assigned.technicianId, scene.tech);
    assert.equal(assigned.history.at(-1)?.actorId, 'dispatch');
  });

  it('reassigns to another qualified technician', () => {
    const job = open(scene);
    const other = seedTechnician(scene.app, { name: 'Morgan Reyes' });
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    assert.equal(«assignJob»(scene.ctx, 'dispatch', job.id, other.id).technicianId, other.id);
  });

  it('names the skills a technician lacks', () => {
    const job = open(scene, 'heat-pump-service', { assetId: seedAsset(scene.app, scene.site.id, 'heat-pump').id });
    assert.throws(
      () => «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech),
      (error: any) => error.code === 'technician-not-qualified' && error.status === 409 && error.details.missing.join() === 'refrigerant',
    );
  });

  it('accepts technicians who hold every required skill', () => {
    const job = open(scene, 'heat-pump-service', { assetId: seedAsset(scene.app, scene.site.id, 'heat-pump').id });
    const expert = seedTechnician(scene.app, { skills: ['refrigerant', 'electrical'] });
    assert.equal(«assignJob»(scene.ctx, 'dispatch', job.id, expert.id).technicianId, expert.id);
  });

  it('checks certification expiry against the scheduled day', () => {
    const job = open(scene, 'boiler-repair', { assetId: seedAsset(scene.app, scene.site.id, 'boiler').id });
    const technician = call(scene.app, 'POST', '/technicians', 'admin', {
      name: 'Dana Whitfield',
      email: 'dana@fieldline.example',
      region: 'metro-east',
      home: { lat: 52.5, lon: 13.38 },
      hourlyCostCents: 3000,
      certifications: [{ skill: 'gas-safe', expiresOn: '2026-03-10' }],
    });
    «scheduleJob»(scene.ctx, 'dispatch', job.id, at('2026-03-11', '09:00'), 60);
    assert.throws(() => «assignJob»(scene.ctx, 'dispatch', job.id, technician.id), (error: any) => error.code === 'technician-not-qualified');
    «scheduleJob»(scene.ctx, 'dispatch', job.id, at('2026-03-10', '09:00'), 60);
    assert.equal(«assignJob»(scene.ctx, 'dispatch', job.id, technician.id).status, 'assigned');
  });

  it('rejects unknown and deactivated technicians', () => {
    const job = open(scene);
    assert.throws(() => «assignJob»(scene.ctx, 'dispatch', job.id, 'TEC-000404'), /technician TEC-000404 was not found/);
    const retired = seedTechnician(scene.app, { name: 'Pat Lindgren' });
    deactivateTechnician(scene.ctx, 'admin', retired.id, 0);
    assert.throws(() => «assignJob»(scene.ctx, 'dispatch', job.id, retired.id), (error: any) => error.code === 'technician-inactive');
  });
});

describe('startJob', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('moves assigned work into progress and records the start', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    scene.app.clock.set(at(MONDAY, '09:15'));
    const started = «startJob»(scene.ctx, scene.tech, job.id);
    assert.equal(started.status, 'in_progress');
    assert.equal(started.startedAt, at(MONDAY, '09:15'));
  });

  it('keeps the first start when work resumes after a reopen', () => {
    const job = open(scene);
    work(scene, job.id);
    «reopenJob»(scene.ctx, 'dispatch', job.id, 'Leak is back');
    scene.app.clock.set(at('2026-03-03', '10:00'));
    const second = «startJob»(scene.ctx, scene.tech, job.id);
    assert.equal(second.startedAt, at(MONDAY, '08:00'));
  });

  it('cannot start twice', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    assert.throws(() => «startJob»(scene.ctx, scene.tech, job.id), (error: any) => error.code === 'invalid-transition');
  });
});

describe('cancelJob', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('cancels open work with a trimmed reason', () => {
    const job = open(scene);
    const cancelled = «cancelJob»(scene.ctx, 'dispatch', job.id, '  Customer fixed it themselves ');
    assert.equal(cancelled.status, 'cancelled');
    assert.equal(cancelled.cancelReason, 'Customer fixed it themselves');
    assert.equal(cancelled.history.at(-1)?.note, 'Customer fixed it themselves');
  });

  it('requires a reason', () => {
    const job = open(scene);
    assert.throws(() => «cancelJob»(scene.ctx, 'dispatch', job.id, '   '), (error: any) => error.code === 'reason-required');
    assert.equal(«requireJob»(scene.ctx, job.id).status, 'new');
  });

  it('releases an active hold', () => {
    const job = open(scene);
    «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), 60);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'awaiting_approval');
    scene.app.clock.advance(45);
    const cancelled = «cancelJob»(scene.ctx, 'dispatch', job.id, 'Quote declined');
    assert.equal(cancelled.holds[0].releasedAt, at(MONDAY, '08:45'));
  });

  it('cannot cancel finished work', () => {
    const job = open(scene);
    work(scene, job.id);
    assert.throws(() => «cancelJob»(scene.ctx, 'dispatch', job.id, 'Too late'), (error: any) => error.code === 'invalid-transition');
  });

  it('cannot cancel twice', () => {
    const job = open(scene);
    «cancelJob»(scene.ctx, 'dispatch', job.id, 'Duplicate');
    assert.throws(() => «cancelJob»(scene.ctx, 'dispatch', job.id, 'Duplicate'), (error: any) => error.code === 'invalid-transition');
  });
});

describe('closeJob and reopenJob', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('closes completed work', () => {
    const job = open(scene);
    work(scene, job.id);
    assert.equal(«closeJob»(scene.ctx, 'dispatch', job.id).status, 'closed');
  });

  it('cannot close twice', () => {
    const job = open(scene);
    work(scene, job.id);
    «closeJob»(scene.ctx, 'dispatch', job.id);
    assert.throws(() => «closeJob»(scene.ctx, 'dispatch', job.id), (error: any) => error.code === 'not-completed');
  });

  it('reopens assigned work for the same technician and clears the completion', () => {
    const job = open(scene);
    work(scene, job.id);
    const reopened = «reopenJob»(scene.ctx, 'dispatch', job.id, 'Leak is back');
    assert.equal(reopened.status, 'assigned');
    assert.equal(reopened.technicianId, scene.tech);
    assert.equal(reopened.reopenCount, 1);
    assert.equal(reopened.completion, undefined);
    assert.equal(reopened.history.at(-1)?.note, 'Leak is back');
  });

  it('reopens closed work', () => {
    const job = open(scene);
    work(scene, job.id);
    «closeJob»(scene.ctx, 'dispatch', job.id);
    assert.equal(«reopenJob»(scene.ctx, 'dispatch', job.id, 'Customer complaint').status, 'assigned');
  });

  it('keeps checklist progress when reopening', () => {
    const job = open(scene);
    work(scene, job.id);
    const reopened = «reopenJob»(scene.ctx, 'dispatch', job.id, 'Please re-check');
    assert.deepEqual(Object.values(reopened.checklist), [true]);
  });

  it('allows at most three reopenings', () => {
    const job = open(scene);
    work(scene, job.id);
    for (let round = 1; round <= 3; round += 1) {
      assert.equal(«reopenJob»(scene.ctx, 'dispatch', job.id, `Round ${round}`).reopenCount, round);
      «startJob»(scene.ctx, scene.tech, job.id);
      «completeJob»(scene.ctx, scene.tech, job.id, { resolution: 'Adjusted the closer again' });
    }
    assert.throws(
      () => «reopenJob»(scene.ctx, 'dispatch', job.id, 'Round 4'),
      (error: any) => error.code === 'reopen-limit' && error.status === 409,
    );
  });

  it('cannot reopen work that has not been completed', () => {
    const job = open(scene);
    assert.throws(() => «reopenJob»(scene.ctx, 'dispatch', job.id, 'Not done yet'), (error: any) => error.code === 'invalid-transition');
  });

  it('keeps the original technician on reopened work', () => {
    const job = open(scene);
    work(scene, job.id);
    deactivateTechnician(scene.ctx, 'admin', scene.tech, 0);
    const reopened = «reopenJob»(scene.ctx, 'dispatch', job.id, 'Revisit');
    assert.equal(reopened.status, 'assigned');
    assert.equal(reopened.technicianId, scene.tech);
  });
});
'''

HOLDS = r'''
describe('holdJob', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('puts scheduled work on hold and remembers where to resume', () => {
    const job = open(scene);
    «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), 60);
    scene.app.clock.advance(20);
    const held = «holdJob»(scene.ctx, 'dispatch', job.id, 'access_denied', 'Gate locked');
    assert.equal(held.status, 'on_hold');
    assert.deepEqual(held.holds, [{ reason: 'access_denied', since: at(MONDAY, '08:20'), note: 'Gate locked', resumeTo: 'scheduled' }]);
    assert.equal(held.history.at(-1)?.note, 'Gate locked');
  });

  it('omits the note when none is given', () => {
    const job = open(scene);
    const held = «holdJob»(scene.ctx, 'dispatch', job.id, 'weather');
    assert.equal('note' in held.holds[0], false);
    assert.equal('note' in held.history.at(-1)!, false);
  });

  it('accepts every documented reason', () => {
    for (const reason of «HOLD_REASONS») {
      const job = open(scene);
      «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), 60);
      assert.equal(«holdJob»(scene.ctx, 'dispatch', job.id, reason).holds[0].reason, reason);
    }
    assert.equal(«HOLD_REASONS».length, 6);
  });

  it('rejects unknown reasons', () => {
    const job = open(scene);
    assert.throws(() => «holdJob»(scene.ctx, 'dispatch', job.id, 'lunch' as «HoldReason»), (error: any) => error.code === 'invalid-hold-reason');
  });

  it('allows only one hold at a time', () => {
    const job = open(scene);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'weather');
    assert.throws(() => «holdJob»(scene.ctx, 'dispatch', job.id, 'safety'), (error: any) => error.code === 'already-on-hold');
  });

  it('requires scheduling before work can wait for parts', () => {
    const job = open(scene);
    assert.throws(() => «holdJob»(scene.ctx, 'dispatch', job.id, 'awaiting_parts'), (error: any) => error.code === 'not-diagnosed');
    «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), 60);
    assert.equal(«holdJob»(scene.ctx, 'dispatch', job.id, 'awaiting_parts').status, 'on_hold');
  });

  it('refuses holds on finished work', () => {
    const job = open(scene);
    work(scene, job.id);
    assert.throws(() => «holdJob»(scene.ctx, 'dispatch', job.id, 'weather'), (error: any) => error.code === 'invalid-transition');
  });
});

describe('resumeJob', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('returns to the status the hold started from', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    «holdJob»(scene.ctx, scene.tech, job.id, 'safety');
    assert.equal(«resumeJob»(scene.ctx, scene.tech, job.id).status, 'in_progress');
  });

  it('returns new work to new', () => {
    const job = open(scene);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'customer_unavailable');
    assert.equal(«resumeJob»(scene.ctx, 'dispatch', job.id).status, 'new');
  });

  it('stamps the release and keeps the hold in the history', () => {
    const job = open(scene);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'weather');
    scene.app.clock.advance(90);
    const resumed = «resumeJob»(scene.ctx, 'dispatch', job.id, 'Storm passed');
    assert.equal(resumed.holds[0].releasedAt, at(MONDAY, '09:30'));
    assert.equal(resumed.history.at(-1)?.note, 'Storm passed');
    assert.equal(resumed.history.length, 3);
  });

  it('supports several holds in sequence', () => {
    const job = open(scene);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'weather');
    «resumeJob»(scene.ctx, 'dispatch', job.id);
    scene.app.clock.advance(10);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'safety');
    const resumed = «resumeJob»(scene.ctx, 'dispatch', job.id);
    assert.deepEqual(resumed.holds.map((hold) => hold.reason), ['weather', 'safety']);
    assert.ok(resumed.holds.every((hold) => hold.releasedAt !== undefined));
  });

  it('requires an active hold', () => {
    const job = open(scene);
    assert.throws(() => «resumeJob»(scene.ctx, 'dispatch', job.id), (error: any) => error.code === 'not-on-hold');
  });

  it('cannot resume twice', () => {
    const job = open(scene);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'weather');
    «resumeJob»(scene.ctx, 'dispatch', job.id);
    assert.throws(() => «resumeJob»(scene.ctx, 'dispatch', job.id), (error: any) => error.code === 'not-on-hold');
  });
});

describe('jobAge', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('counts minutes since the request', () => {
    const job = open(scene);
    assert.equal(«jobAge»(job, at(MONDAY, '08:00')), 0);
    assert.equal(«jobAge»(job, at(MONDAY, '10:30')), 150);
  });

  it('excludes finished holds', () => {
    const job = open(scene);
    scene.app.clock.advance(30);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'weather');
    scene.app.clock.advance(60);
    «resumeJob»(scene.ctx, 'dispatch', job.id);
    assert.equal(«jobAge»(«requireJob»(scene.ctx, job.id), at(MONDAY, '10:00')), 60);
  });

  it('excludes the running hold up to the reference time', () => {
    const job = open(scene);
    scene.app.clock.advance(30);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'weather');
    assert.equal(«jobAge»(«requireJob»(scene.ctx, job.id), at(MONDAY, '09:00')), 30);
  });

  it('stops counting at completion', () => {
    const job = open(scene);
    scene.app.clock.advance(120);
    const done = work(scene, job.id);
    assert.equal(«jobAge»(done, at('2026-03-05', '08:00')), 120);
  });

  it('never goes negative', () => {
    const job = open(scene);
    assert.equal(«jobAge»(job, at(MONDAY, '07:00')), 0);
  });
});
'''

COMPLETION = r'''
describe('recordChecklistItem', () => {
  let scene: Scene;
  let expert: string;

  beforeEach(() => {
    scene = start();
    expert = seedTechnician(scene.app, { skills: ['drainage', 'hvac-commercial', 'generators', 'electrical', 'gas-safe'] }).id;
  });

  const inProgress = (workType = 'general', extra: Partial<«OpenJobInput»> = {}) => {
    const job = open(scene, workType, extra);
    «assignJob»(scene.ctx, 'dispatch', job.id, expert);
    «startJob»(scene.ctx, expert, job.id);
    return job;
  };

  it('ticks and unticks items', () => {
    const job = inProgress('drain-clearance');
    assert.deepEqual(«recordChecklistItem»(scene.ctx, expert, job.id, 'flow test', true).checklist, { 'flow test': true });
    assert.deepEqual(«recordChecklistItem»(scene.ctx, expert, job.id, 'flow test', false).checklist, { 'flow test': false });
  });

  it('keeps the other items untouched', () => {
    const job = inProgress('duct-cleaning');
    const updated = «recordChecklistItem»(scene.ctx, expert, job.id, 'before photos', true);
    assert.deepEqual(updated.checklist, { 'access panels fitted': false, 'before photos': true, 'after photos': false });
  });

  it('rejects items the work type does not define', () => {
    const job = inProgress();
    assert.throws(
      () => «recordChecklistItem»(scene.ctx, expert, job.id, 'wash the van', true),
      (error: any) => error.code === 'unknown-checklist-item' && /wash the van/.test(error.message),
    );
  });

  it('only records while work is in progress', () => {
    const job = open(scene);
    assert.throws(() => «recordChecklistItem»(scene.ctx, expert, job.id, 'site left clean', true), (error: any) => error.code === 'not-in-progress');
  });

  it('does not touch the status history', () => {
    const job = inProgress();
    const before = «requireJob»(scene.ctx, job.id).history.length;
    «recordChecklistItem»(scene.ctx, expert, job.id, 'site left clean', true);
    assert.equal(«requireJob»(scene.ctx, job.id).history.length, before);
  });
});

describe('completeJob', () => {
  let scene: Scene;
  let expert: string;

  beforeEach(() => {
    scene = start();
    expert = seedTechnician(scene.app, { skills: ['drainage', 'hvac-commercial', 'generators', 'electrical', 'gas-safe'] }).id;
  });

  const ready = (workType = 'general', extra: Partial<«OpenJobInput»> = {}, tick = true) => {
    const job = open(scene, workType, extra);
    «assignJob»(scene.ctx, 'dispatch', job.id, expert);
    «startJob»(scene.ctx, expert, job.id);
    if (tick) for (const item of Object.keys(job.checklist)) «recordChecklistItem»(scene.ctx, expert, job.id, item, true);
    return job;
  };

  it('completes work and stores the report', () => {
    const job = ready();
    scene.app.clock.set(at(MONDAY, '11:30'));
    const done = «completeJob»(scene.ctx, expert, job.id, {
      resolution: '  Replaced worn hinge and adjusted closer  ',
      signatureName: '  Avery Collins ',
      followUpRequired: true,
    });
    assert.equal(done.status, 'completed');
    assert.deepEqual(done.completion, {
      completedAt: at(MONDAY, '11:30'),
      resolution: 'Replaced worn hinge and adjusted closer',
      partsUsed: [],
      signatureName: 'Avery Collins',
      followUpRequired: true,
      meterReading: undefined,
    });
  });

  it('defaults optional report fields', () => {
    const job = ready();
    const done = «completeJob»(scene.ctx, expert, job.id, { resolution: 'Tightened all fixings', signatureName: '   ' });
    assert.equal(done.completion?.signatureName, undefined);
    assert.equal(done.completion?.followUpRequired, false);
  });

  it('drops parts lines without a positive quantity', () => {
    const job = ready();
    const done = «completeJob»(scene.ctx, expert, job.id, {
      resolution: 'Tightened all fixings',
      partsUsed: [
        { sku: 'SCREW-M6', quantity: 8 },
        { sku: 'WASHER-M6', quantity: 0 },
        { sku: 'SEAL-GEN', quantity: -2 },
      ],
    });
    assert.deepEqual(done.completion?.partsUsed, [{ sku: 'SCREW-M6', quantity: 8 }]);
  });

  it('refuses while checklist items are open and lists them', () => {
    const job = ready('duct-cleaning', {}, false);
    «recordChecklistItem»(scene.ctx, expert, job.id, 'before photos', true);
    assert.throws(
      () => «completeJob»(scene.ctx, expert, job.id, { resolution: 'Cleaned all ducts' }),
      (error: any) =>
        error.code === 'checklist-incomplete' &&
        error.status === 409 &&
        error.details.open.join('|') === 'access panels fitted|after photos',
    );
  });

  it('requires a resolution', () => {
    const job = ready();
    assert.throws(() => «completeJob»(scene.ctx, expert, job.id, { resolution: '   ' }), (error: any) => error.code === 'resolution-required');
  });

  it('applies the work type completion rules', () => {
    const job = ready();
    assert.throws(
      () => «completeJob»(scene.ctx, expert, job.id, { resolution: 'Fixed' }),
      (error: any) =>
        error.code === 'completion-rejected' &&
        error.status === 422 &&
        error.details.problems.length === 1 &&
        error.message === 'describe the work performed in at least 10 characters',
    );
    assert.equal(«requireJob»(scene.ctx, job.id).status, 'in_progress');
  });

  it('joins several completion problems into one message', () => {
    const installer = seedTechnician(scene.app, { skills: ['refrigerant', 'electrical', 'commissioning'] });
    const job = open(scene, 'heat-pump-install');
    «assignJob»(scene.ctx, 'dispatch', job.id, installer.id);
    «startJob»(scene.ctx, installer.id, job.id);
    for (const item of Object.keys(job.checklist)) «recordChecklistItem»(scene.ctx, installer.id, job.id, item, true);
    assert.throws(
      () => «completeJob»(scene.ctx, installer.id, job.id, { resolution: 'Installed the new unit' }),
      (error: any) =>
        error.code === 'completion-rejected' &&
        error.details.problems.length === 2 &&
        error.message === 'exactly one heat pump unit must be recorded; installations require a customer handover signature',
    );
  });

  it('requires work to be in progress', () => {
    const job = open(scene);
    assert.throws(
      () => «completeJob»(scene.ctx, expert, job.id, { resolution: 'Nothing to do here' }),
      (error: any) => error.code === 'not-in-progress' && /is new/.test(error.message),
    );
  });

  it('records the service on the asset, including the meter reading', () => {
    const asset = seedAsset(scene.app, scene.site.id, 'generator');
    const job = ready('generator-load-test', { assetId: asset.id });
    scene.app.clock.set(at('2026-03-04', '12:00'));
    «completeJob»(scene.ctx, expert, job.id, { resolution: 'Load test passed at all steps', meterReading: 812 });
    const stored = assets(scene.ctx).require(asset.id);
    assert.equal(stored.lastServicedOn, '2026-03-04');
    assert.equal(stored.meterReading, 812);
  });

  it('does not service an asset when the completion is rejected', () => {
    const asset = seedAsset(scene.app, scene.site.id, 'generator');
    const job = ready('generator-load-test', { assetId: asset.id });
    assert.throws(() => «completeJob»(scene.ctx, expert, job.id, { resolution: 'Load test passed at all steps' }), (error: any) => error.code === 'completion-rejected');
    assert.equal(assets(scene.ctx).require(asset.id).lastServicedOn, undefined);
  });
});
'''

FOLLOWUPS = r'''
describe('createFollowUp', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('needs completed work', () => {
    const job = open(scene);
    assert.throws(
      () => «createFollowUp»(scene.ctx, 'dispatch', job.id, 'Return visit'),
      (error: any) => error.code === 'not-completed' && /\(new\)/.test(error.message),
    );
  });

  it('inherits site and work type and reassigns the technician', () => {
    const job = open(scene, 'gas-safety-check', { tags: ['landlord'] });
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    for (const item of Object.keys(job.checklist)) «recordChecklistItem»(scene.ctx, scene.tech, job.id, item, true);
    «completeJob»(scene.ctx, scene.tech, job.id, { resolution: 'All appliances tested', signatureName: 'Avery Collins' });
    const followUp = «createFollowUp»(scene.ctx, 'dispatch', job.id, 'Replace flue seal');
    assert.equal(followUp.followUpOf, job.id);
    assert.equal(followUp.workType, 'gas-safety-check');
    assert.equal(followUp.siteId, job.siteId);
    assert.equal(followUp.status, 'assigned');
    assert.equal(followUp.technicianId, scene.tech);
    assert.deepEqual(followUp.tags, ['follow-up']);
    assert.equal(followUp.description, `Follow-up to ${job.number}: Investigate reported fault`);
  });

  it('lifts low priority to normal and keeps other priorities', () => {
    const low = open(scene, 'gas-safety-check');
    assert.equal(low.priority, 'low');
    work(scene, low.id);
    assert.equal(«createFollowUp»(scene.ctx, 'dispatch', low.id, 'Second visit').priority, 'normal');
    const high = open(scene, 'leak-investigation');
    work(scene, high.id);
    assert.equal(«createFollowUp»(scene.ctx, 'dispatch', high.id, 'Second visit').priority, 'high');
  });

  it('leaves the follow-up unassigned when the technician is no longer active', () => {
    const job = open(scene);
    work(scene, job.id);
    deactivateTechnician(scene.ctx, 'admin', scene.tech, 0);
    const followUp = «createFollowUp»(scene.ctx, 'dispatch', job.id, 'Second visit');
    assert.equal(followUp.status, 'new');
    assert.equal(followUp.technicianId, undefined);
  });

  it('allows one open follow-up per work order', () => {
    const job = open(scene);
    work(scene, job.id);
    const first = «createFollowUp»(scene.ctx, 'dispatch', job.id, 'Second visit');
    assert.throws(
      () => «createFollowUp»(scene.ctx, 'dispatch', job.id, 'Third visit'),
      (error: any) => error.code === 'follow-up-exists' && error.details.followUpId === first.id,
    );
  });

  it('allows a new follow-up once the earlier one was cancelled', () => {
    const job = open(scene);
    work(scene, job.id);
    const first = «createFollowUp»(scene.ctx, 'dispatch', job.id, 'Second visit');
    «cancelJob»(scene.ctx, 'dispatch', first.id, 'Customer declined');
    assert.notEqual(«createFollowUp»(scene.ctx, 'dispatch', job.id, 'Third visit').id, first.id);
  });

  it('works for closed work orders', () => {
    const job = open(scene);
    work(scene, job.id);
    «closeJob»(scene.ctx, 'dispatch', job.id);
    assert.equal(«createFollowUp»(scene.ctx, 'dispatch', job.id, 'Warranty visit').followUpOf, job.id);
  });

  it('can itself be followed up after completion', () => {
    const job = open(scene);
    work(scene, job.id);
    const first = «createFollowUp»(scene.ctx, 'dispatch', job.id, 'Second visit');
    work(scene, first.id);
    const second = «createFollowUp»(scene.ctx, 'dispatch', first.id, 'Third visit');
    assert.equal(second.followUpOf, first.id);
    assert.match(second.description, new RegExp(`^Follow-up to ${first.number}:`));
  });

  it('takes the next number in sequence', () => {
    const job = open(scene);
    work(scene, job.id);
    assert.equal(«createFollowUp»(scene.ctx, 'dispatch', job.id, 'Second visit').number, 'FL-2026-00002');
  });
});
'''

NOTES = r'''
describe('job notes', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('stores internal notes by default', () => {
    const job = open(scene);
    const note = «addJobNote»(scene.ctx, job.id, 'dispatch', '  Gate code 4471  ');
    assert.deepEqual(note, {
      id: 'NOTE-000001',
      jobId: job.id,
      authorId: 'dispatch',
      at: at(MONDAY, '08:00'),
      text: 'Gate code 4471',
      visibility: 'internal',
    });
  });

  it('rejects empty notes', () => {
    const job = open(scene);
    assert.throws(() => «addJobNote»(scene.ctx, job.id, 'dispatch', '  \n '), (error: any) => error.code === 'empty-note');
  });

  it('limits notes to 4000 characters', () => {
    const job = open(scene);
    assert.equal(«addJobNote»(scene.ctx, job.id, 'dispatch', 'x'.repeat(4000)).text.length, 4000);
    assert.throws(() => «addJobNote»(scene.ctx, job.id, 'dispatch', 'x'.repeat(4001)), (error: any) => error.code === 'note-too-long');
  });

  it('measures the limit after trimming', () => {
    const job = open(scene);
    assert.equal(«addJobNote»(scene.ctx, job.id, 'dispatch', `  ${'y'.repeat(4000)}  `).text.length, 4000);
  });

  it('rejects notes for unknown work orders', () => {
    assert.throws(() => «addJobNote»(scene.ctx, 'WO-000404', 'dispatch', 'Hello'), /work order WO-000404 was not found/);
  });

  it('lets customers see only shared notes', () => {
    const job = open(scene);
    «addJobNote»(scene.ctx, job.id, 'dispatch', 'Internal reminder');
    «addJobNote»(scene.ctx, job.id, 'dispatch', 'Technician arrives Monday', 'customer');
    assert.deepEqual(«listJobNotes»(scene.ctx, job.id, 'customer').map((note) => note.text), ['Technician arrives Monday']);
    assert.deepEqual(«listJobNotes»(scene.ctx, job.id, 'staff').map((note) => note.text), ['Internal reminder', 'Technician arrives Monday']);
  });

  it('orders notes by time and then by identifier', () => {
    const job = open(scene);
    «addJobNote»(scene.ctx, job.id, 'dispatch', 'first');
    «addJobNote»(scene.ctx, job.id, 'dispatch', 'second');
    scene.app.clock.advance(5);
    «addJobNote»(scene.ctx, job.id, 'dispatch', 'third');
    assert.deepEqual(«listJobNotes»(scene.ctx, job.id, 'staff').map((note) => note.text), ['first', 'second', 'third']);
  });

  it('keeps notes of different work orders apart', () => {
    const one = open(scene);
    const two = open(scene);
    «addJobNote»(scene.ctx, one.id, 'dispatch', 'only on one');
    assert.deepEqual(«listJobNotes»(scene.ctx, two.id, 'staff'), []);
    assert.equal(«listJobNotes»(scene.ctx, one.id, 'staff').length, 1);
  });
});
'''

NUMBERING = r'''
describe('work order numbers', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('formats the year and a five-digit sequence', () => {
    assert.equal(«nextJobNumber»(scene.ctx, '2026-03-02'), 'FL-2026-00001');
    assert.equal(«nextJobNumber»(scene.ctx, '2026-03-03'), 'FL-2026-00002');
  });

  it('restarts the sequence every calendar year', () => {
    assert.equal(«nextJobNumber»(scene.ctx, '2026-12-31'), 'FL-2026-00001');
    assert.equal(«nextJobNumber»(scene.ctx, '2027-01-01'), 'FL-2027-00001');
    assert.equal(«nextJobNumber»(scene.ctx, '2026-12-31'), 'FL-2026-00002');
  });

  it('keeps counting past five digits', () => {
    for (let index = 0; index < 99_999; index += 1) scene.ctx.ids.next('FL2030');
    assert.equal(«nextJobNumber»(scene.ctx, '2030-01-01'), 'FL-2030-100000');
  });

  it('numbers opened work orders by the day they were requested', () => {
    scene.app.clock.set('2026-12-31T22:00:00Z');
    const last = open(scene);
    scene.app.clock.set('2027-01-01T09:00:00Z');
    const first = open(scene);
    assert.equal(last.number, 'FL-2026-00001');
    assert.equal(first.number, 'FL-2027-00001');
  });

  it('never reuses the number of a cancelled work order', () => {
    const cancelled = open(scene);
    «cancelJob»(scene.ctx, 'dispatch', cancelled.id, 'Duplicate request');
    assert.equal(open(scene).number, 'FL-2026-00002');
  });
});

describe('parseJobNumber', () => {
  it('reads year and sequence', () => {
    assert.deepEqual(parseJobNumber('FL-2026-00042'), { year: 2026, sequence: 42 });
    assert.deepEqual(parseJobNumber('FL-2031-123456'), { year: 2031, sequence: 123456 });
  });

  it('ignores case and surrounding whitespace', () => {
    assert.deepEqual(parseJobNumber('  fl-2026-00007 '), { year: 2026, sequence: 7 });
  });

  it('rejects malformed numbers', () => {
    for (const value of ['FL-26-00001', 'FL-2026-0001', 'XX-2026-00001', 'FL2026-00001', 'FL-2026-00001-1', '', 'WO-000001']) {
      assert.equal(parseJobNumber(value), undefined, value);
    }
  });

  it('round-trips generated numbers', () => {
    const scene = start();
    const number = open(scene).number;
    assert.deepEqual(parseJobNumber(number), { year: 2026, sequence: 1 });
  });
});
'''

PRIORITIES = r'''
const handlerOf = (scene: Scene, key: string) => scene.ctx.workTypes.resolve(key);
const premium = { plan: 'premium' } as Contract;
const basic = { plan: 'basic' } as Contract;

describe('priorityRank', () => {
  it('orders the four priorities', () => {
    assert.deepEqual((['low', 'normal', 'high', 'emergency'] as «JobPriority»[]).map(priorityRank), [0, 1, 2, 3]);
  });

  it('sorts a list by urgency', () => {
    const list: «JobPriority»[] = ['high', 'low', 'emergency', 'normal'];
    assert.deepEqual([...list].sort((left, right) => priorityRank(right) - priorityRank(left)), ['emergency', 'high', 'normal', 'low']);
  });
});

describe('escalatePriority', () => {
  const cases: Array<[«JobPriority», «JobPriority»]> = [
    ['low', 'normal'],
    ['normal', 'high'],
    ['high', 'emergency'],
    ['emergency', 'emergency'],
  ];
  for (const [from, to] of cases) {
    it(`raises ${from} to ${to}`, () => {
      assert.equal(escalatePriority(from), to);
    });
  }
});

describe('defaultPriorityFor', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('uses the handler default without a contract', () => {
    assert.equal(defaultPriorityFor(handlerOf(scene, 'boiler-service')), 'low');
    assert.equal(defaultPriorityFor(handlerOf(scene, 'general')), 'normal');
    assert.equal(defaultPriorityFor(handlerOf(scene, 'leak-investigation')), 'high');
    assert.equal(defaultPriorityFor(handlerOf(scene, 'emergency-callout')), 'emergency');
  });

  it('ignores plans below premium', () => {
    assert.equal(defaultPriorityFor(handlerOf(scene, 'boiler-service'), basic), 'low');
    assert.equal(defaultPriorityFor(handlerOf(scene, 'boiler-service'), { plan: 'priority' } as Contract), 'low');
  });

  it('raises low and normal work one level on premium contracts', () => {
    assert.equal(defaultPriorityFor(handlerOf(scene, 'boiler-service'), premium), 'normal');
    assert.equal(defaultPriorityFor(handlerOf(scene, 'general'), premium), 'high');
  });

  it('keeps high priority and emergencies as they are on premium contracts', () => {
    assert.equal(defaultPriorityFor(handlerOf(scene, 'leak-investigation'), premium), 'high');
    assert.equal(defaultPriorityFor(handlerOf(scene, 'emergency-callout'), premium), 'emergency');
  });

  it('never defaults to emergency except for the emergency work type', () => {
    for (const key of scene.ctx.workTypes.keys()) {
      const handler = handlerOf(scene, key);
      assert.equal(defaultPriorityFor(handler, premium) === 'emergency', key === 'emergency-callout', key);
    }
  });
});
'''

QUERIES = r'''
describe('listJobs', () => {
  let scene: Scene;
  let ids: Record<string, string>;

  beforeEach(() => {
    scene = start();
    const other = seedCustomer(scene.app, { name: 'Neighbour Ltd' });
    ids = {};
    ids.low = open(scene, 'general', { priority: 'low', summary: 'Repaint skirting', tags: ['cosmetic'] }).id;
    scene.app.clock.advance(10);
    ids.normal = open(scene, 'general', { summary: 'Squeaking door' }).id;
    scene.app.clock.advance(10);
    ids.high = open(scene, 'leak-investigation', { summary: 'Ceiling stain in corridor' }).id;
    scene.app.clock.advance(10);
    ids.emergency = open(scene, 'emergency-callout', { summary: 'Water pouring from ceiling' }).id;
    scene.app.clock.advance(10);
    ids.other = «openJob»(scene.ctx, 'dispatch', { customerId: other.customer.id, siteId: other.site.id, workType: 'general', summary: 'Broken letterbox' }).id;
    «assignJob»(scene.ctx, 'dispatch', ids.normal, scene.tech);
    «cancelJob»(scene.ctx, 'dispatch', ids.low, 'Not needed');
  });

  const idsOf = (jobs: Array<{ id: string }>) => jobs.map((job) => job.id);

  it('returns everything, most urgent first and then oldest first', () => {
    assert.deepEqual(idsOf(«listJobs»(scene.ctx)), [ids.emergency, ids.high, ids.normal, ids.other, ids.low]);
  });

  it('breaks priority and time ties by identifier', () => {
    const first = open(scene, 'general', { summary: 'Tie one' });
    const second = open(scene, 'general', { summary: 'Tie two' });
    const tied = idsOf(«listJobs»(scene.ctx, { requestedFrom: first.requestedAt }));
    assert.deepEqual(tied.slice(-2), [first.id, second.id]);
  });

  it('filters by status', () => {
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { status: ['cancelled'] })), [ids.low]);
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { status: ['assigned', 'cancelled'] })), [ids.normal, ids.low]);
  });

  it('treats an empty status list as no filter', () => {
    assert.equal(«listJobs»(scene.ctx, { status: [] }).length, 5);
  });

  it('filters by customer, site and technician', () => {
    assert.equal(«listJobs»(scene.ctx, { customerId: scene.customer.id }).length, 4);
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { siteId: scene.site.id, status: ['new'] })), [ids.emergency, ids.high]);
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { technicianId: scene.tech })), [ids.normal]);
  });

  it('filters by work type and priority', () => {
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { workType: 'general' })), [ids.normal, ids.other, ids.low]);
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { priority: 'high' })), [ids.high]);
  });

  it('includes the start of the request window and excludes its end', () => {
    const from = at(MONDAY, '08:10');
    const to = at(MONDAY, '08:30');
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { requestedFrom: from, requestedTo: to })), [ids.high, ids.normal]);
  });

  it('searches the number, the summary and exact tags', () => {
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { text: 'CEILING' })), [ids.emergency, ids.high]);
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { text: 'fl-2026-00002' })), [ids.normal]);
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { text: 'cosmetic' })), [ids.low]);
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { text: 'cosmet' })), []);
  });

  it('ignores blank search text', () => {
    assert.equal(«listJobs»(scene.ctx, { text: '   ' }).length, 5);
  });

  it('combines criteria', () => {
    assert.deepEqual(idsOf(«listJobs»(scene.ctx, { customerId: scene.customer.id, workType: 'general', status: ['new', 'assigned'] })), [ids.normal]);
  });
});

describe('job lookups', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('getJob returns undefined for unknown identifiers while requireJob throws', () => {
    assert.equal(«getJob»(scene.ctx, 'WO-000404'), undefined);
    assert.throws(() => «requireJob»(scene.ctx, 'WO-000404'), (error: any) => error.status === 404 && error.details.entity === 'work order');
  });

  it('repository helpers find work orders by site, customer and number', () => {
    const job = open(scene);
    assert.deepEqual(jobsForSite(scene.ctx, scene.site.id).map((entry) => entry.id), [job.id]);
    assert.deepEqual(jobsForCustomer(scene.ctx, scene.customer.id).map((entry) => entry.id), [job.id]);
    assert.equal(findJobByNumber(scene.ctx, job.number)?.id, job.id);
    assert.equal(findJobByNumber(scene.ctx, 'FL-2026-09999'), undefined);
  });

  it('active work for a technician excludes finished and cancelled orders', () => {
    const done = open(scene);
    work(scene, done.id);
    const cancelled = open(scene);
    «assignJob»(scene.ctx, 'dispatch', cancelled.id, scene.tech);
    «cancelJob»(scene.ctx, 'dispatch', cancelled.id, 'Duplicate');
    const live = open(scene);
    «assignJob»(scene.ctx, 'dispatch', live.id, scene.tech);
    assert.deepEqual(activeJobsForTechnician(scene.ctx, scene.tech).map((entry) => entry.id), [live.id]);
  });

  it('jobsInStatus selects any of the given statuses', () => {
    const one = open(scene);
    const two = open(scene);
    «cancelJob»(scene.ctx, 'dispatch', two.id, 'Duplicate');
    assert.deepEqual(jobsInStatus(scene.ctx, 'new', 'cancelled').map((entry) => entry.id), [one.id, two.id]);
    assert.deepEqual(jobsInStatus(scene.ctx, 'completed'), []);
  });
});
'''

TIMELINE = r'''
describe('jobTimeline', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('starts with the opening entry', () => {
    const job = open(scene);
    assert.deepEqual(«jobTimeline»(scene.ctx, job.id), [{ at: at(MONDAY, '08:00'), kind: 'status', label: 'new', actorId: 'dispatch' }]);
  });

  it('lists status changes with their actors in order', () => {
    const job = open(scene);
    scene.app.clock.advance(15);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    scene.app.clock.advance(15);
    «startJob»(scene.ctx, scene.tech, job.id);
    const items = «jobTimeline»(scene.ctx, job.id);
    assert.deepEqual(items.map((item) => [item.at, item.label, item.actorId]), [
      [at(MONDAY, '08:00'), 'new', 'dispatch'],
      [at(MONDAY, '08:15'), 'assigned', 'dispatch'],
      [at(MONDAY, '08:30'), 'in_progress', scene.tech],
    ]);
  });

  it('inserts hold periods between the status entries', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    scene.app.clock.advance(30);
    «holdJob»(scene.ctx, scene.tech, job.id, 'awaiting_approval', 'Quote sent');
    scene.app.clock.advance(60);
    «resumeJob»(scene.ctx, scene.tech, job.id);
    const labels = «jobTimeline»(scene.ctx, job.id).map((item) => `${item.kind}:${item.label}`);
    assert.deepEqual(labels, [
      'status:new',
      'status:assigned',
      'status:in_progress',
      'status:on_hold: Quote sent',
      'hold:on hold (awaiting approval)',
      'status:in_progress',
      'release:hold released',
    ]);
  });

  it('puts status entries before the hold and release at the same instant', () => {
    const job = open(scene);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'weather');
    «resumeJob»(scene.ctx, 'dispatch', job.id);
    assert.deepEqual(«jobTimeline»(scene.ctx, job.id).map((item) => item.kind), ['status', 'status', 'status', 'hold', 'release']);
  });

  it('includes cancellation reasons in the label', () => {
    const job = open(scene);
    «cancelJob»(scene.ctx, 'dispatch', job.id, 'Customer moved out');
    assert.equal(«jobTimeline»(scene.ctx, job.id).at(-1)?.label, 'cancelled: Customer moved out');
  });

  it('fails for unknown work orders', () => {
    assert.throws(() => «jobTimeline»(scene.ctx, 'WO-000404'), (error: any) => error.status === 404);
  });
});
'''

EVENTS_BODY = r'''
describe('work order events', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('uses dotted event names', () => {
    assert.deepEqual({ ...«JobEvents» }, {
      opened: '«prefix».opened',
      scheduled: '«prefix».scheduled',
      assigned: '«prefix».assigned',
      started: '«prefix».started',
      held: '«prefix».held',
      resumed: '«prefix».resumed',
      completed: '«prefix».completed',
      cancelled: '«prefix».cancelled',
      closed: '«prefix».closed',
      reopened: '«prefix».reopened',
    });
  });

  it('publishes the opening with work type and priority', () => {
    const job = open(scene, 'leak-investigation');
    const [event] = eventsOf(scene, «JobEvents».opened);
    assert.equal(event.jobId, job.id);
    assert.equal(event.number, job.number);
    assert.equal(event.customerId, scene.customer.id);
    assert.equal(event.siteId, scene.site.id);
    assert.equal(event.status, 'new');
    assert.equal(event.actorId, 'dispatch');
    assert.equal(event.workType, 'leak-investigation');
    assert.equal(event.priority, 'high');
    assert.equal(event.at, at(MONDAY, '08:00'));
  });

  it('publishes scheduling with the slot', () => {
    const job = open(scene);
    «scheduleJob»(scene.ctx, 'dispatch', job.id, at(MONDAY, '10:00'), 45);
    const [event] = eventsOf(scene, «JobEvents».scheduled);
    assert.equal(event.start, at(MONDAY, '10:00'));
    assert.equal(event.end, at(MONDAY, '10:45'));
    assert.equal(event.status, 'scheduled');
  });

  it('publishes assignment with the previous technician', () => {
    const job = open(scene);
    const other = seedTechnician(scene.app, { name: 'Morgan Reyes' });
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «assignJob»(scene.ctx, 'dispatch', job.id, other.id);
    const events = eventsOf(scene, «JobEvents».assigned);
    assert.deepEqual(events.map((event) => [event.technicianId, event.previousTechnicianId]), [
      [scene.tech, undefined],
      [other.id, scene.tech],
    ]);
  });

  it('marks the first start of a work order', () => {
    const job = open(scene);
    work(scene, job.id);
    «reopenJob»(scene.ctx, 'dispatch', job.id, 'Back again');
    «startJob»(scene.ctx, scene.tech, job.id);
    assert.deepEqual(eventsOf(scene, «JobEvents».started).map((event) => event.firstStart), [true, false]);
  });

  it('publishes holds and resumptions with the elapsed time', () => {
    const job = open(scene);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'access_denied', 'Gate locked');
    scene.app.clock.advance(75);
    «resumeJob»(scene.ctx, 'dispatch', job.id);
    const [held] = eventsOf(scene, «JobEvents».held);
    const [resumed] = eventsOf(scene, «JobEvents».resumed);
    assert.equal(held.reason, 'access_denied');
    assert.equal(held.note, 'Gate locked');
    assert.equal(held.status, 'on_hold');
    assert.equal(resumed.reason, 'access_denied');
    assert.equal(resumed.heldMinutes, 75);
    assert.equal(resumed.status, 'new');
  });

  it('publishes the completion with the parts used', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    «recordChecklistItem»(scene.ctx, scene.tech, job.id, 'site left clean', true);
    «completeJob»(scene.ctx, scene.tech, job.id, {
      resolution: 'Replaced the door closer',
      partsUsed: [{ sku: 'CLOSER-EN3', quantity: 1 }],
      followUpRequired: true,
    });
    const [event] = eventsOf(scene, «JobEvents».completed);
    assert.deepEqual(event.partsUsed, [{ sku: 'CLOSER-EN3', quantity: 1 }]);
    assert.equal(event.followUpRequired, true);
    assert.equal(event.technicianId, scene.tech);
  });

  it('publishes cancellations with the reason and whether work had started', () => {
    const idle = open(scene);
    «cancelJob»(scene.ctx, 'dispatch', idle.id, 'Duplicate');
    const busy = open(scene);
    «assignJob»(scene.ctx, 'dispatch', busy.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, busy.id);
    «cancelJob»(scene.ctx, 'dispatch', busy.id, 'Site flooded');
    assert.deepEqual(eventsOf(scene, «JobEvents».cancelled).map((event) => [event.reason, event.wasStarted]), [
      ['Duplicate', false],
      ['Site flooded', true],
    ]);
  });

  it('publishes closing and reopening', () => {
    const job = open(scene);
    work(scene, job.id);
    «closeJob»(scene.ctx, 'billing-clerk', job.id);
    «reopenJob»(scene.ctx, 'dispatch', job.id, 'Customer complaint');
    assert.equal(eventsOf(scene, «JobEvents».closed)[0].actorId, 'billing-clerk');
    assert.equal(eventsOf(scene, «JobEvents».reopened)[0].note, 'Customer complaint');
  });

  it('does not publish anything when a transition is rejected', () => {
    const job = open(scene);
    «cancelJob»(scene.ctx, 'dispatch', job.id, 'Duplicate');
    const before = scene.ctx.bus.recent(200).length;
    assert.throws(() => «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech));
    assert.equal(scene.ctx.bus.recent(200).length, before);
  });

  it('delivers events to subscribers synchronously and in order', () => {
    const seen: string[] = [];
    scene.ctx.bus.on(«JobEvents».opened, (event) => seen.push(`first:${event.number}`));
    scene.ctx.bus.on(«JobEvents».opened, (event) => seen.push(`second:${event.number}`));
    open(scene);
    assert.deepEqual(seen, ['first:FL-2026-00001', 'second:FL-2026-00001']);
  });
});
'''


API_QUERIES = r'''
describe('work order listing over the API', () => {
  let world: World;
  let other: any;

  beforeEach(() => {
    world = createWorld();
    other = seedCustomer(world.app, { name: 'Neighbour Ltd' });
  });

  const idsOf = (page: any) => page.items.map((item: any) => item.id);

  it('rejects unknown status filters and bad paging', () => {
    expectStatus(world.app, 422, 'GET', `${JOBS}?status=new,lost`, 'dispatch');
    expectStatus(world.app, 422, 'GET', `${JOBS}?page=0`, 'dispatch');
    expectStatus(world.app, 422, 'GET', `${JOBS}?pageSize=201`, 'dispatch');
    expectStatus(world.app, 422, 'GET', `${JOBS}?page=abc`, 'dispatch');
  });

  it('orders by priority and filters by comma-separated statuses', () => {
    const { app } = world;
    const normal = openWorkOrder(app, world.customer.id, world.site.id, 'general');
    const high = openWorkOrder(app, world.customer.id, world.site.id, 'leak-investigation');
    const cancelled = openWorkOrder(app, world.customer.id, world.site.id, 'general');
    call(app, 'POST', `${JOBS}/${cancelled.id}/cancel`, 'dispatch', { reason: 'Duplicate request' });
    assert.deepEqual(idsOf(call(app, 'GET', JOBS, 'dispatch')), [high.id, normal.id, cancelled.id]);
    assert.deepEqual(idsOf(call(app, 'GET', `${JOBS}?status=cancelled,new`, 'dispatch')), [high.id, normal.id, cancelled.id]);
    assert.deepEqual(idsOf(call(app, 'GET', `${JOBS}?status=cancelled`, 'dispatch')), [cancelled.id]);
  });

  it('filters by customer, work type and free text', () => {
    const { app } = world;
    const mine = openWorkOrder(app, world.customer.id, world.site.id, 'general', { summary: 'Squeaking door' });
    const theirs = openWorkOrder(app, other.customer.id, other.site.id, 'leak-investigation', { summary: 'Dripping tap' });
    assert.deepEqual(idsOf(call(app, 'GET', `${JOBS}?customerId=${other.customer.id}`, 'dispatch')), [theirs.id]);
    assert.deepEqual(idsOf(call(app, 'GET', `${JOBS}?workType=general`, 'dispatch')), [mine.id]);
    assert.deepEqual(idsOf(call(app, 'GET', `${JOBS}?q=DRIPPING`, 'dispatch')), [theirs.id]);
    assert.deepEqual(idsOf(call(app, 'GET', `${JOBS}?q=${mine.number}`, 'dispatch')), [mine.id]);
  });

  it('exposes summaries without the detail fields', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    const [summary] = call(app, 'GET', JOBS, 'dispatch').items;
    assert.deepEqual(Object.keys(summary).sort(), ['customerId', 'id', 'number', 'priority', 'requestedAt', 'siteId', 'status', 'summary', 'workType']);
    assert.equal(summary.id, job.id);
    assert.equal('checklist' in summary, false);
  });

  it('shows customers only their own work orders, whatever filter they send', () => {
    const { app } = world;
    const portal = call(app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' });
    const mine = openWorkOrder(app, world.customer.id, world.site.id);
    const theirs = openWorkOrder(app, other.customer.id, other.site.id);
    assert.deepEqual(idsOf(call(app, 'GET', JOBS, portal.id)), [mine.id]);
    assert.deepEqual(idsOf(call(app, 'GET', `${JOBS}?customerId=${other.customer.id}`, portal.id)), [mine.id]);
    expectStatus(app, 404, 'GET', `${JOBS}/${theirs.id}`, portal.id);
    expectStatus(app, 404, 'GET', `${JOBS}/${theirs.id}/timeline`, portal.id);
  });

  it('shows technicians only the work orders assigned to them', () => {
    const { app } = world;
    const assigned = openWorkOrder(app, world.customer.id, world.site.id);
    openWorkOrder(app, world.customer.id, world.site.id);
    call(app, 'POST', `${JOBS}/${assigned.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    assert.deepEqual(idsOf(call(app, 'GET', JOBS, world.technician.id)), [assigned.id]);
    assert.deepEqual(idsOf(call(app, 'GET', `${JOBS}?technicianId=TEC-000099`, world.technician.id)), [assigned.id]);
  });

  it('serves every staff role that may read work orders', () => {
    const { app } = world;
    openWorkOrder(app, world.customer.id, world.site.id);
    for (const actor of ['admin', 'dispatch', 'billing', 'warehouse']) {
      assert.equal(call(app, 'GET', JOBS, actor).total, 1, actor);
    }
  });

  it('answers 404 for unknown work orders', () => {
    const body = expectStatus(world.app, 404, 'GET', `${JOBS}/WO-000404`, 'dispatch');
    assert.equal(body.error.code, 'not-found');
    assert.equal(body.error.details.entity, 'work order');
  });

  it('pages through large result sets', () => {
    const { app } = world;
    for (let index = 0; index < 7; index += 1) openWorkOrder(app, world.customer.id, world.site.id, 'general', { summary: `Request number ${index}` });
    const first = call(app, 'GET', `${JOBS}?pageSize=3`, 'dispatch');
    const last = call(app, 'GET', `${JOBS}?pageSize=3&page=3`, 'dispatch');
    const beyond = call(app, 'GET', `${JOBS}?pageSize=3&page=4`, 'dispatch');
    assert.deepEqual([first.items.length, first.pages, first.total], [3, 3, 7]);
    assert.equal(last.items.length, 1);
    assert.deepEqual(beyond.items, []);
  });
});
'''

API_FLOW = r'''
describe('work order flow over the API', () => {
  let world: World;

  beforeEach(() => {
    world = createWorld();
  });

  const finish = (jobId: string, workType = 'general') => {
    const { app } = world;
    call(app, 'POST', `${JOBS}/${jobId}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(app, 'POST', `${JOBS}/${jobId}/start`, world.technician.id);
    completeChecklist(app, jobId, world.technician.id);
    return call(app, 'POST', `${JOBS}/${jobId}/complete`, world.technician.id, {
      resolution: 'Replaced worn hinge and adjusted closer',
      signatureName: 'Avery Collins',
    });
  };

  it('validates the opening payload', () => {
    const body = expectStatus(world.app, 422, 'POST', JOBS, 'dispatch', { customerId: world.customer.id, siteId: world.site.id, workType: 'general', summary: 'x', priority: 'urgent' });
    const paths = body.error.details.issues.map((issue: any) => issue.path);
    assert.deepEqual(paths, ['summary', 'priority']);
  });

  it('lets customers open work for their own account only', () => {
    const { app } = world;
    const portal = call(app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' });
    const other = seedCustomer(app, { name: 'Neighbour Ltd' });
    const own = expectStatus(app, 201, 'POST', JOBS, portal.id, { customerId: world.customer.id, siteId: world.site.id, workType: 'general', summary: 'Door stuck' });
    assert.equal(own.source, 'portal');
    assert.equal(own.tags, undefined);
    expectStatus(app, 403, 'POST', JOBS, portal.id, { customerId: other.customer.id, siteId: other.site.id, workType: 'general', summary: 'Door stuck' });
  });

  it('keeps scheduling and assignment to staff', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    for (const path of ['schedule', 'assign', 'cancel', 'close', 'reopen', 'follow-ups']) {
      expectStatus(app, 403, 'POST', `${JOBS}/${job.id}/${path}`, world.technician.id, {});
    }
    const portal = call(app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' });
    expectStatus(app, 403, 'POST', `${JOBS}/${job.id}/assign`, portal.id, { technicianId: world.technician.id });
  });

  it('defaults the visit length to the work type duration', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id, 'leak-investigation');
    call(app, 'POST', `${JOBS}/${job.id}/schedule`, 'dispatch', { start: at(MONDAY, '10:00') });
    assert.equal(call(app, 'GET', `${JOBS}/${job.id}`, 'dispatch').scheduledEnd, at(MONDAY, '11:30'));
    call(app, 'POST', `${JOBS}/${job.id}/schedule`, 'dispatch', { start: at(MONDAY, '10:00'), durationMinutes: 30 });
    assert.equal(call(app, 'GET', `${JOBS}/${job.id}`, 'dispatch').scheduledEnd, at(MONDAY, '10:30'));
  });

  it('validates the visit length', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    expectStatus(app, 422, 'POST', `${JOBS}/${job.id}/schedule`, 'dispatch', { start: at(MONDAY, '10:00'), durationMinutes: 10 });
    expectStatus(app, 422, 'POST', `${JOBS}/${job.id}/schedule`, 'dispatch', { start: at(MONDAY, '10:00'), durationMinutes: 721 });
    expectStatus(app, 422, 'POST', `${JOBS}/${job.id}/schedule`, 'dispatch', { start: 'Monday morning' });
  });

  it('reports unqualified technicians', () => {
    const { app } = world;
    const asset = seedAsset(app, world.site.id, 'heat-pump');
    const job = openWorkOrder(app, world.customer.id, world.site.id, 'heat-pump-service', { assetId: asset.id });
    const body = expectStatus(app, 409, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    assert.equal(body.error.code, 'technician-not-qualified');
    assert.deepEqual(body.error.details.missing, ['refrigerant']);
  });

  it('rejects completion that breaks the work type rules', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    call(app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    completeChecklist(app, job.id, world.technician.id);
    const body = expectStatus(app, 422, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Fixed it' });
    assert.equal(body.error.code, 'completion-rejected');
    assert.deepEqual(body.error.details.problems, ['describe the work performed in at least 10 characters']);
  });

  it('returns the checklist state after each tick', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id, 'gas-safety-check');
    call(app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    const checklist = call(app, 'PUT', `${JOBS}/${job.id}/checklist`, world.technician.id, { item: 'flues inspected', done: true });
    assert.deepEqual(checklist, { 'appliances tested': false, 'flues inspected': true, 'certificate issued': false });
    expectStatus(app, 422, 'PUT', `${JOBS}/${job.id}/checklist`, world.technician.id, { item: 'flues inspected' });
    expectStatus(app, 422, 'PUT', `${JOBS}/${job.id}/checklist`, world.technician.id, { item: 'sweep floor', done: true });
  });

  it('closes, reopens and follows up completed work', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    finish(job.id);
    assert.equal(call(app, 'POST', `${JOBS}/${job.id}/close`, 'dispatch').status, 'closed');
    expectStatus(app, 422, 'POST', `${JOBS}/${job.id}/reopen`, 'dispatch', { reason: 'no' });
    const reopened = call(app, 'POST', `${JOBS}/${job.id}/reopen`, 'dispatch', { reason: 'Leak is back' });
    assert.equal(reopened.status, 'assigned');
    assert.equal(call(app, 'GET', `${JOBS}/${job.id}`, 'dispatch').reopenCount, 1);
  });

  it('creates follow-up visits and refuses a second one', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    finish(job.id);
    const followUp = expectStatus(app, 201, 'POST', `${JOBS}/${job.id}/follow-ups`, 'dispatch', { summary: 'Check closer tension' });
    assert.equal(followUp.status, 'assigned');
    assert.equal(followUp.technicianId, world.technician.id);
    const body = expectStatus(app, 409, 'POST', `${JOBS}/${job.id}/follow-ups`, 'dispatch', { summary: 'Check closer tension again' });
    assert.equal(body.error.details.followUpId, followUp.id);
  });

  it('lets customers add shared notes only', () => {
    const { app } = world;
    const portal = call(app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' });
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    const note = expectStatus(app, 201, 'POST', `${JOBS}/${job.id}/notes`, portal.id, { text: 'Please call before arriving', visibility: 'internal' });
    assert.equal(note.visibility, 'customer');
    assert.equal(call(app, 'GET', `${JOBS}/${job.id}`, 'dispatch').notes.length, 1);
    expectStatus(app, 422, 'POST', `${JOBS}/${job.id}/notes`, 'dispatch', { text: '' });
  });

  it('reports the active hold in the detail view and clears it on resume', () => {
    const { app } = world;
    const job = openWorkOrder(app, world.customer.id, world.site.id);
    call(app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    call(app, 'POST', `${JOBS}/${job.id}/hold`, world.technician.id, { reason: 'access_denied' });
    assert.equal(call(app, 'GET', `${JOBS}/${job.id}`, 'dispatch').hold.reason, 'access_denied');
    call(app, 'POST', `${JOBS}/${job.id}/resume`, world.technician.id, {});
    assert.equal(call(app, 'GET', `${JOBS}/${job.id}`, 'dispatch').hold, undefined);
    expectStatus(app, 422, 'POST', `${JOBS}/${job.id}/hold`, world.technician.id, { reason: 'lunch' });
  });
});
'''

FILES = {
    "tests/workorders/opening.test.ts": scene_spec(
        {WORLD: ["at", "call", "MONDAY", "seedAsset", "seedContract", "seedCustomer"]}, OPENING),
    "tests/workorders/transitions.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY"]}, TRANSITIONS,
        jobs=["type JobStatus", "canTransition", "isOpenStatus", "OPEN_STATUSES", "JOB_PRIORITIES", "scheduleJob",
              "cancelJob", "closeJob"]),
    "tests/workorders/lifecycle.test.ts": scene_spec(
        {WORLD: ["at", "call", "MONDAY", "seedAsset", "seedTechnician"], TECH_SERVICE: ["deactivateTechnician"]},
        LIFECYCLE, jobs=["scheduleJob", "cancelJob", "closeJob", "reopenJob", "holdJob"]),
    "tests/workorders/holds.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY"]}, HOLDS,
        jobs=["holdJob", "resumeJob", "scheduleJob", "jobAge", "HOLD_REASONS", "type HoldReason"]),
    "tests/workorders/completion.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY", "seedAsset", "seedTechnician"], "src/adapters/repositories/assets.ts": ["assets"]},
        COMPLETION),
    "tests/workorders/followups.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY"], TECH_SERVICE: ["deactivateTechnician"]}, FOLLOWUPS,
        jobs=["createFollowUp", "cancelJob", "closeJob"]),
    "tests/workorders/notes.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY"]}, NOTES, jobs=["addJobNote", "listJobNotes"]),
    "tests/workorders/numbering.test.ts": scene_spec(
        lambda ctx: {WORLD: ["createWorld"], f"{ctx.jobs_dir}/numbering.ts": ["parseJobNumber"]}, NUMBERING,
        jobs=["nextJobNumber", "cancelJob"]),
    "tests/workorders/priorities.test.ts": scene_spec(
        {"src/contracts/model.ts": ["type Contract"]}, PRIORITIES,
        jobs=["priorityRank", "escalatePriority", "defaultPriorityFor", "type JobPriority"]),
    "tests/workorders/queries.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY", "seedCustomer"], "src/adapters/repositories/jobs.ts": [
            "activeJobsForTechnician", "findJobByNumber", "jobsForCustomer", "jobsForSite", "jobsInStatus"]},
        QUERIES, jobs=["listJobs", "getJob", "cancelJob"]),
    "tests/workorders/timeline.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY"]}, TIMELINE, jobs=["jobTimeline", "holdJob", "resumeJob", "cancelJob"]),
    "tests/workorders/events.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY", "seedTechnician"]}, EVENTS_BODY,
        jobs=["JobEvents", "scheduleJob", "holdJob", "resumeJob", "cancelJob", "closeJob", "reopenJob"],
        subs=lambda ctx: {"prefix": "workorder" if ctx.split else "job"}),
    "tests/api/workorder-queries.test.ts": Spec(
        {WORLD: ["call", "createWorld", "expectStatus", "JOBS", "openWorkOrder", "seedCustomer", "type World"]},
        API_QUERIES, node=WITH_SETUP),
    "tests/api/workorder-flow.test.ts": Spec(
        {WORLD: ["at", "call", "completeChecklist", "createWorld", "expectStatus", "JOBS", "MONDAY",
                 "openWorkOrder", "seedAsset", "seedCustomer", "type World"]},
        API_FLOW, node=WITH_SETUP),
}


def generate(ctx):
    register(ctx, FILES)
