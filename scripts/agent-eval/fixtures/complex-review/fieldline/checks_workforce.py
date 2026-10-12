"""Tests for technicians, scheduling, dispatch and their API routes."""

from .checks_kit import APP, WITH_SETUP, Spec, register
from .checks_workorders import SCENE, scene_spec

WORLD = "tests/support/world.ts"
TECH = "src/technicians/service.ts"
SKILLS = "src/technicians/skills.ts"
SLOTS = "src/scheduling/slots.ts"
SCHED = "src/scheduling/service.ts"
TRAVEL = "src/scheduling/travel-time.ts"
WINDOWS = "src/scheduling/windows.ts"
CAPACITY = "src/scheduling/capacity.ts"
ROUTE = "src/scheduling/route.ts"
TECHS_REPO = "src/adapters/repositories/technicians.ts"
APPTS_REPO = "src/adapters/repositories/appointments.ts"

TECHNICIAN_SERVICE = r'''
function fresh() {
  const app = createApp({ now: '2026-03-02T08:00:00Z' });
  return { app, ctx: app.ctx };
}

const input = (overrides: Record<string, unknown> = {}) => ({
  name: 'Robin Hale',
  email: 'Robin.Hale@Fieldline.example',
  region: 'metro-east',
  home: { lat: 52.5, lon: 13.38 },
  hourlyCostCents: 3100,
  ...overrides,
});

describe('createTechnician', () => {
  it('stores the technician with defaults and a van location', () => {
    const { ctx } = fresh();
    const technician = createTechnician(ctx, 'admin', input());
    assert.equal(technician.id, 'TEC-000001');
    assert.equal(technician.vanLocationId, 'VAN-000001');
    assert.equal(technician.active, true);
    assert.equal(technician.roaming, false);
    assert.deepEqual(technician.certifications, []);
    assert.deepEqual(technician.absences, []);
    assert.equal(technician.maxDailyAppointments, undefined);
    assert.equal(technicians(ctx).get('TEC-000001')?.name, 'Robin Hale');
  });

  it('lower-cases the email address', () => {
    const { ctx } = fresh();
    assert.equal(createTechnician(ctx, 'admin', input()).email, 'robin.hale@fieldline.example');
  });

  it('numbers technicians sequentially', () => {
    const { ctx } = fresh();
    createTechnician(ctx, 'admin', input());
    const second = createTechnician(ctx, 'admin', input({ email: 'sam@fieldline.example' }));
    assert.equal(second.id, 'TEC-000002');
    assert.equal(second.vanLocationId, 'VAN-000002');
  });

  it('rejects a duplicate email regardless of case', () => {
    const { ctx } = fresh();
    createTechnician(ctx, 'admin', input());
    assert.throws(
      () => createTechnician(ctx, 'admin', input({ email: 'ROBIN.HALE@fieldline.example' })),
      (error: any) => error.code === 'duplicate-email' && error.status === 409,
    );
    assert.equal(technicians(ctx).count(() => true), 1);
  });

  it('keeps certifications and the daily appointment cap', () => {
    const { ctx } = fresh();
    const technician = createTechnician(
      ctx,
      'admin',
      input({ certifications: [{ skill: 'gas-safe', expiresOn: '2027-01-31' }, { skill: 'plumbing' }], maxDailyAppointments: 3, roaming: true }),
    );
    assert.deepEqual(technician.certifications, [{ skill: 'gas-safe', expiresOn: '2027-01-31' }, { skill: 'plumbing' }]);
    assert.equal(technician.maxDailyAppointments, 3);
    assert.equal(technician.roaming, true);
  });

  it('records an audit entry', () => {
    const { ctx } = fresh();
    createTechnician(ctx, 'dispatch', input());
    const [entry] = auditTrail(ctx, 'TEC-000001');
    assert.equal(entry.action, 'technician.created');
    assert.equal(entry.actorId, 'dispatch');
    assert.equal(entry.at, '2026-03-02T08:00:00Z');
  });

  const invalid: Array<[string, Record<string, unknown>, string]> = [
    ['a one-letter name', { name: 'R' }, 'name'],
    ['an email without a domain', { email: 'robin' }, 'email'],
    ['an empty region', { region: '' }, 'region'],
    ['a latitude beyond 90', { home: { lat: 91, lon: 0 } }, 'home.lat'],
    ['a longitude beyond 180', { home: { lat: 0, lon: 181 } }, 'home.lon'],
    ['a negative hourly cost', { hourlyCostCents: -1 }, 'hourlyCostCents'],
    ['a fractional hourly cost', { hourlyCostCents: 31.5 }, 'hourlyCostCents'],
    ['a zero daily appointment cap', { maxDailyAppointments: 0 }, 'maxDailyAppointments'],
    ['a daily appointment cap above twelve', { maxDailyAppointments: 13 }, 'maxDailyAppointments'],
    ['a malformed expiry date', { certifications: [{ skill: 'gas-safe', expiresOn: '31/01/2027' }] }, 'certifications'],
  ];
  for (const [label, overrides, path] of invalid) {
    it(`rejects ${label}`, () => {
      const { ctx } = fresh();
      assert.throws(
        () => createTechnician(ctx, 'admin', input(overrides)),
        (error: any) => error.code === 'invalid-input' && error.status === 422 && JSON.stringify(error.details).includes(path),
      );
      assert.equal(technicians(ctx).count(() => true), 0);
    });
  }
});

describe('deactivateTechnician', () => {
  it('deactivates a technician without open work', () => {
    const { ctx } = fresh();
    const technician = createTechnician(ctx, 'admin', input());
    const updated = deactivateTechnician(ctx, 'admin', technician.id, 0);
    assert.equal(updated.active, false);
    assert.deepEqual(activeTechnicians(ctx), []);
    assert.equal(auditTrail(ctx, technician.id).at(-1)?.action, 'technician.deactivated');
  });

  it('refuses while work orders remain', () => {
    const { ctx } = fresh();
    const technician = createTechnician(ctx, 'admin', input());
    assert.throws(
      () => deactivateTechnician(ctx, 'admin', technician.id, 2),
      (error: any) => error.code === 'technician-has-work' && error.status === 409 && error.details.openWork === 2,
    );
    assert.equal(technicians(ctx).get(technician.id)?.active, true);
  });
});

describe('technician regions', () => {
  it('lists active technicians of a region plus roaming technicians', () => {
    const { ctx } = fresh();
    const east = createTechnician(ctx, 'admin', input({ email: 'east@fieldline.example' }));
    createTechnician(ctx, 'admin', input({ email: 'west@fieldline.example', region: 'metro-west' }));
    const roaming = createTechnician(ctx, 'admin', input({ email: 'roam@fieldline.example', region: 'metro-west', roaming: true }));
    const retired = createTechnician(ctx, 'admin', input({ email: 'gone@fieldline.example' }));
    deactivateTechnician(ctx, 'admin', retired.id, 0);
    assert.deepEqual(techniciansInRegion(ctx, 'metro-east').map((technician) => technician.id), [east.id, roaming.id]);
    assert.equal(techniciansInRegion(ctx, 'metro-north').length, 1);
    assert.equal(activeTechnicians(ctx).length, 3);
  });
});
'''

SKILLS_BODY = r'''
function fresh() {
  const app = createApp({ now: '2026-03-02T08:00:00Z' });
  const technician = createTechnician(app.ctx, 'admin', {
    name: 'Robin Hale',
    email: 'robin@fieldline.example',
    region: 'metro-east',
    home: { lat: 52.5, lon: 13.38 },
    hourlyCostCents: 3100,
    certifications: [
      { skill: 'gas-safe', expiresOn: '2026-03-20' },
      { skill: 'plumbing' },
      { skill: 'refrigerant', expiresOn: '2026-03-02' },
    ],
  });
  return { ctx: app.ctx, technician };
}

describe('hasSkill', () => {
  it('accepts a certification without an expiry date on any day', () => {
    const { technician } = fresh();
    assert.equal(hasSkill(technician, 'plumbing', '2026-03-02'), true);
    assert.equal(hasSkill(technician, 'plumbing', '2040-01-01'), true);
  });

  it('accepts a certification up to and including its expiry date', () => {
    const { technician } = fresh();
    assert.equal(hasSkill(technician, 'gas-safe', '2026-03-19'), true);
    assert.equal(hasSkill(technician, 'gas-safe', '2026-03-20'), true);
  });

  it('rejects a certification the day after it lapses', () => {
    const { technician } = fresh();
    assert.equal(hasSkill(technician, 'gas-safe', '2026-03-21'), false);
  });

  it('rejects skills the technician never held', () => {
    const { technician } = fresh();
    assert.equal(hasSkill(technician, 'lifts', '2026-03-02'), false);
  });

  it('matches skill names exactly', () => {
    const { technician } = fresh();
    assert.equal(hasSkill(technician, 'Gas-Safe', '2026-03-02'), false);
    assert.equal(hasSkill(technician, 'gas', '2026-03-02'), false);
  });
});

describe('missingSkills', () => {
  it('returns nothing when every skill is valid', () => {
    const { technician } = fresh();
    assert.deepEqual(missingSkills(technician, ['gas-safe', 'plumbing'], '2026-03-10'), []);
  });

  it('returns lapsed and unknown skills in the requested order', () => {
    const { technician } = fresh();
    assert.deepEqual(missingSkills(technician, ['lifts', 'gas-safe', 'plumbing'], '2026-04-01'), ['lifts', 'gas-safe']);
  });

  it('returns nothing for an empty requirement', () => {
    const { technician } = fresh();
    assert.deepEqual(missingSkills(technician, [], '2026-03-02'), []);
  });
});

describe('expiringCertifications', () => {
  it('lists certifications that expire within the window, soonest first', () => {
    const { ctx } = fresh();
    const found = expiringCertifications(ctx, '2026-03-02', 30);
    assert.deepEqual(
      found.map((entry) => [entry.skill, entry.expiresOn, entry.daysLeft]),
      [['refrigerant', '2026-03-02', 0], ['gas-safe', '2026-03-20', 18]],
    );
    assert.equal(found[0].technicianId, 'TEC-000001');
  });

  it('excludes certifications beyond the window', () => {
    const { ctx } = fresh();
    assert.deepEqual(expiringCertifications(ctx, '2026-03-02', 10).map((entry) => entry.skill), ['refrigerant']);
    assert.deepEqual(expiringCertifications(ctx, '2026-03-02', 18).map((entry) => entry.skill), ['refrigerant', 'gas-safe']);
  });

  it('skips certifications that have already lapsed', () => {
    const { ctx } = fresh();
    assert.deepEqual(expiringCertifications(ctx, '2026-03-03', 60).map((entry) => entry.skill), ['gas-safe']);
  });

  it('ignores deactivated technicians', () => {
    const { ctx, technician } = fresh();
    deactivateTechnician(ctx, 'admin', technician.id, 0);
    assert.deepEqual(expiringCertifications(ctx, '2026-03-02', 365), []);
  });

  it('breaks ties by technician id', () => {
    const { ctx } = fresh();
    createTechnician(ctx, 'admin', {
      name: 'Sam Ortiz',
      email: 'sam@fieldline.example',
      region: 'metro-east',
      home: { lat: 52.5, lon: 13.38 },
      hourlyCostCents: 3000,
      certifications: [{ skill: 'gas-safe', expiresOn: '2026-03-20' }],
    });
    const found = expiringCertifications(ctx, '2026-03-02', 30);
    assert.deepEqual(found.map((entry) => entry.technicianId), ['TEC-000001', 'TEC-000001', 'TEC-000002']);
    assert.deepEqual(found.map((entry) => entry.skill), ['refrigerant', 'gas-safe', 'gas-safe']);
  });
});
'''

