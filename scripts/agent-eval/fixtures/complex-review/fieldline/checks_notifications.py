"""Tests for notifications: quiet hours, preferences, templates, delivery and the notification routes."""

from .checks_kit import DESCRIBE_IT, WITH_SETUP, Spec, register
from .checks_workorders import scene_spec

WORLD = "tests/support/world.ts"
NOTIFY = "src/notifications"
NOTE_REPO = "src/adapters/repositories/notifications.ts"

QUIET = r'''
const contextWith = (notifications: Record<string, unknown> = {}) => createApp({ now: at(MONDAY, '12:00'), config: { notifications } as any }).ctx;
const defaults = contextWith();

describe('inQuietHours', () => {
  const cases: Array<[string, boolean]> = [
    ['00:00', true],
    ['03:30', true],
    ['06:59', true],
    ['07:00', false],
    ['12:00', false],
    ['20:59', false],
    ['21:00', true],
    ['23:59', true],
  ];
  for (const [clock, quiet] of cases) {
    it(`is ${quiet ? 'quiet' : 'not quiet'} at ${clock} with the default hours`, () => {
      assert.equal(inQuietHours(defaults, at(MONDAY, clock)), quiet);
    });
  }

  it('supports a window inside a single day', () => {
    const ctx = contextWith({ quietStartMinute: 600, quietEndMinute: 720 });
    assert.equal(inQuietHours(ctx, at(MONDAY, '09:59')), false);
    assert.equal(inQuietHours(ctx, at(MONDAY, '10:00')), true);
    assert.equal(inQuietHours(ctx, at(MONDAY, '11:59')), true);
    assert.equal(inQuietHours(ctx, at(MONDAY, '12:00')), false);
    assert.equal(inQuietHours(ctx, at(MONDAY, '23:00')), false);
  });

  it('is never quiet when the period is empty', () => {
    const ctx = contextWith({ quietStartMinute: 480, quietEndMinute: 480 });
    for (const clock of ['00:00', '07:59', '08:00', '12:00', '23:59']) assert.equal(inQuietHours(ctx, at(MONDAY, clock)), false, clock);
  });

  it('does not depend on the date', () => {
    assert.equal(inQuietHours(defaults, at('2026-03-07', '22:00')), true);
    assert.equal(inQuietHours(defaults, at('2026-04-03', '12:00')), false);
  });
});

describe('sendAfter', () => {
  it('sends immediately outside quiet hours', () => {
    const when = at(MONDAY, '12:00');
    assert.equal(sendAfter(defaults, when, 'sms', false), when);
  });

  it('holds evening text messages until the next morning', () => {
    assert.equal(sendAfter(defaults, at(MONDAY, '22:00'), 'sms', false), at('2026-03-03', '07:00'));
    assert.equal(sendAfter(defaults, at(MONDAY, '21:00'), 'sms', false), at('2026-03-03', '07:00'));
    assert.equal(sendAfter(defaults, at(MONDAY, '23:59'), 'sms', false), at('2026-03-03', '07:00'));
  });

  it('holds night text messages until the same morning', () => {
    assert.equal(sendAfter(defaults, at(MONDAY, '00:00'), 'sms', false), at(MONDAY, '07:00'));
    assert.equal(sendAfter(defaults, at(MONDAY, '03:30'), 'sms', false), at(MONDAY, '07:00'));
    assert.equal(sendAfter(defaults, at(MONDAY, '06:59'), 'sms', false), at(MONDAY, '07:00'));
  });

  it('sends at the end of the quiet period', () => {
    const when = at(MONDAY, '07:00');
    assert.equal(sendAfter(defaults, when, 'sms', false), when);
  });

  it('never defers email or portal messages', () => {
    const when = at(MONDAY, '23:30');
    assert.equal(sendAfter(defaults, when, 'email', false), when);
    assert.equal(sendAfter(defaults, when, 'portal', false), when);
  });

  it('never defers urgent messages', () => {
    const when = at(MONDAY, '02:15');
    assert.equal(sendAfter(defaults, when, 'sms', true), when);
  });

  it('moves the end of quiet hours to the next quarter hour', () => {
    const ctx = contextWith({ quietEndMinute: 430 });
    assert.equal(sendAfter(ctx, at(MONDAY, '23:00'), 'sms', false), at('2026-03-03', '07:15'));
    assert.equal(sendAfter(ctx, at(MONDAY, '07:05'), 'sms', false), at(MONDAY, '07:15'));
  });

  it('keeps a quarter hour boundary as it is', () => {
    const ctx = contextWith({ quietEndMinute: 435 });
    assert.equal(sendAfter(ctx, at(MONDAY, '05:00'), 'sms', false), at(MONDAY, '07:15'));
  });

  it('follows a configured quiet period', () => {
    const ctx = contextWith({ quietStartMinute: 1080, quietEndMinute: 480 });
    assert.equal(sendAfter(ctx, at(MONDAY, '17:59'), 'sms', false), at(MONDAY, '17:59'));
    assert.equal(sendAfter(ctx, at(MONDAY, '18:00'), 'sms', false), at('2026-03-03', '08:00'));
  });
});
'''

