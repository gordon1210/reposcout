"""Technicians, scheduling and dispatch."""

from .core import imports, render, rounding, rounding_names


def technician_model(ctx):
    shift = "  shift: ShiftPattern;\n" if ctx.early else ""
    groups = {
        "src/shared/geo.ts": ["type Coordinates"],
        "src/shared/money.ts": ["type Cents"],
        "src/shared/time.ts": ["type CalendarDate"],
    }
    if ctx.early:
        groups["src/config/schema.ts"] = ["type ShiftPattern"]
    return render(ctx, '''
«imports»

export interface Certification {
  skill: string;
  /** Certifications without an expiry date never lapse. */
  expiresOn?: CalendarDate;
}

export interface Absence {
  from: CalendarDate;
  to: CalendarDate;
  kind: 'leave' | 'sick' | 'training';
}

export interface Technician {
  id: string;
  name: string;
  email: string;
  region: string;
  roaming: boolean;
  certifications: Certification[];
  home: Coordinates;
  vanLocationId: string;
  active: boolean;
  hourlyCostCents: Cents;
  absences: Absence[];
  maxDailyAppointments?: number;
«shift»}
''', imports=imports(ctx, "src/technicians/model.ts", groups), shift=shift)


def technician_skills(ctx):
    return render(ctx, '''
«imports»

export function hasSkill(technician: Technician, skill: string, on: CalendarDate): boolean {
  return technician.certifications.some(
    (certification) => certification.skill === skill && (certification.expiresOn === undefined || certification.expiresOn >= on),
  );
}

/** Required skills the technician does not hold a valid certification for on `on`. */
export function missingSkills(technician: Technician, required: readonly string[], on: CalendarDate): string[] {
  return required.filter((skill) => !hasSkill(technician, skill, on));
}

export interface ExpiringCertification {
  technicianId: string;
  skill: string;
  expiresOn: CalendarDate;
  daysLeft: number;
}

export function expiringCertifications(ctx: AppContext, on: CalendarDate, withinDays: number): ExpiringCertification[] {
  const result: ExpiringCertification[] = [];
  for (const technician of activeTechnicians(ctx)) {
    for (const certification of technician.certifications) {
      if (certification.expiresOn === undefined || certification.expiresOn < on) continue;
      const daysLeft = daysBetween(on, certification.expiresOn);
      if (daysLeft <= withinDays) {
        result.push({ technicianId: technician.id, skill: certification.skill, expiresOn: certification.expiresOn, daysLeft });
      }
    }
  }
  return result.sort((left, right) => left.daysLeft - right.daysLeft || left.technicianId.localeCompare(right.technicianId));
}
''', imports=imports(ctx, "src/technicians/skills.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/technicians.ts": ["activeTechnicians"],
        "src/shared/time.ts": ["daysBetween", "type CalendarDate"],
        "src/technicians/model.ts": ["type Technician"],
    }))


