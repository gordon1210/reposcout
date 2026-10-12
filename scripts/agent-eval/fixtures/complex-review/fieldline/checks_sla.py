"""Tests for service levels: policies, credits, clocks, monitoring and the SLA routes."""

from .checks_kit import DESCRIBE_IT, WITH_SETUP, Spec, register
from .checks_workorders import scene_spec

WORLD = "tests/support/world.ts"
SLA = "src/sla"
CLOCKS = "src/adapters/repositories/sla-clocks.ts"

POLICIES = r'''
const policies = Object.values(SLA_POLICIES);

describe('SLA_POLICIES', () => {
  it('is keyed by policy id', () => {
    assert.deepEqual(Object.keys(SLA_POLICIES).sort(), ['emergency', 'premium', 'priority', 'standard']);
    for (const [key, policy] of Object.entries(SLA_POLICIES)) assert.equal(policy.id, key);
  });

  const targets: Array<[string, number, number, string, number]> = [
    ['standard', 1200, 3000, 'business', 50],
    ['priority', 240, 960, 'extended', 100],
    ['premium', 120, 480, 'always', 200],
    ['emergency', 60, 240, 'always', 250],
  ];
  for (const [id, response, resolution, coverage, credit] of targets) {
    it(`defines the ${id} targets`, () => {
      const policy = policyById(id);
      assert.equal(policy.responseMinutes, response);
      assert.equal(policy.resolutionMinutes, resolution);
      assert.equal(policy.coverage, coverage);
      assert.equal(policy.creditBasisPointsPerHour, credit);
    });
  }

  it('always asks for a response before the resolution', () => {
    for (const policy of policies) assert.ok(policy.responseMinutes < policy.resolutionMinutes, policy.id);
  });

  it('pays higher credits for stricter policies', () => {
    const byStrictness = [...policies].sort((left, right) => right.resolutionMinutes - left.resolutionMinutes);
    const rates = byStrictness.map((policy) => policy.creditBasisPointsPerHour);
    assert.deepEqual(rates, [...rates].sort((left, right) => left - right));
  });
});

describe('policyById', () => {
  it('returns the registered policy', () => {
    assert.equal(policyById('premium'), SLA_POLICIES.premium);
  });

  it('rejects unknown policy ids with a domain error', () => {
    assert.throws(
      () => policyById('platinum'),
      (error: any) => error instanceof DomainError && error.code === 'unknown-sla-policy' && error.status === 422,
    );
  });
});

describe('policyFor', () => {
  it('keeps the contract policy for ordinary work', () => {
    for (const id of ['standard', 'priority', 'premium']) assert.equal(policyFor(id, false).id, id);
  });

  it('applies the emergency policy when it is stricter', () => {
    for (const id of ['standard', 'priority', 'premium']) assert.equal(policyFor(id, true).id, 'emergency');
  });

  it('does not replace the emergency policy itself', () => {
    assert.equal(policyFor('emergency', true), SLA_POLICIES.emergency);
    assert.equal(policyFor('emergency', false), SLA_POLICIES.emergency);
  });

  it('fails for contracts that reference an unknown policy', () => {
    assert.throws(() => policyFor('gold', true), /Unknown SLA policy gold/);
  });
});

describe('coverageCalendar', () => {
  const holidays = ['2026-04-03', '2026-04-06'];

  it('counts business cover on weekdays between 08:00 and 18:00', () => {
    const calendar = coverageCalendar(SLA_POLICIES.standard, holidays);
    assert.equal(calendar.dayStartMinute, 480);
    assert.equal(calendar.dayEndMinute, 1080);
    assert.deepEqual([...calendar.workingDays], [1, 2, 3, 4, 5]);
    assert.deepEqual([...calendar.holidays], holidays);
  });

  it('adds Saturdays and longer hours for extended cover', () => {
    const calendar = coverageCalendar(SLA_POLICIES.priority, holidays);
    assert.equal(calendar.dayStartMinute, 420);
    assert.equal(calendar.dayEndMinute, 1260);
    assert.deepEqual([...calendar.workingDays], [1, 2, 3, 4, 5, 6]);
  });

  it('ignores holidays for round-the-clock cover', () => {
    for (const policy of [SLA_POLICIES.premium, SLA_POLICIES.emergency]) {
      const calendar = coverageCalendar(policy, holidays);
      assert.equal(calendar, ALWAYS_OPEN);
      assert.deepEqual([...calendar.holidays], []);
    }
  });

  it('skips the weekend under business cover', () => {
    const calendar = coverageCalendar(SLA_POLICIES.standard, []);
    assert.equal(workingMinutesBetween(calendar, at('2026-03-06', '16:00'), at('2026-03-09', '10:00')), 240);
  });

  it('works on Saturdays under extended cover', () => {
    const calendar = coverageCalendar(SLA_POLICIES.priority, []);
    assert.equal(workingMinutesBetween(calendar, at('2026-03-06', '16:00'), at('2026-03-09', '10:00')), 1320);
  });

  it('skips public holidays unless cover is round the clock', () => {
    const from = at('2026-04-02', '16:00');
    const to = at('2026-04-07', '10:00');
    assert.equal(workingMinutesBetween(coverageCalendar(SLA_POLICIES.standard, holidays), from, to), 240);
    assert.equal(workingMinutesBetween(coverageCalendar(SLA_POLICIES.premium, holidays), from, to), 6840);
  });

  it('recognises working time on the business calendar', () => {
    const calendar = coverageCalendar(SLA_POLICIES.standard, holidays);
    assert.equal(isWorkingTime(calendar, at(MONDAY, '07:59')), false);
    assert.equal(isWorkingTime(calendar, at(MONDAY, '08:00')), true);
    assert.equal(isWorkingTime(calendar, at(MONDAY, '17:59')), true);
    assert.equal(isWorkingTime(calendar, at(MONDAY, '18:00')), false);
    assert.equal(isWorkingTime(calendar, at('2026-04-03', '12:00')), false);
  });
});
'''

