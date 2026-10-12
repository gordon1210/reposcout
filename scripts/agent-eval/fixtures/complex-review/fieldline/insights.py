"""Reporting, exports and outbound integrations."""

from .core import imports, render, rounding, rounding_names


def kpis(ctx):
    return render(ctx, '''
«imports»

export interface Period {
  from: Timestamp;
  to: Timestamp;
}

function completedIn(ctx: AppContext, period: Period): «Job»[] {
  return «listJobs»(ctx, { status: ['completed', 'closed'] }).filter(
    (job) => job.completion !== undefined && job.completion.completedAt >= period.from && job.completion.completedAt < period.to,
  );
}

/**
 * Share of completed work orders that needed no follow-up and were not reopened, in percent
 * with one decimal.
 */
export function firstTimeFixRate(ctx: AppContext, period: Period): number | undefined {
  const done = completedIn(ctx, period);
  if (done.length === 0) return undefined;
  const fixed = done.filter((job) => job.reopenCount === 0 && !job.completion?.followUpRequired).length;
  return «ftf»;
}

/** Mean hours from request to completion, excluding hold time, to one decimal. */
export function meanHoursToComplete(ctx: AppContext, period: Period): number | undefined {
  const done = completedIn(ctx, period);
  if (done.length === 0) return undefined;
  const minutes = done.reduce((sum, job) => sum + «jobAge»(job, period.to), 0) / done.length;
  return «hours»;
}

export function slaCompliancePercent(ctx: AppContext, period: Period): number | undefined {
  const rate = complianceRate(ctx, period.from, period.to);
  return rate === undefined ? undefined : «sla»;
}

export interface KpiSnapshot {
  period: Period;
  completed: number;
  firstTimeFixRate?: number;
  meanHoursToComplete?: number;
  slaCompliancePercent?: number;
  openBacklog: number;
}

export function kpiSnapshot(ctx: AppContext, period: Period): KpiSnapshot {
  return {
    period,
    completed: completedIn(ctx, period).length,
    firstTimeFixRate: firstTimeFixRate(ctx, period),
    meanHoursToComplete: meanHoursToComplete(ctx, period),
    slaCompliancePercent: slaCompliancePercent(ctx, period),
    openBacklog: «listJobs»(ctx, { status: [...OPEN_STATUSES] }).length,
  };
}
''', ftf=rounding(ctx, "(fixed / done.length) * 100", 0.1, "nearest"),
                  hours=rounding(ctx, "minutes / 60", 0.1, "nearest"),
                  sla=rounding(ctx, "rate * 100", 0.1, "nearest-explicit"),
                  imports=imports(ctx, "src/reports/kpis.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/shared/rounding.ts": rounding_names(ctx, "nearest"),
                      "src/shared/time.ts": ["type Timestamp"],
                      "src/sla/monitor.ts": ["complianceRate"],
                  }, jobs=("type Job", "listJobs", "jobAge", "OPEN_STATUSES")))