def technician_service(ctx):
    shift_field = "    shift: data.shift ?? ctx.config.scheduling.defaultShift,\n" if ctx.early else ""
    shift_input = "  shift: optional(oneOf(['early', 'standard'] as const)),\n" if ctx.early else ""
    available = '''export function isAvailable(technician: Technician, on: CalendarDate): boolean {
  return technician.active && !technician.absences.some((absence) => absence.from <= on && on <= absence.to);
}''' if not ctx.refactor else '''export function isAvailable(technician: Technician, on: CalendarDate): boolean {
  if (!technician.active) return false;
  return technician.absences.every((absence) => on < absence.from || absence.to < on);
}'''
    return render(ctx, '''
«imports»

const technicianInput = object({
  name: string({ min: 2, max: 80 }),
  email: string({ pattern: /^[^@\\s]+@[^@\\s]+$/ }),
  region: string({ min: 1, max: 40 }),
  roaming: withDefault(boolean(), false),
  home: object({ lat: number({ min: -90, max: 90 }), lon: number({ min: -180, max: 180 }) }),
  hourlyCostCents: integer({ min: 0 }),
  certifications: withDefault(
    array(object({ skill: string({ min: 1, max: 40 }), expiresOn: optional(date()) }), { max: 40 }),
    [],
  ),
  maxDailyAppointments: optional(integer({ min: 1, max: 12 })),
«shift_input»});

export function createTechnician(ctx: AppContext, actorId: string, input: unknown): Technician {
  const data = parseInput(technicianInput, input);
  const email = data.email.toLowerCase();
  if (technicians(ctx).first((technician) => technician.email === email)) {
    throw new ConflictError('duplicate-email', `A technician with ${email} already exists`);
  }
  const id = ctx.ids.next('TEC');
  const technician: Technician = {
    id,
    name: data.name,
    email,
    region: data.region,
    roaming: data.roaming,
    certifications: data.certifications,
    home: data.home,
    vanLocationId: `VAN-${id.slice(4)}`,
    active: true,
    hourlyCostCents: data.hourlyCostCents,
    absences: [],
    maxDailyAppointments: data.maxDailyAppointments,
«shift_field»  };
  technicians(ctx).insert(technician);
  recordAudit(ctx, actorId, 'technician.created', id);
  return technician;
}

export function updateCertifications(
  ctx: AppContext,
  actorId: string,
  technicianId: string,
  certifications: Certification[],
): Technician {
  const bySkill = new Map<string, Certification>();
  for (const certification of certifications) {
    const existing = bySkill.get(certification.skill);
    const keepExisting =
      existing !== undefined &&
      (existing.expiresOn === undefined || (certification.expiresOn !== undefined && existing.expiresOn > certification.expiresOn));
    if (!keepExisting) bySkill.set(certification.skill, certification);
  }
  const updated = technicians(ctx).update(technicianId, { certifications: [...bySkill.values()] });
  recordAudit(ctx, actorId, 'technician.certifications', technicianId, { count: bySkill.size });
  return updated;
}

export function recordAbsence(ctx: AppContext, actorId: string, technicianId: string, absence: Absence): Technician {
  if (absence.to < absence.from) throw new DomainError('invalid-absence', 'An absence cannot end before it starts');
  const technician = technicians(ctx).require(technicianId, 'technician');
  const overlaps = technician.absences.some((existing) => existing.from <= absence.to && absence.from <= existing.to);
  if (overlaps) throw new ConflictError('overlapping-absence', 'The absence overlaps an existing one');
  const updated = technicians(ctx).update(technicianId, {
    absences: [...technician.absences, absence].sort((left, right) => left.from.localeCompare(right.from)),
  });
  recordAudit(ctx, actorId, 'technician.absence', technicianId, { ...absence });
  return updated;
}

export function deactivateTechnician(ctx: AppContext, actorId: string, technicianId: string, openWork: number): Technician {
  if (openWork > 0) {
    throw new ConflictError('technician-has-work', `Reassign ${openWork} open work orders before deactivating`, { openWork });
  }
  const updated = technicians(ctx).update(technicianId, { active: false });
  recordAudit(ctx, actorId, 'technician.deactivated', technicianId);
  return updated;
}

«available»
''', available=available, shift_field=shift_field, shift_input=shift_input,
                  imports=imports(ctx, "src/technicians/service.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/adapters/repositories/technicians.ts": ["technicians"],
                      "src/adapters/store/audit.ts": ["recordAudit"],
                      "src/shared/errors.ts": ["ConflictError", "DomainError"],
                      "src/shared/time.ts": ["type CalendarDate"],
                      "src/shared/validate.ts": ["array", "boolean", "date", "integer", "number", "object",
                                                 "optional", "parseInput", "string", "withDefault"]
                      + (["oneOf"] if ctx.early else []),
                      "src/technicians/model.ts": ["type Absence", "type Certification", "type Technician"],
                  }))


def scheduling_model(ctx):
    return render(ctx, '''
«imports»

export type AppointmentStatus = 'booked' | 'confirmed' | 'missed' | 'cancelled' | 'completed';

export interface Appointment {
  id: string;
  jobId: string;
  technicianId: string;
  start: Timestamp;
  end: Timestamp;
  windowStart: Timestamp;
  windowEnd: Timestamp;
  travelMinutes: number;
  status: AppointmentStatus;
  createdAt: Timestamp;
  cancelReason?: string;
}

export const AppointmentEvents = {
  booked: 'appointment.booked',
  rescheduled: 'appointment.rescheduled',
  cancelled: 'appointment.cancelled',
  noAccess: 'appointment.no-access',
  rebooked: 'appointment.rebooked',
} as const;

export interface Slot {
  start: Timestamp;
  end: Timestamp;
  travelMinutes: number;
}
''', imports=imports(ctx, "src/scheduling/model.ts", {"src/shared/time.ts": ["type Timestamp"]}))


def scheduling_travel(ctx):
    return render(ctx, '''
«imports»

/** Driving time between two points at the configured average speed, in whole five-minute steps. */
export function travelMinutes(from: Coordinates, to: Coordinates, speedKmh: number): number {
  if (speedKmh <= 0) throw new RangeError('Travel speed must be positive');
  const km = roadDistanceKm(from, to);
  if (km < 0.5) return 0;
  return «rounded»;
}

export function travelKm(from: Coordinates, to: Coordinates): number {
  return Math.round(roadDistanceKm(from, to) * 10) / 10;
}
''', rounded=rounding(ctx, "(km / speedKmh) * 60", 5, "up"),
                  imports=imports(ctx, "src/scheduling/travel-time.ts", {
                      "src/shared/geo.ts": ["roadDistanceKm", "type Coordinates"],
                      "src/shared/rounding.ts": rounding_names(ctx, "up"),
                  }))


