"""Timesheets and inventory."""

from .core import imports, render, rounding, rounding_names


def timesheet_model(ctx):
    return render(ctx, '''
«imports»

export type EntryKind = 'labor' | 'travel' | 'break' | 'admin';
export type EntryStatus = 'open' | 'submitted' | 'approved' | 'rejected';

export interface TimeEntry {
  id: string;
  technicianId: string;
  jobId?: string;
  kind: EntryKind;
  start: Timestamp;
  end?: Timestamp;
  minutes: number;
  /** Distance driven, recorded on travel entries. */
  km?: number;
  status: EntryStatus;
  note?: string;
  rejectedReason?: string;
}

export const BILLABLE_KINDS: readonly EntryKind[] = ['labor', 'travel'];
''', imports=imports(ctx, "src/timesheets/model.ts", {"src/shared/time.ts": ["type Timestamp"]}))


def timesheet_clock(ctx):
    return render(ctx, '''
«imports»

function roundedClock(ctx: AppContext, at: Timestamp, edge: 'in' | 'out'): Timestamp {
  const step = ctx.config.timesheets.clockRoundingMinutes;
  const minutes = toEpochMinutes(at);
  return fromEpochMinutes(edge === 'in' ? «clock_in» : «clock_out»);
}

/** Starts a time entry; a technician can only have one open entry at a time. */
export function clockIn(ctx: AppContext, technicianId: string, kind: EntryKind, jobId?: string): TimeEntry {
  technicians(ctx).require(technicianId, 'technician');
  const open = timeEntries(ctx).first((entry) => entry.technicianId === technicianId && entry.status === 'open');
  if (open) throw new ConflictError('already-clocked-in', `Technician ${technicianId} is still clocked in on ${open.id}`);
  if ((kind === 'labor' || kind === 'travel') && jobId === undefined) {
    throw new DomainError('job-required', `${kind} time must be booked against a work order`);
  }
  if (jobId !== undefined) jobs(ctx).require(jobId, 'work order');
  const entry: TimeEntry = {
    id: ctx.ids.next('TIME'),
    technicianId,
    jobId,
    kind,
    start: roundedClock(ctx, ctx.clock.now(), 'in'),
    minutes: 0,
    status: 'open',
  };
  return timeEntries(ctx).insert(entry);
}

export function clockOut(ctx: AppContext, entryId: string, km?: number, note?: string): TimeEntry {
  const entry = timeEntries(ctx).require(entryId, 'time entry');
  if (entry.status !== 'open') throw new ConflictError('not-open', `Time entry ${entryId} is already closed`);
  if (km !== undefined && entry.kind !== 'travel') throw new DomainError('km-on-travel-only', 'Distance is only recorded on travel');
  const end = roundedClock(ctx, ctx.clock.now(), 'out');
  const minutes = Math.max(0, minutesBetween(entry.start, end));
  if (minutes > ctx.config.timesheets.maxShiftMinutes) {
    throw new DomainError('entry-too-long', `Entries are limited to ${ctx.config.timesheets.maxShiftMinutes} minutes`);
  }
  return timeEntries(ctx).update(entryId, { end, minutes, status: 'submitted', ...(km !== undefined ? { km } : {}), ...(note ? { note } : {}) });
}
''', clock_in=rounding(ctx, "minutes", "step", "nearest"),
                  clock_out=rounding(ctx, "minutes", "step", "nearest-explicit"),
                  imports=imports(ctx, "src/timesheets/clock.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/adapters/repositories/jobs.ts": ["jobs"],
                      "src/adapters/repositories/technicians.ts": ["technicians"],
                      "src/adapters/repositories/time-entries.ts": ["timeEntries"],
                      "src/shared/errors.ts": ["ConflictError", "DomainError"],
                      "src/shared/rounding.ts": rounding_names(ctx, "nearest"),
                      "src/shared/time.ts": ["fromEpochMinutes", "minutesBetween", "toEpochMinutes", "type Timestamp"],
                      "src/timesheets/model.ts": ["type EntryKind", "type TimeEntry"],
                  }))