CREDITS = r'''
const policy = (id: string) => policyById(id);

describe('slaCreditBasisPoints', () => {
  const cases: Array<[string, number, number, number]> = [
    ['premium', 0, 2000, 0],
    ['premium', -30, 2000, 0],
    ['premium', 1, 2000, 200],
    ['premium', 60, 2000, 200],
    ['premium', 61, 2000, 400],
    ['premium', 135, 2000, 600],
    ['premium', 480, 2000, 1600],
    ['premium', 600, 2000, 2000],
    ['premium', 5000, 2000, 2000],
    ['priority', 15, 2000, 100],
    ['priority', 150, 2000, 300],
    ['emergency', 15, 2000, 200],
    ['emergency', 75, 2000, 500],
    ['emergency', 120, 2000, 500],
    ['emergency', 135, 2000, 700],
    ['emergency', 1000, 2000, 2000],
    ['emergency', 180, 300, 300],
    ['premium', 180, 1000, 600],
  ];
  for (const [id, breach, cap, expected] of cases) {
    it(`grants ${expected} basis points for ${breach} minutes on ${id} with a cap of ${cap}`, () => {
      assert.equal(slaCreditBasisPoints(policy(id), breach, cap), expected);
    });
  }

  it('grants nothing for the first hours of the standard policy', () => {
    assert.equal(slaCreditBasisPoints(policy('standard'), 30, 2000), 0);
    assert.equal(slaCreditBasisPoints(policy('standard'), 60, 2000), 0);
    assert.equal(slaCreditBasisPoints(policy('standard'), 90, 2000), 100);
    assert.equal(slaCreditBasisPoints(policy('standard'), 180, 2000), 100);
    assert.equal(slaCreditBasisPoints(policy('standard'), 240, 2000), 200);
  });

  it('always returns whole percent steps within the cap', () => {
    for (const id of ['standard', 'priority', 'premium', 'emergency']) {
      for (let minutes = 0; minutes <= 900; minutes += 15) {
        const credit = slaCreditBasisPoints(policy(id), minutes, 1500);
        assert.equal(credit % 100, 0, `${id} ${minutes}`);
        assert.ok(credit >= 0 && credit <= 1500, `${id} ${minutes}`);
      }
    }
  });

  it('never decreases with a longer breach', () => {
    for (const id of ['standard', 'priority', 'premium', 'emergency']) {
      let previous = 0;
      for (let minutes = 0; minutes <= 1200; minutes += 5) {
        const credit = slaCreditBasisPoints(policy(id), minutes, 2000);
        assert.ok(credit >= previous, `${id} ${minutes}`);
        previous = credit;
      }
    }
  });

  it('grants nothing when the cap is zero', () => {
    assert.equal(slaCreditBasisPoints(policy('emergency'), 300, 0), 0);
  });
});
'''