CERTIFICATIONS = r'''
function fresh() {
  const app = createApp({ now: '2026-03-02T08:00:00Z' });
  const technician = createTechnician(app.ctx, 'admin', {
    name: 'Robin Hale',
    email: 'robin@fieldline.example',
    region: 'metro-east',
    home: { lat: 52.5, lon: 13.38 },
    hourlyCostCents: 3100,
  });
  return { ctx: app.ctx, technician };
}

describe('updateCertifications', () => {
  it('replaces the certification list', () => {
    const { ctx, technician } = fresh();
    const updated = updateCertifications(ctx, 'admin', technician.id, [{ skill: 'gas-safe', expiresOn: '2027-01-01' }]);
    assert.deepEqual(updated.certifications, [{ skill: 'gas-safe', expiresOn: '2027-01-01' }]);
    const replaced = updateCertifications(ctx, 'admin', technician.id, [{ skill: 'plumbing' }]);
    assert.deepEqual(replaced.certifications, [{ skill: 'plumbing' }]);
  });

  it('keeps the later expiry when a skill is listed twice', () => {
    const { ctx, technician } = fresh();
    const updated = updateCertifications(ctx, 'admin', technician.id, [
      { skill: 'gas-safe', expiresOn: '2026-06-01' },
      { skill: 'gas-safe', expiresOn: '2027-06-01' },
    ]);
    assert.deepEqual(updated.certifications, [{ skill: 'gas-safe', expiresOn: '2027-06-01' }]);
  });

  it('keeps the later expiry regardless of order', () => {
    const { ctx, technician } = fresh();
    const updated = updateCertifications(ctx, 'admin', technician.id, [
      { skill: 'gas-safe', expiresOn: '2027-06-01' },
      { skill: 'gas-safe', expiresOn: '2026-06-01' },
    ]);
    assert.deepEqual(updated.certifications, [{ skill: 'gas-safe', expiresOn: '2027-06-01' }]);
  });

  it('prefers a certification that never expires', () => {
    const { ctx, technician } = fresh();
    const updated = updateCertifications(ctx, 'admin', technician.id, [
      { skill: 'plumbing', expiresOn: '2027-01-01' },
      { skill: 'plumbing' },
    ]);
    assert.deepEqual(updated.certifications, [{ skill: 'plumbing' }]);
    const reversed = updateCertifications(ctx, 'admin', technician.id, [{ skill: 'plumbing' }, { skill: 'plumbing', expiresOn: '2027-01-01' }]);
    assert.deepEqual(reversed.certifications, [{ skill: 'plumbing' }]);
  });

  it('clears all certifications with an empty list', () => {
    const { ctx, technician } = fresh();
    updateCertifications(ctx, 'admin', technician.id, [{ skill: 'plumbing' }]);
    assert.deepEqual(updateCertifications(ctx, 'admin', technician.id, []).certifications, []);
  });

  it('audits the number of distinct certifications', () => {
    const { ctx, technician } = fresh();
    updateCertifications(ctx, 'dispatch', technician.id, [{ skill: 'plumbing' }, { skill: 'plumbing' }, { skill: 'gas-safe' }]);
    const entry = auditTrail(ctx, technician.id).at(-1);
    assert.equal(entry?.action, 'technician.certifications');
    assert.deepEqual(entry?.details, { count: 2 });
  });

  it('rejects unknown technicians', () => {
    const { ctx } = fresh();
    assert.throws(() => updateCertifications(ctx, 'admin', 'TEC-999999', []), (error: any) => error.status === 404);
  });
});
'''

ABSENCES = r'''
function fresh() {
  const app = createApp({ now: '2026-03-02T08:00:00Z' });
  const technician = createTechnician(app.ctx, 'admin', {
    name: 'Robin Hale',
    email: 'robin@fieldline.example',
    region: 'metro-east',
    home: { lat: 52.5, lon: 13.38 },
    hourlyCostCents: 3100,
  });
  return { ctx: app.ctx, technician };
}

describe('recordAbsence', () => {
  it('stores an absence on the technician', () => {
    const { ctx, technician } = fresh();
    const updated = recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-09', to: '2026-03-13', kind: 'leave' });
    assert.deepEqual(updated.absences, [{ from: '2026-03-09', to: '2026-03-13', kind: 'leave' }]);
  });

  it('accepts a single-day absence', () => {
    const { ctx, technician } = fresh();
    const updated = recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-09', to: '2026-03-09', kind: 'sick' });
    assert.equal(updated.absences.length, 1);
  });

  it('rejects an absence that ends before it starts', () => {
    const { ctx, technician } = fresh();
    assert.throws(
      () => recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-10', to: '2026-03-09', kind: 'leave' }),
      (error: any) => error.code === 'invalid-absence' && error.status === 422,
    );
  });

  it('keeps absences ordered by start date', () => {
    const { ctx, technician } = fresh();
    recordAbsence(ctx, 'admin', technician.id, { from: '2026-04-06', to: '2026-04-10', kind: 'training' });
    recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-09', to: '2026-03-10', kind: 'sick' });
    const updated = recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-23', to: '2026-03-27', kind: 'leave' });
    assert.deepEqual(updated.absences.map((absence) => absence.from), ['2026-03-09', '2026-03-23', '2026-04-06']);
  });

  const overlapping: Array<[string, string, string]> = [
    ['identical range', '2026-03-09', '2026-03-13'],
    ['overlap at the start', '2026-03-05', '2026-03-09'],
    ['overlap at the end', '2026-03-13', '2026-03-17'],
    ['contained range', '2026-03-10', '2026-03-11'],
    ['enclosing range', '2026-03-01', '2026-03-31'],
  ];
  for (const [label, from, to] of overlapping) {
    it(`rejects an absence with ${label}`, () => {
      const { ctx, technician } = fresh();
      recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-09', to: '2026-03-13', kind: 'leave' });
      assert.throws(
        () => recordAbsence(ctx, 'admin', technician.id, { from, to, kind: 'sick' }),
        (error: any) => error.code === 'overlapping-absence' && error.status === 409,
      );
      assert.equal(technicians(ctx).get(technician.id)?.absences.length, 1);
    });
  }

  it('accepts adjacent absences', () => {
    const { ctx, technician } = fresh();
    recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-09', to: '2026-03-13', kind: 'leave' });
    const updated = recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-14', to: '2026-03-15', kind: 'leave' });
    assert.equal(updated.absences.length, 2);
  });

  it('audits the absence', () => {
    const { ctx, technician } = fresh();
    recordAbsence(ctx, 'dispatch', technician.id, { from: '2026-03-09', to: '2026-03-13', kind: 'leave' });
    const entry = auditTrail(ctx, technician.id).at(-1);
    assert.equal(entry?.action, 'technician.absence');
    assert.deepEqual(entry?.details, { from: '2026-03-09', to: '2026-03-13', kind: 'leave' });
  });
});

describe('isAvailable', () => {
  it('is true on ordinary days', () => {
    const { technician } = fresh();
    assert.equal(isAvailable(technician, '2026-03-02'), true);
  });

  it('is false during an absence, including both boundary days', () => {
    const { ctx, technician } = fresh();
    const away = recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-09', to: '2026-03-13', kind: 'leave' });
    assert.equal(isAvailable(away, '2026-03-08'), true);
    assert.equal(isAvailable(away, '2026-03-09'), false);
    assert.equal(isAvailable(away, '2026-03-11'), false);
    assert.equal(isAvailable(away, '2026-03-13'), false);
    assert.equal(isAvailable(away, '2026-03-14'), true);
  });

  it('is false for inactive technicians', () => {
    const { ctx, technician } = fresh();
    const retired = deactivateTechnician(ctx, 'admin', technician.id, 0);
    assert.equal(isAvailable(retired, '2026-03-02'), false);
  });

  it('considers every absence', () => {
    const { ctx, technician } = fresh();
    recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-09', to: '2026-03-09', kind: 'sick' });
    const away = recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-16', to: '2026-03-16', kind: 'training' });
    assert.deepEqual(['2026-03-09', '2026-03-10', '2026-03-16'].map((day) => isAvailable(away, day)), [false, true, false]);
  });
});
'''

TRAVEL_BODY = r'''
const origin = { lat: 0, lon: 0 };
const north = (degrees: number) => ({ lat: degrees, lon: 0 });

describe('travelMinutes', () => {
  it('is zero for identical points', () => {
    assert.equal(travelMinutes(origin, origin, 40), 0);
  });

  it('is zero below half a road kilometre', () => {
    assert.equal(travelMinutes(origin, north(0.003), 40), 0);
  });

  it('rounds up to whole five-minute steps', () => {
    assert.equal(travelMinutes(origin, north(0.1), 40), 25);
    assert.equal(travelMinutes(origin, north(0.2), 40), 45);
  });

  it('is faster at a higher average speed', () => {
    assert.equal(travelMinutes(origin, north(0.1), 80), 15);
    assert.equal(travelMinutes(origin, north(0.1), 20), 45);
  });

  it('is symmetric', () => {
    const from = { lat: 52.5, lon: 13.38 };
    const to = { lat: 52.6, lon: 13.5 };
    assert.equal(travelMinutes(from, to, 40), travelMinutes(to, from, 40));
  });

  it('grows with distance', () => {
    const minutes = [0.05, 0.1, 0.2, 0.4, 0.8].map((degrees) => travelMinutes(origin, north(degrees), 40));
    assert.deepEqual(minutes, [15, 25, 45, 90, 175]);
  });

  it('rejects a non-positive speed', () => {
    assert.throws(() => travelMinutes(origin, north(0.1), 0), RangeError);
    assert.throws(() => travelMinutes(origin, north(0.1), -10), RangeError);
  });

  it('returns multiples of five', () => {
    for (let step = 1; step <= 30; step += 1) {
      assert.equal(travelMinutes(origin, north(step * 0.037), 40) % 5, 0);
    }
  });
});

describe('travelKm', () => {
  it('measures road kilometres with one decimal', () => {
    assert.equal(travelKm(origin, north(0.1)), 14.5);
    assert.equal(travelKm(origin, north(0.2)), 28.9);
    assert.equal(travelKm(origin, north(1)), 144.6);
  });

  it('is zero for identical points', () => {
    assert.equal(travelKm(origin, origin), 0);
  });

  it('applies the detour factor to the great-circle distance', () => {
    const direct = distanceKm(origin, north(0.5));
    assert.equal(travelKm(origin, north(0.5)), Math.round(direct * 1.3 * 10) / 10);
  });

  it('is symmetric', () => {
    const from = { lat: 52.5, lon: 13.38 };
    const to = { lat: 52.52, lon: 13.4 };
    assert.equal(travelKm(from, to), travelKm(to, from));
  });
});
'''

WINDOWS_BODY = r'''
describe('arrivalWindow', () => {
  const ctx = () => createApp({ now: '2026-03-02T08:00:00Z' }).ctx;

  it('promises the two hours containing the start', () => {
    assert.deepEqual(arrivalWindow(ctx(), '2026-03-02T09:00:00Z'), {
      windowStart: '2026-03-02T08:00:00Z',
      windowEnd: '2026-03-02T10:00:00Z',
    });
    assert.deepEqual(arrivalWindow(ctx(), '2026-03-02T11:30:00Z'), {
      windowStart: '2026-03-02T10:00:00Z',
      windowEnd: '2026-03-02T12:00:00Z',
    });
  });

  it('aligns starts on a window boundary to that window', () => {
    assert.deepEqual(arrivalWindow(ctx(), '2026-03-02T12:00:00Z'), {
      windowStart: '2026-03-02T12:00:00Z',
      windowEnd: '2026-03-02T14:00:00Z',
    });
  });

  it('clips the last window to the end of the working day', () => {
    assert.deepEqual(arrivalWindow(ctx(), '2026-03-02T16:30:00Z'), {
      windowStart: '2026-03-02T16:00:00Z',
      windowEnd: '2026-03-02T17:00:00Z',
    });
  });

  it('follows the configured window size', () => {
    const wide = createApp({ now: '2026-03-02T08:00:00Z', config: { scheduling: { windowMinutes: 240 } } }).ctx;
    assert.deepEqual(arrivalWindow(wide, '2026-03-02T13:00:00Z'), {
      windowStart: '2026-03-02T12:00:00Z',
      windowEnd: '2026-03-02T16:00:00Z',
    });
    const narrow = createApp({ now: '2026-03-02T08:00:00Z', config: { scheduling: { windowMinutes: 60 } } }).ctx;
    assert.deepEqual(arrivalWindow(narrow, '2026-03-02T13:45:00Z'), {
      windowStart: '2026-03-02T13:00:00Z',
      windowEnd: '2026-03-02T14:00:00Z',
    });
  });

  it('stays on the date of the start', () => {
    const window = arrivalWindow(ctx(), '2026-03-04T10:00:00Z');
    assert.equal(window.windowStart.slice(0, 10), '2026-03-04');
    assert.equal(window.windowEnd.slice(0, 10), '2026-03-04');
  });
});

describe('describeWindow', () => {
  it('formats both ends as clock times', () => {
    assert.equal(
      describeWindow({ windowStart: '2026-03-02T10:00:00Z', windowEnd: '2026-03-02T12:00:00Z' }),
      'between 10:00 and 12:00',
    );
    assert.equal(
      describeWindow({ windowStart: '2026-03-02T13:00:00Z', windowEnd: '2026-03-02T13:45:00Z' }),
      'between 13:00 and 13:45',
    );
  });
});

describe('windowStarts', () => {
  it('lists the windows that fit inside the day', () => {
    const ctx = createApp({ now: '2026-03-02T08:00:00Z' }).ctx;
    assert.deepEqual(windowStarts(ctx), [480, 600, 720, 840]);
  });

  it('lists one-hour windows when configured', () => {
    const ctx = createApp({ now: '2026-03-02T08:00:00Z', config: { scheduling: { windowMinutes: 60 } } }).ctx;
    assert.deepEqual(windowStarts(ctx), [420, 480, 540, 600, 660, 720, 780, 840, 900, 960].filter((minute) => minute >= ctx.config.scheduling.dayStartMinute));
  });

  it('lists three four-hour windows only when they fit', () => {
    const ctx = createApp({ now: '2026-03-02T08:00:00Z', config: { scheduling: { windowMinutes: 240 } } }).ctx;
    assert.deepEqual(windowStarts(ctx), [480, 720]);
  });
});
'''