def timesheet_entries(ctx):
    overlap = '''  const clash = entriesForTechnician(ctx, data.technicianId, addMinutes(data.start, -24 * 60), data.end).find(
    (entry) => entry.end !== undefined && entry.start < data.end && data.start < entry.end,
  );''' if not ctx.refactor else '''  const clash = entriesForTechnician(ctx, data.technicianId, addMinutes(data.start, -24 * 60), data.end).find(
    (entry) => entry.end !== undefined && !(entry.end <= data.start || data.end <= entry.start),
  );'''
    return render(ctx, '''
«imports»

const manualEntry = object({
  technicianId: string({ min: 1 }),
  jobId: optional(string({ min: 1 })),
  kind: oneOf(['labor', 'travel', 'break', 'admin'] as const),
  start: timestamp(),
  end: timestamp(),
  km: optional(number({ min: 0, max: 2000 })),
  note: optional(string({ max: 500 })),
});

/**
 * Records a completed entry after the fact, for example travel logged from the van's trip
 * computer. Entries of one technician may not overlap.
 */
export function logEntry(ctx: AppContext, input: unknown): TimeEntry {
  const data = parseInput(manualEntry, input);
  if (data.end <= data.start) throw new DomainError('invalid-period', 'An entry must end after it starts');
  if (data.km !== undefined && data.kind !== 'travel') {
    throw new DomainError('km-on-travel-only', 'Distance is only recorded on travel entries');
  }
  if ((data.kind === 'labor' || data.kind === 'travel') && data.jobId === undefined) {
    throw new DomainError('job-required', `${data.kind} time must be booked against a work order`);
  }
  technicians(ctx).require(data.technicianId, 'technician');
  if (data.jobId !== undefined) jobs(ctx).require(data.jobId, 'work order');
  const minutes = minutesBetween(data.start, data.end);
  if (minutes > ctx.config.timesheets.maxShiftMinutes) {
    throw new DomainError('entry-too-long', `Entries are limited to ${ctx.config.timesheets.maxShiftMinutes} minutes`);
  }
«overlap»
  if (clash) throw new ConflictError('overlapping-entry', `The entry overlaps ${clash.id}`, { entryId: clash.id });
  return timeEntries(ctx).insert({
    id: ctx.ids.next('TIME'),
    technicianId: data.technicianId,
    jobId: data.jobId,
    kind: data.kind,
    start: data.start,
    end: data.end,
    minutes,
    km: data.km,
    status: 'submitted',
    note: data.note,
  });
}

export function correctEntry(ctx: AppContext, entryId: string, minutes: number, note: string): TimeEntry {
  const entry = timeEntries(ctx).require(entryId, 'time entry');
  if (entry.status === 'approved') throw new ConflictError('entry-approved', 'Approved entries cannot be corrected');
  if (!Number.isInteger(minutes) || minutes < 0) throw new DomainError('invalid-minutes', 'Minutes must be a whole number');
  return timeEntries(ctx).update(entryId, {
    minutes,
    end: addMinutes(entry.start, minutes),
    note: note.trim() || entry.note,
    status: entry.status === 'rejected' ? 'submitted' : entry.status,
  });
}
''', overlap=overlap, imports=imports(ctx, "src/timesheets/entries.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/jobs.ts": ["jobs"],
        "src/adapters/repositories/technicians.ts": ["technicians"],
        "src/adapters/repositories/time-entries.ts": ["entriesForTechnician", "timeEntries"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/time.ts": ["addMinutes", "minutesBetween"],
        "src/shared/validate.ts": ["number", "object", "oneOf", "optional", "parseInput", "string", "timestamp"],
        "src/timesheets/model.ts": ["type TimeEntry"],
    }))


def timesheet_mileage(ctx):
    return render(ctx, '''
«imports»

export interface MileageClaim {
  technicianId: string;
  trips: number;
  km: number;
  amountCents: Cents;
}

/**
 * Mileage paid to technicians for approved travel. Every trip is rounded to the nearest whole
 * kilometre before it is added up, matching the van logbook.
 */
export function mileageClaim(entries: readonly TimeEntry[], technicianId: string, ratePerKmCents: Cents): MileageClaim {
  let km = 0;
  let trips = 0;
  for (const entry of entries) {
    if (entry.technicianId !== technicianId || entry.kind !== 'travel' || entry.status !== 'approved') continue;
    if (entry.km === undefined) continue;
    km += «trip»;
    trips += 1;
  }
  return { technicianId, trips, km, amountCents: multiplyCents(ratePerKmCents, km) };
}
''', trip=rounding(ctx, "entry.km", 1, "nearest-explicit"),
                  imports=imports(ctx, "src/timesheets/mileage.ts", {
                      "src/shared/money.ts": ["multiplyCents", "type Cents"],
                      "src/shared/rounding.ts": rounding_names(ctx, "nearest"),
                      "src/timesheets/model.ts": ["type TimeEntry"],
                  }))