CLOCK = r'''
let scene: Scene;

beforeEach(() => {
  scene = start();
});

function contracted(plan: string, extra: Partial<«OpenJobInput»> = {}) {
  seedContract(scene.app, scene.customer.id, [scene.site.id], plan, ['general']);
  return open(scene, 'general', extra);
}

const clockOf = (jobId: string) => {
  const clock = clockForJob(scene.ctx, jobId);
  assert.ok(clock, `no clock for ${jobId}`);
  return clock;
};

function beginAt(jobId: string, clock: string) {
  scene.app.clock.set(at(MONDAY, clock));
  «assignJob»(scene.ctx, 'dispatch', jobId, scene.tech);
  «startJob»(scene.ctx, scene.tech, jobId);
}

function finishAt(jobId: string, clock: string) {
  scene.app.clock.set(at(MONDAY, clock));
  for (const item of Object.keys(«requireJob»(scene.ctx, jobId).checklist)) «recordChecklistItem»(scene.ctx, scene.tech, jobId, item, true);
  return «completeJob»(scene.ctx, scene.tech, jobId, { resolution: 'Cleared the blockage and flushed the line' });
}

describe('clock creation', () => {
  it('starts a running clock when contracted work is opened', () => {
    const job = contracted('premium');
    assert.deepEqual(clockOf(job.id), {
      id: `SLA-${job.id}`,
      jobId: job.id,
      policyId: 'premium',
      startedAt: at(MONDAY, '08:00'),
      status: 'running',
      pausedMinutes: 0,
      pauses: 0,
      responseBreached: false,
      warned: false,
    });
  });

  it('does not start a clock for work outside a contract', () => {
    const job = open(scene);
    assert.equal(clockForJob(scene.ctx, job.id), undefined);
    assert.equal(eventsOf(scene, SlaEvents.started).length, 0);
  });

  it('uses the policy of the contract plan', () => {
    const job = contracted('priority');
    assert.equal(clockOf(job.id).policyId, 'priority');
  });

  it('switches to the emergency policy for emergency work', () => {
    const job = contracted('basic', { priority: 'emergency' });
    assert.equal(clockOf(job.id).policyId, 'emergency');
  });

  it('announces the started clock on the bus', () => {
    const job = contracted('basic');
    const [event] = eventsOf(scene, SlaEvents.started);
    assert.equal(event.jobId, job.id);
    assert.equal(event.policyId, 'standard');
    assert.equal(event.at, at(MONDAY, '08:00'));
  });
});

describe('elapsed time', () => {
  it('counts every minute for round-the-clock cover', () => {
    const job = contracted('premium');
    assert.equal(elapsedMinutes(scene.ctx, clockOf(job.id), at(MONDAY, '09:30')), 90);
    assert.equal(elapsedMinutes(scene.ctx, clockOf(job.id), at('2026-03-03', '08:00')), 1440);
  });

  it('counts only business hours under the standard policy', () => {
    scene.app.clock.set(at('2026-03-06', '16:00'));
    const job = contracted('basic');
    const clock = clockOf(job.id);
    assert.equal(elapsedMinutes(scene.ctx, clock, at('2026-03-06', '17:00')), 60);
    assert.equal(elapsedMinutes(scene.ctx, clock, at('2026-03-06', '20:00')), 120);
    assert.equal(elapsedMinutes(scene.ctx, clock, at('2026-03-08', '12:00')), 120);
    assert.equal(elapsedMinutes(scene.ctx, clock, at('2026-03-09', '09:00')), 180);
  });

  it('never goes negative before the start', () => {
    const job = contracted('premium');
    assert.equal(elapsedMinutes(scene.ctx, clockOf(job.id), at(MONDAY, '07:00')), 0);
  });

  it('uses the configured holidays', () => {
    scene = start({ config: { holidays: ['2026-03-03'] } });
    scene.app.clock.set(at(MONDAY, '17:00'));
    const job = contracted('basic');
    assert.equal(elapsedMinutes(scene.ctx, clockOf(job.id), at('2026-03-04', '09:00')), 120);
  });

  it('stays frozen once the work is resolved', () => {
    const job = contracted('premium');
    beginAt(job.id, '09:00');
    finishAt(job.id, '11:00');
    const resolved = clockOf(job.id);
    assert.equal(elapsedMinutes(scene.ctx, resolved, at(MONDAY, '11:00')), 180);
    assert.equal(elapsedMinutes(scene.ctx, resolved, at('2026-03-05', '11:00')), 180);
  });
});

describe('response tracking', () => {
  it('records the first start as the response', () => {
    const job = contracted('premium');
    beginAt(job.id, '09:30');
    const clock = clockOf(job.id);
    assert.equal(clock.respondedAt, at(MONDAY, '09:30'));
    assert.equal(clock.responseBreached, false);
  });

  it('flags a response later than the policy allows', () => {
    const job = contracted('premium');
    beginAt(job.id, '10:30');
    assert.equal(clockOf(job.id).responseBreached, true);
  });

  it('accepts a response exactly at the target', () => {
    const job = contracted('premium');
    beginAt(job.id, '10:00');
    assert.equal(clockOf(job.id).responseBreached, false);
  });

  it('keeps the first response when recorded again', () => {
    const job = contracted('premium');
    beginAt(job.id, '09:00');
    scene.app.clock.set(at(MONDAY, '13:00'));
    recordResponse(scene.ctx, job.id);
    const clock = clockOf(job.id);
    assert.equal(clock.respondedAt, at(MONDAY, '09:00'));
    assert.equal(clock.responseBreached, false);
  });

  it('ignores work orders without a clock', () => {
    assert.equal(recordResponse(scene.ctx, open(scene).id), undefined);
  });
});

describe('resolution', () => {
  it('meets the target when work finishes in time', () => {
    const job = contracted('premium');
    beginAt(job.id, '09:00');
    const done = finishAt(job.id, '11:30');
    const clock = clockOf(job.id);
    assert.equal(done.status, 'completed');
    assert.equal(clock.status, 'met');
    assert.equal(clock.resolvedAt, at(MONDAY, '11:30'));
    assert.equal(clock.breachMinutes, 0);
    assert.equal(clock.creditBasisPoints, undefined);
  });

  it('meets the target exactly at the limit', () => {
    const job = contracted('premium');
    beginAt(job.id, '09:00');
    finishAt(job.id, '16:00');
    assert.equal(clockOf(job.id).status, 'met');
  });

  it('breaches one minute past the limit and credits the started quarter hour', () => {
    const job = contracted('premium');
    beginAt(job.id, '09:00');
    finishAt(job.id, '16:01');
    const clock = clockOf(job.id);
    assert.equal(clock.status, 'breached');
    assert.equal(clock.breachMinutes, 15);
    assert.equal(clock.creditBasisPoints, 200);
  });

  const emergencyCases: Array<[number, number, number]> = [
    [1, 15, 200],
    [15, 15, 200],
    [16, 30, 200],
    [60, 60, 200],
    [61, 75, 500],
    [120, 120, 500],
    [121, 135, 700],
    [180, 180, 700],
  ];
  for (const [over, breach, credit] of emergencyCases) {
    it(`measures a ${over} minute overrun of an emergency as ${breach} minutes and ${credit} basis points`, () => {
      scene.app.clock.set(at(MONDAY, '09:00'));
      const job = contracted('basic', { priority: 'emergency' });
      beginAt(job.id, '09:00');
      const finish = 13 * 60 + over;
      finishAt(job.id, `${String(Math.floor(finish / 60)).padStart(2, '0')}:${String(finish % 60).padStart(2, '0')}`);
      const clock = clockOf(job.id);
      assert.equal(clock.status, 'breached');
      assert.equal(clock.breachMinutes, breach);
      assert.equal(clock.creditBasisPoints, credit);
    });
  }

  it('limits the credit to the configured cap', () => {
    scene = start({ config: { sla: { creditCapBasisPoints: 300 } } });
    scene.app.clock.set(at(MONDAY, '09:00'));
    const job = contracted('basic', { priority: 'emergency' });
    beginAt(job.id, '09:00');
    finishAt(job.id, '16:00');
    assert.equal(clockOf(job.id).breachMinutes, 180);
    assert.equal(clockOf(job.id).creditBasisPoints, 300);
  });

  it('publishes the outcome', () => {
    const met = contracted('premium');
    beginAt(met.id, '09:00');
    finishAt(met.id, '10:00');
    const [metEvent] = eventsOf(scene, SlaEvents.met);
    assert.equal(metEvent.jobId, met.id);
    assert.equal(metEvent.elapsedMinutes, 120);
    const late = open(scene);
    «assignJob»(scene.ctx, 'dispatch', late.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, late.id);
    finishAt(late.id, '20:00');
    const [breachEvent] = eventsOf(scene, SlaEvents.breached);
    assert.equal(breachEvent.jobId, late.id);
    assert.equal(breachEvent.breachMinutes, 120);
    assert.equal(breachEvent.creditBasisPoints, 400);
  });

  it('keeps counting while the work waits for parts', () => {
    const job = contracted('premium');
    beginAt(job.id, '09:00');
    scene.app.clock.set(at(MONDAY, '09:30'));
    «holdJob»(scene.ctx, scene.tech, job.id, 'awaiting_parts');
    assert.equal(clockOf(job.id).status, 'running');
    scene.app.clock.set(at(MONDAY, '12:00'));
    «resumeJob»(scene.ctx, scene.tech, job.id);
    assert.equal(clockOf(job.id).pausedMinutes, 0);
    finishAt(job.id, '14:00');
    const clock = clockOf(job.id);
    assert.equal(clock.status, 'met');
    assert.equal(eventsOf(scene, SlaEvents.met)[0].elapsedMinutes, 360);
  });
});

describe('cancellation and reopening', () => {
  it('stops the clock of cancelled work without an outcome', () => {
    const job = contracted('premium');
    scene.app.clock.set(at(MONDAY, '10:00'));
    «cancelJob»(scene.ctx, 'dispatch', job.id, 'Customer resolved it themselves');
    const clock = clockOf(job.id);
    assert.equal(clock.status, 'stopped');
    assert.equal(clock.resolvedAt, at(MONDAY, '10:00'));
    assert.equal(clock.breachMinutes, undefined);
    assert.equal(eventsOf(scene, SlaEvents.breached).length, 0);
  });

  it('leaves a resolved clock alone when stopped', () => {
    const job = contracted('premium');
    beginAt(job.id, '09:00');
    finishAt(job.id, '10:00');
    assert.equal(stopClock(scene.ctx, job.id)?.status, 'met');
  });

  it('does not stop a clock twice', () => {
    const job = contracted('premium');
    scene.app.clock.set(at(MONDAY, '10:00'));
    stopClock(scene.ctx, job.id);
    scene.app.clock.set(at(MONDAY, '12:00'));
    assert.equal(stopClock(scene.ctx, job.id)?.resolvedAt, at(MONDAY, '10:00'));
  });

  it('resumes the original clock when completed work is reopened', () => {
    const job = contracted('premium');
    beginAt(job.id, '09:00');
    finishAt(job.id, '11:00');
    scene.app.clock.set(at(MONDAY, '12:00'));
    «reopenJob»(scene.ctx, 'dispatch', job.id, 'The leak returned');
    const reopened = clockOf(job.id);
    assert.equal(reopened.status, 'running');
    assert.equal(reopened.resolvedAt, undefined);
    assert.equal(reopened.startedAt, at(MONDAY, '08:00'));
    assert.equal(elapsedMinutes(scene.ctx, reopened, at(MONDAY, '13:00')), 300);
  });

  it('clears the credit of a breached clock on reopening', () => {
    const job = contracted('premium');
    beginAt(job.id, '09:00');
    finishAt(job.id, '20:00');
    assert.equal(clockOf(job.id).status, 'breached');
    «reopenJob»(scene.ctx, 'dispatch', job.id, 'Pressure drop again');
    const reopened = clockOf(job.id);
    assert.equal(reopened.status, 'running');
    assert.equal(reopened.breachMinutes, undefined);
    assert.equal(reopened.creditBasisPoints, undefined);
  });

  it('ignores reopening for clocks that are still running', () => {
    const job = contracted('premium');
    assert.equal(reopenClock(scene.ctx, job.id)?.status, 'running');
    assert.equal(reopenClock(scene.ctx, 'WO-999999'), undefined);
  });
});
'''