PREFERENCES = r'''
let ctx: AppContext;

beforeEach(() => {
  ctx = createApp().ctx;
});

const withConfig = (channels: Array<'email' | 'sms' | 'portal'>) => createApp({ config: { notifications: { channels } } }).ctx;

describe('setPreference', () => {
  it('stores the accepted channels and opted-out templates', () => {
    const stored = setPreference(ctx, 'CUS-000001', ['sms', 'email'], ['work-started']);
    assert.deepEqual(stored, { id: 'CUS-000001', channels: ['sms', 'email'], optOut: ['work-started'] });
    assert.deepEqual(preferenceFor(ctx, 'CUS-000001'), stored);
  });

  it('removes duplicates', () => {
    const stored = setPreference(ctx, 'CUS-000001', ['email', 'email', 'sms'], ['parts-delay', 'parts-delay']);
    assert.deepEqual(stored.channels, ['email', 'sms']);
    assert.deepEqual(stored.optOut, ['parts-delay']);
  });

  it('replaces an earlier preference', () => {
    setPreference(ctx, 'CUS-000001', ['sms'], ['work-started']);
    const stored = setPreference(ctx, 'CUS-000001', ['email']);
    assert.deepEqual(stored, { id: 'CUS-000001', channels: ['email'], optOut: [] });
    assert.equal(preferences(ctx).count(), 1);
  });

  it('keeps preferences of different recipients apart', () => {
    setPreference(ctx, 'CUS-000001', ['sms']);
    setPreference(ctx, 'CUS-000002', ['email']);
    assert.deepEqual(preferenceFor(ctx, 'CUS-000001')?.channels, ['sms']);
    assert.deepEqual(preferenceFor(ctx, 'CUS-000002')?.channels, ['email']);
  });

  it('requires at least one channel', () => {
    assert.throws(
      () => setPreference(ctx, 'CUS-000001', []),
      (error: any) => error instanceof DomainError && error.code === 'no-channels' && error.status === 422,
    );
    assert.equal(preferenceFor(ctx, 'CUS-000001'), undefined);
  });
});

describe('preferenceFor', () => {
  it('is undefined for recipients that never chose', () => {
    assert.equal(preferenceFor(ctx, 'CUS-000009'), undefined);
  });
});

describe('chooseChannel', () => {
  it('takes the first offered channel without a preference', () => {
    assert.equal(chooseChannel(ctx, 'CUS-000001', ['sms', 'email']), 'sms');
    assert.equal(chooseChannel(ctx, 'CUS-000001', ['email', 'sms']), 'email');
  });

  it('respects the order of the template, not of the preference', () => {
    setPreference(ctx, 'CUS-000001', ['sms', 'email']);
    assert.equal(chooseChannel(ctx, 'CUS-000001', ['email', 'sms']), 'email');
  });

  it('skips channels the recipient does not accept', () => {
    setPreference(ctx, 'CUS-000001', ['email']);
    assert.equal(chooseChannel(ctx, 'CUS-000001', ['sms', 'email']), 'email');
  });

  it('gives up when nothing offered is accepted', () => {
    setPreference(ctx, 'CUS-000001', ['portal']);
    assert.equal(chooseChannel(ctx, 'CUS-000001', ['sms', 'email']), undefined);
  });

  it('skips channels the organisation has not enabled', () => {
    const emailOnly = withConfig(['email']);
    assert.equal(chooseChannel(emailOnly, 'CUS-000001', ['sms', 'email']), 'email');
    assert.equal(chooseChannel(emailOnly, 'CUS-000001', ['sms']), undefined);
  });

  it('always allows the portal', () => {
    const silent = withConfig([]);
    assert.equal(chooseChannel(silent, 'CUS-000001', ['sms', 'portal']), 'portal');
    assert.equal(chooseChannel(silent, 'CUS-000001', ['email', 'sms']), undefined);
  });

  it('still honours the recipient for the portal', () => {
    setPreference(ctx, 'USR-000001', ['email']);
    assert.equal(chooseChannel(ctx, 'USR-000001', ['portal']), undefined);
  });

  it('returns nothing for an empty offer', () => {
    assert.equal(chooseChannel(ctx, 'CUS-000001', []), undefined);
  });
});
'''