def timesheet_overtime(ctx):
    weekly = '''  let total = 0;
  for (const day of days) total += day.workedMinutes;''' if not ctx.refactor else '''  const total = days.reduce((sum, day) => sum + day.workedMinutes, 0);'''
    return render(ctx, '''
«imports»

export interface DayTotals {
  date: CalendarDate;
  workedMinutes: number;
  overtimeMinutes: number;
}

/** Labour, travel and admin count as worked time; breaks do not. */
export function dailyTotals(entries: readonly TimeEntry[], config: AppConfig['timesheets']): DayTotals[] {
  const byDay = groupBy(
    entries.filter((entry) => entry.kind !== 'break' && entry.status !== 'rejected'),
    (entry) => dateOf(entry.start),
  );
  return [...byDay.entries()]
    .map(([date, list]) => {
      const workedMinutes = list.reduce((sum, entry) => sum + entry.minutes, 0);
      const beyond = Math.max(0, workedMinutes - config.overtimeAfterMinutes);
      return { date, workedMinutes, overtimeMinutes: «overtime» };
    })
    .sort((left, right) => left.date.localeCompare(right.date));
}

/**
 * Weekly overtime: the larger of the summed daily overtime and the time worked beyond the
 * weekly threshold, so hours are never paid twice.
 */
export function weeklyOvertime(days: readonly DayTotals[], config: AppConfig['timesheets']): number {
«weekly»
  const daily = days.reduce((sum, day) => sum + day.overtimeMinutes, 0);
  const weekly = «weekly_round»;
  return Math.max(daily, weekly);
}
''', weekly=weekly, overtime=rounding(ctx, "beyond", "config.overtimeStepMinutes", "down"),
                  weekly_round=rounding(ctx, "Math.max(0, total - config.weeklyOvertimeAfterMinutes)",
                                        "config.overtimeStepMinutes", "down"),
                  imports=imports(ctx, "src/timesheets/overtime.ts", {
                      "src/config/schema.ts": ["type AppConfig"],
                      "src/shared/collections.ts": ["groupBy"],
                      "src/shared/rounding.ts": rounding_names(ctx, "down"),
                      "src/shared/time.ts": ["dateOf", "type CalendarDate"],
                      "src/timesheets/model.ts": ["type TimeEntry"],
                  }))


def timesheet_approvals(ctx):
    return render(ctx, '''
«imports»

export function approveEntries(ctx: AppContext, actorId: string, entryIds: readonly string[]): TimeEntry[] {
  const entries = entryIds.map((id) => timeEntries(ctx).require(id, 'time entry'));
  const blocked = entries.filter((entry) => entry.status !== 'submitted');
  if (blocked.length > 0) {
    throw new ConflictError('not-submitted', `Only submitted entries can be approved: ${blocked.map((entry) => entry.id).join(', ')}`);
  }
  if (entries.some((entry) => entry.technicianId === actorId)) {
    throw new ConflictError('self-approval', 'Technicians cannot approve their own time');
  }
  const approved = entries.map((entry) => timeEntries(ctx).update(entry.id, { status: 'approved' }));
  recordAudit(ctx, actorId, 'timesheet.approved', approved.map((entry) => entry.id).join(','), { count: approved.length });
  return approved;
}

export function rejectEntry(ctx: AppContext, actorId: string, entryId: string, reason: string): TimeEntry {
  const entry = timeEntries(ctx).require(entryId, 'time entry');
  if (entry.status !== 'submitted') throw new ConflictError('not-submitted', `Entry ${entryId} is ${entry.status}`);
  if (!reason.trim()) throw new DomainError('reason-required', 'Rejections need a reason');
  const updated = timeEntries(ctx).update(entryId, { status: 'rejected', rejectedReason: reason.trim() });
  recordAudit(ctx, actorId, 'timesheet.rejected', entryId, { reason });
  return updated;
}

/** Submitted entries waiting for approval, oldest first. */
export function pendingApprovals(ctx: AppContext): TimeEntry[] {
  return timeEntries(ctx)
    .find((entry) => entry.status === 'submitted')
    .sort((left, right) => left.start.localeCompare(right.start) || left.id.localeCompare(right.id));
}
''', imports=imports(ctx, "src/timesheets/approvals.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/time-entries.ts": ["timeEntries"],
        "src/adapters/store/audit.ts": ["recordAudit"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/timesheets/model.ts": ["type TimeEntry"],
    }))