SLOT_SETUP = r'''
function fresh(options: Parameters<typeof createWorld>[0] = {}) {
  const world = createWorld(options);
  const ctx = world.app.ctx;
  const technician = technicians(ctx).require(world.technician.id, 'technician');
  const destination = world.site.address.location;
  return { ...world, ctx, technician, destination };
}

const labels = (slots: Array<{ start: string }>) => slots.map((slot) => slot.start.slice(11, 16));
'''

SLOTS_BODY = SLOT_SETUP + r'''
describe('workingBounds', () => {
  it('uses the working day on a weekday', () => {
    const { ctx, technician } = fresh();
    assert.deepEqual(workingBounds(ctx, technician, '2026-03-02'), [480, 1020]);
  });

  it('is undefined on weekends', () => {
    const { ctx, technician } = fresh();
    assert.equal(workingBounds(ctx, technician, '2026-03-07'), undefined);
    assert.equal(workingBounds(ctx, technician, '2026-03-08'), undefined);
  });

  it('is undefined on public holidays', () => {
    const { ctx, technician } = fresh();
    assert.equal(workingBounds(ctx, technician, '2026-04-03'), undefined);
    assert.equal(workingBounds(ctx, technician, '2026-04-06'), undefined);
  });

  it('is undefined while the technician is absent', () => {
    const { ctx, technician } = fresh();
    const away = recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-03', to: '2026-03-03', kind: 'sick' });
    assert.equal(workingBounds(ctx, away, '2026-03-03'), undefined);
    assert.deepEqual(workingBounds(ctx, away, '2026-03-04'), [480, 1020]);
  });
});

describe('availableSlots', () => {
  it('offers every half hour of an empty day that fits the visit', () => {
    const { ctx, technician, destination } = fresh();
    const slots = availableSlots(ctx, technician, '2026-03-02', 60, destination, new Map());
    assert.equal(slots.length, 17);
    assert.equal(slots[0].start, '2026-03-02T08:00:00Z');
    assert.equal(slots.at(-1)?.start, '2026-03-02T16:00:00Z');
    assert.equal(slots.at(-1)?.end, '2026-03-02T17:00:00Z');
  });

  it('shortens the list for longer visits', () => {
    const { ctx, technician, destination } = fresh();
    const slots = availableSlots(ctx, technician, '2026-03-02', 240, destination, new Map());
    assert.equal(slots.at(-1)?.start, '2026-03-02T13:00:00Z');
    assert.equal(slots.at(-1)?.end, '2026-03-02T17:00:00Z');
  });

  it('offers nothing when the visit is longer than the day', () => {
    const { ctx, technician, destination } = fresh();
    assert.deepEqual(availableSlots(ctx, technician, '2026-03-02', 600, destination, new Map()), []);
  });

  it('offers nothing on non-working days', () => {
    const { ctx, technician, destination } = fresh();
    assert.deepEqual(availableSlots(ctx, technician, '2026-03-07', 60, destination, new Map()), []);
  });

  it('reports the drive from home for the first visit of the day', () => {
    const { ctx, technician, destination } = fresh();
    const [first] = availableSlots(ctx, technician, '2026-03-02', 60, destination, new Map());
    assert.equal(first.travelMinutes, 10);
  });

  it('keeps the distance from home out of the start time of the first visit', () => {
    const { ctx, technician } = fresh();
    const far = { lat: 52.9, lon: 13.38 };
    const [first] = availableSlots(ctx, technician, '2026-03-02', 60, far, new Map());
    assert.equal(first.start, '2026-03-02T08:00:00Z');
    assert.ok(first.travelMinutes >= 60);
  });

  it('keeps clear of booked visits and the buffer around them', () => {
    const { app, ctx, technician, customer, site, destination } = fresh();
    const job = openWorkOrder(app, customer.id, site.id, 'general');
    call(app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId: technician.id, start: '2026-03-02T11:00:00Z' });
    const other = openWorkOrder(app, customer.id, site.id, 'general');
    const locations = appointmentLocations(ctx, technician.id, '2026-03-02');
    locations.delete(other.id);
    const slots = availableSlots(ctx, technician, '2026-03-02', 60, destination, locations);
    assert.deepEqual(labels(slots), [
      '08:00', '08:30', '09:00', '09:30', '12:30', '13:00', '13:30', '14:00', '14:30', '15:00', '15:30', '16:00',
    ]);
  });

  it('adds driving time between stops on top of the buffer', () => {
    const { app, ctx, technician, customer } = fresh();
    const distant = seedCustomer(app, { name: 'Distant Works', location: { lat: 52.62, lon: 13.4 } });
    const job = openWorkOrder(app, distant.customer.id, distant.site.id, 'general');
    call(app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId: technician.id, start: '2026-03-02T09:00:00Z' });
    const slots = availableSlots(
      ctx,
      technician,
      '2026-03-02',
      60,
      { lat: 52.52, lon: 13.4 },
      appointmentLocations(ctx, technician.id, '2026-03-02'),
    );
    assert.ok(!labels(slots).includes('10:30'));
    assert.ok(labels(slots).includes('11:30'));
    assert.ok(slots.every((slot) => slot.start >= '2026-03-02T11:00:00Z' || slot.start <= '2026-03-02T07:30:00Z'));
    assert.ok(customer.id.length > 0);
  });

  it('applies the global cap on daily appointments', () => {
    const { app, ctx, technician, destination, customer, site } = fresh({ config: { scheduling: { maxDailyAppointments: 2 } } });
    for (const start of ['09:00', '11:00']) {
      const job = openWorkOrder(app, customer.id, site.id, 'general');
      call(app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId: technician.id, start: `2026-03-02T${start}:00Z` });
    }
    assert.deepEqual(availableSlots(ctx, technician, '2026-03-02', 60, destination, appointmentLocations(ctx, technician.id, '2026-03-02')), []);
  });

  it('prefers the per-technician cap over the global one', () => {
    const { app, ctx, destination, customer, site } = fresh();
    const limited = seedTechnician(app, { skills: ['plumbing'] });
    technicians(ctx).update(limited.id, { maxDailyAppointments: 1 });
    const job = openWorkOrder(app, customer.id, site.id, 'general');
    call(app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId: limited.id, start: '2026-03-02T09:00:00Z' });
    const refreshed = technicians(ctx).require(limited.id, 'technician');
    assert.deepEqual(availableSlots(ctx, refreshed, '2026-03-02', 60, destination, appointmentLocations(ctx, limited.id, '2026-03-02')), []);
  });

  it('does not count missed visits as busy time', () => {
    const { app, ctx, technician, destination, customer, site } = fresh();
    const job = openWorkOrder(app, customer.id, site.id, 'general');
    const booked = call(app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId: technician.id, start: '2026-03-02T11:00:00Z' });
    call(app, 'POST', `/appointments/${booked.id}/no-access`, technician.id, { reason: 'Nobody on site' });
    const slots = availableSlots(ctx, technician, '2026-03-02', 60, destination, new Map());
    assert.equal(slots.length, 17);
  });

  it('honours a configured slot size', () => {
    const { ctx, technician, destination } = fresh({ config: { scheduling: { slotMinutes: 60 } } });
    const slots = availableSlots(ctx, technician, '2026-03-02', 60, destination, new Map());
    assert.deepEqual(labels(slots), ['08:00', '09:00', '10:00', '11:00', '12:00', '13:00', '14:00', '15:00', '16:00']);
  });
});

describe('firstAvailableSlot', () => {
  it('returns the first slot of the first working day', () => {
    const { ctx, technician, destination } = fresh();
    const slot = firstAvailableSlot(ctx, technician, '2026-03-02', 60, destination, new Map());
    assert.equal(slot?.start, '2026-03-02T08:00:00Z');
  });

  it('skips weekends', () => {
    const { ctx, technician, destination } = fresh();
    const slot = firstAvailableSlot(ctx, technician, '2026-03-07', 60, destination, new Map());
    assert.equal(slot?.start, '2026-03-09T08:00:00Z');
  });

  it('skips absences', () => {
    const { ctx, technician, destination } = fresh();
    const away = recordAbsence(ctx, 'admin', technician.id, { from: '2026-03-09', to: '2026-03-10', kind: 'leave' });
    const slot = firstAvailableSlot(ctx, away, '2026-03-07', 60, destination, new Map());
    assert.equal(slot?.start, '2026-03-11T08:00:00Z');
  });

  it('gives up beyond the planning horizon', () => {
    const { ctx, technician, destination } = fresh({ config: { scheduling: { horizonDays: 2 } } });
    assert.equal(firstAvailableSlot(ctx, technician, '2026-03-07', 60, destination, new Map()), undefined);
    assert.equal(firstAvailableSlot(ctx, technician, '2026-03-06', 60, destination, new Map())?.start, '2026-03-06T08:00:00Z');
  });
});
'''