TEMPLATES = r'''
const CHANNELS = ['email', 'sms', 'portal'];
const KNOWN_PLACEHOLDERS = new Set([
  'available', 'breachMinutes', 'customerName', 'date', 'dueOn', 'invoiceNumber', 'locationId', 'number', 'amount', 'percent',
  'priority', 'reason', 'resolution', 'siteLabel', 'sku', 'summary', 'technicianName', 'window',
]);
const SAMPLE = Object.fromEntries([...KNOWN_PLACEHOLDERS].map((name) => [name, `<${name}>`]));
const byKey = (key: string) => {
  const template = BUILT_IN_TEMPLATES.find((candidate) => candidate.key === key);
  assert.ok(template, key);
  return template;
};

describe('built-in templates', () => {
  it('have unique keys', () => {
    const keys = BUILT_IN_TEMPLATES.map((template) => template.key);
    assert.equal(new Set(keys).size, keys.length);
  });

  it('offer known channels without repeating them', () => {
    for (const template of BUILT_IN_TEMPLATES) {
      assert.ok(template.channels.length > 0, template.key);
      assert.equal(new Set(template.channels).size, template.channels.length, template.key);
      for (const channel of template.channels) assert.ok(CHANNELS.includes(channel), `${template.key} ${channel}`);
    }
  });

  it('only use placeholders the senders provide', () => {
    for (const template of BUILT_IN_TEMPLATES) {
      for (const name of [...placeholdersOf(template.subject), ...placeholdersOf(template.body)]) {
        assert.ok(KNOWN_PLACEHOLDERS.has(name), `${template.key} uses {{${name}}}`);
      }
    }
  });

  it('render completely when every placeholder has a value', () => {
    for (const template of BUILT_IN_TEMPLATES) {
      assert.doesNotMatch(renderTemplate(template.subject, SAMPLE), /\{\{/, template.key);
      assert.doesNotMatch(renderTemplate(template.body, SAMPLE), /\{\{/, template.key);
    }
  });

  it('keep subjects short enough for a text message', () => {
    for (const template of BUILT_IN_TEMPLATES) assert.ok(template.subject.length <= 40, template.key);
  });

  it('mark exactly the operational alerts as urgent', () => {
    const urgent = BUILT_IN_TEMPLATES.filter((template) => template.urgent).map((template) => template.key);
    assert.deepEqual(urgent.sort(), ['emergency-opened', 'sla-breached', 'sla-warning']);
  });

  it('never offer the portal to customers', () => {
    for (const template of BUILT_IN_TEMPLATES.filter((candidate) => candidate.audience === 'customer')) {
      assert.ok(!template.channels.includes('portal'), template.key);
    }
  });

  it('always offer the portal to staff', () => {
    for (const template of BUILT_IN_TEMPLATES.filter((candidate) => candidate.audience === 'staff')) {
      assert.ok(template.channels.includes('portal'), template.key);
    }
  });

  it('greet customers by name', () => {
    for (const template of BUILT_IN_TEMPLATES.filter((candidate) => candidate.audience === 'customer' && candidate.body.startsWith('Hello'))) {
      assert.match(template.body, /^Hello \{\{customerName\}\}, /, template.key);
    }
  });

  it('describe the arrival window of confirmed visits', () => {
    const template = byKey('appointment-confirmed');
    assert.equal(
      renderTemplate(template.body, { customerName: 'Marlow Hospice', technicianName: 'Sam Rivera', siteLabel: 'Main building', date: '2026-03-02', window: 'between 10:00 and 12:00', number: 'FL-2026-00007' }),
      'Hello Marlow Hospice, Sam Rivera will visit Main building on 2026-03-02 between 10:00 and 12:00. Reference FL-2026-00007.',
    );
  });
});

describe('createTemplateRegistry', () => {
  it('registers every built-in template under its key', () => {
    const registry = createTemplateRegistry();
    assert.deepEqual(registry.keys(), BUILT_IN_TEMPLATES.map((template) => template.key).sort());
    for (const template of BUILT_IN_TEMPLATES) assert.equal(registry.lookup(template.key), template);
  });

  it('lists the keys in alphabetical order', () => {
    assert.deepEqual(createTemplateRegistry().keys(), [
      'appointment-cancelled',
      'appointment-confirmed',
      'appointment-rescheduled',
      'emergency-opened',
      'follow-up-required',
      'invoice-issued',
      'low-stock',
      'no-access',
      'parts-delay',
      'sla-breached',
      'sla-warning',
      'technician-assigned',
      'work-completed',
      'work-started',
    ]);
  });

  it('creates independent registries', () => {
    const first = createTemplateRegistry();
    const second = createTemplateRegistry();
    first.register('custom', { key: 'custom', audience: 'staff', channels: ['portal'], urgent: false, subject: 'Custom', body: 'Custom body' });
    assert.equal(first.has('custom'), true);
    assert.equal(second.has('custom'), false);
  });

  it('refuses to register a key twice', () => {
    const registry = createTemplateRegistry();
    assert.throws(() => registry.register('low-stock', byKey('low-stock')), /duplicate registration for low-stock/);
  });

  it('matches keys exactly', () => {
    const registry = createTemplateRegistry();
    assert.equal(registry.lookup('Low-Stock'), undefined);
    assert.throws(() => registry.resolve('missing'), /nothing registered for missing/);
  });
});
'''