def utilisation(ctx):
    return render(ctx, '''
«imports»

export interface UtilisationRow {
  technicianId: string;
  name: string;
  laborHours: number;
  travelHours: number;
  availableHours: number;
  utilisationPercent: number;
}

/**
 * Billable utilisation per technician: labour and travel time against the working time of the
 * standard calendar in the period, in whole percent.
 */
export function utilisationReport(ctx: AppContext, from: CalendarDate, to: CalendarDate): UtilisationRow[] {
  const calendar = businessCalendar(ctx);
  const availableMinutes = workingMinutesBetween(calendar, atMinute(from, 0), atMinute(to, 0));
  return activeTechnicians(ctx)
    .map((technician) => {
      const entries = entriesForTechnician(ctx, technician.id, atMinute(from, 0), atMinute(to, 0)).filter(
        (entry) => entry.status === 'approved' || entry.status === 'submitted',
      );
      const labor = entries.filter((entry) => entry.kind === 'labor').reduce((sum, entry) => sum + entry.minutes, 0);
      const travel = entries.filter((entry) => entry.kind === 'travel').reduce((sum, entry) => sum + entry.minutes, 0);
      const ratio = availableMinutes === 0 ? 0 : ((labor + travel) / availableMinutes) * 100;
      return {
        technicianId: technician.id,
        name: technician.name,
        laborHours: Math.round((labor / 60) * 10) / 10,
        travelHours: Math.round((travel / 60) * 10) / 10,
        availableHours: Math.round((availableMinutes / 60) * 10) / 10,
        utilisationPercent: «percent»,
      };
    })
    .sort((left, right) => right.utilisationPercent - left.utilisationPercent || left.name.localeCompare(right.name));
}
''', percent=rounding(ctx, "ratio", 1, "nearest-explicit"), imports=imports(ctx, "src/reports/utilisation.ts", {
        "src/context.ts": ["businessCalendar", "type AppContext"],
        "src/adapters/repositories/technicians.ts": ["activeTechnicians"],
        "src/adapters/repositories/time-entries.ts": ["entriesForTechnician"],
        "src/shared/calendar.ts": ["workingMinutesBetween"],
        "src/shared/rounding.ts": rounding_names(ctx, "nearest"),
        "src/shared/time.ts": ["atMinute", "type CalendarDate"],
    }))


def backlog(ctx):
    return render(ctx, '''
«imports»

export interface BacklogBucket {
  ageDays: number;
  count: number;
  emergencies: number;
}

/** Open work grouped by whole days of age (time on hold excluded), oldest bucket last. */
export function backlogAging(ctx: AppContext): BacklogBucket[] {
  const now = ctx.clock.now();
  const buckets = new Map<number, BacklogBucket>();
  for (const job of «listJobs»(ctx, { status: [...OPEN_STATUSES] })) {
    const ageDays = «days» / 1440;
    const bucket = buckets.get(ageDays) ?? { ageDays, count: 0, emergencies: 0 };
    bucket.count += 1;
    if (job.priority === 'emergency') bucket.emergencies += 1;
    buckets.set(ageDays, bucket);
  }
  return [...buckets.values()].sort((left, right) => left.ageDays - right.ageDays);
}

/** Open work per region and status, for the operations dashboard. */
export function backlogByRegion(ctx: AppContext): Record<string, Record<string, number>> {
  const result: Record<string, Record<string, number>> = {};
  for (const job of «listJobs»(ctx, { status: [...OPEN_STATUSES] })) {
    const region = customers(ctx).get(job.customerId)?.billingRegion ?? 'unknown';
    const row = (result[region] ??= {});
    row[job.status] = (row[job.status] ?? 0) + 1;
  }
  return result;
}
''', days=rounding(ctx, f"{ctx.sym('jobAge')}(job, now)", 1440, "down"), imports=imports(ctx, "src/reports/backlog.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/shared/rounding.ts": rounding_names(ctx, "down"),
    }, jobs=("listJobs", "jobAge", "OPEN_STATUSES")))


def revenue(ctx):
    loop = '''  for (const invoice of issued) {
    const customer = customers(ctx).get(invoice.customerId);
    const region = customer?.billingRegion ?? 'unknown';
    const month = (invoice.issuedOn ?? '').slice(0, 7);
    const key = `${region}|${month}`;
    const row = rows.get(key) ?? { region, month, invoices: 0, netCents: 0, taxCents: 0 };
    row.invoices += 1;
    row.netCents += invoice.subtotalCents;
    row.taxCents += invoice.taxCents;
    rows.set(key, row);
  }''' if not ctx.refactor else '''  for (const invoice of issued) {
    const region = customers(ctx).get(invoice.customerId)?.billingRegion ?? 'unknown';
    const month = (invoice.issuedOn ?? '').slice(0, 7);
    const key = `${region}|${month}`;
    const row = rows.get(key) ?? { region, month, invoices: 0, netCents: 0, taxCents: 0 };
    rows.set(key, {
      ...row,
      invoices: row.invoices + 1,
      netCents: row.netCents + invoice.subtotalCents,
      taxCents: row.taxCents + invoice.taxCents,
    });
  }'''
    return render(ctx, '''
«imports»

export interface RevenueRow {
  region: string;
  month: string;
  invoices: number;
  netCents: Cents;
  taxCents: Cents;
}

/** Issued and paid invoices by billing region and month of issue; void invoices are excluded. */
export function revenueByRegion(ctx: AppContext, from: CalendarDate, to: CalendarDate): RevenueRow[] {
  const issued = invoices(ctx).find(
    (invoice) =>
      (invoice.status === 'issued' || invoice.status === 'paid') &&
      invoice.issuedOn !== undefined &&
      invoice.issuedOn >= from &&
      invoice.issuedOn < to,
  );
  const rows = new Map<string, RevenueRow>();
«loop»
  return [...rows.values()].sort((left, right) => left.month.localeCompare(right.month) || left.region.localeCompare(right.region));
}
''', loop=loop, imports=imports(ctx, "src/reports/revenue.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/adapters/repositories/invoices.ts": ["invoices"],
        "src/shared/money.ts": ["type Cents"],
        "src/shared/time.ts": ["type CalendarDate"],
    }))