def scheduling_windows(ctx):
    return render(ctx, '''
«imports»

export interface ArrivalWindow {
  windowStart: Timestamp;
  windowEnd: Timestamp;
}

/**
 * The window promised to the customer: the planned start rounded down to the window size and
 * one window long, clipped to the working day.
 */
export function arrivalWindow(ctx: AppContext, start: Timestamp): ArrivalWindow {
  const { windowMinutes, dayStartMinute, dayEndMinute } = ctx.config.scheduling;
  const date = dateOf(start);
  const opening = «down»;
  const from = Math.max(dayStartMinute, opening);
  const to = Math.min(dayEndMinute, from + windowMinutes);
  return { windowStart: atMinute(date, from), windowEnd: atMinute(date, to) };
}

/** Customer-facing text such as "between 10:00 and 12:00". */
export function describeWindow(window: ArrivalWindow): string {
  return `between ${formatClock(minuteOfDay(window.windowStart))} and ${formatClock(minuteOfDay(window.windowEnd))}`;
}

/** Windows of the day that can still be offered, as minutes of the day, aligned to the window size. */
export function windowStarts(ctx: AppContext): number[] {
  const { windowMinutes, dayStartMinute, dayEndMinute } = ctx.config.scheduling;
  const first = «up»;
  const starts: number[] = [];
  for (let minute = first; minute + windowMinutes <= dayEndMinute; minute += windowMinutes) starts.push(minute);
  return starts;
}
''', down=rounding(ctx, "minuteOfDay(start)", "windowMinutes", "down"),
                  up=rounding(ctx, "dayStartMinute", "windowMinutes", "up"),
                  imports=imports(ctx, "src/scheduling/windows.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/shared/rounding.ts": rounding_names(ctx, "down", "up"),
                      "src/shared/time.ts": ["atMinute", "dateOf", "formatClock", "minuteOfDay", "type Timestamp"],
                  }))


def scheduling_slots(ctx):
    if ctx.early:
        bounds = '''/** Working minutes of the technician's shift on a date, or undefined when not working. */
export function workingBounds(ctx: AppContext, technician: Technician, date: CalendarDate): [number, number] | undefined {
  if (!isWorkingDay(businessCalendar(ctx), date) || !isAvailable(technician, date)) return undefined;
  const shift = ctx.config.scheduling.shifts[technician.shift] ?? ctx.config.scheduling.shifts.standard;
  return [shift.startMinute, shift.endMinute];
}'''
    else:
        bounds = '''/** Working minutes of the technician on a date, or undefined when not working. */
export function workingBounds(ctx: AppContext, technician: Technician, date: CalendarDate): [number, number] | undefined {
  if (!isWorkingDay(businessCalendar(ctx), date) || !isAvailable(technician, date)) return undefined;
  return [ctx.config.scheduling.dayStartMinute, ctx.config.scheduling.dayEndMinute];
}'''
    return render(ctx, '''
«imports»

«bounds»

interface Busy {
  from: number;
  to: number;
  location: Coordinates;
}

function busyPeriods(ctx: AppContext, technicianId: string, date: CalendarDate, sites: Map<string, Coordinates>): Busy[] {
  const dayStart = toEpochMinutes(atMinute(date, 0));
  return appointmentsForTechnician(ctx, technicianId, date)
    .filter((appointment) => appointment.status !== 'missed')
    .map((appointment) => ({
      from: toEpochMinutes(appointment.start) - dayStart,
      to: toEpochMinutes(appointment.end) - dayStart,
      location: sites.get(appointment.jobId) ?? { lat: 0, lon: 0 },
    }));
}

/**
 * Start times on `date` at which the technician can begin `durationMinutes` of work at
 * `destination`, respecting existing appointments, the buffer between visits and driving time
 * from the previous stop. The first visit of the day may begin at the start of working hours;
 * its reported travel time is measured from the technician's home.
 */
export function availableSlots(
  ctx: AppContext,
  technician: Technician,
  date: CalendarDate,
  durationMinutes: number,
  destination: Coordinates,
  jobLocations: Map<string, Coordinates>,
): Slot[] {
  const bounds = workingBounds(ctx, technician, date);
  if (!bounds) return [];
  const { slotMinutes, bufferMinutes, travelSpeedKmh, maxDailyAppointments } = ctx.config.scheduling;
  const busy = busyPeriods(ctx, technician.id, date, jobLocations);
  if (busy.length >= (technician.maxDailyAppointments ?? maxDailyAppointments)) return [];
  const slots: Slot[] = [];
  for (let start = bounds[0]; start + durationMinutes <= bounds[1]; start += slotMinutes) {
    const previous = busy.filter((period) => period.to <= start).sort((left, right) => right.to - left.to)[0];
    const origin = previous?.location ?? technician.home;
    const drive = travelMinutes(origin, destination, travelSpeedKmh);
    const earliest = previous ? previous.to + bufferMinutes + drive : bounds[0];
    const aligned = «aligned»;
    if (aligned !== start) continue;
    const end = start + durationMinutes;
    const next = busy.filter((period) => period.from >= end).sort((left, right) => left.from - right.from)[0];
    if (next && travelMinutes(destination, next.location, travelSpeedKmh) + bufferMinutes > next.from - end) continue;
    if (busy.some((period) => period.from < end && start < period.to)) continue;
    slots.push({ start: atMinute(date, start), end: atMinute(date, end), travelMinutes: drive });
  }
  return slots;
}

export function firstAvailableSlot(
  ctx: AppContext,
  technician: Technician,
  from: CalendarDate,
  durationMinutes: number,
  destination: Coordinates,
  jobLocations: Map<string, Coordinates>,
): Slot | undefined {
  for (let offset = 0; offset < ctx.config.scheduling.horizonDays; offset += 1) {
    const date = addDays(from, offset);
    const [slot] = availableSlots(ctx, technician, date, durationMinutes, destination, jobLocations);
    if (slot) return slot;
  }
  return undefined;
}
''', bounds=bounds,
                  aligned=rounding(ctx, "Math.max(start, earliest)", "slotMinutes", "up"),
                  imports=imports(ctx, "src/scheduling/slots.ts", {
                      "src/context.ts": ["businessCalendar", "type AppContext"],
                      "src/adapters/repositories/appointments.ts": ["appointmentsForTechnician"],
                      "src/shared/calendar.ts": ["isWorkingDay"],
                      "src/shared/geo.ts": ["type Coordinates"],
                      "src/shared/rounding.ts": rounding_names(ctx, "up"),
                      "src/shared/time.ts": ["addDays", "atMinute", "toEpochMinutes", "type CalendarDate"],
                      "src/technicians/model.ts": ["type Technician"],
                      "src/technicians/service.ts": ["isAvailable"],
                      "src/scheduling/model.ts": ["type Slot"],
                      "src/scheduling/travel-time.ts": ["travelMinutes"],
                  }))