NOTIFY_BODY = r'''
let scene: Scene;

beforeEach(() => {
  scene = start();
});

const startedValues = (extra: Record<string, string> = {}) => ({ technicianName: 'Morgan Reyes', number: 'FL-2026-00042', ...extra });
const stored = (recipient = scene.customer.id) => notificationsFor(scene.ctx, recipient);

describe('notify', () => {
  it('stores a rendered, queued notification', () => {
    const sent = notify(scene.ctx, 'work-started', scene.customer.id, startedValues(), 'WO-000042');
    assert.deepEqual(sent, {
      id: 'NTF-000001',
      recipientId: scene.customer.id,
      channel: 'sms',
      templateKey: 'work-started',
      subject: 'We have arrived',
      body: 'Morgan Reyes has started work on FL-2026-00042.',
      jobId: 'WO-000042',
      status: 'queued',
      createdAt: at(MONDAY, '08:00'),
      sendAfter: at(MONDAY, '08:00'),
    });
    assert.deepEqual(stored(), [sent]);
  });

  it('numbers notifications sequentially', () => {
    const first = notify(scene.ctx, 'work-started', scene.customer.id, startedValues());
    const second = notify(scene.ctx, 'work-started', scene.customer.id, startedValues());
    assert.deepEqual([first?.id, second?.id], ['NTF-000001', 'NTF-000002']);
  });

  it('works without a work order', () => {
    const sent = notify(scene.ctx, 'low-stock', 'warehouse', { sku: 'PUMP-CIRC', available: 2, locationId: 'WH-CENTRAL' });
    assert.equal(sent?.jobId, undefined);
    assert.equal(sent?.body, 'PUMP-CIRC is down to 2 at WH-CENTRAL.');
  });

  it('leaves placeholders without a value visible', () => {
    const sent = notify(scene.ctx, 'work-started', scene.customer.id, {});
    assert.equal(sent?.body, '{{technicianName}} has started work on {{number}}.');
  });

  it('offers the sender name to templates', () => {
    scene.ctx.templates.register('greeting', { key: 'greeting', audience: 'customer', channels: ['email'], urgent: false, subject: 'Hello', body: 'Regards, {{sender}}' });
    assert.equal(notify(scene.ctx, 'greeting', scene.customer.id, {})?.body, 'Regards, Fieldline Service Desk');
    const renamed = start({ config: { notifications: { senderName: 'Harbour Desk' } } });
    renamed.ctx.templates.register('greeting', { key: 'greeting', audience: 'customer', channels: ['email'], urgent: false, subject: 'Hello', body: 'Regards, {{sender}}' });
    assert.equal(notify(renamed.ctx, 'greeting', renamed.customer.id, {})?.body, 'Regards, Harbour Desk');
  });

  it('ignores unknown templates and logs a warning', () => {
    assert.equal(notify(scene.ctx, 'no-such-template', scene.customer.id, {}), undefined);
    assert.deepEqual(stored(), []);
    assert.deepEqual(scene.ctx.log.records('warn').map((record) => record.fields.templateKey), ['no-such-template']);
  });

  it('sends nothing when the recipient accepts none of the channels', () => {
    setPreference(scene.ctx, scene.customer.id, ['portal']);
    assert.equal(notify(scene.ctx, 'work-started', scene.customer.id, startedValues()), undefined);
    assert.deepEqual(stored(), []);
  });

  it('uses the channel the recipient prefers among those offered', () => {
    setPreference(scene.ctx, scene.customer.id, ['sms']);
    assert.equal(notify(scene.ctx, 'appointment-rescheduled', scene.customer.id, {})?.channel, 'sms');
    setPreference(scene.ctx, scene.customer.id, ['email']);
    assert.equal(notify(scene.ctx, 'appointment-rescheduled', scene.customer.id, {})?.channel, 'email');
  });

  it('keeps opted-out messages as suppressed', () => {
    setPreference(scene.ctx, scene.customer.id, ['sms', 'email'], ['work-started']);
    const suppressed = notify(scene.ctx, 'work-started', scene.customer.id, startedValues());
    assert.equal(suppressed?.status, 'suppressed');
    assert.equal(notify(scene.ctx, 'no-access', scene.customer.id, {})?.status, 'queued');
    assert.equal(stored().length, 2);
  });

  it('defers text messages that fall into quiet hours', () => {
    scene.app.clock.set(at(MONDAY, '22:15'));
    const deferred = notify(scene.ctx, 'work-started', scene.customer.id, startedValues());
    assert.equal(deferred?.status, 'deferred');
    assert.equal(deferred?.createdAt, at(MONDAY, '22:15'));
    assert.equal(deferred?.sendAfter, at('2026-03-03', '07:00'));
  });

  it('does not defer email in quiet hours', () => {
    scene.app.clock.set(at(MONDAY, '22:15'));
    const mail = notify(scene.ctx, 'work-completed', scene.customer.id, { customerName: 'Harbour View', number: 'FL-2026-00001', resolution: 'Done' });
    assert.equal(mail?.channel, 'email');
    assert.equal(mail?.status, 'queued');
  });

  it('does not defer urgent alerts', () => {
    scene.app.clock.set(at(MONDAY, '03:00'));
    const alert = notify(scene.ctx, 'emergency-opened', 'dispatch', { number: 'FL-2026-00001', siteLabel: 'Main building', summary: 'Burst pipe' });
    assert.equal(alert?.channel, 'sms');
    assert.equal(alert?.status, 'queued');
    assert.equal(alert?.subject, 'EMERGENCY FL-2026-00001');
  });

  it('suppresses opted-out messages even in quiet hours', () => {
    scene.app.clock.set(at(MONDAY, '22:15'));
    setPreference(scene.ctx, scene.customer.id, ['sms'], ['work-started']);
    assert.equal(notify(scene.ctx, 'work-started', scene.customer.id, startedValues())?.status, 'suppressed');
  });
});

describe('flushOutbox', () => {
  it('marks queued messages as sent', () => {
    notify(scene.ctx, 'work-started', scene.customer.id, startedValues());
    notify(scene.ctx, 'no-access', scene.customer.id, {});
    assert.equal(flushOutbox(scene.ctx), 2);
    assert.deepEqual(stored().map((entry) => entry.status), ['sent', 'sent']);
  });

  it('does not send the same message twice', () => {
    notify(scene.ctx, 'work-started', scene.customer.id, startedValues());
    assert.equal(flushOutbox(scene.ctx), 1);
    assert.equal(flushOutbox(scene.ctx), 0);
  });

  it('sends deferred messages once they are due', () => {
    scene.app.clock.set(at(MONDAY, '22:15'));
    notify(scene.ctx, 'work-started', scene.customer.id, startedValues());
    assert.equal(flushOutbox(scene.ctx), 0);
    scene.app.clock.set(at('2026-03-03', '06:59'));
    assert.equal(flushOutbox(scene.ctx), 0);
    scene.app.clock.set(at('2026-03-03', '07:00'));
    assert.equal(flushOutbox(scene.ctx), 1);
    assert.equal(stored()[0].status, 'sent');
  });

  it('never sends suppressed messages', () => {
    setPreference(scene.ctx, scene.customer.id, ['sms'], ['work-started']);
    notify(scene.ctx, 'work-started', scene.customer.id, startedValues());
    assert.equal(flushOutbox(scene.ctx), 0);
    assert.equal(stored()[0].status, 'suppressed');
  });
});

describe('notification queries', () => {
  it('lists a recipient\'s notifications in creation order', () => {
    notify(scene.ctx, 'no-access', scene.customer.id, {});
    notify(scene.ctx, 'work-started', 'dispatch', {});
    notify(scene.ctx, 'work-started', scene.customer.id, {});
    assert.deepEqual(notificationsFor(scene.ctx, scene.customer.id).map((entry) => entry.templateKey), ['no-access', 'work-started']);
    assert.deepEqual(notificationsFor(scene.ctx, 'dispatch').map((entry) => entry.id), ['NTF-000002']);
    assert.deepEqual(notificationsFor(scene.ctx, 'nobody'), []);
  });

  it('lists the notifications of a work order', () => {
    notify(scene.ctx, 'no-access', scene.customer.id, {}, 'WO-000001');
    notify(scene.ctx, 'work-started', scene.customer.id, {}, 'WO-000002');
    notify(scene.ctx, 'parts-delay', scene.customer.id, {}, 'WO-000001');
    assert.deepEqual(notificationsForJob(scene.ctx, 'WO-000001').map((entry) => entry.templateKey), ['no-access', 'parts-delay']);
    assert.deepEqual(notificationsForJob(scene.ctx, 'WO-000099'), []);
  });
});
'''