def timesheet_summary(ctx):
    return render(ctx, '''
«imports»

export interface WeeklySummary {
  technicianId: string;
  weekStart: CalendarDate;
  hours: Record<EntryKind, number>;
  overtimeHours: number;
  days: DayTotals[];
}

/** Hours per entry kind for the week starting on `weekStart`, in quarter hours. */
export function weeklySummary(ctx: AppContext, technicianId: string, weekStart: CalendarDate): WeeklySummary {
  const entries = entriesForTechnician(ctx, technicianId, atMinute(weekStart, 0), atMinute(addDays(weekStart, 7), 0)).filter(
    (entry) => entry.status !== 'rejected' && entry.status !== 'open',
  );
  const minutes: Record<EntryKind, number> = { labor: 0, travel: 0, break: 0, admin: 0 };
  for (const entry of entries) minutes[entry.kind] += entry.minutes;
  const toHours = (value: number) => «quarter»;
  const days = dailyTotals(entries, ctx.config.timesheets);
  return {
    technicianId,
    weekStart,
    hours: {
      labor: toHours(minutes.labor),
      travel: toHours(minutes.travel),
      break: toHours(minutes.break),
      admin: toHours(minutes.admin),
    },
    overtimeHours: toHours(weeklyOvertime(days, ctx.config.timesheets)),
    days,
  };
}
''', quarter=rounding(ctx, "value / 60", 0.25, "nearest"),
                  imports=imports(ctx, "src/timesheets/summary.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/adapters/repositories/time-entries.ts": ["entriesForTechnician"],
                      "src/shared/rounding.ts": rounding_names(ctx, "nearest"),
                      "src/shared/time.ts": ["addDays", "atMinute", "type CalendarDate"],
                      "src/timesheets/model.ts": ["type EntryKind"],
                      "src/timesheets/overtime.ts": ["dailyTotals", "weeklyOvertime", "type DayTotals"],
                  }))


def inventory_model(ctx):
    return render(ctx, '''
«imports»

export interface Part {
  id: string;
  name: string;
  category: string;
  unitCostCents: Cents;
  listPriceCents: Cents;
  packSize: number;
  reorderPoint: number;
  reorderQuantity: number;
  discontinued: boolean;
  warrantyMonths: number;
}

export interface StockLevel {
  id: string;
  locationId: string;
  sku: string;
  onHand: number;
  reserved: number;
}

export type ReservationStatus = 'held' | 'backordered' | 'consumed' | 'released';

export interface Reservation {
  id: string;
  jobId: string;
  sku: string;
  quantity: number;
  locationId?: string;
  status: ReservationStatus;
  createdAt: Timestamp;
}

export interface Receipt {
  id: string;
  locationId: string;
  sku: string;
  quantity: number;
  unitCostCents: Cents;
  receivedAt: Timestamp;
  reference?: string;
}

export function available(level: StockLevel): number {
  return Math.max(0, level.onHand - level.reserved);
}
''', imports=imports(ctx, "src/inventory/model.ts", {
        "src/shared/money.ts": ["type Cents"],
        "src/shared/time.ts": ["type Timestamp"],
    }))


def inventory_events(ctx):
    return '''
/** Inventory event types published on the application bus. */
export const InventoryEvents = {
  reserved: 'parts.reserved',
  backordered: 'parts.backordered',
  available: 'parts.available',
  consumed: 'parts.consumed',
  lowStock: 'stock.low',
} as const;
'''