def scheduling_locations(ctx):
    return render(ctx, '''
«imports»

/** Site coordinates of every work order with a live appointment for the technician on `date`. */
export function appointmentLocations(ctx: AppContext, technicianId: string, date: CalendarDate): Map<string, Coordinates> {
  const locations = new Map<string, Coordinates>();
  for (const appointment of appointmentsForTechnician(ctx, technicianId, date)) {
    const job = jobs(ctx).get(appointment.jobId);
    const site = job ? sites(ctx).get(job.siteId) : undefined;
    if (site) locations.set(appointment.jobId, site.address.location);
  }
  return locations;
}

export function locationOfJob(ctx: AppContext, jobId: string): Coordinates {
  const job = jobs(ctx).require(jobId, 'work order');
  return sites(ctx).require(job.siteId, 'site').address.location;
}
''', imports=imports(ctx, "src/scheduling/locations.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/appointments.ts": ["appointmentsForTechnician"],
        "src/adapters/repositories/jobs.ts": ["jobs"],
        "src/adapters/repositories/sites.ts": ["sites"],
        "src/shared/geo.ts": ["type Coordinates"],
        "src/shared/time.ts": ["type CalendarDate"],
    }))


def scheduling_service(ctx):
    return render(ctx, '''
«imports»

export interface BookingRequest {
  jobId: string;
  technicianId: string;
  start: Timestamp;
  durationMinutes?: number;
}

function requireOpenSlot(ctx: AppContext, request: BookingRequest, durationMinutes: number): Slot {
  const technician = technicians(ctx).require(request.technicianId, 'technician');
  const date = dateOf(request.start);
  const destination = locationOfJob(ctx, request.jobId);
  const locations = appointmentLocations(ctx, technician.id, date);
  locations.delete(request.jobId);
  const slot = availableSlots(ctx, technician, date, durationMinutes, destination, locations).find(
    (candidate) => candidate.start === request.start,
  );
  if (!slot) {
    throw new ConflictError('slot-unavailable', `${technician.name} is not available at ${request.start}`, {
      technicianId: technician.id,
      start: request.start,
    });
  }
  return slot;
}

/**
 * Books a visit: checks the slot, assigns the technician and schedules the work order. A work
 * order has at most one live appointment; rebooking after a missed visit is reported separately
 * so SLA clocks can resume.
 */
export function bookAppointment(ctx: AppContext, actorId: string, request: BookingRequest): Appointment {
  const job = «requireJob»(ctx, request.jobId);
  if (!isOpenStatus(job.status) || job.status === 'in_progress') {
    throw new ConflictError('not-schedulable', `Work order ${job.number} is ${job.status}`);
  }
  if (appointmentForJob(ctx, job.id)) {
    throw new ConflictError('already-booked', `Work order ${job.number} already has an appointment`);
  }
  const handler = ctx.workTypes.resolve(job.workType);
  const duration = request.durationMinutes ?? handler.defaultDurationMinutes;
  const slot = requireOpenSlot(ctx, request, duration);
  const window = arrivalWindow(ctx, slot.start);
  const missedBefore = appointments(ctx).count((appointment) => appointment.jobId === job.id && appointment.status === 'missed');
  const appointment: Appointment = {
    id: ctx.ids.next('APT'),
    jobId: job.id,
    technicianId: request.technicianId,
    start: slot.start,
    end: slot.end,
    ...window,
    travelMinutes: slot.travelMinutes,
    status: 'booked',
    createdAt: ctx.clock.now(),
  };
  appointments(ctx).insert(appointment);
  try {
    if (job.status === 'on_hold') «resumeJob»(ctx, actorId, job.id, 'appointment booked');
    if («requireJob»(ctx, job.id).technicianId !== request.technicianId) «assignJob»(ctx, actorId, job.id, request.technicianId);
    «scheduleJob»(ctx, actorId, job.id, slot.start, duration);
  } catch (error) {
    appointments(ctx).remove(appointment.id);
    throw error;
  }
  ctx.bus.emit({
    type: missedBefore > 0 ? AppointmentEvents.rebooked : AppointmentEvents.booked,
    at: ctx.clock.now(),
    appointmentId: appointment.id,
    jobId: job.id,
    technicianId: appointment.technicianId,
    windowStart: appointment.windowStart,
    windowEnd: appointment.windowEnd,
  });
  return appointment;
}

export function cancelAppointment(ctx: AppContext, actorId: string, appointmentId: string, reason: string): Appointment {
  const appointment = appointments(ctx).require(appointmentId, 'appointment');
  if (appointment.status !== 'booked' && appointment.status !== 'confirmed') {
    throw new ConflictError('appointment-not-live', `Appointment ${appointmentId} is ${appointment.status}`);
  }
  const updated = appointments(ctx).update(appointmentId, { status: 'cancelled', cancelReason: reason });
  ctx.bus.emit({ type: AppointmentEvents.cancelled, at: ctx.clock.now(), appointmentId, jobId: appointment.jobId, actorId, reason });
  return updated;
}

export function rescheduleAppointment(ctx: AppContext, actorId: string, appointmentId: string, start: Timestamp): Appointment {
  const current = appointments(ctx).require(appointmentId, 'appointment');
  const duration = minutesBetween(current.start, current.end);
  cancelAppointment(ctx, actorId, appointmentId, 'rescheduled');
  try {
    const next = bookAppointment(ctx, actorId, { jobId: current.jobId, technicianId: current.technicianId, start, durationMinutes: duration });
    ctx.bus.emit({ type: AppointmentEvents.rescheduled, at: ctx.clock.now(), appointmentId: next.id, previousId: appointmentId, jobId: current.jobId });
    return next;
  } catch (error) {
    appointments(ctx).update(appointmentId, { status: current.status, cancelReason: undefined });
    throw error;
  }
}

export function confirmAppointment(ctx: AppContext, appointmentId: string): Appointment {
  const appointment = appointments(ctx).require(appointmentId, 'appointment');
  if (appointment.status !== 'booked') throw new ConflictError('appointment-not-booked', `Appointment ${appointmentId} is ${appointment.status}`);
  return appointments(ctx).update(appointmentId, { status: 'confirmed' });
}

/** The technician arrived but could not get in; the visit is marked missed and must be rebooked. */
export function recordNoAccess(ctx: AppContext, actorId: string, appointmentId: string, note: string): Appointment {
  const appointment = appointments(ctx).require(appointmentId, 'appointment');
  if (appointment.status !== 'booked' && appointment.status !== 'confirmed') {
    throw new ConflictError('appointment-not-live', `Appointment ${appointmentId} is ${appointment.status}`);
  }
  const updated = appointments(ctx).update(appointmentId, { status: 'missed', cancelReason: note });
  ctx.bus.emit({ type: AppointmentEvents.noAccess, at: ctx.clock.now(), appointmentId, jobId: appointment.jobId, actorId, note });
  return updated;
}
''', imports=imports(ctx, "src/scheduling/service.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/appointments.ts": ["appointmentForJob", "appointments"],
        "src/adapters/repositories/technicians.ts": ["technicians"],
        "src/shared/errors.ts": ["ConflictError"],
        "src/shared/time.ts": ["dateOf", "minutesBetween", "type Timestamp"],
        "src/scheduling/locations.ts": ["appointmentLocations", "locationOfJob"],
        "src/scheduling/model.ts": ["AppointmentEvents", "type Appointment", "type Slot"],
        "src/scheduling/slots.ts": ["availableSlots"],
        "src/scheduling/windows.ts": ["arrivalWindow"],
    }, jobs=("requireJob", "isOpenStatus", "resumeJob", "assignJob", "scheduleJob")))