def activity(ctx):
    prefix = "workorder." if ctx.split else "job."
    return render(ctx, '''
«imports»

export interface ActivitySummary {
  workOrderEvents: number;
  byType: Record<string, number>;
  lastEventAt?: string;
}

/** Counts of recent bus events, used by the operations dashboard's activity ticker. */
export function recentActivity(ctx: AppContext, limit = 200): ActivitySummary {
  const events = ctx.bus.recent(limit);
  const byType = countBy(events, (event) => event.type);
  return {
    workOrderEvents: events.filter((event) => event.type.startsWith('«prefix»')).length,
    byType,
    lastEventAt: events[events.length - 1]?.at,
  };
}
''', prefix=prefix, imports=imports(ctx, "src/reports/activity.ts", {
        "src/context.ts": ["type AppContext"],
        "src/shared/collections.ts": ["countBy"],
    }))


def csv_module(ctx):
    return '''
export type Row = Record<string, unknown>;

function cell(value: unknown): string {
  if (value === undefined || value === null) return '';
  const text = typeof value === 'object' ? JSON.stringify(value) : String(value);
  return /[",\\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** RFC 4180 CSV with a header row; columns default to the keys of the first row. */
export function toCsv(rows: readonly Row[], columns?: readonly string[]): string {
  const header = columns ?? (rows[0] ? Object.keys(rows[0]) : []);
  const lines = [header.map(cell).join(',')];
  for (const row of rows) lines.push(header.map((column) => cell(row[column])).join(','));
  return lines.join('\\n') + '\\n';
}
'''


def integration_types(ctx):
    return render(ctx, '''
«imports»

export interface ExportFormat {
  key: string;
  contentType: string;
  render(rows: readonly Row[]): string;
}

export interface Webhook {
  id: string;
  customerId?: string;
  url: string;
  topics: string[];
  secret: string;
  active: boolean;
  failures: number;
}

export interface Delivery {
  id: string;
  webhookId: string;
  topic: string;
  payload: string;
  signature: string;
  createdAt: Timestamp;
  status: 'pending' | 'delivered' | 'failed';
  attempts: number;
}
''', imports=imports(ctx, "src/integrations/types.ts", {
        "src/reports/csv.ts": ["type Row"],
        "src/shared/time.ts": ["type Timestamp"],
    }))