SUBSCRIPTIONS = r'''
let scene: Scene;

beforeEach(() => {
  scene = start();
});

const received = (recipient: string, template?: string) =>
  notificationsFor(scene.ctx, recipient).filter((entry) => template === undefined || entry.templateKey === template);

function bookVisit(jobId: string, clock = '10:00') {
  return bookAppointment(scene.ctx, 'dispatch', { jobId, technicianId: scene.tech, start: at(MONDAY, clock) });
}

describe('work order notifications', () => {
  it('tells the technician about a new assignment', () => {
    const job = open(scene, 'general', { priority: 'high' });
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    const [message] = received(scene.tech);
    assert.equal(message.templateKey, 'technician-assigned');
    assert.equal(message.channel, 'sms');
    assert.equal(message.status, 'queued');
    assert.equal(message.subject, `New work order ${job.number}`);
    assert.equal(message.body, `${job.number} at Main building: Investigate reported fault. Priority high.`);
    assert.equal(message.jobId, job.id);
  });

  it('tells the customer when work starts', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    assert.equal(received(scene.customer.id).length, 0);
    «startJob»(scene.ctx, scene.tech, job.id);
    const [message] = received(scene.customer.id);
    assert.equal(message.templateKey, 'work-started');
    assert.equal(message.body, `${scene.technician.name} has started work on ${job.number}.`);
  });

  it('announces the start of work only the first time', () => {
    const job = open(scene);
    work(scene, job.id);
    «reopenJob»(scene.ctx, 'dispatch', job.id, 'The fault returned');
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    assert.equal(received(scene.customer.id, 'work-started').length, 1);
  });

  it('warns the customer when work waits for parts', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    «holdJob»(scene.ctx, scene.tech, job.id, 'awaiting_parts');
    const [message] = received(scene.customer.id, 'parts-delay');
    assert.equal(message.channel, 'email');
    assert.equal(message.subject, 'Waiting for parts');
    assert.equal(message.body, `Hello ${scene.customer.name}, work order ${job.number} is waiting for parts. We will rebook as soon as they arrive.`);
  });

  it('stays quiet about other kinds of hold', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    «holdJob»(scene.ctx, scene.tech, job.id, 'weather');
    assert.equal(received(scene.customer.id, 'parts-delay').length, 0);
  });

  it('reports the resolution when work is completed', () => {
    const job = open(scene);
    work(scene, job.id, 'Replaced the expansion vessel');
    const [message] = received(scene.customer.id, 'work-completed');
    assert.equal(message.channel, 'email');
    assert.equal(message.body, `Hello ${scene.customer.name}, ${job.number} was completed: Replaced the expansion vessel.`);
    assert.equal(received('dispatch', 'follow-up-required').length, 0);
  });

  it('asks dispatch for a follow-up visit when the technician flags one', () => {
    const job = open(scene);
    «assignJob»(scene.ctx, 'dispatch', job.id, scene.tech);
    «startJob»(scene.ctx, scene.tech, job.id);
    for (const item of Object.keys(«requireJob»(scene.ctx, job.id).checklist)) «recordChecklistItem»(scene.ctx, scene.tech, job.id, item, true);
    «completeJob»(scene.ctx, scene.tech, job.id, { resolution: 'Temporary repair in place', followUpRequired: true });
    const [message] = received('dispatch', 'follow-up-required');
    assert.equal(message.channel, 'portal');
    assert.equal(message.subject, `Follow-up needed for ${job.number}`);
    assert.equal(message.body, `${scene.technician.name} flagged ${job.number} for a follow-up visit.`);
  });

  it('alerts dispatch about emergencies even at night', () => {
    scene.app.clock.set(at(MONDAY, '23:30'));
    const job = open(scene, 'general', { priority: 'emergency', summary: 'Gas smell in the plant room' });
    const [message] = received('dispatch', 'emergency-opened');
    assert.equal(message.channel, 'sms');
    assert.equal(message.status, 'queued');
    assert.equal(message.subject, `EMERGENCY ${job.number}`);
    assert.equal(message.body, `Emergency work order ${job.number} at Main building: Gas smell in the plant room.`);
  });

  it('does not alert dispatch about ordinary work', () => {
    open(scene, 'general', { priority: 'high' });
    assert.equal(received('dispatch', 'emergency-opened').length, 0);
  });

  it('respects customers who opted out of a message', () => {
    setPreference(scene.ctx, scene.customer.id, ['email', 'sms'], ['work-completed']);
    work(scene, open(scene).id);
    assert.deepEqual(received(scene.customer.id, 'work-completed').map((entry) => entry.status), ['suppressed']);
  });
});

describe('appointment notifications', () => {
  it('confirms a booked visit with the arrival window', () => {
    const job = open(scene);
    bookVisit(job.id);
    const [message] = received(scene.customer.id, 'appointment-confirmed');
    assert.equal(message.channel, 'email');
    assert.equal(message.subject, `Your visit on ${MONDAY}`);
    assert.equal(
      message.body,
      `Hello ${scene.customer.name}, ${scene.technician.name} will visit Main building on ${MONDAY} between 10:00 and 12:00. Reference ${job.number}.`,
    );
  });

  it('announces a moved visit with the new date and window', () => {
    const job = open(scene);
    const booked = bookVisit(job.id);
    rescheduleAppointment(scene.ctx, 'dispatch', booked.id, at('2026-03-03', '14:00'));
    const [message] = received(scene.customer.id, 'appointment-rescheduled');
    assert.equal(message.subject, 'Your visit has moved');
    assert.equal(message.body, `Hello ${scene.customer.name}, your visit for ${job.number} is now on 2026-03-03 between 14:00 and 16:00.`);
  });

  it('asks the customer to rebook after a missed visit', () => {
    const job = open(scene);
    const booked = bookVisit(job.id);
    scene.app.clock.set(at(MONDAY, '10:15'));
    recordNoAccess(scene.ctx, scene.tech, booked.id, 'Nobody answered the door');
    const [message] = received(scene.customer.id, 'no-access');
    assert.equal(message.channel, 'email');
    assert.equal(message.body, `Hello ${scene.customer.name}, our technician could not access Main building for ${job.number}. Please contact us to rebook.`);
  });
});

describe('service level notifications', () => {
  function emergency() {
    scene.app.clock.set(at(MONDAY, '09:00'));
    seedContract(scene.app, scene.customer.id, [scene.site.id], 'basic', ['general']);
    return open(scene, 'general', { priority: 'emergency' });
  }

  it('warns dispatch about work at risk of breaching', () => {
    const job = emergency();
    scene.app.clock.set(at(MONDAY, '12:30'));
    scanForWarnings(scene.ctx);
    const [message] = received('dispatch', 'sla-warning');
    assert.equal(message.channel, 'portal');
    assert.equal(message.subject, `SLA at risk: ${job.number}`);
    assert.equal(message.body, `${job.number} has used 88% of its resolution time.`);
  });

  it('reports a breach with the minutes missed', () => {
    const job = emergency();
    scene.app.clock.set(at(MONDAY, '16:00'));
    work(scene, job.id);
    const [message] = received('dispatch', 'sla-breached');
    assert.equal(message.channel, 'email');
    assert.equal(message.status, 'queued');
    assert.equal(message.subject, `SLA breached: ${job.number}`);
    assert.equal(message.body, `${job.number} missed its resolution target by 180 minutes.`);
  });

  it('stays silent for work that meets its target', () => {
    const job = emergency();
    scene.app.clock.set(at(MONDAY, '10:00'));
    work(scene, job.id);
    assert.equal(received('dispatch', 'sla-breached').length, 0);
  });
});

describe('stock notifications', () => {
  it('tells the warehouse when a count falls to the reorder point', () => {
    addPart(scene.ctx, 'warehouse', { id: 'PUMP-CIRC', name: 'Circulation pump', category: 'heating', unitCostCents: 10000, listPriceCents: 18000, reorderPoint: 5, reorderQuantity: 10 });
    adjustCount(scene.ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 8);
    assert.equal(received('warehouse').length, 0);
    adjustCount(scene.ctx, 'warehouse', 'WH-CENTRAL', 'PUMP-CIRC', 3);
    const [message] = received('warehouse');
    assert.equal(message.templateKey, 'low-stock');
    assert.equal(message.channel, 'portal');
    assert.equal(message.subject, 'Low stock: PUMP-CIRC');
    assert.equal(message.body, 'PUMP-CIRC is down to 3 at WH-CENTRAL.');
    assert.equal(message.jobId, undefined);
  });
});
'''

