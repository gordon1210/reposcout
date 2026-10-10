"""Configuration, application context, storage adapters, repositories and authorization."""

from .core import imports, render


def config_schema(ctx):
    early = ctx.early
    shift_type = '''
export type ShiftPattern = 'early' | 'standard';

export interface ShiftDefinition {
  startMinute: number;
  endMinute: number;
}
''' if early else ""
    shift_field = "    shifts: Record<ShiftPattern, ShiftDefinition>;\n    defaultShift: ShiftPattern;\n" if early else ""
    shift_checks = '''
  for (const [name, shift] of Object.entries(config.scheduling.shifts)) {
    if (shift.startMinute < config.scheduling.dayStartMinute || shift.endMinute > config.scheduling.dayEndMinute) {
      problems.push(`scheduling.shifts.${name} must fall within the scheduling day`);
    }
    if (shift.endMinute - shift.startMinute < config.scheduling.slotMinutes) {
      problems.push(`scheduling.shifts.${name} must be at least one slot long`);
    }
  }
  if (!(config.scheduling.defaultShift in config.scheduling.shifts)) {
    problems.push('scheduling.defaultShift must name a configured shift');
  }
''' if early else ""
    return render(ctx, '''
import type { CalendarDate } from '../shared/time.ts';
«shift_type»
export interface AppConfig {
  organization: {
    name: string;
    currency: string;
    defaultRegion: string;
  };
  scheduling: {
    dayStartMinute: number;
    dayEndMinute: number;
    workingDays: number[];
    slotMinutes: number;
    windowMinutes: number;
    maxDailyAppointments: number;
    bufferMinutes: number;
    horizonDays: number;
    travelSpeedKmh: number;
«shift_field»  };
  billing: {
    paymentTermsDays: number;
    invoicePrefix: string;
    creditNotePrefix: string;
    minimumInvoiceCents: number;
    lateFeeBasisPoints: number;
  };
  sla: {
    warningRatio: number;
    creditCapBasisPoints: number;
  };
  timesheets: {
    clockRoundingMinutes: number;
    overtimeAfterMinutes: number;
    overtimeStepMinutes: number;
    maxShiftMinutes: number;
    weeklyOvertimeAfterMinutes: number;
    /** Reimbursement per private kilometre driven on company business. */
    mileageRateCents: number;
  };
  inventory: {
    reorderLeadDays: number;
    vanSafetyStock: number;
    reservationTtlHours: number;
    defaultWarehouse: string;
  };
  notifications: {
    channels: Array<'email' | 'sms' | 'portal'>;
    quietStartMinute: number;
    quietEndMinute: number;
    reminderLeadMinutes: number;
    senderName: string;
  };
  dispatch: {
    maxDistanceKm: number;
    weights: { skill: number; distance: number; load: number; continuity: number };
    preferContinuity: boolean;
  };
  holidays: CalendarDate[];
  features: {
    autoInvoiceOnCompletion: boolean;
    partsAutoResume: boolean;
    customerPortal: boolean;
    overtimeApprovals: boolean;
  };
}

export type DeepPartial<T> = {
  [K in keyof T]?: T[K] extends Array<unknown> ? T[K] : T[K] extends object ? DeepPartial<T[K]> : T[K];
};

function minuteOfDay(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 1440;
}

/** Returns every configuration problem; an empty list means the configuration is usable. */
export function configProblems(config: AppConfig): string[] {
  const problems: string[] = [];
  const { scheduling, timesheets, dispatch } = config;
  if (!minuteOfDay(scheduling.dayStartMinute) || !minuteOfDay(scheduling.dayEndMinute)) {
    problems.push('scheduling day bounds must be minutes of the day');
  } else if (scheduling.dayStartMinute >= scheduling.dayEndMinute) {
    problems.push('scheduling.dayStartMinute must be before dayEndMinute');
  }
  if (scheduling.workingDays.some((day) => !Number.isInteger(day) || day < 0 || day > 6)) {
    problems.push('scheduling.workingDays must contain weekday numbers 0-6');
  }
  if (scheduling.slotMinutes <= 0 || 1440 % scheduling.slotMinutes !== 0) {
    problems.push('scheduling.slotMinutes must divide the day evenly');
  }
  if (scheduling.windowMinutes < scheduling.slotMinutes || scheduling.windowMinutes % scheduling.slotMinutes !== 0) {
    problems.push('scheduling.windowMinutes must be a multiple of slotMinutes');
  }
«shift_checks»  if (timesheets.clockRoundingMinutes <= 0 || timesheets.overtimeStepMinutes <= 0) {
    problems.push('timesheet rounding steps must be positive');
  }
  if (timesheets.overtimeAfterMinutes > timesheets.maxShiftMinutes) {
    problems.push('timesheets.overtimeAfterMinutes cannot exceed maxShiftMinutes');
  }
  const weightTotal = Object.values(dispatch.weights).reduce((sum, weight) => sum + weight, 0);
  if (weightTotal !== 100) problems.push('dispatch.weights must add up to 100');
  if (config.sla.warningRatio <= 0 || config.sla.warningRatio >= 1) {
    problems.push('sla.warningRatio must be between 0 and 1');
  }
  if (!/^[A-Z]{3}$/.test(config.organization.currency)) problems.push('organization.currency must be an ISO code');
  return problems;
}
''', shift_type=shift_type, shift_field=shift_field, shift_checks=shift_checks)