SCHEDULING_SERVICE = r'''
const book = (scene: Scene, jobId: string, start: string, technicianId = scene.tech, durationMinutes?: number) =>
  bookAppointment(scene.ctx, 'dispatch', { jobId, technicianId, start, durationMinutes });

describe('bookAppointment', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('books the slot and assigns the technician', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    assert.equal(appointment.id, 'APT-000001');
    assert.equal(appointment.status, 'booked');
    assert.equal(appointment.start, at(MONDAY, '09:00'));
    assert.equal(appointment.end, at(MONDAY, '10:00'));
    assert.equal(appointment.technicianId, scene.tech);
    assert.equal(appointment.createdAt, at(MONDAY, '08:00'));
    const updated = «requireJob»(scene.ctx, job.id);
    assert.equal(updated.status, 'assigned');
    assert.equal(updated.technicianId, scene.tech);
    assert.equal(updated.scheduledStart, at(MONDAY, '09:00'));
    assert.equal(updated.scheduledEnd, at(MONDAY, '10:00'));
  });

  it('uses the default duration of the work type', () => {
    const asset = seedAsset(scene.app, scene.site.id, 'boiler');
    const job = open(scene, 'boiler-service', { assetId: asset.id });
    const technician = seedTechnician(scene.app, { skills: ['gas-safe'] });
    const appointment = book(scene, job.id, at(MONDAY, '09:00'), technician.id);
    assert.equal(appointment.end, at(MONDAY, '10:15'));
  });

  it('prefers an explicit duration', () => {
    const job = open(scene);
    assert.equal(book(scene, job.id, at(MONDAY, '09:00'), scene.tech, 150).end, at(MONDAY, '11:30'));
  });

  it('attaches the arrival window and the drive from home', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '10:30'));
    assert.equal(appointment.windowStart, at(MONDAY, '10:00'));
    assert.equal(appointment.windowEnd, at(MONDAY, '12:00'));
    assert.equal(appointment.travelMinutes, 10);
  });

  it('announces the booking on the event bus', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    const [event] = eventsOf(scene, 'appointment.booked');
    assert.equal(event.appointmentId, appointment.id);
    assert.equal(event.jobId, job.id);
    assert.equal(event.technicianId, scene.tech);
    assert.equal(event.windowStart, at(MONDAY, '08:00'));
    assert.equal(event.windowEnd, at(MONDAY, '10:00'));
  });

  it('rejects a second appointment for the same work order', () => {
    const job = open(scene);
    book(scene, job.id, at(MONDAY, '09:00'));
    assert.throws(
      () => book(scene, job.id, at(MONDAY, '13:00')),
      (error: any) => error.code === 'already-booked' && error.status === 409,
    );
  });

  it('rejects unavailable slots', () => {
    const first = open(scene);
    const second = open(scene);
    book(scene, first.id, at(MONDAY, '09:00'));
    for (const clock of ['09:00', '09:30', '10:00']) {
      assert.throws(
        () => book(scene, second.id, at(MONDAY, clock)),
        (error: any) => error.code === 'slot-unavailable' && error.details.start === at(MONDAY, clock),
        clock,
      );
    }
    assert.equal(book(scene, second.id, at(MONDAY, '10:30')).status, 'booked');
  });

  it('rejects slots that are not on the slot grid', () => {
    const job = open(scene);
    assert.throws(() => book(scene, job.id, at(MONDAY, '09:10')), (error: any) => error.code === 'slot-unavailable');
  });

  it('rejects weekends', () => {
    const job = open(scene);
    assert.throws(() => book(scene, job.id, '2026-03-07T10:00:00Z'), (error: any) => error.code === 'slot-unavailable');
  });

  it('rejects work orders that are already being worked on', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    assert.throws(() => book(scene, job.id, at(MONDAY, '09:00')), (error: any) => error.code === 'not-schedulable');
  });

  it('rejects cancelled work orders', () => {
    const job = open(scene);
    «cancelJob»(scene.ctx, 'dispatch', job.id, 'Customer withdrew the request');
    assert.throws(() => book(scene, job.id, at(MONDAY, '09:00')), (error: any) => error.code === 'not-schedulable');
  });

  it('rejects unknown work orders and technicians', () => {
    const job = open(scene);
    assert.throws(() => book(scene, 'WO-999999', at(MONDAY, '09:00')), (error: any) => error.status === 404);
    assert.throws(() => book(scene, job.id, at(MONDAY, '09:00'), 'TEC-999999'), (error: any) => error.status === 404);
  });

  it('refuses technicians without the required skills', () => {
    const asset = seedAsset(scene.app, scene.site.id, 'boiler');
    const job = open(scene, 'boiler-service', { assetId: asset.id });
    const unqualified = seedTechnician(scene.app, { skills: ['plumbing'] });
    assert.throws(
      () => book(scene, job.id, at(MONDAY, '09:00'), unqualified.id),
      (error: any) => error.code === 'technician-not-qualified',
    );
  });

  it('keeps an assigned technician when booking the same person', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    book(scene, job.id, at(MONDAY, '09:00'));
    assert.equal(«requireJob»(scene.ctx, job.id).technicianId, scene.tech);
    assert.equal(«requireJob»(scene.ctx, job.id).status, 'assigned');
  });

  it('reassigns when another technician is booked', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    const other = seedTechnician(scene.app, { skills: ['plumbing'] });
    book(scene, job.id, at(MONDAY, '09:00'), other.id);
    assert.equal(«requireJob»(scene.ctx, job.id).technicianId, other.id);
  });
});

describe('cancelAppointment', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('cancels a booked appointment and keeps the reason', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    const cancelled = cancelAppointment(scene.ctx, 'dispatch', appointment.id, 'Customer asked to move');
    assert.equal(cancelled.status, 'cancelled');
    assert.equal(cancelled.cancelReason, 'Customer asked to move');
    const [event] = eventsOf(scene, 'appointment.cancelled');
    assert.equal(event.reason, 'Customer asked to move');
    assert.equal(event.actorId, 'dispatch');
  });

  it('frees the slot again', () => {
    const first = open(scene);
    const second = open(scene);
    const appointment = book(scene, first.id, at(MONDAY, '09:00'));
    cancelAppointment(scene.ctx, 'dispatch', appointment.id, 'Customer asked to move');
    assert.equal(book(scene, second.id, at(MONDAY, '09:00')).status, 'booked');
  });

  it('allows the work order to be booked again', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    cancelAppointment(scene.ctx, 'dispatch', appointment.id, 'Customer asked to move');
    assert.equal(book(scene, job.id, at(MONDAY, '13:00')).id, 'APT-000002');
  });

  it('refuses to cancel twice', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    cancelAppointment(scene.ctx, 'dispatch', appointment.id, 'Customer asked to move');
    assert.throws(
      () => cancelAppointment(scene.ctx, 'dispatch', appointment.id, 'Again'),
      (error: any) => error.code === 'appointment-not-live' && error.status === 409,
    );
  });

  it('cancels confirmed appointments too', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    confirmAppointment(scene.ctx, appointment.id);
    assert.equal(cancelAppointment(scene.ctx, 'dispatch', appointment.id, 'Customer asked to move').status, 'cancelled');
  });
});

describe('rescheduleAppointment', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('moves the visit and keeps its duration', () => {
    const job = open(scene);
    const original = book(scene, job.id, at(MONDAY, '09:00'), scene.tech, 90);
    const moved = rescheduleAppointment(scene.ctx, 'dispatch', original.id, at(MONDAY, '13:00'));
    assert.equal(moved.start, at(MONDAY, '13:00'));
    assert.equal(moved.end, at(MONDAY, '14:30'));
    assert.equal(moved.technicianId, scene.tech);
    assert.equal(appointments(scene.ctx).require(original.id, 'appointment').status, 'cancelled');
    assert.equal(appointments(scene.ctx).require(original.id, 'appointment').cancelReason, 'rescheduled');
  });

  it('can move to a slot that overlaps the original', () => {
    const job = open(scene);
    const original = book(scene, job.id, at(MONDAY, '09:00'));
    const moved = rescheduleAppointment(scene.ctx, 'dispatch', original.id, at(MONDAY, '09:30'));
    assert.equal(moved.start, at(MONDAY, '09:30'));
  });

  it('announces the reschedule with the previous appointment', () => {
    const job = open(scene);
    const original = book(scene, job.id, at(MONDAY, '09:00'));
    const moved = rescheduleAppointment(scene.ctx, 'dispatch', original.id, at(MONDAY, '13:00'));
    const [event] = eventsOf(scene, 'appointment.rescheduled');
    assert.equal(event.previousId, original.id);
    assert.equal(event.appointmentId, moved.id);
  });

  it('restores the original when the new slot is unavailable', () => {
    const first = open(scene);
    const second = open(scene);
    const original = book(scene, first.id, at(MONDAY, '09:00'));
    book(scene, second.id, at(MONDAY, '13:00'));
    assert.throws(
      () => rescheduleAppointment(scene.ctx, 'dispatch', original.id, at(MONDAY, '13:00')),
      (error: any) => error.code === 'slot-unavailable',
    );
    const restored = appointments(scene.ctx).require(original.id, 'appointment');
    assert.equal(restored.status, 'booked');
    assert.equal(restored.cancelReason, undefined);
    assert.equal(«requireJob»(scene.ctx, first.id).scheduledStart, at(MONDAY, '09:00'));
  });

  it('can move onto a later day', () => {
    const job = open(scene);
    const original = book(scene, job.id, at(MONDAY, '09:00'));
    const moved = rescheduleAppointment(scene.ctx, 'dispatch', original.id, '2026-03-04T10:00:00Z');
    assert.equal(moved.windowStart, '2026-03-04T10:00:00Z');
    assert.equal(«requireJob»(scene.ctx, job.id).scheduledStart, '2026-03-04T10:00:00Z');
  });
});

describe('confirmAppointment and recordNoAccess', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('confirms a booked appointment once', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    assert.equal(confirmAppointment(scene.ctx, appointment.id).status, 'confirmed');
    assert.throws(() => confirmAppointment(scene.ctx, appointment.id), (error: any) => error.code === 'appointment-not-booked');
  });

  it('marks a visit as missed and announces it', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    const missed = recordNoAccess(scene.ctx, scene.tech, appointment.id, 'Gate was locked');
    assert.equal(missed.status, 'missed');
    assert.equal(missed.cancelReason, 'Gate was locked');
    const [event] = eventsOf(scene, 'appointment.no-access');
    assert.equal(event.note, 'Gate was locked');
    assert.equal(event.jobId, job.id);
  });

  it('cannot record no-access twice or after a cancellation', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    recordNoAccess(scene.ctx, scene.tech, appointment.id, 'Gate was locked');
    assert.throws(() => recordNoAccess(scene.ctx, scene.tech, appointment.id, 'Again'), (error: any) => error.code === 'appointment-not-live');
    const other = open(scene);
    const live = book(scene, other.id, at(MONDAY, '13:00'));
    cancelAppointment(scene.ctx, 'dispatch', live.id, 'Customer asked to move');
    assert.throws(() => recordNoAccess(scene.ctx, scene.tech, live.id, 'Late'), (error: any) => error.code === 'appointment-not-live');
  });

  it('cannot confirm a missed appointment', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    recordNoAccess(scene.ctx, scene.tech, appointment.id, 'Gate was locked');
    assert.throws(() => confirmAppointment(scene.ctx, appointment.id), (error: any) => error.code === 'appointment-not-booked');
  });

  it('announces a rebooking after a missed visit', () => {
    const job = open(scene);
    const appointment = book(scene, job.id, at(MONDAY, '09:00'));
    recordNoAccess(scene.ctx, scene.tech, appointment.id, 'Gate was locked');
    const rebooked = book(scene, job.id, at(MONDAY, '13:00'));
    assert.equal(rebooked.status, 'booked');
    assert.equal(eventsOf(scene, 'appointment.rebooked').length, 1);
    assert.equal(eventsOf(scene, 'appointment.booked').length, 1);
  });
});
'''