API = r'''
let world: World;
let portal: string;

beforeEach(() => {
  world = createWorld();
  portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' }).id;
});

const stranger = () => {
  const other = seedCustomer(world.app, { name: 'Brightwater Dental' }).customer;
  return call(world.app, 'POST', `/customers/${other.id}/portal-users`, 'admin', { name: 'Sam Rivera' }).id as string;
};

function assignedJob() {
  const job = openWorkOrder(world.app, world.customer.id, world.site.id);
  call(world.app, 'POST', `${JOBS}/${job.id}/assign`, 'dispatch', { technicianId: world.technician.id });
  call(world.app, 'POST', `${JOBS}/${job.id}/start`, world.technician.id);
  return job;
}

describe('GET /notifications', () => {
  it('shows staff their own inbox', () => {
    assignedJob();
    const mine = call(world.app, 'GET', '/notifications', 'dispatch');
    assert.deepEqual(mine.items, []);
    const technician = call(world.app, 'GET', `/notifications?recipient=${world.technician.id}`, 'dispatch');
    assert.deepEqual(technician.items.map((item: any) => item.templateKey), ['technician-assigned']);
  });

  it('shows customers the messages addressed to their account', () => {
    assignedJob();
    const items = call(world.app, 'GET', '/notifications', portal).items;
    assert.deepEqual(items.map((item: any) => item.templateKey), ['work-started']);
    assert.equal(items[0].recipientId, world.customer.id);
  });

  it('ignores the recipient filter of customers', () => {
    assignedJob();
    const other = stranger();
    assert.deepEqual(call(world.app, 'GET', `/notifications?recipient=${world.technician.id}`, other).items, []);
    const items = call(world.app, 'GET', `/notifications?recipient=${world.technician.id}`, portal).items;
    assert.deepEqual(items.map((item: any) => item.recipientId), [world.customer.id]);
  });

  it('is limited to roles with notification access', () => {
    for (const actor of ['admin', 'dispatch', portal]) expectStatus(world.app, 200, 'GET', '/notifications', actor);
    for (const actor of ['billing', 'warehouse', world.technician.id]) {
      assert.equal(expectStatus(world.app, 403, 'GET', '/notifications', actor).error.code, 'forbidden');
    }
  });
});

describe('GET /notifications/templates', () => {
  it('lists the registered template keys for administrators', () => {
    const items = call(world.app, 'GET', '/notifications/templates', 'admin').items;
    assert.equal(items.length, 14);
    assert.deepEqual(items, [...items].sort());
    assert.ok(items.includes('work-completed'));
  });

  it('is reserved for administrators', () => {
    for (const actor of ['dispatch', 'billing', portal]) expectStatus(world.app, 403, 'GET', '/notifications/templates', actor);
  });
});

describe('PUT /preferences/:recipient', () => {
  it('stores the preference of a customer', () => {
    const preference = expectStatus(world.app, 200, 'PUT', `/preferences/${world.customer.id}`, portal, {
      channels: ['email', 'email', 'sms'],
      optOut: ['work-started'],
    });
    assert.deepEqual(preference, { id: world.customer.id, channels: ['email', 'sms'], optOut: ['work-started'] });
  });

  it('defaults the opt-out list to empty and replaces earlier choices', () => {
    call(world.app, 'PUT', `/preferences/${world.customer.id}`, portal, { channels: ['sms'], optOut: ['no-access'] });
    const preference = call(world.app, 'PUT', `/preferences/${world.customer.id}`, portal, { channels: ['email'] });
    assert.deepEqual(preference.optOut, []);
    assert.deepEqual(preference.channels, ['email']);
  });

  it('lets office staff change any recipient', () => {
    for (const actor of ['admin', 'dispatch']) {
      expectStatus(world.app, 200, 'PUT', `/preferences/${world.customer.id}`, actor, { channels: ['portal'] });
    }
  });

  it('keeps customers to their own account', () => {
    const body = expectStatus(world.app, 403, 'PUT', `/preferences/${world.customer.id}`, stranger(), { channels: ['sms'] });
    assert.equal(body.error.code, 'forbidden');
    assert.match(body.error.message, /change preferences of /);
  });

  it('is closed to roles without notification access', () => {
    for (const actor of ['billing', 'warehouse', world.technician.id]) {
      expectStatus(world.app, 403, 'PUT', `/preferences/${world.customer.id}`, actor, { channels: ['sms'] });
    }
  });

  it('validates the payload', () => {
    const invalid: unknown[] = [{}, { channels: [] }, { channels: ['fax'] }, { channels: ['sms', 'email', 'portal', 'sms'] }, { channels: ['sms'], optOut: 'all' }];
    for (const payload of invalid) expectStatus(world.app, 422, 'PUT', `/preferences/${world.customer.id}`, portal, payload);
  });

  it('changes how later messages are delivered', () => {
    call(world.app, 'PUT', `/preferences/${world.customer.id}`, portal, { channels: ['email'] });
    assignedJob();
    assert.deepEqual(call(world.app, 'GET', '/notifications', portal).items, []);
  });
});

describe('POST /notifications/flush', () => {
  it('sends the queued messages', () => {
    assignedJob();
    assert.deepEqual(call(world.app, 'POST', '/notifications/flush', 'admin'), { sent: 2 });
    assert.deepEqual(call(world.app, 'POST', '/notifications/flush', 'admin'), { sent: 0 });
    const items = call(world.app, 'GET', `/notifications?recipient=${world.technician.id}`, 'admin').items;
    assert.deepEqual(items.map((item: any) => item.status), ['sent']);
  });

  it('is reserved for administrators', () => {
    for (const actor of ['dispatch', 'billing', 'warehouse', portal]) expectStatus(world.app, 403, 'POST', '/notifications/flush', actor);
  });
});

describe('GET /jobs/:id/notifications', () => {
  it('lists the messages sent for one work order', () => {
    const job = assignedJob();
    const other = openWorkOrder(world.app, world.customer.id, world.site.id);
    call(world.app, 'POST', `${JOBS}/${other.id}/assign`, 'dispatch', { technicianId: world.technician.id });
    const items = call(world.app, 'GET', `${JOBS}/${job.id}/notifications`, 'dispatch').items;
    assert.deepEqual(items.map((item: any) => item.templateKey), ['technician-assigned', 'work-started']);
    assert.ok(items.every((item: any) => item.jobId === job.id));
  });

  it('follows work order visibility', () => {
    const job = assignedJob();
    const url = `${JOBS}/${job.id}/notifications`;
    expectStatus(world.app, 200, 'GET', url, portal);
    expectStatus(world.app, 200, 'GET', url, world.technician.id);
    expectStatus(world.app, 404, 'GET', url, stranger());
    expectStatus(world.app, 404, 'GET', url, seedTechnician(world.app, { name: 'Morgan Reyes' }).id);
    expectStatus(world.app, 404, 'GET', `${JOBS}/WO-565656/notifications`, 'admin');
  });
});
'''