def config_defaults(ctx):
    early = ctx.early
    day_start = "420" if early else "480"
    shifts = '''    shifts: {
      early: { startMinute: 420, endMinute: 900 },
      standard: { startMinute: 480, endMinute: 1020 },
    },
    defaultShift: 'standard',
''' if early else ""
    day_end = "1020"
    return render(ctx, '''
import type { AppConfig } from './schema.ts';

/**
 * Built-in defaults. Deployments override individual values through `resolveConfig`;
 * see docs/configuration.md for the meaning of each setting.
 */
export const DEFAULT_CONFIG: AppConfig = {
  organization: {
    name: 'Fieldline Services',
    currency: 'EUR',
    defaultRegion: 'metro-east',
  },
  scheduling: {
    dayStartMinute: «day_start»,
    dayEndMinute: «day_end»,
    workingDays: [1, 2, 3, 4, 5],
    slotMinutes: 30,
    windowMinutes: 120,
    maxDailyAppointments: 6,
    bufferMinutes: 15,
    horizonDays: 14,
    travelSpeedKmh: 40,
«shifts»  },
  billing: {
    paymentTermsDays: 30,
    invoicePrefix: 'INV',
    creditNotePrefix: 'CN',
    minimumInvoiceCents: 0,
    lateFeeBasisPoints: 150,
  },
  sla: {
    warningRatio: 0.8,
    creditCapBasisPoints: 2000,
  },
  timesheets: {
    clockRoundingMinutes: 5,
    overtimeAfterMinutes: 480,
    overtimeStepMinutes: 15,
    maxShiftMinutes: 720,
    weeklyOvertimeAfterMinutes: 2400,
    mileageRateCents: 30,
  },
  inventory: {
    reorderLeadDays: 3,
    vanSafetyStock: 1,
    reservationTtlHours: 72,
    defaultWarehouse: 'WH-CENTRAL',
  },
  notifications: {
    channels: ['email', 'sms'],
    quietStartMinute: 1260,
    quietEndMinute: 420,
    reminderLeadMinutes: 1440,
    senderName: 'Fieldline Service Desk',
  },
  dispatch: {
    maxDistanceKm: 80,
    weights: { skill: 40, distance: 30, load: 20, continuity: 10 },
    preferContinuity: true,
  },
  holidays: ['2026-01-01', '2026-04-03', '2026-04-06', '2026-05-01', '2026-12-25', '2026-12-26'],
  features: {
    autoInvoiceOnCompletion: false,
    partsAutoResume: true,
    customerPortal: true,
    overtimeApprovals: true,
  },
};
''', day_start=day_start, day_end=day_end, shifts=shifts)