CAPACITY_BODY = SLOT_SETUP + r'''
const book = (world: ReturnType<typeof fresh>, start: string, workType = 'general', technicianId = world.technician.id, durationMinutes = 60) => {
  const job = openWorkOrder(world.app, world.customer.id, world.site.id, workType);
  return call(world.app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId, start, durationMinutes });
};

describe('dailyLoad', () => {
  it('is empty for an idle technician', () => {
    const world = fresh();
    assert.deepEqual(dailyLoad(world.ctx, world.technician, '2026-03-02'), {
      technicianId: world.technician.id,
      date: '2026-03-02',
      appointments: 0,
      bookedMinutes: 0,
      travelMinutes: 0,
      availableMinutes: 540,
      utilisationPercent: 0,
    });
  });

  it('adds booked and travel minutes', () => {
    const world = fresh();
    book(world, '2026-03-02T09:00:00Z');
    const load = dailyLoad(world.ctx, world.technician, '2026-03-02');
    assert.equal(load.appointments, 1);
    assert.equal(load.bookedMinutes, 60);
    assert.equal(load.travelMinutes, 10);
    assert.equal(load.utilisationPercent, 13);
  });

  it('accumulates over several appointments', () => {
    const world = fresh();
    book(world, '2026-03-02T09:00:00Z');
    book(world, '2026-03-02T11:00:00Z');
    book(world, '2026-03-02T13:00:00Z');
    const load = dailyLoad(world.ctx, world.technician, '2026-03-02');
    assert.equal(load.appointments, 3);
    assert.equal(load.bookedMinutes, 180);
    assert.equal(load.travelMinutes, 10);
    assert.equal(load.utilisationPercent, 35);
  });

  it('counts longer visits by their length', () => {
    const world = fresh();
    book(world, '2026-03-02T08:00:00Z', 'general', world.technician.id, 240);
    const load = dailyLoad(world.ctx, world.technician, '2026-03-02');
    assert.equal(load.bookedMinutes, 240);
  });

  it('ignores missed visits', () => {
    const world = fresh();
    const appointment = book(world, '2026-03-02T09:00:00Z');
    call(world.app, 'POST', `/appointments/${appointment.id}/no-access`, world.technician.id, { reason: 'Nobody on site' });
    assert.equal(dailyLoad(world.ctx, world.technician, '2026-03-02').appointments, 0);
  });

  it('ignores cancelled visits', () => {
    const world = fresh();
    const appointment = book(world, '2026-03-02T09:00:00Z');
    call(world.app, 'POST', `/appointments/${appointment.id}/cancel`, 'dispatch', { reason: 'Customer asked to move' });
    assert.equal(dailyLoad(world.ctx, world.technician, '2026-03-02').bookedMinutes, 0);
  });

  it('only counts the requested day', () => {
    const world = fresh();
    book(world, '2026-03-03T09:00:00Z');
    assert.equal(dailyLoad(world.ctx, world.technician, '2026-03-02').appointments, 0);
    assert.equal(dailyLoad(world.ctx, world.technician, '2026-03-03').appointments, 1);
  });

  it('has no capacity on a weekend', () => {
    const world = fresh();
    const load = dailyLoad(world.ctx, world.technician, '2026-03-07');
    assert.equal(load.availableMinutes, 0);
    assert.equal(load.utilisationPercent, 0);
  });
});

describe('leastLoaded', () => {
  it('orders technicians from least to most loaded', () => {
    const world = fresh();
    const second = seedTechnician(world.app, { skills: ['plumbing'] });
    const third = seedTechnician(world.app, { skills: ['plumbing'] });
    book(world, '2026-03-02T09:00:00Z', 'general', world.technician.id);
    book(world, '2026-03-02T11:00:00Z', 'general', world.technician.id);
    book(world, '2026-03-02T09:00:00Z', 'general', third.id);
    const ranking = leastLoaded(world.ctx, 'metro-east', '2026-03-02');
    assert.deepEqual(ranking.map((load) => load.technicianId), [second.id, third.id, world.technician.id]);
    assert.deepEqual(ranking.map((load) => load.appointments), [0, 1, 2]);
  });

  it('breaks ties by technician id', () => {
    const world = fresh();
    const second = seedTechnician(world.app, { skills: ['plumbing'] });
    assert.deepEqual(
      leastLoaded(world.ctx, 'metro-east', '2026-03-02').map((load) => load.technicianId),
      [world.technician.id, second.id],
    );
  });

  it('omits technicians with no working time on the date', () => {
    const world = fresh();
    assert.deepEqual(leastLoaded(world.ctx, 'metro-east', '2026-03-07'), []);
  });

  it('omits technicians from other regions', () => {
    const world = fresh();
    seedTechnician(world.app, { region: 'metro-west', skills: ['plumbing'] });
    assert.equal(leastLoaded(world.ctx, 'metro-east', '2026-03-02').length, 1);
    assert.equal(leastLoaded(world.ctx, 'metro-west', '2026-03-02').length, 2 - 1);
  });
});
'''

ROUTE_BODY = SLOT_SETUP + r'''
const book = (world: ReturnType<typeof fresh>, start: string, customer = world.customer, site = world.site) => {
  const job = openWorkOrder(world.app, customer.id, site.id, 'general');
  return call(world.app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId: world.technician.id, start });
};

describe('dailyRoute', () => {
  it('is empty without visits', () => {
    const world = fresh();
    assert.deepEqual(dailyRoute(world.ctx, world.technician, '2026-03-02'), {
      technicianId: world.technician.id,
      date: '2026-03-02',
      stops: [],
      totalDriveMinutes: 0,
      totalKm: 0,
    });
  });

  it('drives from home to the site and back', () => {
    const world = fresh();
    const appointment = book(world, '2026-03-02T09:00:00Z');
    const route = dailyRoute(world.ctx, world.technician, '2026-03-02');
    assert.equal(route.stops.length, 1);
    assert.deepEqual(route.stops[0], {
      appointmentId: appointment.id,
      jobId: appointment.jobId,
      arrival: '2026-03-02T09:00:00Z',
      departure: '2026-03-02T10:00:00Z',
      driveMinutes: 10,
      driveKm: 3.4,
    });
    assert.equal(route.totalDriveMinutes, 20);
    assert.equal(route.totalKm, 6.8);
  });

  it('visits stops in time order regardless of booking order', () => {
    const world = fresh();
    const late = book(world, '2026-03-02T14:00:00Z');
    const early = book(world, '2026-03-02T09:00:00Z');
    const route = dailyRoute(world.ctx, world.technician, '2026-03-02');
    assert.deepEqual(route.stops.map((stop) => stop.appointmentId), [early.id, late.id]);
  });

  it('measures later legs from the previous stop', () => {
    const world = fresh();
    book(world, '2026-03-02T09:00:00Z');
    book(world, '2026-03-02T11:00:00Z');
    const route = dailyRoute(world.ctx, world.technician, '2026-03-02');
    assert.equal(route.stops[1].driveMinutes, 0);
    assert.equal(route.stops[1].driveKm, 0);
    assert.equal(route.totalDriveMinutes, 20);
  });

  it('adds legs between different sites', () => {
    const world = fresh();
    const other = seedCustomer(world.app, { name: 'Distant Works', location: { lat: 52.62, lon: 13.4 } });
    book(world, '2026-03-02T09:00:00Z');
    book(world, '2026-03-02T13:00:00Z', other.customer, other.site);
    const route = dailyRoute(world.ctx, world.technician, '2026-03-02');
    assert.ok(route.stops[1].driveKm > 10);
    assert.ok(route.totalKm > route.stops[0].driveKm + route.stops[1].driveKm);
    assert.ok(route.totalDriveMinutes > 40);
  });

  it('skips cancelled and missed visits', () => {
    const world = fresh();
    const cancelled = book(world, '2026-03-02T09:00:00Z');
    const missed = book(world, '2026-03-02T11:00:00Z');
    const kept = book(world, '2026-03-02T13:00:00Z');
    call(world.app, 'POST', `/appointments/${cancelled.id}/cancel`, 'dispatch', { reason: 'Customer asked to move' });
    call(world.app, 'POST', `/appointments/${missed.id}/no-access`, world.technician.id, { reason: 'Nobody on site' });
    const route = dailyRoute(world.ctx, world.technician, '2026-03-02');
    assert.deepEqual(route.stops.map((stop) => stop.appointmentId), [kept.id]);
  });

  it('only includes the requested day', () => {
    const world = fresh();
    book(world, '2026-03-03T09:00:00Z');
    assert.equal(dailyRoute(world.ctx, world.technician, '2026-03-02').stops.length, 0);
    assert.equal(dailyRoute(world.ctx, world.technician, '2026-03-03').stops.length, 1);
  });
});
'''

DISPATCH_RULES_BODY = r'''
const technician = {
  id: 'TEC-000001',
  name: 'Robin Hale',
  email: 'robin@fieldline.example',
  region: 'metro-east',
  roaming: false,
  certifications: [{ skill: 'gas-safe' }, { skill: 'plumbing' }],
  home: { lat: 0, lon: 0 },
  vanLocationId: 'VAN-000001',
  active: true,
  hourlyCostCents: 3000,
  absences: [],
};

const candidate = (overrides: Record<string, unknown> = {}): any => ({
  technician,
  job: {},
  distanceKm: 10,
  loadPercent: 50,
  previousVisits: 0,
  missingSkills: [],
  ...overrides,
});

const config = resolveConfig();
const withDispatch = (dispatch: Record<string, unknown>) => resolveConfig({ dispatch } as any);

describe('skillMatch', () => {
  it('is registered under its key and weight', () => {
    assert.equal(skillMatch.key, 'skill-match');
    assert.equal(skillMatch.weight, 'skill');
  });

  it('scores one for a fully qualified technician', () => {
    assert.equal(skillMatch.score(candidate(), config), 1);
  });

  it('scores half of the share of skills held for partially qualified technicians', () => {
    assert.equal(skillMatch.score(candidate({ missingSkills: ['lifts'] }), config), 0.5 * (1 - 1 / 3));
  });

  it('stays below one half when skills are missing', () => {
    const score = skillMatch.score(candidate({ missingSkills: ['a', 'b', 'c', 'd', 'e'] }), config);
    assert.ok(score > 0 && score < 0.5);
  });
});

describe('proximity', () => {
  it('is registered under its key and weight', () => {
    assert.equal(proximity.key, 'distance');
    assert.equal(proximity.weight, 'distance');
  });

  it('scores one at zero distance', () => {
    assert.equal(proximity.score(candidate({ distanceKm: 0 }), config), 1);
  });

  it('falls linearly towards the dispatch radius, in steps of five percent', () => {
    assert.equal(proximity.score(candidate({ distanceKm: 40 }), config), 0.5);
    assert.equal(proximity.score(candidate({ distanceKm: 20 }), config), 0.75);
    assert.equal(proximity.score(candidate({ distanceKm: 60 }), config), 0.25);
  });

  it('is zero at and beyond the radius', () => {
    assert.equal(proximity.score(candidate({ distanceKm: 80 }), config), 0);
    assert.equal(proximity.score(candidate({ distanceKm: 120 }), config), 0);
  });

  it('follows the configured radius', () => {
    assert.equal(proximity.score(candidate({ distanceKm: 10 }), withDispatch({ maxDistanceKm: 20 })), 0.5);
    assert.equal(proximity.score(candidate({ distanceKm: 25 }), withDispatch({ maxDistanceKm: 20 })), 0);
  });
});

describe('workload', () => {
  it('is registered under its key and weight', () => {
    assert.equal(workload.key, 'load');
    assert.equal(workload.weight, 'load');
  });

  it('favours idle technicians', () => {
    assert.equal(workload.score(candidate({ loadPercent: 0 }), config), 1);
    assert.equal(workload.score(candidate({ loadPercent: 25 }), config), 0.75);
    assert.equal(workload.score(candidate({ loadPercent: 80 }), config), 0.19999999999999996);
  });

  it('is zero for a technician at or above full capacity', () => {
    assert.equal(workload.score(candidate({ loadPercent: 100 }), config), 0);
    assert.equal(workload.score(candidate({ loadPercent: 130 }), config), 0);
  });
});

describe('continuity', () => {
  it('is registered under its key and weight', () => {
    assert.equal(continuity.key, 'continuity');
    assert.equal(continuity.weight, 'continuity');
  });

  it('grows with previous visits up to three', () => {
    assert.equal(continuity.score(candidate({ previousVisits: 0 }), config), 0);
    assert.equal(continuity.score(candidate({ previousVisits: 1 }), config), 1 / 3);
    assert.equal(continuity.score(candidate({ previousVisits: 3 }), config), 1);
    assert.equal(continuity.score(candidate({ previousVisits: 9 }), config), 1);
  });

  it('is neutral when continuity is not preferred', () => {
    const neutral = withDispatch({ preferContinuity: false });
    assert.equal(continuity.score(candidate({ previousVisits: 0 }), neutral), 0.5);
    assert.equal(continuity.score(candidate({ previousVisits: 5 }), neutral), 0.5);
  });
});

describe('scoring registry', () => {
  it('registers the four built-in rules', () => {
    const registry = createScoringRegistry();
    assert.deepEqual(registry.keys(), ['continuity', 'distance', 'load', 'skill-match']);
  });

  it('keeps one weight key per rule', () => {
    const weights = createScoringRegistry().entries().map(([, rule]) => rule.weight).sort();
    assert.deepEqual(weights, ['continuity', 'distance', 'load', 'skill']);
  });

  it('describes every rule', () => {
    for (const [, rule] of createScoringRegistry().entries()) assert.ok(rule.describe.length > 10, rule.key);
  });

  it('keeps every score between zero and one', () => {
    const registry = createScoringRegistry();
    for (const [, rule] of registry.entries()) {
      for (const distanceKm of [0, 1, 40, 79, 500]) {
        for (const loadPercent of [0, 50, 99, 200]) {
          for (const previousVisits of [0, 2, 10]) {
            const score = rule.score(candidate({ distanceKm, loadPercent, previousVisits }), config);
            assert.ok(score >= 0 && score <= 1, `${rule.key} ${distanceKm} ${loadPercent} ${previousVisits}`);
          }
        }
      }
    }
  });

  it('weights add up to one hundred by default', () => {
    const weights = Object.values(config.dispatch.weights);
    assert.equal(weights.reduce((sum, weight) => sum + weight, 0), 100);
  });
});
'''