FILES = {
    "tests/notifications/quiet-hours.test.ts": Spec(
        {"src/app.ts": ["createApp"], f"{NOTIFY}/quiet-hours.ts": ["inQuietHours", "sendAfter"], WORLD: ["at", "MONDAY"]},
        QUIET),
    "tests/notifications/preferences.test.ts": Spec(
        {"src/app.ts": ["createApp"], f"{NOTIFY}/preferences.ts": ["chooseChannel", "preferenceFor", "setPreference"],
         NOTE_REPO: ["preferences"], "src/shared/errors.ts": ["DomainError"], "src/context.ts": ["type AppContext"]},
        PREFERENCES, node=WITH_SETUP),
    "tests/notifications/templates.test.ts": Spec(
        {f"{NOTIFY}/templates.ts": ["BUILT_IN_TEMPLATES", "createTemplateRegistry"],
         "src/shared/text.ts": ["placeholdersOf", "renderTemplate"]}, TEMPLATES),
    "tests/notifications/notify.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY"], f"{NOTIFY}/notify.ts": ["flushOutbox", "notify"],
         f"{NOTIFY}/preferences.ts": ["setPreference"], NOTE_REPO: ["notificationsFor", "notificationsForJob"]},
        NOTIFY_BODY),
    "tests/notifications/subscriptions.test.ts": scene_spec(
        {WORLD: ["at", "MONDAY", "seedContract"], NOTE_REPO: ["notificationsFor"],
         f"{NOTIFY}/preferences.ts": ["setPreference"], "src/scheduling/service.ts": [
             "bookAppointment", "recordNoAccess", "rescheduleAppointment"],
         "src/sla/monitor.ts": ["scanForWarnings"], "src/inventory/catalog.ts": ["addPart"],
         "src/inventory/stock.ts": ["adjustCount"]},
        SUBSCRIPTIONS, jobs=["holdJob", "reopenJob"]),
    "tests/api/notifications.test.ts": Spec(
        {WORLD: ["call", "createWorld", "expectStatus", "JOBS", "openWorkOrder", "seedCustomer", "seedTechnician",
                 "type World"]}, API, node=WITH_SETUP),
}


def generate(ctx):
    register(ctx, FILES)