def scheduling_capacity(ctx):
    return render(ctx, '''
«imports»

export interface DailyLoad {
  technicianId: string;
  date: CalendarDate;
  appointments: number;
  bookedMinutes: number;
  travelMinutes: number;
  availableMinutes: number;
  utilisationPercent: number;
}

export function dailyLoad(ctx: AppContext, technician: Technician, date: CalendarDate): DailyLoad {
  const bounds = workingBounds(ctx, technician, date);
  const live = appointmentsForTechnician(ctx, technician.id, date).filter((appointment) => appointment.status !== 'missed');
  const bookedMinutes = live.reduce((sum, appointment) => sum + minutesBetween(appointment.start, appointment.end), 0);
  const travel = live.reduce((sum, appointment) => sum + appointment.travelMinutes, 0);
  const availableMinutes = bounds ? bounds[1] - bounds[0] : 0;
  const utilisation = availableMinutes === 0 ? 0 : ((bookedMinutes + travel) / availableMinutes) * 100;
  return {
    technicianId: technician.id,
    date,
    appointments: live.length,
    bookedMinutes,
    travelMinutes: travel,
    availableMinutes,
    utilisationPercent: «percent»,
  };
}

/** Technicians in a region ordered from least to most loaded on `date`. */
export function leastLoaded(ctx: AppContext, region: string, date: CalendarDate): DailyLoad[] {
  return techniciansInRegion(ctx, region)
    .map((technician) => dailyLoad(ctx, technician, date))
    .filter((load) => load.availableMinutes > 0)
    .sort((left, right) => left.utilisationPercent - right.utilisationPercent || left.technicianId.localeCompare(right.technicianId));
}
''', percent=rounding(ctx, "utilisation", 1, "nearest"),
                  imports=imports(ctx, "src/scheduling/capacity.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/adapters/repositories/appointments.ts": ["appointmentsForTechnician"],
                      "src/adapters/repositories/technicians.ts": ["techniciansInRegion"],
                      "src/shared/rounding.ts": rounding_names(ctx, "nearest"),
                      "src/shared/time.ts": ["minutesBetween", "type CalendarDate"],
                      "src/technicians/model.ts": ["type Technician"],
                      "src/scheduling/slots.ts": ["workingBounds"],
                  }))