RECOMMEND_BODY = r'''
describe('recommendTechnicians', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  const ranked = (jobId: string, limit?: number) => recommendTechnicians(scene.ctx, jobId, MONDAY, limit);

  it('ranks a single qualified technician with a breakdown', () => {
    const job = open(scene);
    const [first, ...rest] = ranked(job.id);
    assert.deepEqual(rest, []);
    assert.equal(first.technicianId, scene.tech);
    assert.equal(first.score, 88.5);
    assert.deepEqual(first.breakdown, { continuity: 0, distance: 0.95, load: 1, 'skill-match': 1 });
    assert.equal(first.distanceKm, 3.4);
    assert.equal(first.firstSlot?.start, at(MONDAY, '08:00'));
  });

  it('excludes technicians who lack a required skill', () => {
    const asset = seedAsset(scene.app, scene.site.id, 'boiler');
    const job = open(scene, 'boiler-service', { assetId: asset.id });
    seedTechnician(scene.app, { skills: ['plumbing'] });
    assert.deepEqual(ranked(job.id).map((entry) => entry.technicianId), [scene.tech]);
  });

  it('excludes technicians from other regions unless they roam', () => {
    const job = open(scene);
    seedTechnician(scene.app, { region: 'metro-west', skills: ['plumbing'] });
    assert.equal(ranked(job.id).length, 1);
    const roaming = createTechnician(scene.ctx, 'admin', {
      name: 'Roamer',
      email: 'roamer@fieldline.example',
      region: 'metro-west',
      roaming: true,
      home: { lat: 52.5, lon: 13.38 },
      hourlyCostCents: 3000,
    });
    assert.deepEqual(ranked(job.id).map((entry) => entry.technicianId).sort(), [scene.tech, roaming.id].sort());
  });

  it('excludes technicians who are away', () => {
    const job = open(scene);
    const away = seedTechnician(scene.app, { skills: ['plumbing'] });
    recordAbsence(scene.ctx, 'admin', away.id, { from: MONDAY, to: MONDAY, kind: 'sick' });
    assert.deepEqual(ranked(job.id).map((entry) => entry.technicianId), [scene.tech]);
  });

  it('excludes technicians who live beyond the dispatch radius', () => {
    const job = open(scene);
    seedTechnician(scene.app, { skills: ['plumbing'], home: { lat: 54.0, lon: 13.38 } });
    assert.equal(ranked(job.id).length, 1);
  });

  it('prefers the closer technician', () => {
    const job = open(scene);
    const closer = seedTechnician(scene.app, { skills: ['plumbing'], home: { lat: 52.521, lon: 13.401 } });
    const result = ranked(job.id);
    assert.equal(result[0].technicianId, closer.id);
    assert.ok(result[0].score > result[1].score);
    assert.ok(result[0].distanceKm < result[1].distanceKm);
  });

  it('prefers a technician who has already visited the site', () => {
    const earlier = open(scene);
    const veteran = seedTechnician(scene.app, { skills: ['plumbing'] });
    «assignJob»(scene.ctx, 'dispatch', earlier.id, veteran.id);
    const job = open(scene);
    const result = ranked(job.id);
    const best = result.find((entry) => entry.technicianId === veteran.id);
    const other = result.find((entry) => entry.technicianId === scene.tech);
    assert.equal(best?.breakdown.continuity, 1 / 3);
    assert.equal(other?.breakdown.continuity, 0);
    assert.ok((best?.score ?? 0) > (other?.score ?? 0));
  });

  it('penalises busy technicians', () => {
    const job = open(scene);
    const busy = open(scene);
    call(scene.app, 'POST', '/appointments', 'dispatch', { jobId: busy.id, technicianId: scene.tech, start: at(MONDAY, '09:00') });
    const idle = seedTechnician(scene.app, { skills: ['plumbing'] });
    const result = ranked(job.id);
    assert.equal(result.find((entry) => entry.technicianId === idle.id)?.breakdown.load, 1);
    assert.ok((result.find((entry) => entry.technicianId === scene.tech)?.breakdown.load ?? 1) < 1);
  });

  it('breaks equal scores by technician id', () => {
    const job = open(scene);
    const twin = seedTechnician(scene.app, { skills: ['gas-safe', 'plumbing', 'electrical'] });
    assert.deepEqual(ranked(job.id).map((entry) => entry.technicianId), [scene.tech, twin.id]);
  });

  it('truncates to the requested limit', () => {
    const job = open(scene);
    for (let index = 0; index < 4; index += 1) seedTechnician(scene.app, { skills: ['plumbing'] });
    assert.equal(ranked(job.id).length, 5);
    assert.equal(ranked(job.id, 2).length, 2);
    assert.equal(ranked(job.id, 1).length, 1);
  });

  it('reports the first slot after existing appointments', () => {
    const job = open(scene);
    const busy = open(scene);
    call(scene.app, 'POST', '/appointments', 'dispatch', { jobId: busy.id, technicianId: scene.tech, start: at(MONDAY, '08:00') });
    const [first] = ranked(job.id);
    assert.equal(first.firstSlot?.start, at(MONDAY, '09:30'));
  });

  it('looks ahead to the next working day for the first slot', () => {
    const job = open(scene);
    const [first] = recommendTechnicians(scene.ctx, job.id, '2026-03-07');
    assert.equal(first.firstSlot?.start, '2026-03-09T08:00:00Z');
  });

  it('follows configured weights', () => {
    const custom = start({ config: { dispatch: { weights: { skill: 100, distance: 0, load: 0, continuity: 0 } } } });
    const job = open(custom);
    assert.equal(recommendTechnicians(custom.ctx, job.id, MONDAY)[0].score, 100);
  });

  it('rejects unknown work orders', () => {
    assert.throws(() => ranked('WO-999999'), (error: any) => error.status === 404);
  });
});
'''

BOARD_BODY = r'''
describe('dispatchBoard', () => {
  let scene: Scene;

  beforeEach(() => {
    scene = start();
  });

  it('is empty for a quiet day apart from the technician column', () => {
    const board = dispatchBoard(scene.ctx, MONDAY);
    assert.deepEqual(board.unassigned, []);
    assert.deepEqual(board.onHold, []);
    assert.equal(board.columns.length, 1);
    assert.equal(board.columns[0].name, scene.technician.name);
    assert.deepEqual(board.columns[0].jobs, []);
  });

  it('lists new work orders as unassigned with their age', () => {
    const job = open(scene, 'general', { priority: 'high' });
    scene.app.clock.advance(90);
    const board = dispatchBoard(scene.ctx, MONDAY);
    assert.deepEqual(board.unassigned, [{ jobId: job.id, number: job.number, priority: 'high', ageMinutes: 90 }]);
  });

  it('moves booked work orders into the technician column', () => {
    const job = open(scene);
    call(scene.app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId: scene.tech, start: at(MONDAY, '09:00') });
    const board = dispatchBoard(scene.ctx, MONDAY);
    assert.deepEqual(board.unassigned, []);
    assert.deepEqual(board.columns[0].jobs, [job.id]);
    assert.equal(board.columns[0].load.appointments, 1);
  });

  it('keeps assigned work orders without an appointment off the unassigned list', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    assert.deepEqual(dispatchBoard(scene.ctx, MONDAY).unassigned, []);
  });

  it('lists held work orders with the latest hold reason', () => {
    const job = open(scene);
    «holdJob»(scene.ctx, 'dispatch', job.id, 'awaiting_approval');
    const board = dispatchBoard(scene.ctx, MONDAY);
    assert.deepEqual(board.unassigned, []);
    assert.deepEqual(board.onHold.map((entry) => [entry.jobId, entry.reason]), [[job.id, 'awaiting_approval']]);
  });

  it('leaves completed and cancelled work orders off the board', () => {
    const done = open(scene);
    work(scene, done.id);
    const gone = open(scene);
    «cancelJob»(scene.ctx, 'dispatch', gone.id, 'Customer withdrew the request');
    const board = dispatchBoard(scene.ctx, MONDAY);
    assert.deepEqual(board.unassigned, []);
    assert.deepEqual(board.onHold, []);
  });

  it('filters by billing region', () => {
    const west = seedCustomer(scene.app, { name: 'West Works', region: 'metro-west' });
    const eastJob = open(scene);
    const westJob = open(scene, 'general', { customerId: west.customer.id, siteId: west.site.id });
    seedTechnician(scene.app, { region: 'metro-west', skills: ['plumbing'] });
    const all = dispatchBoard(scene.ctx, MONDAY);
    assert.deepEqual(all.unassigned.map((entry) => entry.jobId).sort(), [eastJob.id, westJob.id].sort());
    assert.equal(all.columns.length, 2);
    const east = dispatchBoard(scene.ctx, MONDAY, 'metro-east');
    assert.deepEqual(east.unassigned.map((entry) => entry.jobId), [eastJob.id]);
    assert.equal(east.columns.length, 1);
    const westOnly = dispatchBoard(scene.ctx, MONDAY, 'metro-west');
    assert.deepEqual(westOnly.unassigned.map((entry) => entry.jobId), [westJob.id]);
  });

  it('sorts columns by technician name', () => {
    seedTechnician(scene.app, { name: 'Aaron Pike', skills: ['plumbing'] });
    seedTechnician(scene.app, { name: 'Zoe Marsh', skills: ['plumbing'] });
    assert.deepEqual(dispatchBoard(scene.ctx, MONDAY).columns.map((column) => column.name), ['Aaron Pike', scene.technician.name, 'Zoe Marsh']);
  });

  it('omits deactivated technicians', () => {
    const extra = seedTechnician(scene.app, { skills: ['plumbing'] });
    deactivateTechnician(scene.ctx, 'admin', extra.id, 0);
    assert.equal(dispatchBoard(scene.ctx, MONDAY).columns.length, 1);
  });

  it('shows the requested date in the response', () => {
    assert.equal(dispatchBoard(scene.ctx, '2026-03-04').date, '2026-03-04');
  });
});
'''