def inventory_catalog(ctx):
    return render(ctx, '''
«imports»

const partInput = object({
  id: string({ min: 3, max: 32, pattern: /^[A-Z0-9-]+$/ }),
  name: string({ min: 2, max: 120 }),
  category: string({ min: 2, max: 40 }),
  unitCostCents: integer({ min: 0 }),
  listPriceCents: integer({ min: 0 }),
  packSize: withDefault(integer({ min: 1, max: 1000 }), 1),
  reorderPoint: withDefault(integer({ min: 0 }), 0),
  reorderQuantity: withDefault(integer({ min: 0 }), 0),
  warrantyMonths: withDefault(integer({ min: 0, max: 120 }), 12),
});

export function addPart(ctx: AppContext, actorId: string, input: unknown): Part {
  const data = parseInput(partInput, input);
  if (parts(ctx).get(data.id)) throw new ConflictError('duplicate-sku', `Part ${data.id} already exists`);
  if (data.listPriceCents < data.unitCostCents) {
    throw new DomainError('price-below-cost', `List price of ${data.id} is below its cost`);
  }
  const part: Part = { ...data, discontinued: false };
  parts(ctx).insert(part);
  recordAudit(ctx, actorId, 'part.added', part.id);
  return part;
}

export function updateListPrice(ctx: AppContext, actorId: string, sku: string, listPriceCents: number): Part {
  const part = parts(ctx).require(sku, 'part');
  if (!Number.isInteger(listPriceCents) || listPriceCents < part.unitCostCents) {
    throw new DomainError('price-below-cost', `List price of ${sku} cannot be below cost`);
  }
  const updated = parts(ctx).update(sku, { listPriceCents });
  recordAudit(ctx, actorId, 'part.repriced', sku, { from: part.listPriceCents, to: listPriceCents });
  return updated;
}

export function discontinuePart(ctx: AppContext, actorId: string, sku: string): Part {
  const updated = parts(ctx).update(sku, { discontinued: true, reorderPoint: 0, reorderQuantity: 0 });
  recordAudit(ctx, actorId, 'part.discontinued', sku);
  return updated;
}
''', imports=imports(ctx, "src/inventory/catalog.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/parts.ts": ["parts"],
        "src/adapters/store/audit.ts": ["recordAudit"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/validate.ts": ["integer", "object", "parseInput", "string", "withDefault"],
        "src/inventory/model.ts": ["type Part"],
    }))


def inventory_stock(ctx):
    return render(ctx, '''
«imports»

/** Moving average cost after receiving `quantity` units at `unitCostCents`. */
export function averageCost(onHand: number, currentCost: Cents, quantity: number, unitCostCents: Cents): Cents {
  if (onHand + quantity <= 0) return unitCostCents;
  const blended = (Math.max(0, onHand) * currentCost + quantity * unitCostCents) / (Math.max(0, onHand) + quantity);
  return «average»;
}

/**
 * Books received stock, re-averages the part cost and fulfils backorders oldest first. Each work
 * order whose last backorder is fulfilled is announced with `parts.available`.
 */
export function receiveStock(
  ctx: AppContext,
  actorId: string,
  locationId: string,
  sku: string,
  quantity: number,
  unitCostCents: Cents,
  reference?: string,
): Receipt {
  if (!Number.isInteger(quantity) || quantity <= 0) throw new DomainError('invalid-quantity', 'Quantity must be a positive whole number');
  const part = parts(ctx).require(sku, 'part');
  const totalOnHand = levelsForSku(ctx, sku).reduce((sum, level) => sum + level.onHand, 0);
  parts(ctx).update(sku, { unitCostCents: averageCost(totalOnHand, part.unitCostCents, quantity, unitCostCents) });
  const level = levelAt(ctx, locationId, sku);
  saveLevel(ctx, { ...level, onHand: level.onHand + quantity });
  const receipt: Receipt = {
    id: ctx.ids.next('RCV'),
    locationId,
    sku,
    quantity,
    unitCostCents,
    receivedAt: ctx.clock.now(),
    reference,
  };
  ctx.store.collection<Receipt>('receipts').insert(receipt);
  recordAudit(ctx, actorId, 'stock.received', receipt.id, { sku, quantity, locationId });
  fulfilBackorders(ctx, locationId, sku);
  return receipt;
}

function fulfilBackorders(ctx: AppContext, locationId: string, sku: string): void {
  const touched = new Set<string>();
  for (const reservation of backorderedReservations(ctx, sku)) {
    const level = levelAt(ctx, locationId, sku);
    if (available(level) < reservation.quantity) break;
    saveLevel(ctx, { ...level, reserved: level.reserved + reservation.quantity });
    reservations(ctx).update(reservation.id, { status: 'held', locationId });
    touched.add(reservation.jobId);
  }
  for (const jobId of touched) {
    const waiting = reservationsForJob(ctx, jobId).some((reservation) => reservation.status === 'backordered');
    if (!waiting) ctx.bus.emit({ type: InventoryEvents.available, at: ctx.clock.now(), jobId, sku });
  }
}

export function transferStock(ctx: AppContext, actorId: string, from: string, to: string, sku: string, quantity: number): void {
  if (from === to) throw new DomainError('same-location', 'Source and destination must differ');
  const source = levelAt(ctx, from, sku);
  if (available(source) < quantity) {
    throw new ConflictError('insufficient-stock', `Only ${available(source)} of ${sku} available at ${from}`);
  }
  const target = levelAt(ctx, to, sku);
  saveLevel(ctx, { ...source, onHand: source.onHand - quantity });
  saveLevel(ctx, { ...target, onHand: target.onHand + quantity });
  recordAudit(ctx, actorId, 'stock.transferred', `${from}->${to}`, { sku, quantity });
  fulfilBackorders(ctx, to, sku);
}

/** Corrects the counted quantity after a stocktake; reserved stock can never be counted away. */
export function adjustCount(ctx: AppContext, actorId: string, locationId: string, sku: string, counted: number): StockLevel {
  const level = levelAt(ctx, locationId, sku);
  if (counted < level.reserved) {
    throw new ConflictError('below-reserved', `${counted} is below the ${level.reserved} reserved units of ${sku}`);
  }
  const updated = saveLevel(ctx, { ...level, onHand: counted });
  recordAudit(ctx, actorId, 'stock.counted', level.id, { from: level.onHand, to: counted });
  const part = parts(ctx).get(sku);
  if (part && available(updated) <= part.reorderPoint) {
    ctx.bus.emit({ type: InventoryEvents.lowStock, at: ctx.clock.now(), sku, locationId, available: available(updated) });
  }
  return updated;
}
''', average=rounding(ctx, "blended", 1, "nearest"),
                  imports=imports(ctx, "src/inventory/stock.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/adapters/repositories/parts.ts": ["parts"],
                      "src/adapters/repositories/reservations.ts": ["backorderedReservations", "reservations",
                                                                   "reservationsForJob"],
                      "src/adapters/repositories/stock.ts": ["levelAt", "levelsForSku", "saveLevel"],
                      "src/adapters/store/audit.ts": ["recordAudit"],
                      "src/shared/errors.ts": ["ConflictError", "DomainError"],
                      "src/shared/money.ts": ["type Cents"],
                      "src/shared/rounding.ts": rounding_names(ctx, "nearest"),
                      "src/inventory/events.ts": ["InventoryEvents"],
                      "src/inventory/model.ts": ["available", "type Receipt", "type StockLevel"],
                  }))