def config_load(ctx):
    return '''
import { DEFAULT_CONFIG } from './defaults.ts';
import { configProblems } from './schema.ts';
import type { AppConfig, DeepPartial } from './schema.ts';

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** Objects merge key by key; arrays and scalars in `override` replace the default. */
export function mergeConfig<T>(base: T, override: DeepPartial<T> | undefined): T {
  if (override === undefined) return structuredClone(base);
  const result = structuredClone(base) as Record<string, unknown>;
  for (const [key, value] of Object.entries(override as Record<string, unknown>)) {
    if (value === undefined) continue;
    if (!(key in result)) throw new Error(`Unknown configuration key: ${key}`);
    const current = result[key];
    result[key] = isPlainObject(current) && isPlainObject(value)
      ? mergeConfig(current, value as DeepPartial<typeof current>)
      : structuredClone(value);
  }
  return result as T;
}

function deepFreeze<T>(value: T): T {
  if (typeof value === 'object' && value !== null) {
    for (const child of Object.values(value as Record<string, unknown>)) deepFreeze(child);
    Object.freeze(value);
  }
  return value;
}

export class ConfigurationError extends Error {
  readonly problems: string[];

  constructor(problems: string[]) {
    super(`Invalid configuration: ${problems.join('; ')}`);
    this.name = 'ConfigurationError';
    this.problems = problems;
  }
}

export function resolveConfig(overrides?: DeepPartial<AppConfig>): AppConfig {
  const config = mergeConfig(DEFAULT_CONFIG, overrides);
  const problems = configProblems(config);
  if (problems.length > 0) throw new ConfigurationError(problems);
  return deepFreeze(config);
}
'''


def context_module(ctx):
    here = "src/context.ts"
    head = imports(ctx, here, {
        "src/config/schema.ts": ["type AppConfig"],
        "src/adapters/store/memory.ts": ["type Store"],
        "src/shared/events.ts": ["type EventBus"],
        "src/shared/clock.ts": ["type Clock"],
        "src/shared/ids.ts": ["type IdSource"],
        "src/shared/log.ts": ["type Logger"],
        "src/shared/registry.ts": ["type Registry"],
        "src/worktypes/types.ts": ["type WorkTypeHandler"],
        "src/notifications/types.ts": ["type NotificationTemplate"],
        "src/dispatch/types.ts": ["type ScoringRule"],
        "src/integrations/types.ts": ["type ExportFormat"],
        "src/shared/calendar.ts": ["type BusinessCalendar"],
    })
    return render(ctx, '''
«imports»

/** Everything a service needs; created once per application instance by `createApp`. */
export interface AppContext {
  config: AppConfig;
  store: Store;
  bus: EventBus;
  clock: Clock;
  ids: IdSource;
  log: Logger;
  workTypes: Registry<WorkTypeHandler>;
  templates: Registry<NotificationTemplate>;
  scoringRules: Registry<ScoringRule>;
  exportFormats: Registry<ExportFormat>;
}

/** The organisation's standard working calendar, including configured public holidays. */
export function businessCalendar(ctx: AppContext): BusinessCalendar {
  const { scheduling, holidays } = ctx.config;
  return {
    dayStartMinute: scheduling.dayStartMinute,
    dayEndMinute: scheduling.dayEndMinute,
    workingDays: scheduling.workingDays,
    holidays,
  };
}
''', imports=head)