API_TECHNICIANS = r'''
let world: World;

beforeEach(() => {
  world = createWorld();
});

const payload = (overrides: Record<string, unknown> = {}) => ({
  name: 'Robin Hale',
  email: 'robin@fieldline.example',
  region: 'metro-east',
  home: { lat: 52.5, lon: 13.38 },
  hourlyCostCents: 3100,
  ...overrides,
});

describe('POST /technicians', () => {
  it('creates a technician and registers a matching actor', () => {
    const body = expectStatus(world.app, 201, 'POST', '/technicians', 'admin', payload());
    assert.equal(body.id, 'TEC-000002');
    assert.equal(body.vanLocationId, 'VAN-000002');
    assert.equal(call(world.app, 'GET', '/technicians', 'admin').items.length, 2);
  });

  it('lets dispatch create technicians', () => {
    expectStatus(world.app, 201, 'POST', '/technicians', 'dispatch', payload());
  });

  it('refuses roles without technician management', () => {
    for (const actor of ['billing', 'warehouse', world.technician.id]) {
      const body = expectStatus(world.app, 403, 'POST', '/technicians', actor, payload());
      assert.equal(body.error.code, 'forbidden', actor);
    }
  });

  it('rejects unknown actors', () => {
    expectStatus(world.app, 401, 'POST', '/technicians', 'nobody', payload());
  });

  it('answers a duplicate email with a conflict', () => {
    expectStatus(world.app, 201, 'POST', '/technicians', 'admin', payload());
    const body = expectStatus(world.app, 409, 'POST', '/technicians', 'admin', payload());
    assert.equal(body.error.code, 'duplicate-email');
  });

  it('answers invalid input with details', () => {
    const body = expectStatus(world.app, 422, 'POST', '/technicians', 'admin', payload({ name: '', hourlyCostCents: -5 }));
    assert.equal(body.error.code, 'invalid-input');
    assert.deepEqual(body.error.details.issues.map((issue: any) => issue.path).sort(), ['hourlyCostCents', 'name']);
  });
});

describe('GET /technicians', () => {
  it('lists active technicians', () => {
    const { items } = call(world.app, 'GET', '/technicians', 'dispatch');
    assert.deepEqual(items.map((item: any) => item.id), [world.technician.id]);
  });

  it('filters by region including roaming technicians', () => {
    call(world.app, 'POST', '/technicians', 'admin', payload({ region: 'metro-west' }));
    call(world.app, 'POST', '/technicians', 'admin', payload({ email: 'roam@fieldline.example', region: 'metro-west', roaming: true }));
    const east = call(world.app, 'GET', '/technicians?region=metro-east', 'dispatch').items;
    assert.equal(east.length, 2);
    const west = call(world.app, 'GET', '/technicians?region=metro-west', 'dispatch').items;
    assert.equal(west.length, 2);
  });

  it('hides deactivated technicians', () => {
    const extra = call(world.app, 'POST', '/technicians', 'admin', payload());
    call(world.app, 'POST', `/technicians/${extra.id}/deactivate`, 'admin');
    assert.equal(call(world.app, 'GET', '/technicians', 'dispatch').items.length, 1);
  });

  it('is limited to roles that schedule work', () => {
    expectStatus(world.app, 200, 'GET', '/technicians', 'admin');
    expectStatus(world.app, 403, 'GET', '/technicians', 'billing');
    expectStatus(world.app, 403, 'GET', '/technicians', 'warehouse');
    expectStatus(world.app, 403, 'GET', '/technicians', world.technician.id);
  });
});

describe('technician maintenance routes', () => {
  it('replaces certifications', () => {
    const body = call(world.app, 'PUT', `/technicians/${world.technician.id}/certifications`, 'admin', {
      certifications: [{ skill: 'lifts', expiresOn: '2027-05-01' }],
    });
    assert.deepEqual(body.certifications, [{ skill: 'lifts', expiresOn: '2027-05-01' }]);
  });

  it('rejects malformed certifications', () => {
    const body = expectStatus(world.app, 422, 'PUT', `/technicians/${world.technician.id}/certifications`, 'admin', {
      certifications: [{ skill: '' }],
    });
    assert.equal(body.error.code, 'invalid-input');
  });

  it('records absences and reports overlaps', () => {
    const url = `/technicians/${world.technician.id}/absences`;
    const first = expectStatus(world.app, 201, 'POST', url, 'dispatch', { from: '2026-03-09', to: '2026-03-13', kind: 'leave' });
    assert.equal(first.absences.length, 1);
    const clash = expectStatus(world.app, 409, 'POST', url, 'dispatch', { from: '2026-03-12', to: '2026-03-16', kind: 'sick' });
    assert.equal(clash.error.code, 'overlapping-absence');
  });

  it('rejects absences that end before they start and unknown kinds', () => {
    const url = `/technicians/${world.technician.id}/absences`;
    assert.equal(expectStatus(world.app, 422, 'POST', url, 'admin', { from: '2026-03-13', to: '2026-03-09', kind: 'leave' }).error.code, 'invalid-absence');
    assert.equal(expectStatus(world.app, 422, 'POST', url, 'admin', { from: '2026-03-09', to: '2026-03-13', kind: 'holiday' }).error.code, 'invalid-input');
  });

  it('deactivates technicians without open work', () => {
    const body = call(world.app, 'POST', `/technicians/${world.technician.id}/deactivate`, 'admin');
    assert.equal(body.active, false);
  });

  it('refuses to deactivate a technician with open work', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    const body = expectStatus(world.app, 409, 'POST', `/technicians/${world.technician.id}/deactivate`, 'admin');
    assert.equal(body.error.code, 'technician-has-work');
    assert.equal(body.error.details.openWork, 1);
  });

  it('answers unknown technicians with 404', () => {
    expectStatus(world.app, 404, 'PUT', '/technicians/TEC-999999/certifications', 'admin', { certifications: [] });
    expectStatus(world.app, 404, 'POST', '/technicians/TEC-999999/absences', 'admin', { from: '2026-03-09', to: '2026-03-09', kind: 'leave' });
  });
});

describe('GET /technicians/certifications/expiring', () => {
  it('lists certifications that expire within the window', () => {
    call(world.app, 'PUT', `/technicians/${world.technician.id}/certifications`, 'admin', {
      certifications: [{ skill: 'gas-safe', expiresOn: '2026-03-20' }, { skill: 'plumbing', expiresOn: '2026-09-01' }],
    });
    const soon = call(world.app, 'GET', '/technicians/certifications/expiring', 'admin').items;
    assert.deepEqual(soon.map((entry: any) => [entry.skill, entry.daysLeft]), [['gas-safe', 18]]);
    const later = call(world.app, 'GET', '/technicians/certifications/expiring?within=200', 'admin').items;
    assert.equal(later.length, 2);
  });

  it('validates the window', () => {
    expectStatus(world.app, 422, 'GET', '/technicians/certifications/expiring?within=0', 'admin');
    expectStatus(world.app, 422, 'GET', '/technicians/certifications/expiring?within=400', 'admin');
    expectStatus(world.app, 422, 'GET', '/technicians/certifications/expiring?within=soon', 'admin');
  });

  it('is limited to technician managers', () => {
    expectStatus(world.app, 200, 'GET', '/technicians/certifications/expiring', 'dispatch');
    expectStatus(world.app, 403, 'GET', '/technicians/certifications/expiring', 'billing');
  });
});
'''

API_APPOINTMENTS = r'''
let world: World;
let job: any;

beforeEach(() => {
  world = createWorld();
  job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
});

const booking = (start: string, extra: Record<string, unknown> = {}) => ({
  jobId: job.id,
  technicianId: world.technician.id,
  start,
  ...extra,
});

describe('GET /technicians/:id/slots', () => {
  const url = (query: string) => `/technicians/${world.technician.id}/slots?${query}`;

  it('lists slots for a work order on a date', () => {
    const body = call(world.app, 'GET', url(`date=2026-03-02&jobId=${job.id}`), 'dispatch');
    assert.equal(body.technicianId, world.technician.id);
    assert.equal(body.date, '2026-03-02');
    assert.equal(body.durationMinutes, 60);
    assert.equal(body.slots.length, 17);
    assert.deepEqual(body.slots[0], { start: '2026-03-02T08:00:00Z', end: '2026-03-02T09:00:00Z', travelMinutes: 10 });
  });

  it('honours an explicit duration', () => {
    const body = call(world.app, 'GET', url(`date=2026-03-02&jobId=${job.id}&duration=120`), 'dispatch');
    assert.equal(body.durationMinutes, 120);
    assert.equal(body.slots.at(-1).end, '2026-03-02T17:00:00Z');
  });

  it('validates the parameters', () => {
    expectStatus(world.app, 422, 'GET', url(`jobId=${job.id}`), 'dispatch');
    expectStatus(world.app, 422, 'GET', url('date=2026-03-02'), 'dispatch');
    expectStatus(world.app, 422, 'GET', url(`date=2026-3-2&jobId=${job.id}`), 'dispatch');
    expectStatus(world.app, 422, 'GET', url(`date=2026-03-02&jobId=${job.id}&duration=5`), 'dispatch');
    expectStatus(world.app, 422, 'GET', url(`date=2026-03-02&jobId=${job.id}&duration=900`), 'dispatch');
  });

  it('answers 404 for unknown technicians or work orders', () => {
    expectStatus(world.app, 404, 'GET', '/technicians/TEC-999999/slots?date=2026-03-02&jobId=' + job.id, 'dispatch');
    expectStatus(world.app, 404, 'GET', url('date=2026-03-02&jobId=WO-999999'), 'dispatch');
  });

  it('is limited to roles that schedule work', () => {
    expectStatus(world.app, 403, 'GET', url(`date=2026-03-02&jobId=${job.id}`), world.technician.id);
    expectStatus(world.app, 403, 'GET', url(`date=2026-03-02&jobId=${job.id}`), 'billing');
  });
});

describe('POST /appointments', () => {
  it('books and returns the appointment view with its window text', () => {
    const body = expectStatus(world.app, 201, 'POST', '/appointments', 'dispatch', booking('2026-03-02T10:30:00Z'));
    assert.equal(body.id, 'APT-000001');
    assert.equal(body.status, 'booked');
    assert.equal(body.window, 'between 10:00 and 12:00');
    assert.equal(call(world.app, 'GET', `${JOBS}/${job.id}`, 'dispatch').status, 'assigned');
  });

  it('accepts an explicit duration', () => {
    const body = call(world.app, 'POST', '/appointments', 'dispatch', booking('2026-03-02T09:00:00Z', { durationMinutes: 180 }));
    assert.equal(body.end, '2026-03-02T12:00:00Z');
  });

  it('answers busy slots with a conflict', () => {
    call(world.app, 'POST', '/appointments', 'dispatch', booking('2026-03-02T09:00:00Z'));
    const other = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    const body = expectStatus(world.app, 409, 'POST', '/appointments', 'dispatch', { ...booking('2026-03-02T09:00:00Z'), jobId: other.id });
    assert.equal(body.error.code, 'slot-unavailable');
    assert.equal(body.error.details.start, '2026-03-02T09:00:00Z');
  });

  it('answers a second booking of the same work order with a conflict', () => {
    call(world.app, 'POST', '/appointments', 'dispatch', booking('2026-03-02T09:00:00Z'));
    assert.equal(expectStatus(world.app, 409, 'POST', '/appointments', 'dispatch', booking('2026-03-02T13:00:00Z')).error.code, 'already-booked');
  });

  it('validates the body', () => {
    expectStatus(world.app, 422, 'POST', '/appointments', 'dispatch', {});
    expectStatus(world.app, 422, 'POST', '/appointments', 'dispatch', booking('2026-03-02'));
    expectStatus(world.app, 422, 'POST', '/appointments', 'dispatch', booking('2026-03-02T09:00:00Z', { durationMinutes: 10 }));
    expectStatus(world.app, 422, 'POST', '/appointments', 'dispatch', booking('2026-03-02T09:00:00Z', { durationMinutes: 721 }));
  });

  it('is limited to roles that schedule work', () => {
    expectStatus(world.app, 403, 'POST', '/appointments', world.technician.id, booking('2026-03-02T09:00:00Z'));
    expectStatus(world.app, 403, 'POST', '/appointments', 'warehouse', booking('2026-03-02T09:00:00Z'));
    expectStatus(world.app, 201, 'POST', '/appointments', 'admin', booking('2026-03-02T09:00:00Z'));
  });
});

describe('appointment actions', () => {
  let appointment: any;

  beforeEach(() => {
    appointment = call(world.app, 'POST', '/appointments', 'dispatch', booking('2026-03-02T09:00:00Z'));
  });

  it('cancels with a reason of at least three characters', () => {
    expectStatus(world.app, 422, 'POST', `/appointments/${appointment.id}/cancel`, 'dispatch', { reason: 'no' });
    const body = expectStatus(world.app, 200, 'POST', `/appointments/${appointment.id}/cancel`, 'dispatch', { reason: 'Customer asked to move' });
    assert.equal(body.status, 'cancelled');
    assert.equal(expectStatus(world.app, 409, 'POST', `/appointments/${appointment.id}/cancel`, 'dispatch', { reason: 'Again please' }).error.code, 'appointment-not-live');
  });

  it('reschedules and returns the new appointment', () => {
    const body = expectStatus(world.app, 200, 'POST', `/appointments/${appointment.id}/reschedule`, 'dispatch', { start: '2026-03-03T10:00:00Z' });
    assert.equal(body.id, 'APT-000002');
    assert.equal(body.start, '2026-03-03T10:00:00Z');
    assert.equal(call(world.app, 'GET', `${JOBS}/${job.id}`, 'dispatch').scheduledStart, '2026-03-03T10:00:00Z');
  });

  it('keeps the old appointment live when rescheduling fails', () => {
    const body = expectStatus(world.app, 409, 'POST', `/appointments/${appointment.id}/reschedule`, 'dispatch', { start: '2026-03-07T10:00:00Z' });
    assert.equal(body.error.code, 'slot-unavailable');
    const again = expectStatus(world.app, 409, 'POST', '/appointments', 'dispatch', booking('2026-03-02T13:00:00Z'));
    assert.equal(again.error.code, 'already-booked');
  });

  it('lets the assigned technician confirm', () => {
    const body = expectStatus(world.app, 200, 'POST', `/appointments/${appointment.id}/confirm`, world.technician.id);
    assert.equal(body.status, 'confirmed');
  });

  it('hides appointments from technicians of other work orders', () => {
    const other = seedTechnician(world.app, { skills: ['plumbing'] });
    expectStatus(world.app, 404, 'POST', `/appointments/${appointment.id}/confirm`, other.id);
  });

  it('records no-access only for the assigned technician', () => {
    const other = seedTechnician(world.app, { skills: ['plumbing'] });
    expectStatus(world.app, 404, 'POST', `/appointments/${appointment.id}/no-access`, other.id, { reason: 'Nobody on site' });
    expectStatus(world.app, 403, 'POST', `/appointments/${appointment.id}/no-access`, 'dispatch', { reason: 'Nobody on site' });
    const body = expectStatus(world.app, 200, 'POST', `/appointments/${appointment.id}/no-access`, world.technician.id, { reason: 'Nobody on site' });
    assert.equal(body.status, 'missed');
  });

  it('answers unknown appointments with 404', () => {
    expectStatus(world.app, 404, 'POST', '/appointments/APT-999999/cancel', 'dispatch', { reason: 'Customer asked to move' });
    expectStatus(world.app, 404, 'POST', '/appointments/APT-999999/reschedule', 'dispatch', { start: '2026-03-03T10:00:00Z' });
    expectStatus(world.app, 404, 'POST', '/appointments/APT-999999/confirm', 'dispatch');
  });
});

describe('technician day views', () => {
  it('reports the daily load', () => {
    call(world.app, 'POST', '/appointments', 'dispatch', booking('2026-03-02T09:00:00Z'));
    const load = call(world.app, 'GET', `/technicians/${world.technician.id}/load?date=2026-03-02`, 'dispatch');
    assert.equal(load.appointments, 1);
    assert.equal(load.utilisationPercent, 13);
  });

  it('defaults the date to today', () => {
    const load = call(world.app, 'GET', `/technicians/${world.technician.id}/load`, 'dispatch');
    assert.equal(load.date, '2026-03-02');
  });

  it('reports the route and lets technicians see their own only', () => {
    call(world.app, 'POST', '/appointments', 'dispatch', booking('2026-03-02T09:00:00Z'));
    const own = call(world.app, 'GET', `/technicians/${world.technician.id}/route?date=2026-03-02`, world.technician.id);
    assert.equal(own.stops.length, 1);
    assert.equal(own.totalKm, 6.8);
    const other = seedTechnician(world.app, { skills: ['plumbing'] });
    expectStatus(world.app, 403, 'GET', `/technicians/${world.technician.id}/route?date=2026-03-02`, other.id);
    expectStatus(world.app, 200, 'GET', `/technicians/${world.technician.id}/route?date=2026-03-02`, 'dispatch');
  });

  it('rejects malformed dates', () => {
    expectStatus(world.app, 422, 'GET', `/technicians/${world.technician.id}/load?date=tomorrow`, 'dispatch');
    expectStatus(world.app, 422, 'GET', `/technicians/${world.technician.id}/route?date=2026-13-01`, 'dispatch');
  });

  it('answers unknown technicians with 404', () => {
    expectStatus(world.app, 404, 'GET', '/technicians/TEC-999999/load', 'dispatch');
    expectStatus(world.app, 404, 'GET', '/technicians/TEC-999999/route', 'dispatch');
  });
});
'''