def inventory_reservations(ctx):
    return render(ctx, '''
«imports»

export interface PartRequest {
  sku: string;
  quantity: number;
}

/**
 * Reserves parts for a work order: from the assigned technician's van first, then the default
 * warehouse. Anything not in stock is backordered and the work order is put on hold.
 */
export function reservePartsForJob(ctx: AppContext, actorId: string, jobId: string, requests: readonly PartRequest[]): Reservation[] {
  const job = jobs(ctx).require(jobId, 'work order');
  const van = job.technicianId ? technicians(ctx).get(job.technicianId)?.vanLocationId : undefined;
  const sources = [van, ctx.config.inventory.defaultWarehouse].filter((value): value is string => value !== undefined);
  const created: Reservation[] = [];
  for (const request of requests) {
    const part = parts(ctx).require(request.sku, 'part');
    if (part.discontinued) throw new ConflictError('discontinued', `Part ${part.id} is discontinued`);
    if (!Number.isInteger(request.quantity) || request.quantity <= 0) {
      throw new DomainError('invalid-quantity', `Invalid quantity for ${part.id}`);
    }
    const source = sources.find((locationId) => available(levelAt(ctx, locationId, part.id)) >= request.quantity);
    const reservation: Reservation = {
      id: ctx.ids.next('RES'),
      jobId,
      sku: part.id,
      quantity: request.quantity,
      locationId: source,
      status: source ? 'held' : 'backordered',
      createdAt: ctx.clock.now(),
    };
    if (source) {
      const level = levelAt(ctx, source, part.id);
      saveLevel(ctx, { ...level, reserved: level.reserved + request.quantity });
    }
    created.push(reservations(ctx).insert(reservation));
  }
  const backordered = created.filter((reservation) => reservation.status === 'backordered');
  for (const reservation of created) {
    ctx.bus.emit({
      type: reservation.status === 'held' ? InventoryEvents.reserved : InventoryEvents.backordered,
      at: ctx.clock.now(),
      jobId,
      sku: reservation.sku,
      quantity: reservation.quantity,
    });
  }
  const current = jobs(ctx).require(jobId, 'work order');
  if (backordered.length > 0 && current.status !== 'on_hold' && current.status !== 'new') {
    «holdJob»(ctx, actorId, jobId, 'awaiting_parts', `backordered: ${backordered.map((item) => item.sku).join(', ')}`);
  }
  return created;
}

export function releaseReservations(ctx: AppContext, jobId: string): number {
  let released = 0;
  for (const reservation of reservationsForJob(ctx, jobId)) {
    if (reservation.status === 'held' && reservation.locationId) {
      const level = levelAt(ctx, reservation.locationId, reservation.sku);
      saveLevel(ctx, { ...level, reserved: Math.max(0, level.reserved - reservation.quantity) });
    }
    if (reservation.status === 'held' || reservation.status === 'backordered') {
      reservations(ctx).update(reservation.id, { status: 'released' });
      released += 1;
    }
  }
  return released;
}

/**
 * Consumes reserved stock for the parts reported on completion. Parts used without a reservation
 * are taken from the technician's van.
 */
export function consumeParts(ctx: AppContext, jobId: string, used: ReadonlyArray<{ sku: string; quantity: number }>, vanLocationId?: string): void {
  for (const line of used) {
    let remaining = line.quantity;
    for (const reservation of reservationsForJob(ctx, jobId).filter((item) => item.sku === line.sku && item.status === 'held')) {
      if (remaining <= 0 || !reservation.locationId) break;
      const take = Math.min(remaining, reservation.quantity);
      const level = levelAt(ctx, reservation.locationId, line.sku);
      saveLevel(ctx, { ...level, onHand: level.onHand - take, reserved: Math.max(0, level.reserved - reservation.quantity) });
      reservations(ctx).update(reservation.id, { status: 'consumed', quantity: take });
      remaining -= take;
    }
    if (remaining > 0 && vanLocationId) {
      const level = levelAt(ctx, vanLocationId, line.sku);
      saveLevel(ctx, { ...level, onHand: Math.max(0, level.onHand - remaining) });
    }
    ctx.bus.emit({ type: InventoryEvents.consumed, at: ctx.clock.now(), jobId, sku: line.sku, quantity: line.quantity });
  }
  releaseReservations(ctx, jobId);
}

export function registerInventorySubscriptions(ctx: AppContext): void {
  ctx.bus.on(«JobEvents».completed, (event) => {
    const jobId = String(event.jobId);
    const used = Array.isArray(event.partsUsed) ? (event.partsUsed as Array<{ sku: string; quantity: number }>) : [];
    const technicianId = typeof event.technicianId === 'string' ? event.technicianId : undefined;
    consumeParts(ctx, jobId, used, technicianId ? technicians(ctx).get(technicianId)?.vanLocationId : undefined);
  });
  ctx.bus.on(«JobEvents».cancelled, (event) => {
    releaseReservations(ctx, String(event.jobId));
  });
}
''', imports=imports(ctx, "src/inventory/reservations.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/jobs.ts": ["jobs"],
        "src/adapters/repositories/parts.ts": ["parts"],
        "src/adapters/repositories/reservations.ts": ["reservations", "reservationsForJob"],
        "src/adapters/repositories/stock.ts": ["levelAt", "saveLevel"],
        "src/adapters/repositories/technicians.ts": ["technicians"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/inventory/events.ts": ["InventoryEvents"],
        "src/inventory/model.ts": ["available", "type Reservation"],
    }, jobs=("holdJob", "JobEvents")))


