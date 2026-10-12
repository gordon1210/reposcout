"""Quotes, customer and staff notifications."""

from .core import imports, render, rounding, rounding_names


def quote_model(ctx):
    return render(ctx, '''
«imports»

export type QuoteStatus = 'draft' | 'sent' | 'accepted' | 'declined' | 'expired';

export interface QuoteLine {
  description: string;
  quantity: number;
  unitCents: Cents;
  amountCents: Cents;
}

export interface Quote {
  id: string;
  number: string;
  customerId: string;
  siteId: string;
  workType: string;
  quantity?: number;
  status: QuoteStatus;
  lines: QuoteLine[];
  estimatedMinutes: number;
  crew: number;
  totalCents: Cents;
  currency: string;
  validUntil: CalendarDate;
  createdAt: Timestamp;
  acceptedJobId?: string;
  declineReason?: string;
}
''', imports=imports(ctx, "src/quotes/model.ts", {
        "src/shared/money.ts": ["type Cents"],
        "src/shared/time.ts": ["type CalendarDate", "type Timestamp"],
    }))


def quote_pricing(ctx):
    return render(ctx, '''
«imports»

export interface PricedQuote {
  lines: QuoteLine[];
  estimatedMinutes: number;
  crew: number;
  totalCents: Cents;
  currency: string;
  notes: string[];
}

/**
 * Prices a job from the work type estimate: labour for every crew member in quarter hours,
 * callout, parts at their invoice price and tax. The total is quoted in whole currency units.
 */
export function priceQuote(ctx: AppContext, customer: Customer, handler: WorkTypeHandler, input: EstimateInput): PricedQuote {
  const tariff = tariffFor(customer.billingRegion);
  const estimate = handler.estimate(input);
  const minutes = «minutes»;
  const rate = tariff.laborRates[handler.laborCategory];
  const lines: QuoteLine[] = [
    { description: 'Callout', quantity: 1, unitCents: tariff.calloutFeeCents, amountCents: tariff.calloutFeeCents },
    {
      description: `${handler.label} labour, ${estimate.crew} × ${minutes} min`,
      quantity: (minutes / 60) * estimate.crew,
      unitCents: rate,
      amountCents: multiplyCents(rate, (minutes / 60) * estimate.crew),
    },
  ];
  for (const line of estimate.parts) {
    const part = parts(ctx).get(line.sku);
    if (!part) continue;
    const unit = partUnitPrice(part, tariff);
    lines.push({ description: part.name, quantity: line.quantity, unitCents: unit, amountCents: unit * line.quantity });
  }
  const net = sumCents(lines.map((line) => line.amountCents));
  const gross = customer.taxExempt ? net : net + applyBasisPoints(net, tariff.taxRateBp);
  return {
    lines,
    estimatedMinutes: minutes,
    crew: estimate.crew,
    totalCents: «total»,
    currency: tariff.currency,
    notes: estimate.notes,
  };
}
''', minutes=rounding(ctx, "estimate.minutes", 15, "up"), total=rounding(ctx, "gross", 100, "nearest-explicit"),
                  imports=imports(ctx, "src/quotes/pricing.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/adapters/repositories/parts.ts": ["parts"],
                      "src/billing/parts.ts": ["partUnitPrice"],
                      "src/billing/tariffs/index.ts": ["tariffFor"],
                      "src/customers/model.ts": ["type Customer"],
                      "src/shared/money.ts": ["applyBasisPoints", "multiplyCents", "sumCents", "type Cents"],
                      "src/shared/rounding.ts": rounding_names(ctx, "up", "nearest"),
                      "src/worktypes/types.ts": ["type EstimateInput", "type WorkTypeHandler"],
                      "src/quotes/model.ts": ["type QuoteLine"],
                  }))