def export_formats(ctx):
    return render(ctx, '''
«imports»

const csv: ExportFormat = { key: 'csv', contentType: 'text/csv', render: (rows) => toCsv(rows) };

const json: ExportFormat = {
  key: 'json',
  contentType: 'application/json',
  render: (rows) => JSON.stringify(rows, null, 2) + '\\n',
};

const ndjson: ExportFormat = {
  key: 'ndjson',
  contentType: 'application/x-ndjson',
  render: (rows) => rows.map((row) => JSON.stringify(row)).join('\\n') + (rows.length > 0 ? '\\n' : ''),
};

const tsv: ExportFormat = {
  key: 'tsv',
  contentType: 'text/tab-separated-values',
  render(rows) {
    const columns = rows[0] ? Object.keys(rows[0]) : [];
    const clean = (value: unknown) => String(value ?? '').replace(/[\\t\\n]/g, ' ');
    return [columns.join('\\t'), ...rows.map((row) => columns.map((column) => clean(row[column])).join('\\t'))].join('\\n') + '\\n';
  },
};

export function createExportRegistry(): Registry<ExportFormat> {
  const registry = createRegistry<ExportFormat>('export formats', { normalizeKey: (key) => key.toLowerCase() });
  for (const format of [csv, json, ndjson, tsv]) registry.register(format.key, format);
  return registry;
}
''', imports=imports(ctx, "src/integrations/formats.ts", {
        "src/reports/csv.ts": ["toCsv"],
        "src/shared/registry.ts": ["createRegistry", "type Registry"],
        "src/integrations/types.ts": ["type ExportFormat"],
    }))


def webhooks(ctx):
    return render(ctx, '''
«imports»

/**
 * Public topic names for subscribers. They are part of the integration contract and stay stable
 * even when internal event names change.
 */
export const EXTERNAL_TOPICS: Readonly<Record<string, string>> = {
  [«JobEvents».opened]: 'job.created',
  [«JobEvents».scheduled]: 'job.scheduled',
  [«JobEvents».started]: 'job.started',
  [«JobEvents».completed]: 'job.completed',
  [«JobEvents».cancelled]: 'job.cancelled',
  [AppointmentEvents.booked]: 'appointment.booked',
  [SlaEvents.breached]: 'sla.breached',
};

export function registerWebhook(ctx: AppContext, actorId: string, url: string, topics: string[], customerId?: string): Webhook {
  if (!/^https:\\/\\/[^\\s]+$/.test(url)) throw new DomainError('invalid-url', 'Webhooks must use https');
  const known = new Set(Object.values(EXTERNAL_TOPICS));
  const unknown = topics.filter((topic) => !known.has(topic));
  if (unknown.length > 0) throw new DomainError('unknown-topic', `Unknown topics: ${unknown.join(', ')}`);
  const webhook: Webhook = {
    id: ctx.ids.next('HOOK'),
    customerId,
    url,
    topics: [...new Set(topics)].sort(),
    secret: createHash('sha256').update(`${url}|${ctx.clock.now()}|${ctx.ids.next('SECRET')}`).digest('hex').slice(0, 32),
    active: true,
    failures: 0,
  };
  ctx.store.collection<Webhook>('webhooks').insert(webhook);
  recordAudit(ctx, actorId, 'webhook.registered', webhook.id, { topics: webhook.topics });
  return webhook;
}

export function sign(secret: string, payload: string): string {
  return createHmac('sha256', secret).update(payload).digest('hex');
}

function deliver(ctx: AppContext, topic: string, data: Record<string, unknown>): number {
  const hooks = ctx.store.collection<Webhook>('webhooks').find(
    (hook) => hook.active && hook.topics.includes(topic) && (hook.customerId === undefined || hook.customerId === data.customerId),
  );
  for (const hook of hooks) {
    const payload = JSON.stringify({ topic, data });
    const delivery: Delivery = {
      id: ctx.ids.next('DLV'),
      webhookId: hook.id,
      topic,
      payload,
      signature: sign(hook.secret, payload),
      createdAt: ctx.clock.now(),
      status: 'pending',
      attempts: 0,
    };
    ctx.store.collection<Delivery>('deliveries').insert(delivery);
  }
  return hooks.length;
}

export function registerWebhookSubscriptions(ctx: AppContext): void {
  for (const [internal, external] of Object.entries(EXTERNAL_TOPICS)) {
    ctx.bus.on(internal, (event) => {
      const { type: _type, ...data } = event;
      deliver(ctx, external, data);
    });
  }
}
''', imports=imports(ctx, "src/integrations/webhooks.ts", {
        "node:crypto": ["createHash", "createHmac"],
        "src/context.ts": ["type AppContext"],
        "src/adapters/store/audit.ts": ["recordAudit"],
        "src/scheduling/model.ts": ["AppointmentEvents"],
        "src/shared/errors.ts": ["DomainError"],
        "src/sla/model.ts": ["SlaEvents"],
        "src/integrations/types.ts": ["type Delivery", "type Webhook"],
    }, jobs=("JobEvents",)))