MONITOR = r'''
let scene: Scene;
let covered: boolean;

beforeEach(() => {
  scene = start();
  covered = false;
});

function contracted(plan: string, priority?: 'emergency') {
  if (!covered) seedContract(scene.app, scene.customer.id, [scene.site.id], plan, ['general']);
  covered = true;
  return open(scene, 'general', priority ? { priority } : {});
}

function finishAt(jobId: string, clock: string) {
  scene.app.clock.set(at(MONDAY, clock));
  «assignJob»(scene.ctx, 'dispatch', jobId, scene.tech);
  «startJob»(scene.ctx, scene.tech, jobId);
  for (const item of Object.keys(«requireJob»(scene.ctx, jobId).checklist)) «recordChecklistItem»(scene.ctx, scene.tech, jobId, item, true);
  «completeJob»(scene.ctx, scene.tech, jobId, { resolution: 'Replaced the thermostat and verified' });
}

describe('slaStatus', () => {
  it('reports the remaining time of a running clock', () => {
    const job = contracted('premium');
    const status = slaStatus(scene.ctx, clockForJob(scene.ctx, job.id)!, at(MONDAY, '10:30'));
    assert.deepEqual(status, {
      jobId: job.id,
      policyId: 'premium',
      status: 'running',
      elapsedMinutes: 150,
      remainingMinutes: 330,
      pausedMinutes: 0,
      responseBreached: false,
      breachMinutes: 0,
      creditBasisPoints: 0,
    });
  });

  it('floors the remaining time at zero once the target has passed', () => {
    const job = contracted('premium');
    const status = slaStatus(scene.ctx, clockForJob(scene.ctx, job.id)!, at(MONDAY, '18:00'));
    assert.equal(status.elapsedMinutes, 600);
    assert.equal(status.remainingMinutes, 0);
    assert.equal(status.status, 'running');
  });

  it('shows breach and credit of a resolved clock', () => {
    const job = contracted('premium');
    finishAt(job.id, '19:20');
    const status = slaStatus(scene.ctx, clockForJob(scene.ctx, job.id)!, at(MONDAY, '23:00'));
    assert.equal(status.status, 'breached');
    assert.equal(status.elapsedMinutes, 680);
    assert.equal(status.breachMinutes, 210);
    assert.equal(status.creditBasisPoints, 800);
    assert.equal(status.remainingMinutes, 0);
  });
});

describe('scanForWarnings', () => {
  it('warns once the configured share of the resolution time is used', () => {
    const job = contracted('premium');
    scene.app.clock.set(at(MONDAY, '14:23'));
    assert.deepEqual(scanForWarnings(scene.ctx), []);
    scene.app.clock.set(at(MONDAY, '14:24'));
    assert.deepEqual(scanForWarnings(scene.ctx), [job.id]);
    assert.equal(clockForJob(scene.ctx, job.id)?.warned, true);
  });

  it('warns only once per clock', () => {
    const job = contracted('premium');
    scene.app.clock.set(at(MONDAY, '15:00'));
    assert.deepEqual(scanForWarnings(scene.ctx), [job.id]);
    scene.app.clock.set(at(MONDAY, '15:30'));
    assert.deepEqual(scanForWarnings(scene.ctx), []);
    assert.equal(eventsOf(scene, SlaEvents.warning).length, 1);
  });

  it('publishes the used share rounded to hundredths', () => {
    const job = contracted('premium');
    scene.app.clock.set(at(MONDAY, '15:00'));
    scanForWarnings(scene.ctx);
    const [event] = eventsOf(scene, SlaEvents.warning);
    assert.equal(event.jobId, job.id);
    assert.equal(event.usedRatio, 0.88);
  });

  it('honours a configured warning ratio', () => {
    scene = start({ config: { sla: { warningRatio: 0.5 } } });
    covered = false;
    const job = contracted('premium');
    scene.app.clock.set(at(MONDAY, '11:59'));
    assert.deepEqual(scanForWarnings(scene.ctx), []);
    scene.app.clock.set(at(MONDAY, '12:00'));
    assert.deepEqual(scanForWarnings(scene.ctx), [job.id]);
  });

  it('skips clocks that are already resolved or stopped', () => {
    const resolved = contracted('premium');
    finishAt(resolved.id, '09:00');
    const cancelled = contracted('premium');
    «cancelJob»(scene.ctx, 'dispatch', cancelled.id, 'Duplicate request');
    scene.app.clock.set(at(MONDAY, '23:00'));
    assert.deepEqual(scanForWarnings(scene.ctx), []);
  });

  it('warns for several clocks in one pass', () => {
    const first = contracted('premium');
    const second = contracted('premium');
    const third = contracted('premium', 'emergency');
    scene.app.clock.set(at(MONDAY, '15:00'));
    assert.deepEqual(scanForWarnings(scene.ctx).sort(), [first.id, second.id, third.id].sort());
  });

  it('measures the share against the policy of the clock', () => {
    const emergency = contracted('basic', 'emergency');
    const ordinary = contracted('basic');
    scene.app.clock.set(at(MONDAY, '11:20'));
    assert.deepEqual(scanForWarnings(scene.ctx), [emergency.id]);
    assert.equal(clockForJob(scene.ctx, ordinary.id)?.warned, false);
  });
});

describe('complianceRate', () => {
  it('is undefined without finished clocks', () => {
    contracted('premium');
    assert.equal(complianceRate(scene.ctx, at('2026-03-01', '00:00'), at('2026-03-31', '00:00')), undefined);
  });

  it('is the share of finished clocks that met their target', () => {
    for (const clock of ['09:00', '10:00', '11:00']) finishAt(contracted('premium').id, clock);
    const late = contracted('premium');
    finishAt(late.id, '20:00');
    assert.equal(complianceRate(scene.ctx, at('2026-03-01', '00:00'), at('2026-03-31', '00:00')), 0.75);
  });

  it('only counts clocks resolved inside the half-open period', () => {
    const early = contracted('premium');
    finishAt(early.id, '09:00');
    const late = contracted('premium');
    finishAt(late.id, '23:00');
    assert.equal(complianceRate(scene.ctx, at(MONDAY, '00:00'), at(MONDAY, '09:00')), undefined);
    assert.equal(complianceRate(scene.ctx, at(MONDAY, '00:00'), at(MONDAY, '09:01')), 1);
    assert.equal(complianceRate(scene.ctx, at(MONDAY, '09:00'), at(MONDAY, '23:00')), 1);
    assert.equal(complianceRate(scene.ctx, at(MONDAY, '09:01'), at(MONDAY, '23:01')), 0);
  });

  it('ignores stopped and running clocks', () => {
    const cancelled = contracted('premium');
    «cancelJob»(scene.ctx, 'dispatch', cancelled.id, 'Raised twice by mistake');
    contracted('premium');
    assert.equal(complianceRate(scene.ctx, at('2026-03-01', '00:00'), at('2026-03-31', '00:00')), undefined);
  });
});
'''