def inventory_reorder(ctx):
    return render(ctx, '''
«imports»

export interface ReorderSuggestion {
  sku: string;
  locationId: string;
  available: number;
  quantity: number;
  packs: number;
}

/**
 * Parts at or below their reorder point. The suggested quantity covers the reorder quantity
 * plus open backorders and is rounded up to whole packs.
 */
export function reorderSuggestions(ctx: AppContext, locationId: string): ReorderSuggestion[] {
  const result: ReorderSuggestion[] = [];
  for (const part of parts(ctx).find((candidate) => !candidate.discontinued && candidate.reorderQuantity > 0)) {
    const level = levelAt(ctx, locationId, part.id);
    const free = available(level);
    const backordered = backorderedReservations(ctx, part.id).reduce((sum, reservation) => sum + reservation.quantity, 0);
    if (free > part.reorderPoint && backordered === 0) continue;
    const quantity = «packs»;
    result.push({ sku: part.id, locationId, available: free, quantity, packs: «count»(quantity, part.packSize) });
  }
  return result.sort((left, right) => left.available - right.available || left.sku.localeCompare(right.sku));
}

export interface VanTopUp {
  sku: string;
  quantity: number;
}

/**
 * Stock to move from the warehouse to a van so it holds the safety stock of each part the
 * technician used in the last 30 days. Only whole packs are moved.
 */
export function vanTopUp(ctx: AppContext, vanLocationId: string, recentlyUsed: ReadonlyMap<string, number>): VanTopUp[] {
  const safety = ctx.config.inventory.vanSafetyStock;
  const result: VanTopUp[] = [];
  for (const [sku, used] of recentlyUsed) {
    const part = parts(ctx).get(sku);
    if (!part || part.discontinued) continue;
    const target = Math.max(safety, Math.ceil(used / 2));
    const missing = target - levelAt(ctx, vanLocationId, sku).onHand;
    if (missing <= 0) continue;
    const quantity = «top_up»;
    if (quantity > 0) result.push({ sku, quantity });
  }
  return result.sort((left, right) => left.sku.localeCompare(right.sku));
}
''', packs=rounding(ctx, "part.reorderQuantity + backordered", "part.packSize", "up"),
                  count="stepsToCover" if ctx.quantize else "incrementsToCover",
                  top_up=rounding(ctx, "missing", "part.packSize", "down"),
                  imports=imports(ctx, "src/inventory/reorder.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/adapters/repositories/parts.ts": ["parts"],
                      "src/adapters/repositories/reservations.ts": ["backorderedReservations"],
                      "src/adapters/repositories/stock.ts": ["levelAt"],
                      "src/shared/rounding.ts": rounding_names(ctx, "up", "down")
                      + ["stepsToCover" if ctx.quantize else "incrementsToCover"],
                      "src/inventory/model.ts": ["available"],
                  }))