def scheduling_route(ctx):
    return render(ctx, '''
«imports»

export interface RouteStop {
  appointmentId: string;
  jobId: string;
  arrival: Timestamp;
  departure: Timestamp;
  driveMinutes: number;
  driveKm: number;
}

export interface DailyRoute {
  technicianId: string;
  date: CalendarDate;
  stops: RouteStop[];
  totalDriveMinutes: number;
  totalKm: number;
}

/** The technician's visits for the day in time order, starting and ending at home. */
export function dailyRoute(ctx: AppContext, technician: Technician, date: CalendarDate): DailyRoute {
  const visits = appointmentsForTechnician(ctx, technician.id, date).filter(
    (appointment) => appointment.status === 'booked' || appointment.status === 'confirmed' || appointment.status === 'completed',
  );
  const locations = appointmentLocations(ctx, technician.id, date);
  const speed = ctx.config.scheduling.travelSpeedKmh;
  let position = technician.home;
  let totalKm = 0;
  let totalDriveMinutes = 0;
  const stops: RouteStop[] = [];
  for (const visit of visits) {
    const destination = locations.get(visit.jobId) ?? position;
    const driveKm = travelKm(position, destination);
    const driveMinutes = travelMinutes(position, destination, speed);
    stops.push({ appointmentId: visit.id, jobId: visit.jobId, arrival: visit.start, departure: visit.end, driveMinutes, driveKm });
    totalKm += driveKm;
    totalDriveMinutes += driveMinutes;
    position = destination;
  }
  if (stops.length > 0) {
    totalKm += travelKm(position, technician.home);
    totalDriveMinutes += travelMinutes(position, technician.home, speed);
  }
  return { technicianId: technician.id, date, stops, totalDriveMinutes, totalKm: Math.round(totalKm * 10) / 10 };
}
''', imports=imports(ctx, "src/scheduling/route.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/appointments.ts": ["appointmentsForTechnician"],
        "src/shared/time.ts": ["type CalendarDate", "type Timestamp"],
        "src/technicians/model.ts": ["type Technician"],
        "src/scheduling/locations.ts": ["appointmentLocations"],
        "src/scheduling/travel-time.ts": ["travelKm", "travelMinutes"],
    }))


def dispatch_types(ctx):
    return render(ctx, '''
«imports»

export interface Candidate {
  technician: Technician;
  job: «Job»;
  distanceKm: number;
  loadPercent: number;
  previousVisits: number;
  missingSkills: string[];
}

export type WeightKey = keyof AppConfig['dispatch']['weights'];

/** A scoring rule returns a value between 0 (worst) and 1 (best) for a candidate. */
export interface ScoringRule {
  key: string;
  weight: WeightKey;
  describe: string;
  score(candidate: Candidate, config: AppConfig): number;
}
''', imports=imports(ctx, "src/dispatch/types.ts", {
        "src/config/schema.ts": ["type AppConfig"],
        "src/technicians/model.ts": ["type Technician"],
    }, jobs=("type Job",)))