def store_module(ctx):
    return '''
import { NotFoundError } from '../../shared/errors.ts';

export interface Entity {
  id: string;
}

export type CollectionName =
  | 'customers'
  | 'sites'
  | 'contracts'
  | 'assets'
  | 'jobs'
  | 'notes'
  | 'technicians'
  | 'appointments'
  | 'parts'
  | 'stock'
  | 'reservations'
  | 'receipts'
  | 'timeEntries'
  | 'invoices'
  | 'creditNotes'
  | 'payments'
  | 'slaClocks'
  | 'quotes'
  | 'notifications'
  | 'preferences'
  | 'webhooks'
  | 'deliveries'
  | 'audit';

export interface Collection<T extends Entity> {
  insert(record: T): T;
  get(id: string): T | undefined;
  require(id: string, entity?: string): T;
  update(id: string, change: Partial<T> | ((current: T) => T)): T;
  remove(id: string): boolean;
  all(): T[];
  find(predicate: (record: T) => boolean): T[];
  first(predicate: (record: T) => boolean): T | undefined;
  count(predicate?: (record: T) => boolean): number;
}

export interface Store {
  collection<T extends Entity>(name: CollectionName): Collection<T>;
  snapshot(): Record<string, Entity[]>;
}

/** Records are copied on the way in and out so callers can never alias stored state. */
function createCollection<T extends Entity>(name: string): Collection<T> {
  const records = new Map<string, T>();
  const entity = name.endsWith('s') ? name.slice(0, -1) : name;
  return {
    insert(record) {
      if (records.has(record.id)) throw new Error(`${name}: duplicate id ${record.id}`);
      records.set(record.id, structuredClone(record));
      return structuredClone(record);
    },
    get(id) {
      const record = records.get(id);
      return record === undefined ? undefined : structuredClone(record);
    },
    require(id, label = entity) {
      const record = records.get(id);
      if (record === undefined) throw new NotFoundError(label, id);
      return structuredClone(record);
    },
    update(id, change) {
      const current = records.get(id);
      if (current === undefined) throw new NotFoundError(entity, id);
      const next = typeof change === 'function' ? change(structuredClone(current)) : { ...current, ...change };
      if (next.id !== id) throw new Error(`${name}: an update cannot change the id`);
      records.set(id, structuredClone(next));
      return structuredClone(next);
    },
    remove(id) {
      return records.delete(id);
    },
    all() {
      return [...records.values()].map((record) => structuredClone(record));
    },
    find(predicate) {
      return [...records.values()].filter((record) => predicate(record)).map((record) => structuredClone(record));
    },
    first(predicate) {
      for (const record of records.values()) if (predicate(record)) return structuredClone(record);
      return undefined;
    },
    count(predicate) {
      if (!predicate) return records.size;
      let total = 0;
      for (const record of records.values()) if (predicate(record)) total += 1;
      return total;
    },
  };
}

export function createMemoryStore(): Store {
  const collections = new Map<string, Collection<Entity>>();
  return {
    collection<T extends Entity>(name: CollectionName): Collection<T> {
      let collection = collections.get(name);
      if (!collection) {
        collection = createCollection<Entity>(name);
        collections.set(name, collection);
      }
      return collection as unknown as Collection<T>;
    },
    snapshot() {
      const result: Record<string, Entity[]> = {};
      for (const [name, collection] of collections) result[name] = collection.all();
      return result;
    },
  };
}
'''


def audit_module(ctx):
    return '''
import type { AppContext } from '../../context.ts';
import type { Timestamp } from '../../shared/time.ts';

export interface AuditEntry {
  id: string;
  at: Timestamp;
  actorId: string;
  action: string;
  subject: string;
  details: Record<string, unknown>;
}

export function recordAudit(
  ctx: AppContext,
  actorId: string,
  action: string,
  subject: string,
  details: Record<string, unknown> = {},
): AuditEntry {
  const entry: AuditEntry = { id: ctx.ids.next('AUD'), at: ctx.clock.now(), actorId, action, subject, details };
  return ctx.store.collection<AuditEntry>('audit').insert(entry);
}

export function auditTrail(ctx: AppContext, subject: string): AuditEntry[] {
  return ctx.store
    .collection<AuditEntry>('audit')
    .find((entry) => entry.subject === subject)
    .sort((left, right) => left.id.localeCompare(right.id));
}

export function actionsBy(ctx: AppContext, actorId: string, since?: Timestamp): AuditEntry[] {
  return ctx.store
    .collection<AuditEntry>('audit')
    .find((entry) => entry.actorId === actorId && (since === undefined || entry.at >= since));
}
'''


def roles_module(ctx):
    return '''
export type Role = 'admin' | 'dispatcher' | 'technician' | 'billing' | 'customer' | 'warehouse';

export interface Actor {
  id: string;
  name: string;
  role: Role;
  /** Customer account for `customer` actors, technician record for `technician` actors. */
  subjectId?: string;
}

export const SYSTEM_ACTOR: Actor = Object.freeze({ id: 'system', name: 'Fieldline automation', role: 'admin' });

export const STAFF_ROLES: readonly Role[] = ['admin', 'dispatcher', 'billing', 'warehouse'];

export function isStaff(actor: Actor): boolean {
  return STAFF_ROLES.includes(actor.role);
}
'''