API = r'''
let world: World;

beforeEach(() => {
  world = createWorld();
});

function contractedJob(plan = 'premium') {
  seedContract(world.app, world.customer.id, [world.site.id], plan, ['general']);
  const job = openWorkOrder(world.app, world.customer.id, world.site.id);
  call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
  return job;
}

function portalUser(customerId: string, name = 'Avery Collins') {
  return call(world.app, 'POST', `/customers/${customerId}/portal-users`, 'admin', { name }).id as string;
}

describe('GET /jobs/:id/sla', () => {
  it('returns the status of the clock', () => {
    const job = contractedJob('priority');
    world.app.clock.set(at(MONDAY, '10:00'));
    const sla = call(world.app, 'GET', `${JOBS}/${job.id}/sla`, 'dispatch');
    assert.equal(sla.jobId, job.id);
    assert.equal(sla.policyId, 'priority');
    assert.equal(sla.elapsedMinutes, 120);
    assert.equal(sla.remainingMinutes, 840);
  });

  it('reports work without a contract as missing', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id);
    const body = expectStatus(world.app, 404, 'GET', `${JOBS}/${job.id}/sla`, 'admin');
    assert.equal(body.error.code, 'not-found');
    assert.equal(body.error.details.entity, 'service level clock');
  });

  it('reports unknown work orders as missing', () => {
    expectStatus(world.app, 404, 'GET', `${JOBS}/WO-424242/sla`, 'admin');
  });

  it('shows the clock to the owning customer only', () => {
    const job = contractedJob();
    const owner = portalUser(world.customer.id);
    const stranger = portalUser(seedCustomer(world.app, { name: 'Brightwater Dental' }).customer.id, 'Sam Rivera');
    assert.equal(expectStatus(world.app, 200, 'GET', `${JOBS}/${job.id}/sla`, owner).policyId, 'premium');
    expectStatus(world.app, 404, 'GET', `${JOBS}/${job.id}/sla`, stranger);
  });

  it('shows the clock to the assigned technician only', () => {
    const job = contractedJob();
    const other = seedTechnician(world.app, { name: 'Morgan Reyes' });
    expectStatus(world.app, 200, 'GET', `${JOBS}/${job.id}/sla`, world.technician.id);
    expectStatus(world.app, 404, 'GET', `${JOBS}/${job.id}/sla`, other.id);
  });

  it('is open to every role with job access', () => {
    const job = contractedJob();
    for (const actor of ['admin', 'dispatch', 'billing', 'warehouse']) {
      expectStatus(world.app, 200, 'GET', `${JOBS}/${job.id}/sla`, actor);
    }
  });

  it('reflects a resolved clock', () => {
    const job = contractedJob();
    call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    completeChecklist(world.app, job.id, world.technician.id);
    world.app.clock.set(at(MONDAY, '09:15'));
    call(world.app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Tightened the compression fitting' });
    const sla = call(world.app, 'GET', `${JOBS}/${job.id}/sla`, 'dispatch');
    assert.equal(sla.status, 'met');
    assert.equal(sla.elapsedMinutes, 75);
    assert.equal(sla.remainingMinutes, 405);
  });
});

describe('POST /sla/scan', () => {
  it('lists the work orders that were warned', () => {
    const job = contractedJob();
    assert.deepEqual(expectStatus(world.app, 200, 'POST', '/sla/scan', 'admin'), { warned: [] });
    world.app.clock.set(at(MONDAY, '15:00'));
    assert.deepEqual(call(world.app, 'POST', '/sla/scan', 'admin'), { warned: [job.id] });
    assert.deepEqual(call(world.app, 'POST', '/sla/scan', 'admin'), { warned: [] });
  });

  it('is reserved for administrators', () => {
    for (const actor of ['dispatch', 'billing', 'warehouse', world.technician.id]) {
      assert.equal(expectStatus(world.app, 403, 'POST', '/sla/scan', actor).error.code, 'forbidden');
    }
  });
});

describe('GET /sla/compliance', () => {
  it('returns a null rate without finished work', () => {
    assert.deepEqual(call(world.app, 'GET', '/sla/compliance?from=2026-03-01&to=2026-04-01', 'billing'), {
      from: '2026-03-01',
      to: '2026-04-01',
      rate: null,
    });
  });

  it('returns the share of work that met its target', () => {
    const job = contractedJob();
    call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
    completeChecklist(world.app, job.id, world.technician.id);
    world.app.clock.set(at(MONDAY, '10:00'));
    call(world.app, 'POST', `${JOBS}/${job.id}/complete`, world.technician.id, { resolution: 'Swapped the circulation pump' });
    const report = call(world.app, 'GET', '/sla/compliance?from=2026-03-01&to=2026-04-01', 'dispatch');
    assert.equal(report.rate, 1);
    assert.equal(call(world.app, 'GET', '/sla/compliance?from=2026-03-03&to=2026-04-01', 'dispatch').rate, null);
  });

  it('requires both dates in calendar form', () => {
    expectStatus(world.app, 422, 'GET', '/sla/compliance?from=2026-03-01', 'admin');
    expectStatus(world.app, 422, 'GET', '/sla/compliance?to=2026-03-01', 'admin');
    expectStatus(world.app, 422, 'GET', '/sla/compliance?from=March&to=April', 'admin');
    expectStatus(world.app, 422, 'GET', '/sla/compliance?from=2026-02-30&to=2026-04-01', 'admin');
  });

  it('is limited to roles that read reports', () => {
    for (const actor of ['admin', 'dispatch', 'billing']) {
      expectStatus(world.app, 200, 'GET', '/sla/compliance?from=2026-03-01&to=2026-04-01', actor);
    }
    for (const actor of ['warehouse', world.technician.id, portalUser(world.customer.id)]) {
      expectStatus(world.app, 403, 'GET', '/sla/compliance?from=2026-03-01&to=2026-04-01', actor);
    }
  });
});
'''