DISPATCH_RULES = {
    "skill-match": ("skillMatch", "skill", "share of required skills the technician holds", '''
    const required = candidate.missingSkills.length + candidate.technician.certifications.length;
    if (candidate.missingSkills.length === 0) return 1;
    return required === 0 ? 0 : Math.max(0, 1 - candidate.missingSkills.length / required) * 0.5;''', ()),
    "distance": ("proximity", "distance", "closer technicians score higher, zero beyond the dispatch radius", '''
    const limit = config.dispatch.maxDistanceKm;
    if (candidate.distanceKm >= limit) return 0;
    const raw = 1 - candidate.distanceKm / limit;
    return «nearest»;''', ("nearest",)),
    "load": ("workload", "load", "favours technicians with spare capacity on the day", '''
    if (candidate.loadPercent >= 100) return 0;
    return 1 - candidate.loadPercent / 100;''', ()),
    "continuity": ("continuity", "continuity", "prefers technicians who have visited the site before", '''
    if (!config.dispatch.preferContinuity) return 0.5;
    return Math.min(1, candidate.previousVisits / 3);''', ()),
}


def dispatch_rule(ctx, key):
    name, weight, describe, body, kinds = DISPATCH_RULES[key]
    body = render(ctx, body, nearest=rounding(ctx, "raw", 0.05, "nearest"))
    groups = {"src/dispatch/types.ts": ["type ScoringRule"]}
    if kinds:
        groups["src/shared/rounding.ts"] = rounding_names(ctx, *kinds)
    config_param = "config" if "config." in body else "_config"
    return render(ctx, '''
«imports»

export const «name»: ScoringRule = {
  key: '«key»',
  weight: '«weight»',
  describe: '«describe»',
  score(candidate, «cp») {«body»
  },
};
''', imports=imports(ctx, f"src/dispatch/rules/{key}.ts", groups), name=name, key=key, weight=weight,
                  describe=describe, body=body, cp=config_param)


def dispatch_registry(ctx):
    groups = {"src/shared/registry.ts": ["createRegistry", "type Registry"], "src/dispatch/types.ts": ["type ScoringRule"]}
    for key, spec in DISPATCH_RULES.items():
        groups[f"src/dispatch/rules/{key}.ts"] = [spec[0]]
    names = ", ".join(spec[0] for spec in DISPATCH_RULES.values())
    return render(ctx, '''
«imports»

export function createScoringRegistry(): Registry<ScoringRule> {
  const registry = createRegistry<ScoringRule>('scoring rules');
  for (const rule of [«names»]) registry.register(rule.key, rule);
  return registry;
}
''', imports=imports(ctx, "src/dispatch/rules/index.ts", groups), names=names)


def dispatch_recommend(ctx):
    return render(ctx, '''
«imports»

export interface Recommendation {
  technicianId: string;
  name: string;
  score: number;
  breakdown: Record<string, number>;
  distanceKm: number;
  firstSlot?: Slot;
}

function candidateFor(ctx: AppContext, job: «Job», technician: Technician, date: CalendarDate, site: Coordinates): Candidate {
  const handler = ctx.workTypes.resolve(job.workType);
  return {
    technician,
    job,
    distanceKm: travelKm(technician.home, site),
    loadPercent: dailyLoad(ctx, technician, date).utilisationPercent,
    previousVisits: jobsForSite(ctx, job.siteId).filter(
      (other) => other.id !== job.id && other.technicianId === technician.id && other.status !== 'cancelled',
    ).length,
    missingSkills: missingSkills(technician, handler.skills, date),
  };
}

/**
 * Ranks qualified technicians for a work order. Every rule registered in the scoring registry
 * contributes its score multiplied by the configured weight; unqualified technicians and those
 * beyond the dispatch radius are excluded.
 */
export function recommendTechnicians(ctx: AppContext, jobId: string, date: CalendarDate, limit = 5): Recommendation[] {
  const job = «requireJob»(ctx, jobId);
  const site = locationOfJob(ctx, job.id);
  const customer = customers(ctx).require(job.customerId, 'customer');
  const handler = ctx.workTypes.resolve(job.workType);
  const weights = ctx.config.dispatch.weights;
  const rules = ctx.scoringRules.entries().map(([, rule]) => rule);
  const ranked: Recommendation[] = [];
  for (const technician of techniciansInRegion(ctx, customer.billingRegion)) {
    if (!isAvailable(technician, date)) continue;
    const candidate = candidateFor(ctx, job, technician, date, site);
    if (candidate.missingSkills.length > 0) continue;
    if (candidate.distanceKm > ctx.config.dispatch.maxDistanceKm) continue;
    const breakdown: Record<string, number> = {};
    let score = 0;
    for (const rule of rules) {
      const value = rule.score(candidate, ctx.config);
      breakdown[rule.key] = value;
      score += value * weights[rule.weight];
    }
    const locations = appointmentLocations(ctx, technician.id, date);
    const firstSlot = firstAvailableSlot(ctx, technician, date, handler.defaultDurationMinutes, site, locations);
    ranked.push({
      technicianId: technician.id,
      name: technician.name,
      score: Math.round(score * 10) / 10,
      breakdown,
      distanceKm: candidate.distanceKm,
      firstSlot,
    });
  }
  return ranked
    .sort((left, right) => right.score - left.score || left.distanceKm - right.distanceKm || left.technicianId.localeCompare(right.technicianId))
    .slice(0, limit);
}
''', imports=imports(ctx, "src/dispatch/recommend.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/adapters/repositories/jobs.ts": ["jobsForSite"],
        "src/adapters/repositories/technicians.ts": ["techniciansInRegion"],
        "src/shared/geo.ts": ["type Coordinates"],
        "src/shared/time.ts": ["type CalendarDate"],
        "src/scheduling/capacity.ts": ["dailyLoad"],
        "src/scheduling/locations.ts": ["appointmentLocations", "locationOfJob"],
        "src/scheduling/model.ts": ["type Slot"],
        "src/scheduling/slots.ts": ["firstAvailableSlot"],
        "src/scheduling/travel-time.ts": ["travelKm"],
        "src/technicians/model.ts": ["type Technician"],
        "src/technicians/service.ts": ["isAvailable"],
        "src/technicians/skills.ts": ["missingSkills"],
        "src/dispatch/types.ts": ["type Candidate"],
    }, jobs=("type Job", "requireJob")))