def quote_service(ctx):
    return render(ctx, '''
«imports»

const quoteInput = object({
  customerId: string({ min: 1 }),
  siteId: string({ min: 1 }),
  workType: string({ min: 1 }),
  quantity: optional(number({ min: 0, max: 10_000 })),
  validDays: withDefault(integer({ min: 1, max: 90 }), 30),
});

export function createQuote(ctx: AppContext, actorId: string, input: unknown): Quote {
  const data = parseInput(quoteInput, input);
  const customer = customers(ctx).require(data.customerId, 'customer');
  const site = requireSiteOf(ctx, customer.id, data.siteId);
  const handler = ctx.workTypes.lookup(data.workType);
  if (!handler) throw new DomainError('unknown-work-type', `Unknown work type ${data.workType}`);
  const priced = priceQuote(ctx, customer, handler, { hazards: site.hazards, quantity: data.quantity });
  const now = ctx.clock.now();
  const quote: Quote = {
    id: ctx.ids.next('QUO'),
    number: `Q-${now.slice(0, 4)}-${ctx.ids.next('QNO').slice(4)}`,
    customerId: customer.id,
    siteId: site.id,
    workType: handler.key,
    quantity: data.quantity,
    status: 'draft',
    lines: priced.lines,
    estimatedMinutes: priced.estimatedMinutes,
    crew: priced.crew,
    totalCents: priced.totalCents,
    currency: priced.currency,
    validUntil: addDays(dateOf(now), data.validDays),
    createdAt: now,
  };
  quotes(ctx).insert(quote);
  recordAudit(ctx, actorId, 'quote.created', quote.id, { totalCents: quote.totalCents });
  return quote;
}

export function sendQuote(ctx: AppContext, actorId: string, quoteId: string): Quote {
  const quote = quotes(ctx).require(quoteId, 'quote');
  if (quote.status !== 'draft') throw new ConflictError('not-draft', `Quote ${quote.number} is ${quote.status}`);
  const updated = quotes(ctx).update(quoteId, { status: 'sent' });
  recordAudit(ctx, actorId, 'quote.sent', quoteId);
  return updated;
}

/** Accepting a quote opens the work order; expired quotes must be re-quoted. */
export function acceptQuote(ctx: AppContext, actorId: string, quoteId: string): { quote: Quote; jobId: string } {
  const quote = quotes(ctx).require(quoteId, 'quote');
  if (quote.status !== 'sent') throw new ConflictError('not-sent', `Quote ${quote.number} is ${quote.status}`);
  if (dateOf(ctx.clock.now()) > quote.validUntil) {
    quotes(ctx).update(quoteId, { status: 'expired' });
    throw new ConflictError('quote-expired', `Quote ${quote.number} expired on ${quote.validUntil}`);
  }
  const job = «openJob»(ctx, actorId, {
    customerId: quote.customerId,
    siteId: quote.siteId,
    workType: quote.workType,
    summary: `Quoted work ${quote.number}`,
    description: quote.lines.map((line) => line.description).join('; '),
    source: 'portal',
    tags: ['quoted'],
  });
  const updated = quotes(ctx).update(quoteId, { status: 'accepted', acceptedJobId: job.id });
  recordAudit(ctx, actorId, 'quote.accepted', quoteId, { jobId: job.id });
  return { quote: updated, jobId: job.id };
}

export function declineQuote(ctx: AppContext, actorId: string, quoteId: string, reason: string): Quote {
  const quote = quotes(ctx).require(quoteId, 'quote');
  if (quote.status !== 'sent' && quote.status !== 'draft') {
    throw new ConflictError('not-open', `Quote ${quote.number} is ${quote.status}`);
  }
  const updated = quotes(ctx).update(quoteId, { status: 'declined', declineReason: reason });
  recordAudit(ctx, actorId, 'quote.declined', quoteId, { reason });
  return updated;
}

export function expireQuotes(ctx: AppContext, today: CalendarDate): string[] {
  const expired = quotes(ctx).find((quote) => (quote.status === 'sent' || quote.status === 'draft') && quote.validUntil < today);
  for (const quote of expired) quotes(ctx).update(quote.id, { status: 'expired' });
  return expired.map((quote) => quote.id);
}
''', imports=imports(ctx, "src/quotes/service.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/adapters/repositories/quotes.ts": ["quotes"],
        "src/adapters/repositories/sites.ts": ["requireSiteOf"],
        "src/adapters/store/audit.ts": ["recordAudit"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/time.ts": ["addDays", "dateOf", "type CalendarDate"],
        "src/shared/validate.ts": ["integer", "number", "object", "optional", "parseInput", "string", "withDefault"],
        "src/quotes/model.ts": ["type Quote"],
        "src/quotes/pricing.ts": ["priceQuote"],
    }, jobs=("openJob",)))