def permissions_module(ctx):
    return '''
import { ForbiddenError } from '../shared/errors.ts';
import type { Actor, Role } from './roles.ts';

export type Permission =
  | 'customers:read'
  | 'customers:write'
  | 'contracts:write'
  | 'jobs:read'
  | 'jobs:open'
  | 'jobs:schedule'
  | 'jobs:work'
  | 'jobs:cancel'
  | 'technicians:write'
  | 'inventory:read'
  | 'inventory:write'
  | 'timesheets:write'
  | 'timesheets:approve'
  | 'billing:read'
  | 'billing:write'
  | 'reports:read'
  | 'quotes:write'
  | 'notifications:read'
  | 'system:admin';

const GRANTS: Record<Role, readonly Permission[]> = {
  admin: [
    'customers:read', 'customers:write', 'contracts:write', 'jobs:read', 'jobs:open', 'jobs:schedule',
    'jobs:work', 'jobs:cancel', 'technicians:write', 'inventory:read', 'inventory:write', 'timesheets:write',
    'timesheets:approve', 'billing:read', 'billing:write', 'reports:read', 'quotes:write', 'notifications:read',
    'system:admin',
  ],
  dispatcher: [
    'customers:read', 'jobs:read', 'jobs:open', 'jobs:schedule', 'jobs:cancel', 'technicians:write',
    'inventory:read', 'timesheets:approve', 'reports:read', 'quotes:write', 'notifications:read',
  ],
  technician: ['jobs:read', 'jobs:work', 'inventory:read', 'timesheets:write'],
  billing: ['customers:read', 'jobs:read', 'billing:read', 'billing:write', 'reports:read', 'quotes:write'],
  warehouse: ['inventory:read', 'inventory:write', 'jobs:read'],
  customer: ['jobs:read', 'jobs:open', 'billing:read', 'notifications:read'],
};

export function can(actor: Actor, permission: Permission): boolean {
  return GRANTS[actor.role].includes(permission);
}

export function authorize(actor: Actor, permission: Permission): void {
  if (!can(actor, permission)) throw new ForbiddenError(permission, actor.id);
}

/** Customers only see their own account; staff see everything they are permitted to read. */
export function canSeeCustomer(actor: Actor, customerId: string): boolean {
  if (actor.role === 'customer') return actor.subjectId === customerId;
  return actor.role !== 'technician';
}

export function assertCustomerScope(actor: Actor, customerId: string): void {
  if (actor.role === 'customer' && actor.subjectId !== customerId) {
    throw new ForbiddenError(`access customer ${customerId}`, actor.id);
  }
}
'''


def actors_module(ctx):
    return '''
import type { AppContext } from '../context.ts';
import { DomainError } from '../shared/errors.ts';
import type { Actor, Role } from './roles.ts';

const actorsByContext = new WeakMap<AppContext, Map<string, Actor>>();

function directory(ctx: AppContext): Map<string, Actor> {
  let entries = actorsByContext.get(ctx);
  if (!entries) {
    entries = new Map();
    actorsByContext.set(ctx, entries);
  }
  return entries;
}

export function registerActor(ctx: AppContext, actor: Actor): Actor {
  const entries = directory(ctx);
  if (entries.has(actor.id)) throw new DomainError('duplicate-actor', `Actor ${actor.id} already exists`, 409);
  entries.set(actor.id, Object.freeze({ ...actor }));
  return actor;
}

export function findActor(ctx: AppContext, id: string): Actor | undefined {
  return directory(ctx).get(id);
}

export function actorsWithRole(ctx: AppContext, role: Role): Actor[] {
  return [...directory(ctx).values()].filter((actor) => actor.role === role);
}

export function seedStaff(ctx: AppContext): void {
  registerActor(ctx, { id: 'admin', name: 'Operations Administrator', role: 'admin' });
  registerActor(ctx, { id: 'dispatch', name: 'Dispatch Desk', role: 'dispatcher' });
  registerActor(ctx, { id: 'billing', name: 'Billing Office', role: 'billing' });
  registerActor(ctx, { id: 'warehouse', name: 'Central Warehouse', role: 'warehouse' });
}
'''