FILES = {
    "tests/sla/policies.test.ts": Spec(
        {f"{SLA}/policies.ts": ["coverageCalendar", "policyById", "policyFor", "SLA_POLICIES"],
         "src/shared/calendar.ts": ["ALWAYS_OPEN", "isWorkingTime", "workingMinutesBetween"],
         "src/shared/errors.ts": ["DomainError"], WORLD: ["at", "MONDAY"]}, POLICIES),
    "tests/sla/credits.test.ts": Spec(
        {f"{SLA}/credits.ts": ["slaCreditBasisPoints"], f"{SLA}/policies.ts": ["policyById"]}, CREDITS),
    "tests/sla/clock.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY", "seedContract"], CLOCKS: ["clockForJob"],
         f"{SLA}/clock.ts": ["elapsedMinutes", "recordResponse", "reopenClock", "stopClock"],
         f"{SLA}/model.ts": ["SlaEvents"]},
        CLOCK, jobs=["cancelJob", "holdJob", "reopenJob", "resumeJob"], node=WITH_SETUP),
    "tests/sla/monitor.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY", "seedContract"], CLOCKS: ["clockForJob"],
         f"{SLA}/monitor.ts": ["complianceRate", "scanForWarnings", "slaStatus"], f"{SLA}/model.ts": ["SlaEvents"]},
        MONITOR, jobs=["cancelJob"], node=WITH_SETUP),
    "tests/api/sla-routes.test.ts": Spec(
        {WORLD: ["at", "call", "completeChecklist", "createWorld", "expectStatus", "JOBS", "MONDAY",
                 "openWorkOrder", "seedContract", "seedCustomer", "seedTechnician", "type World"]},
        API, node=WITH_SETUP),
}


def generate(ctx):
    register(ctx, FILES)