def notification_types(ctx):
    return render(ctx, '''
«imports»

export type Channel = 'email' | 'sms' | 'portal';
export type Audience = 'customer' | 'technician' | 'staff';

export interface NotificationTemplate {
  key: string;
  audience: Audience;
  channels: readonly Channel[];
  subject: string;
  body: string;
  /** Urgent messages ignore quiet hours. */
  urgent: boolean;
}

export interface Notification {
  id: string;
  recipientId: string;
  channel: Channel;
  templateKey: string;
  subject: string;
  body: string;
  jobId?: string;
  status: 'queued' | 'deferred' | 'sent' | 'suppressed';
  createdAt: Timestamp;
  sendAfter: Timestamp;
}

export interface ContactPreference {
  id: string;
  channels: Channel[];
  optOut: string[];
}
''', imports=imports(ctx, "src/notifications/types.ts", {"src/shared/time.ts": ["type Timestamp"]}))


TEMPLATES = [
    ("appointment-confirmed", "customer", ["email", "sms"], False, "Your visit on {{date}}",
     "Hello {{customerName}}, {{technicianName}} will visit {{siteLabel}} on {{date}} {{window}}. Reference {{number}}."),
    ("appointment-rescheduled", "customer", ["email", "sms"], False, "Your visit has moved",
     "Hello {{customerName}}, your visit for {{number}} is now on {{date}} {{window}}."),
    ("appointment-cancelled", "customer", ["email"], False, "Visit cancelled",
     "Hello {{customerName}}, the visit for {{number}} has been cancelled: {{reason}}."),
    ("technician-assigned", "technician", ["sms", "portal"], False, "New work order {{number}}",
     "{{number}} at {{siteLabel}}: {{summary}}. Priority {{priority}}."),
    ("work-started", "customer", ["sms"], False, "We have arrived",
     "{{technicianName}} has started work on {{number}}."),
    ("parts-delay", "customer", ["email"], False, "Waiting for parts",
     "Hello {{customerName}}, work order {{number}} is waiting for parts. We will rebook as soon as they arrive."),
    ("work-completed", "customer", ["email"], False, "Work completed",
     "Hello {{customerName}}, {{number}} was completed: {{resolution}}."),
    ("follow-up-required", "staff", ["portal"], False, "Follow-up needed for {{number}}",
     "{{technicianName}} flagged {{number}} for a follow-up visit."),
    ("emergency-opened", "staff", ["sms", "portal"], True, "EMERGENCY {{number}}",
     "Emergency work order {{number}} at {{siteLabel}}: {{summary}}."),
    ("sla-warning", "staff", ["portal"], True, "SLA at risk: {{number}}",
     "{{number}} has used {{percent}}% of its resolution time."),
    ("sla-breached", "staff", ["email", "portal"], True, "SLA breached: {{number}}",
     "{{number}} missed its resolution target by {{breachMinutes}} minutes."),
    ("low-stock", "staff", ["portal"], False, "Low stock: {{sku}}",
     "{{sku}} is down to {{available}} at {{locationId}}."),
    ("no-access", "customer", ["email", "sms"], False, "We could not get in",
     "Hello {{customerName}}, our technician could not access {{siteLabel}} for {{number}}. Please contact us to rebook."),
    ("invoice-issued", "customer", ["email"], False, "Invoice {{invoiceNumber}}",
     "Hello {{customerName}}, invoice {{invoiceNumber}} for {{amount}} is due on {{dueOn}}."),
]