REPOSITORIES = {
    "customers": ('''
«imports»

export function customers(ctx: AppContext): Collection<Customer> {
  return ctx.store.collection<Customer>('customers');
}

export function findCustomerByAccount(ctx: AppContext, accountNumber: string): Customer | undefined {
  const normalized = accountNumber.trim().toUpperCase();
  return customers(ctx).first((customer) => customer.accountNumber === normalized);
}

export function activeCustomers(ctx: AppContext): Customer[] {
  return customers(ctx)
    .find((customer) => customer.status === 'active')
    .sort((left, right) => left.name.localeCompare(right.name));
}

export function customersInRegion(ctx: AppContext, region: string): Customer[] {
  return customers(ctx).find((customer) => customer.billingRegion === region);
}
''', {"src/customers/model.ts": ["type Customer"]}),
    "sites": ('''
«imports»

export function sites(ctx: AppContext): Collection<Site> {
  return ctx.store.collection<Site>('sites');
}

export function sitesForCustomer(ctx: AppContext, customerId: string): Site[] {
  return sites(ctx)
    .find((site) => site.customerId === customerId)
    .sort((left, right) => left.label.localeCompare(right.label));
}

export function requireSiteOf(ctx: AppContext, customerId: string, siteId: string): Site {
  const site = sites(ctx).require(siteId);
  if (site.customerId !== customerId) throw new NotFoundError('site', siteId);
  return site;
}

export function sitesInRegion(ctx: AppContext, region: string): Site[] {
  return sites(ctx).find((site) => site.address.region === region);
}
''', {"src/customers/model.ts": ["type Site"], "src/shared/errors.ts": ["NotFoundError"]}),
    "contracts": ('''
«imports»

export function contracts(ctx: AppContext): Collection<Contract> {
  return ctx.store.collection<Contract>('contracts');
}

export function contractsForCustomer(ctx: AppContext, customerId: string): Contract[] {
  return contracts(ctx).find((contract) => contract.customerId === customerId);
}

/** The contract covering a site on a date; later start dates win when renewals overlap. */
export function contractForSite(ctx: AppContext, siteId: string, on: CalendarDate): Contract | undefined {
  const candidates = contracts(ctx).find(
    (contract) =>
      contract.status === 'active' &&
      contract.siteIds.includes(siteId) &&
      contract.startsOn <= on &&
      (contract.endsOn === undefined || on <= contract.endsOn),
  );
  return candidates.sort((left, right) => right.startsOn.localeCompare(left.startsOn))[0];
}

export function expiringContracts(ctx: AppContext, before: CalendarDate): Contract[] {
  return contracts(ctx).find(
    (contract) => contract.status === 'active' && contract.endsOn !== undefined && contract.endsOn < before,
  );
}
''', {"src/contracts/model.ts": ["type Contract"], "src/shared/time.ts": ["type CalendarDate"]}),
    "assets": ('''
«imports»

export function assets(ctx: AppContext): Collection<Asset> {
  return ctx.store.collection<Asset>('assets');
}

export function assetsAtSite(ctx: AppContext, siteId: string): Asset[] {
  return assets(ctx)
    .find((asset) => asset.siteId === siteId && asset.retiredOn === undefined)
    .sort((left, right) => left.kind.localeCompare(right.kind) || left.id.localeCompare(right.id));
}

export function findAssetBySerial(ctx: AppContext, serial: string): Asset | undefined {
  return assets(ctx).first((asset) => asset.serial === serial);
}
''', {"src/assets/model.ts": ["type Asset"]}),
    "jobs": ('''
«imports»

export function jobs(ctx: AppContext): Collection<«Job»> {
  return ctx.store.collection<«Job»>('jobs');
}

export function jobsForSite(ctx: AppContext, siteId: string): «Job»[] {
  return jobs(ctx).find((job) => job.siteId === siteId);
}

export function jobsForCustomer(ctx: AppContext, customerId: string): «Job»[] {
  return jobs(ctx).find((job) => job.customerId === customerId);
}

export function activeJobsForTechnician(ctx: AppContext, technicianId: string): «Job»[] {
  return jobs(ctx).find((job) => job.technicianId === technicianId && «isOpenStatus»(job.status));
}

export function jobsInStatus(ctx: AppContext, ...statuses: «JobStatus»[]): «Job»[] {
  return jobs(ctx).find((job) => statuses.includes(job.status));
}

export function findJobByNumber(ctx: AppContext, number: string): «Job» | undefined {
  return jobs(ctx).first((job) => job.number === number);
}
''', {}, ["type Job", "type JobStatus", "isOpenStatus"]),
    "technicians": ('''
«imports»

export function technicians(ctx: AppContext): Collection<Technician> {
  return ctx.store.collection<Technician>('technicians');
}

export function activeTechnicians(ctx: AppContext): Technician[] {
  return technicians(ctx).find((technician) => technician.active);
}

export function techniciansInRegion(ctx: AppContext, region: string): Technician[] {
  return activeTechnicians(ctx).filter((technician) => technician.region === region || technician.roaming);
}
''', {"src/technicians/model.ts": ["type Technician"]}),
    "appointments": ('''
«imports»

export function appointments(ctx: AppContext): Collection<Appointment> {
  return ctx.store.collection<Appointment>('appointments');
}

export function appointmentsForTechnician(ctx: AppContext, technicianId: string, on?: CalendarDate): Appointment[] {
  return appointments(ctx)
    .find(
      (appointment) =>
        appointment.technicianId === technicianId &&
        appointment.status !== 'cancelled' &&
        (on === undefined || dateOf(appointment.start) === on),
    )
    .sort((left, right) => compareTimestamps(left.start, right.start));
}

export function appointmentForJob(ctx: AppContext, jobId: string): Appointment | undefined {
  return appointments(ctx).first(
    (appointment) => appointment.jobId === jobId && (appointment.status === 'booked' || appointment.status === 'confirmed'),
  );
}
''', {"src/scheduling/model.ts": ["type Appointment"],
      "src/shared/time.ts": ["compareTimestamps", "dateOf", "type CalendarDate"]}),
    "parts": ('''
«imports»

export function parts(ctx: AppContext): Collection<Part> {
  return ctx.store.collection<Part>('parts');
}

export function partsInCategory(ctx: AppContext, category: string): Part[] {
  return parts(ctx)
    .find((part) => part.category === category && !part.discontinued)
    .sort((left, right) => left.id.localeCompare(right.id));
}
''', {"src/inventory/model.ts": ["type Part"]}),
    "stock": ('''
«imports»

export function stockLevels(ctx: AppContext): Collection<StockLevel> {
  return ctx.store.collection<StockLevel>('stock');
}

export function stockKey(locationId: string, sku: string): string {
  return `${locationId}:${sku}`;
}

export function levelAt(ctx: AppContext, locationId: string, sku: string): StockLevel {
  return (
    stockLevels(ctx).get(stockKey(locationId, sku)) ?? { id: stockKey(locationId, sku), locationId, sku, onHand: 0, reserved: 0 }
  );
}

export function saveLevel(ctx: AppContext, level: StockLevel): StockLevel {
  const collection = stockLevels(ctx);
  return collection.get(level.id) ? collection.update(level.id, level) : collection.insert(level);
}

export function levelsForSku(ctx: AppContext, sku: string): StockLevel[] {
  return stockLevels(ctx).find((level) => level.sku === sku);
}
''', {"src/inventory/model.ts": ["type StockLevel"]}),
    "reservations": ('''
«imports»

export function reservations(ctx: AppContext): Collection<Reservation> {
  return ctx.store.collection<Reservation>('reservations');
}

export function reservationsForJob(ctx: AppContext, jobId: string): Reservation[] {
  return reservations(ctx).find((reservation) => reservation.jobId === jobId && reservation.status !== 'released');
}

export function backorderedReservations(ctx: AppContext, sku: string): Reservation[] {
  return reservations(ctx)
    .find((reservation) => reservation.sku === sku && reservation.status === 'backordered')
    .sort((left, right) => left.id.localeCompare(right.id));
}
''', {"src/inventory/model.ts": ["type Reservation"]}),
    "time-entries": ('''
«imports»

export function timeEntries(ctx: AppContext): Collection<TimeEntry> {
  return ctx.store.collection<TimeEntry>('timeEntries');
}

export function entriesForJob(ctx: AppContext, jobId: string): TimeEntry[] {
  return timeEntries(ctx)
    .find((entry) => entry.jobId === jobId && entry.status !== 'rejected')
    .sort((left, right) => compareTimestamps(left.start, right.start));
}

export function entriesForTechnician(ctx: AppContext, technicianId: string, from: Timestamp, to: Timestamp): TimeEntry[] {
  return timeEntries(ctx)
    .find((entry) => entry.technicianId === technicianId && entry.start >= from && entry.start < to)
    .sort((left, right) => compareTimestamps(left.start, right.start));
}
''', {"src/timesheets/model.ts": ["type TimeEntry"],
      "src/shared/time.ts": ["compareTimestamps", "type Timestamp"]}),
    "invoices": ('''
«imports»

export function invoices(ctx: AppContext): Collection<Invoice> {
  return ctx.store.collection<Invoice>('invoices');
}

export function invoicesForCustomer(ctx: AppContext, customerId: string): Invoice[] {
  return invoices(ctx)
    .find((invoice) => invoice.customerId === customerId)
    .sort((left, right) => left.id.localeCompare(right.id));
}

export function invoiceForJob(ctx: AppContext, jobId: string): Invoice | undefined {
  return invoices(ctx).first((invoice) => invoice.jobIds.includes(jobId) && invoice.status !== 'void');
}

export function creditNotes(ctx: AppContext): Collection<CreditNote> {
  return ctx.store.collection<CreditNote>('creditNotes');
}

export function payments(ctx: AppContext): Collection<Payment> {
  return ctx.store.collection<Payment>('payments');
}
''', {"src/billing/model.ts": ["type Invoice", "type CreditNote", "type Payment"]}),
    "sla-clocks": ('''
«imports»

export function slaClocks(ctx: AppContext): Collection<SlaClock> {
  return ctx.store.collection<SlaClock>('slaClocks');
}

export function clockForJob(ctx: AppContext, jobId: string): SlaClock | undefined {
  return slaClocks(ctx).get(`SLA-${jobId}`);
}
''', {"src/sla/model.ts": ["type SlaClock"]}),
    "quotes": ('''
«imports»

export function quotes(ctx: AppContext): Collection<Quote> {
  return ctx.store.collection<Quote>('quotes');
}

export function openQuotesForCustomer(ctx: AppContext, customerId: string): Quote[] {
  return quotes(ctx).find((quote) => quote.customerId === customerId && (quote.status === 'draft' || quote.status === 'sent'));
}
''', {"src/quotes/model.ts": ["type Quote"]}),
    "notifications": ('''
«imports»

export function notifications(ctx: AppContext): Collection<Notification> {
  return ctx.store.collection<Notification>('notifications');
}

export function notificationsFor(ctx: AppContext, recipientId: string): Notification[] {
  return notifications(ctx)
    .find((notification) => notification.recipientId === recipientId)
    .sort((left, right) => left.id.localeCompare(right.id));
}

export function notificationsForJob(ctx: AppContext, jobId: string): Notification[] {
  return notifications(ctx)
    .find((notification) => notification.jobId === jobId)
    .sort((left, right) => left.id.localeCompare(right.id));
}

export function preferences(ctx: AppContext): Collection<ContactPreference> {
  return ctx.store.collection<ContactPreference>('preferences');
}
''', {"src/notifications/types.ts": ["type Notification", "type ContactPreference"]}),
}


def repositories(ctx):
    for name, entry in REPOSITORIES.items():
        template, groups = entry[0], entry[1]
        jobs = entry[2] if len(entry) > 2 else ()
        here = f"src/adapters/repositories/{name}.ts"
        merged = {"src/context.ts": ["type AppContext"], "src/adapters/store/memory.ts": ["type Collection"], **groups}
        ctx.add(here, render(ctx, template, imports=imports(ctx, here, merged, jobs=jobs)))


def generate(ctx):
    ctx.add("src/config/schema.ts", config_schema(ctx))
    ctx.add("src/config/defaults.ts", config_defaults(ctx))
    ctx.add("src/config/load.ts", config_load(ctx))
    ctx.add("src/context.ts", context_module(ctx))
    ctx.add("src/adapters/store/memory.ts", store_module(ctx))
    ctx.add("src/adapters/store/audit.ts", audit_module(ctx))
    ctx.add("src/auth/roles.ts", roles_module(ctx))
    ctx.add("src/auth/permissions.ts", permissions_module(ctx))
    ctx.add("src/auth/actors.ts", actors_module(ctx))
    repositories(ctx)