def dispatch_board(ctx):
    return render(ctx, '''
«imports»

export interface BoardColumn {
  technicianId: string;
  name: string;
  load: DailyLoad;
  jobs: string[];
}

export interface DispatchBoard {
  date: CalendarDate;
  unassigned: Array<{ jobId: string; number: string; priority: string; ageMinutes: number }>;
  onHold: Array<{ jobId: string; number: string; reason: string }>;
  columns: BoardColumn[];
}

export function dispatchBoard(ctx: AppContext, date: CalendarDate, region?: string): DispatchBoard {
  const now = ctx.clock.now();
  const open = «listJobs»(ctx, { status: ['new', 'scheduled', 'on_hold'] });
  const inRegion = (customerId: string) => {
    if (region === undefined) return true;
    return customers(ctx).get(customerId)?.billingRegion === region;
  };
  const unassigned = open
    .filter((job) => job.status !== 'on_hold' && job.technicianId === undefined && inRegion(job.customerId))
    .map((job) => ({ jobId: job.id, number: job.number, priority: job.priority, ageMinutes: «jobAge»(job, now) }));
  const onHold = open
    .filter((job) => job.status === 'on_hold' && inRegion(job.customerId))
    .map((job) => ({ jobId: job.id, number: job.number, reason: job.holds[job.holds.length - 1]?.reason ?? 'unknown' }));
  const roster = region === undefined ? activeTechnicians(ctx) : techniciansInRegion(ctx, region);
  const columns = roster
    .map((technician) => ({
      technicianId: technician.id,
      name: technician.name,
      load: dailyLoad(ctx, technician, date),
      jobs: appointmentsForTechnician(ctx, technician.id, date).map((appointment) => appointment.jobId),
    }))
    .sort((left, right) => left.name.localeCompare(right.name));
  return { date, unassigned, onHold, columns };
}
''', imports=imports(ctx, "src/dispatch/board.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/appointments.ts": ["appointmentsForTechnician"],
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/adapters/repositories/technicians.ts": ["activeTechnicians", "techniciansInRegion"],
        "src/shared/time.ts": ["type CalendarDate"],
        "src/scheduling/capacity.ts": ["dailyLoad", "type DailyLoad"],
    }, jobs=("listJobs", "jobAge")))


def generate(ctx):
    ctx.add("src/technicians/model.ts", technician_model(ctx))
    ctx.add("src/technicians/skills.ts", technician_skills(ctx))
    ctx.add("src/technicians/service.ts", technician_service(ctx))
    ctx.add("src/scheduling/model.ts", scheduling_model(ctx))
    ctx.add("src/scheduling/travel-time.ts", scheduling_travel(ctx))
    ctx.add("src/scheduling/windows.ts", scheduling_windows(ctx))
    ctx.add("src/scheduling/slots.ts", scheduling_slots(ctx))
    ctx.add("src/scheduling/locations.ts", scheduling_locations(ctx))
    ctx.add("src/scheduling/service.ts", scheduling_service(ctx))
    ctx.add("src/scheduling/capacity.ts", scheduling_capacity(ctx))
    ctx.add("src/scheduling/route.ts", scheduling_route(ctx))
    ctx.add("src/dispatch/types.ts", dispatch_types(ctx))
    for key in DISPATCH_RULES:
        ctx.add(f"src/dispatch/rules/{key}.ts", dispatch_rule(ctx, key))
    ctx.add("src/dispatch/rules/index.ts", dispatch_registry(ctx))
    ctx.add("src/dispatch/recommend.ts", dispatch_recommend(ctx))
    ctx.add("src/dispatch/board.ts", dispatch_board(ctx))