def notification_templates(ctx):
    entries = []
    for key, audience, channels, urgent, subject, body in TEMPLATES:
        channel_list = ", ".join(f"'{channel}'" for channel in channels)
        entries.append(f'''  {{
    key: '{key}',
    audience: '{audience}',
    channels: [{channel_list}],
    urgent: {str(urgent).lower()},
    subject: '{subject}',
    body: '{body}',
  }},
''')
    return render(ctx, '''
«imports»

export const BUILT_IN_TEMPLATES: readonly NotificationTemplate[] = [
«entries»];

export function createTemplateRegistry(): Registry<NotificationTemplate> {
  const registry = createRegistry<NotificationTemplate>('notification templates');
  for (const template of BUILT_IN_TEMPLATES) {
    const missing = placeholdersOf(template.subject).filter((name) => name.length === 0);
    if (missing.length > 0) throw new Error(`Template ${template.key} has an empty placeholder`);
    registry.register(template.key, template);
  }
  return registry;
}
''', entries="".join(entries), imports=imports(ctx, "src/notifications/templates.ts", {
        "src/shared/registry.ts": ["createRegistry", "type Registry"],
        "src/shared/text.ts": ["placeholdersOf"],
        "src/notifications/types.ts": ["type NotificationTemplate"],
    }))


def quiet_hours(ctx):
    inside = '''  if (start > end) return minute >= start || minute < end;
  return minute >= start && minute < end;''' if not ctx.refactor else '''  return start > end ? minute >= start || minute < end : minute >= start && minute < end;'''
    return render(ctx, '''
«imports»

export function inQuietHours(ctx: AppContext, at: Timestamp): boolean {
  const { quietStartMinute: start, quietEndMinute: end } = ctx.config.notifications;
  const minute = minuteOfDay(at);
«inside»
}

/**
 * When a non-urgent SMS may be sent: immediately outside quiet hours, otherwise at the end of
 * the quiet period on the next quarter hour. Email and portal messages are never deferred.
 */
export function sendAfter(ctx: AppContext, at: Timestamp, channel: Channel, urgent: boolean): Timestamp {
  if (urgent || channel !== 'sms' || !inQuietHours(ctx, at)) return at;
  const { quietEndMinute } = ctx.config.notifications;
  const minute = minuteOfDay(at);
  const date = minute >= quietEndMinute ? addDays(dateOf(at), 1) : dateOf(at);
  return atMinute(date, «quarter»);
}
''', inside=inside, quarter=rounding(ctx, "quietEndMinute", 15, "up"),
                  imports=imports(ctx, "src/notifications/quiet-hours.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/shared/rounding.ts": rounding_names(ctx, "up"),
                      "src/shared/time.ts": ["addDays", "atMinute", "dateOf", "minuteOfDay", "type Timestamp"],
                      "src/notifications/types.ts": ["type Channel"],
                  }))


def preferences_module(ctx):
    return render(ctx, '''
«imports»

export function preferenceFor(ctx: AppContext, recipientId: string): ContactPreference | undefined {
  return preferences(ctx).get(recipientId);
}

export function setPreference(ctx: AppContext, recipientId: string, channels: Channel[], optOut: string[] = []): ContactPreference {
  if (channels.length === 0) throw new DomainError('no-channels', 'At least one channel is required');
  const preference: ContactPreference = { id: recipientId, channels: [...new Set(channels)], optOut: [...new Set(optOut)] };
  return preferences(ctx).get(recipientId) ? preferences(ctx).update(recipientId, preference) : preferences(ctx).insert(preference);
}

/** The first template channel the recipient accepts and the organisation has enabled. */
export function chooseChannel(ctx: AppContext, recipientId: string, offered: readonly Channel[]): Channel | undefined {
  const preference = preferenceFor(ctx, recipientId);
  const enabled = ctx.config.notifications.channels as readonly Channel[];
  const accepted = preference?.channels ?? offered;
  return offered.find((channel) => (enabled.includes(channel) || channel === 'portal') && accepted.includes(channel));
}
''', imports=imports(ctx, "src/notifications/preferences.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/notifications.ts": ["preferences"],
        "src/shared/errors.ts": ["DomainError"],
        "src/notifications/types.ts": ["type Channel", "type ContactPreference"],
    }))