def accounting(ctx):
    return render(ctx, '''
«imports»

export interface JournalLine {
  date: CalendarDate;
  account: string;
  reference: string;
  debitCents: Cents;
  creditCents: Cents;
}

const ACCOUNTS = {
  receivables: '1200',
  revenueLabor: '4000',
  revenueParts: '4100',
  revenueTravel: '4200',
  discounts: '4900',
  taxPayable: '2200',
} as const;

function revenueAccount(kind: InvoiceLine['kind']): string {
  switch (kind) {
    case 'part':
      return ACCOUNTS.revenueParts;
    case 'travel':
    case 'travel-time':
      return ACCOUNTS.revenueTravel;
    case 'discount':
    case 'credit':
    case 'rounding':
      return ACCOUNTS.discounts;
    default:
      return ACCOUNTS.revenueLabor;
  }
}

/** Double-entry journal for issued invoices; every invoice balances to zero. */
export function journalFor(invoice: Invoice): JournalLine[] {
  if (invoice.issuedOn === undefined || invoice.number === undefined) return [];
  const date = invoice.issuedOn;
  const reference = invoice.number;
  const lines: JournalLine[] = [
    { date, account: ACCOUNTS.receivables, reference, debitCents: invoice.totalCents, creditCents: 0 },
  ];
  const byAccount = new Map<string, Cents>();
  for (const line of invoice.lines) {
    const account = revenueAccount(line.kind);
    byAccount.set(account, (byAccount.get(account) ?? 0) + line.amountCents);
  }
  for (const [account, amount] of [...byAccount.entries()].sort(([left], [right]) => left.localeCompare(right))) {
    if (amount === 0) continue;
    lines.push(amount > 0
      ? { date, account, reference, debitCents: 0, creditCents: amount }
      : { date, account, reference, debitCents: -amount, creditCents: 0 });
  }
  if (invoice.taxCents !== 0) {
    lines.push({ date, account: ACCOUNTS.taxPayable, reference, debitCents: 0, creditCents: invoice.taxCents });
  }
  return lines;
}

export function journalExport(ctx: AppContext, from: CalendarDate, to: CalendarDate, format: string): { contentType: string; body: string } {
  const exporter = ctx.exportFormats.lookup(format);
  if (!exporter) throw new DomainError('unknown-format', `Unknown export format ${format}`);
  const rows = invoices(ctx)
    .find((invoice) => invoice.issuedOn !== undefined && invoice.issuedOn >= from && invoice.issuedOn < to && invoice.status !== 'void')
    .sort((left, right) => (left.number ?? '').localeCompare(right.number ?? ''))
    .flatMap((invoice) => journalFor(invoice));
  return { contentType: exporter.contentType, body: exporter.render(rows.map((row) => ({ ...row }))) };
}
''', imports=imports(ctx, "src/integrations/accounting.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/invoices.ts": ["invoices"],
        "src/billing/model.ts": ["type Invoice", "type InvoiceLine"],
        "src/shared/errors.ts": ["DomainError"],
        "src/shared/money.ts": ["type Cents"],
        "src/shared/time.ts": ["type CalendarDate"],
    }))


def generate(ctx):
    ctx.add("src/reports/kpis.ts", kpis(ctx))
    ctx.add("src/reports/utilisation.ts", utilisation(ctx))
    ctx.add("src/reports/backlog.ts", backlog(ctx))
    ctx.add("src/reports/revenue.ts", revenue(ctx))
    ctx.add("src/reports/activity.ts", activity(ctx))
    ctx.add("src/reports/csv.ts", csv_module(ctx))
    ctx.add("src/integrations/types.ts", integration_types(ctx))
    ctx.add("src/integrations/formats.ts", export_formats(ctx))
    ctx.add("src/integrations/webhooks.ts", webhooks(ctx))
    ctx.add("src/integrations/accounting.ts", accounting(ctx))