API_DISPATCH = r'''
let world: World;

beforeEach(() => {
  world = createWorld();
});

describe('GET /dispatch/rules', () => {
  it('lists the scoring rules with their configured weights', () => {
    const { items } = call(world.app, 'GET', '/dispatch/rules', 'dispatch');
    assert.deepEqual(items.map((item: any) => [item.key, item.weight]), [
      ['continuity', 10],
      ['distance', 30],
      ['load', 20],
      ['skill-match', 40],
    ]);
    assert.ok(items.every((item: any) => typeof item.describe === 'string'));
  });

  it('reflects overridden weights', () => {
    const custom = createWorld({ config: { dispatch: { weights: { skill: 25, distance: 25, load: 25, continuity: 25 } } } });
    const { items } = call(custom.app, 'GET', '/dispatch/rules', 'dispatch');
    assert.deepEqual(items.map((item: any) => item.weight), [25, 25, 25, 25]);
  });

  it('is limited to roles that schedule work', () => {
    expectStatus(world.app, 200, 'GET', '/dispatch/rules', 'admin');
    expectStatus(world.app, 403, 'GET', '/dispatch/rules', 'billing');
    expectStatus(world.app, 403, 'GET', '/dispatch/rules', world.technician.id);
  });
});

describe('GET /dispatch/board', () => {
  it('shows unassigned work and technician columns', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    const board = call(world.app, 'GET', '/dispatch/board?date=2026-03-02', 'dispatch');
    assert.equal(board.date, '2026-03-02');
    assert.deepEqual(board.unassigned.map((entry: any) => entry.jobId), [job.id]);
    assert.equal(board.columns.length, 1);
  });

  it('defaults to the current date', () => {
    assert.equal(call(world.app, 'GET', '/dispatch/board', 'dispatch').date, '2026-03-02');
  });

  it('filters by region', () => {
    const west = seedCustomer(world.app, { name: 'West Works', region: 'metro-west' });
    openWorkOrder(world.app, west.customer.id, west.site.id, 'general');
    const board = call(world.app, 'GET', '/dispatch/board?region=metro-east', 'dispatch');
    assert.equal(board.unassigned.length, 0);
    assert.equal(call(world.app, 'GET', '/dispatch/board?region=metro-west', 'dispatch').unassigned.length, 1);
  });

  it('rejects a malformed date', () => {
    expectStatus(world.app, 422, 'GET', '/dispatch/board?date=02-03-2026', 'dispatch');
  });

  it('is limited to roles that schedule work', () => {
    expectStatus(world.app, 403, 'GET', '/dispatch/board', 'warehouse');
  });
});

describe('GET /dispatch/capacity', () => {
  it('requires a region', () => {
    expectStatus(world.app, 422, 'GET', '/dispatch/capacity', 'dispatch');
  });

  it('lists loads of a region from least to most loaded', () => {
    const second = seedTechnician(world.app, { skills: ['plumbing'] });
    const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    call(world.app, 'POST', '/appointments', 'dispatch', { jobId: job.id, technicianId: world.technician.id, start: '2026-03-02T09:00:00Z' });
    const { items } = call(world.app, 'GET', '/dispatch/capacity?region=metro-east&date=2026-03-02', 'dispatch');
    assert.deepEqual(items.map((item: any) => item.technicianId), [second.id, world.technician.id]);
  });

  it('is empty on days without working time', () => {
    assert.deepEqual(call(world.app, 'GET', '/dispatch/capacity?region=metro-east&date=2026-03-08', 'dispatch').items, []);
  });
});

describe('GET /jobs/:id/recommendations', () => {
  it('ranks technicians for a work order', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    const { items } = call(world.app, 'GET', `${JOBS}/${job.id}/recommendations?date=2026-03-02`, 'dispatch');
    assert.equal(items.length, 1);
    assert.equal(items[0].technicianId, world.technician.id);
    assert.equal(items[0].score, 88.5);
  });

  it('limits and validates the result size', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    for (let index = 0; index < 3; index += 1) seedTechnician(world.app, { skills: ['plumbing'] });
    assert.equal(call(world.app, 'GET', `${JOBS}/${job.id}/recommendations?limit=2`, 'dispatch').items.length, 2);
    expectStatus(world.app, 422, 'GET', `${JOBS}/${job.id}/recommendations?limit=0`, 'dispatch');
    expectStatus(world.app, 422, 'GET', `${JOBS}/${job.id}/recommendations?limit=21`, 'dispatch');
  });

  it('answers unknown work orders with 404', () => {
    expectStatus(world.app, 404, 'GET', `${JOBS}/WO-999999/recommendations`, 'dispatch');
  });

  it('is limited to roles that schedule work', () => {
    const job = openWorkOrder(world.app, world.customer.id, world.site.id, 'general');
    expectStatus(world.app, 403, 'GET', `${JOBS}/${job.id}/recommendations`, world.technician.id);
  });
});
'''

SLOT_WORLD = {WORLD: ["call", "createWorld", "openWorkOrder", "seedCustomer", "seedTechnician"], TECHS_REPO: ["technicians"]}
AT = ["MONDAY", "at", "call", "seedAsset", "seedCustomer", "seedTechnician"]
WORLD_API = ["call", "createWorld", "expectStatus", "JOBS", "openWorkOrder", "seedCustomer", "seedTechnician", "type World"]

FILES = {
    "tests/technicians/service.test.ts": Spec(
        {**APP, TECH: ["createTechnician", "deactivateTechnician"], "src/adapters/store/audit.ts": ["auditTrail"],
         TECHS_REPO: ["activeTechnicians", "technicians", "techniciansInRegion"]},
        TECHNICIAN_SERVICE),
    "tests/technicians/skills.test.ts": Spec(
        {**APP, TECH: ["createTechnician", "deactivateTechnician"],
         SKILLS: ["expiringCertifications", "hasSkill", "missingSkills"]},
        SKILLS_BODY),
    "tests/technicians/certifications.test.ts": Spec(
        {**APP, TECH: ["createTechnician", "updateCertifications"], "src/adapters/store/audit.ts": ["auditTrail"]},
        CERTIFICATIONS),
    "tests/technicians/absences.test.ts": Spec(
        {**APP, TECH: ["createTechnician", "deactivateTechnician", "isAvailable", "recordAbsence"],
         "src/adapters/store/audit.ts": ["auditTrail"], TECHS_REPO: ["technicians"]},
        ABSENCES),
    "tests/scheduling/travel-time.test.ts": Spec(
        {TRAVEL: ["travelKm", "travelMinutes"], "src/shared/geo.ts": ["distanceKm"]}, TRAVEL_BODY),
    "tests/scheduling/windows.test.ts": Spec(
        {**APP, WINDOWS: ["arrivalWindow", "describeWindow", "windowStarts"]}, WINDOWS_BODY),
    "tests/scheduling/slots.test.ts": Spec(
        {**SLOT_WORLD, SLOTS: ["availableSlots", "firstAvailableSlot", "workingBounds"], TECH: ["recordAbsence"],
         "src/scheduling/locations.ts": ["appointmentLocations"]},
        SLOTS_BODY, node=WITH_SETUP),
    "tests/scheduling/service.test.ts": scene_spec(
        {WORLD: AT, APPTS_REPO: ["appointments"],
         SCHED: ["bookAppointment", "cancelAppointment", "confirmAppointment", "recordNoAccess",
                 "rescheduleAppointment"]},
        SCHEDULING_SERVICE, jobs=("cancelJob",)),
    "tests/scheduling/capacity.test.ts": Spec(
        {**SLOT_WORLD, CAPACITY: ["dailyLoad", "leastLoaded"]}, CAPACITY_BODY, node=WITH_SETUP),
    "tests/scheduling/route.test.ts": Spec(
        {**SLOT_WORLD, ROUTE: ["dailyRoute"]}, ROUTE_BODY, node=WITH_SETUP),
    "tests/dispatch/rules.test.ts": Spec(
        {"src/config/load.ts": ["resolveConfig"], "src/dispatch/rules/index.ts": ["createScoringRegistry"],
         "src/dispatch/rules/skill-match.ts": ["skillMatch"], "src/dispatch/rules/distance.ts": ["proximity"],
         "src/dispatch/rules/load.ts": ["workload"], "src/dispatch/rules/continuity.ts": ["continuity"]},
        DISPATCH_RULES_BODY),
    "tests/dispatch/recommend.test.ts": scene_spec(
        {WORLD: AT, "src/dispatch/recommend.ts": ["recommendTechnicians"], TECH: ["createTechnician", "recordAbsence"]},
        RECOMMEND_BODY),
    "tests/dispatch/board.test.ts": scene_spec(
        {WORLD: AT, "src/dispatch/board.ts": ["dispatchBoard"], TECH: ["deactivateTechnician"]},
        BOARD_BODY, jobs=("holdJob", "cancelJob")),
    "tests/api/technicians.test.ts": Spec(
        {WORLD: WORLD_API}, API_TECHNICIANS, node=WITH_SETUP),
    "tests/api/appointments.test.ts": Spec(
        {WORLD: WORLD_API}, API_APPOINTMENTS, node=WITH_SETUP),
    "tests/api/dispatch.test.ts": Spec(
        {WORLD: WORLD_API}, API_DISPATCH, node=WITH_SETUP),
}


def generate(ctx):
    register(ctx, FILES)