def notify_module(ctx):
    return render(ctx, '''
«imports»

export type TemplateValues = Record<string, string | number | undefined>;

/**
 * Renders a template for one recipient. Opted-out templates are stored as suppressed so the
 * history shows that a message was intended.
 */
export function notify(ctx: AppContext, templateKey: string, recipientId: string, values: TemplateValues, jobId?: string): Notification | undefined {
  const template = ctx.templates.lookup(templateKey);
  if (!template) {
    ctx.log.warn('unknown notification template', { templateKey });
    return undefined;
  }
  const channel = chooseChannel(ctx, recipientId, template.channels);
  if (!channel) return undefined;
  const now = ctx.clock.now();
  const optedOut = preferenceFor(ctx, recipientId)?.optOut.includes(templateKey) ?? false;
  const after = sendAfter(ctx, now, channel, template.urgent);
  const notification: Notification = {
    id: ctx.ids.next('NTF'),
    recipientId,
    channel,
    templateKey,
    subject: renderTemplate(template.subject, values),
    body: renderTemplate(template.body, { ...values, sender: ctx.config.notifications.senderName }),
    jobId,
    status: optedOut ? 'suppressed' : after === now ? 'queued' : 'deferred',
    createdAt: now,
    sendAfter: after,
  };
  return notifications(ctx).insert(notification);
}

/** Marks queued and due deferred messages as sent; returns how many were sent. */
export function flushOutbox(ctx: AppContext): number {
  const now = ctx.clock.now();
  const due = notifications(ctx).find(
    (notification) => (notification.status === 'queued' || notification.status === 'deferred') && notification.sendAfter <= now,
  );
  for (const notification of due) notifications(ctx).update(notification.id, { status: 'sent' });
  return due.length;
}
''', imports=imports(ctx, "src/notifications/notify.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/notifications.ts": ["notifications"],
        "src/shared/text.ts": ["renderTemplate"],
        "src/notifications/preferences.ts": ["chooseChannel", "preferenceFor"],
        "src/notifications/quiet-hours.ts": ["sendAfter"],
        "src/notifications/types.ts": ["type Notification"],
    }))