def inventory_valuation(ctx):
    return render(ctx, '''
«imports»

export interface ValuationLine {
  sku: string;
  onHand: number;
  unitCostCents: Cents;
  valueCents: Cents;
}

export function stockValuation(ctx: AppContext, locationId?: string): { lines: ValuationLine[]; totalCents: Cents } {
  const levels = stockLevels(ctx).find((level) => locationId === undefined || level.locationId === locationId);
  const bySku = new Map<string, number>();
  for (const level of levels) bySku.set(level.sku, (bySku.get(level.sku) ?? 0) + level.onHand);
  const lines = [...bySku.entries()]
    .filter(([, onHand]) => onHand > 0)
    .map(([sku, onHand]) => {
      const unitCostCents = parts(ctx).get(sku)?.unitCostCents ?? 0;
      return { sku, onHand, unitCostCents, valueCents: multiplyCents(unitCostCents, onHand) };
    })
    .sort((left, right) => left.sku.localeCompare(right.sku));
  return { lines, totalCents: sumCents(lines.map((line) => line.valueCents)) };
}
''', imports=imports(ctx, "src/inventory/valuation.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/parts.ts": ["parts"],
        "src/adapters/repositories/stock.ts": ["stockLevels"],
        "src/shared/money.ts": ["multiplyCents", "sumCents", "type Cents"],
    }))


def generate(ctx):
    ctx.add("src/timesheets/model.ts", timesheet_model(ctx))
    ctx.add("src/timesheets/clock.ts", timesheet_clock(ctx))
    ctx.add("src/timesheets/entries.ts", timesheet_entries(ctx))
    ctx.add("src/timesheets/mileage.ts", timesheet_mileage(ctx))
    ctx.add("src/timesheets/overtime.ts", timesheet_overtime(ctx))
    ctx.add("src/timesheets/approvals.ts", timesheet_approvals(ctx))
    ctx.add("src/timesheets/summary.ts", timesheet_summary(ctx))
    ctx.add("src/inventory/model.ts", inventory_model(ctx))
    ctx.add("src/inventory/events.ts", inventory_events(ctx))
    ctx.add("src/inventory/catalog.ts", inventory_catalog(ctx))
    ctx.add("src/inventory/stock.ts", inventory_stock(ctx))
    ctx.add("src/inventory/reservations.ts", inventory_reservations(ctx))
    ctx.add("src/inventory/reorder.ts", inventory_reorder(ctx))
    ctx.add("src/inventory/valuation.ts", inventory_valuation(ctx))