def notification_subscriptions(ctx):
    return render(ctx, '''
«imports»

function jobValues(ctx: AppContext, jobId: string): { job: «Job»; values: TemplateValues } | undefined {
  const job = «getJob»(ctx, jobId);
  if (!job) return undefined;
  const customer = customers(ctx).get(job.customerId);
  const site = sites(ctx).get(job.siteId);
  const technician = job.technicianId ? technicians(ctx).get(job.technicianId) : undefined;
  return {
    job,
    values: {
      number: job.number,
      summary: job.summary,
      priority: job.priority,
      customerName: customer?.name,
      siteLabel: site?.label,
      technicianName: technician?.name ?? 'Our technician',
    },
  };
}

/** Staff alerts go to the dispatch desk's shared inbox. */
const DISPATCH_DESK = 'dispatch';

export function registerNotificationSubscriptions(ctx: AppContext): void {
  ctx.bus.on(AppointmentEvents.booked, (event) => {
    const context = jobValues(ctx, String(event.jobId));
    if (!context) return;
    const window = { windowStart: String(event.windowStart), windowEnd: String(event.windowEnd) };
    notify(ctx, 'appointment-confirmed', context.job.customerId, {
      ...context.values,
      date: dateOf(window.windowStart),
      window: describeWindow(window),
    }, context.job.id);
  });
  ctx.bus.on(AppointmentEvents.rescheduled, (event) => {
    const context = jobValues(ctx, String(event.jobId));
    if (!context || !context.job.scheduledStart) return;
    notify(ctx, 'appointment-rescheduled', context.job.customerId, {
      ...context.values,
      date: dateOf(context.job.scheduledStart),
      window: describeWindow(arrivalWindow(ctx, context.job.scheduledStart)),
    }, context.job.id);
  });
  ctx.bus.on(AppointmentEvents.noAccess, (event) => {
    const context = jobValues(ctx, String(event.jobId));
    if (context) notify(ctx, 'no-access', context.job.customerId, context.values, context.job.id);
  });
  ctx.bus.on(«JobEvents».opened, (event) => {
    if (event.priority !== 'emergency') return;
    const context = jobValues(ctx, String(event.jobId));
    if (context) notify(ctx, 'emergency-opened', DISPATCH_DESK, context.values, context.job.id);
  });
  ctx.bus.on(«JobEvents».assigned, (event) => {
    const context = jobValues(ctx, String(event.jobId));
    if (context && typeof event.technicianId === 'string') {
      notify(ctx, 'technician-assigned', event.technicianId, context.values, context.job.id);
    }
  });
  ctx.bus.on(«JobEvents».started, (event) => {
    if (event.firstStart !== true) return;
    const context = jobValues(ctx, String(event.jobId));
    if (context) notify(ctx, 'work-started', context.job.customerId, context.values, context.job.id);
  });
  ctx.bus.on(«JobEvents».held, (event) => {
    if (event.reason !== 'awaiting_parts') return;
    const context = jobValues(ctx, String(event.jobId));
    if (context) notify(ctx, 'parts-delay', context.job.customerId, context.values, context.job.id);
  });
  ctx.bus.on(«JobEvents».completed, (event) => {
    const context = jobValues(ctx, String(event.jobId));
    if (!context) return;
    notify(ctx, 'work-completed', context.job.customerId, {
      ...context.values,
      resolution: context.job.completion?.resolution,
    }, context.job.id);
    if (event.followUpRequired === true) notify(ctx, 'follow-up-required', DISPATCH_DESK, context.values, context.job.id);
  });
  ctx.bus.on(SlaEvents.warning, (event) => {
    const context = jobValues(ctx, String(event.jobId));
    if (context) {
      notify(ctx, 'sla-warning', DISPATCH_DESK, { ...context.values, percent: Math.round(Number(event.usedRatio) * 100) }, context.job.id);
    }
  });
  ctx.bus.on(SlaEvents.breached, (event) => {
    const context = jobValues(ctx, String(event.jobId));
    if (context) {
      notify(ctx, 'sla-breached', DISPATCH_DESK, { ...context.values, breachMinutes: Number(event.breachMinutes) }, context.job.id);
    }
  });
  ctx.bus.on(InventoryEvents.lowStock, (event) => {
    notify(ctx, 'low-stock', 'warehouse', {
      sku: String(event.sku),
      available: Number(event.available),
      locationId: String(event.locationId),
    });
  });
}
''', imports=imports(ctx, "src/notifications/subscriptions.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/adapters/repositories/sites.ts": ["sites"],
        "src/adapters/repositories/technicians.ts": ["technicians"],
        "src/inventory/events.ts": ["InventoryEvents"],
        "src/scheduling/model.ts": ["AppointmentEvents"],
        "src/scheduling/windows.ts": ["arrivalWindow", "describeWindow"],
        "src/shared/time.ts": ["dateOf"],
        "src/sla/model.ts": ["SlaEvents"],
        "src/notifications/notify.ts": ["notify", "type TemplateValues"],
    }, jobs=("type Job", "getJob", "JobEvents")))


def generate(ctx):
    ctx.add("src/quotes/model.ts", quote_model(ctx))
    ctx.add("src/quotes/pricing.ts", quote_pricing(ctx))
    ctx.add("src/quotes/service.ts", quote_service(ctx))
    ctx.add("src/notifications/types.ts", notification_types(ctx))
    ctx.add("src/notifications/templates.ts", notification_templates(ctx))
    ctx.add("src/notifications/quiet-hours.ts", quiet_hours(ctx))
    ctx.add("src/notifications/preferences.ts", preferences_module(ctx))
    ctx.add("src/notifications/notify.ts", notify_module(ctx))
    ctx.add("src/notifications/subscriptions.ts", notification_subscriptions(ctx))
