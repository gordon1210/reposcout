"""Shared utilities: rounding, money, time, calendars, validation, events and registries."""

from .core import import_lines, render, rounding, rounding_names


def rounding_module(ctx):
    if ctx.quantize:
        return '''
/**
 * Step rounding used for billable quantities, schedule alignment and reporting.
 *
 * Floating point noise from upstream arithmetic (for example `0.1 + 0.2`) is absorbed before
 * rounding, so a value that is mathematically on a boundary is never pushed to the next step.
 */
export type RoundingMode = 'nearest' | 'up' | 'down';

export interface QuantizeOptions {
  /** Positive step; every result is a multiple of it. */
  step: number;
  /**
   * `nearest` (the default) rounds halves away from zero, `up` lifts to the next multiple and
   * `down` lowers to the previous multiple. Values already on a multiple are returned unchanged.
   */
  mode?: RoundingMode;
  /** Lower bound applied after rounding. */
  minimum?: number;
}

const NOISE = 1e-9;

function settle(value: number): number {
  return Math.round(value * 1e6) / 1e6;
}

function assertStep(step: number): void {
  if (!Number.isFinite(step) || step <= 0) {
    throw new RangeError(`Rounding step must be a positive number, received ${step}`);
  }
}

function wholeSteps(units: number, mode: RoundingMode): number {
  switch (mode) {
    case 'up':
      return Math.ceil(units - NOISE);
    case 'down':
      return Math.floor(units + NOISE);
    case 'nearest':
      return Math.sign(units) * Math.floor(Math.abs(units) + 0.5 + NOISE);
  }
}

export function quantize(value: number, options: QuantizeOptions): number {
  const { step, mode = 'nearest', minimum } = options;
  assertStep(step);
  if (!Number.isFinite(value)) throw new RangeError(`Cannot round non-finite value ${value}`);
  const rounded = settle(wholeSteps(value / step, mode) * step);
  return minimum === undefined ? rounded : Math.max(minimum, rounded);
}

/** Number of whole steps needed to cover `value`; used for pack and block counts. */
export function stepsToCover(value: number, step: number): number {
  assertStep(step);
  if (value <= 0) return 0;
  return Math.ceil(value / step - NOISE);
}

/** Splits `total` into `parts` multiples of `step` whose sum stays within one step of `total`. */
export function spreadInSteps(total: number, parts: number, step: number): number[] {
  assertStep(step);
  if (!Number.isInteger(parts) || parts <= 0) throw new RangeError(`Cannot spread over ${parts} parts`);
  const share = quantize(total / parts, { step, mode: 'down' });
  const result = new Array<number>(parts).fill(share);
  let remainder = settle(total - share * parts);
  for (let index = 0; remainder >= step - NOISE && index < parts; index += 1) {
    result[index] = settle(result[index] + step);
    remainder = settle(remainder - step);
  }
  return result;
}
'''
    return '''
/**
 * Increment rounding used for billable quantities, schedule alignment and reporting.
 *
 * Floating point noise from upstream arithmetic (for example `0.1 + 0.2`) is absorbed before
 * rounding, so a value that is mathematically on a boundary is never pushed to the next increment.
 */
const NOISE = 1e-9;

function settle(value: number): number {
  return Math.round(value * 1e6) / 1e6;
}

function assertIncrement(increment: number): void {
  if (!Number.isFinite(increment) || increment <= 0) {
    throw new RangeError(`Rounding increment must be a positive number, received ${increment}`);
  }
}

/**
 * Rounds to the nearest multiple of `increment`; halves round away from zero.
 * With `roundUp` the value is lifted to the next multiple unless it already is one.
 */
export function roundToIncrement(value: number, increment: number, roundUp = false): number {
  assertIncrement(increment);
  if (!Number.isFinite(value)) throw new RangeError(`Cannot round non-finite value ${value}`);
  const units = value / increment;
  const whole = roundUp
    ? Math.ceil(units - NOISE)
    : Math.sign(units) * Math.floor(Math.abs(units) + 0.5 + NOISE);
  return settle(whole * increment);
}

/** Lowers to the previous multiple of `increment` unless the value already is one. */
export function floorToIncrement(value: number, increment: number): number {
  assertIncrement(increment);
  if (!Number.isFinite(value)) throw new RangeError(`Cannot round non-finite value ${value}`);
  return settle(Math.floor(value / increment + NOISE) * increment);
}

/** Number of whole increments needed to cover `value`; used for pack and block counts. */
export function incrementsToCover(value: number, increment: number): number {
  assertIncrement(increment);
  if (value <= 0) return 0;
  return Math.ceil(value / increment - NOISE);
}

/** Splits `total` into `parts` multiples of `increment` whose sum stays within one increment of `total`. */
export function spreadInIncrements(total: number, parts: number, increment: number): number[] {
  assertIncrement(increment);
  if (!Number.isInteger(parts) || parts <= 0) throw new RangeError(`Cannot spread over ${parts} parts`);
  const share = floorToIncrement(total / parts, increment);
  const result = new Array<number>(parts).fill(share);
  let remainder = settle(total - share * parts);
  for (let index = 0; remainder >= increment - NOISE && index < parts; index += 1) {
    result[index] = settle(result[index] + increment);
    remainder = settle(remainder - increment);
  }
  return result;
}
'''


def cover_name(ctx):
    return "stepsToCover" if ctx.quantize else "incrementsToCover"


def spread_name(ctx):
    return "spreadInSteps" if ctx.quantize else "spreadInIncrements"


def money_module(ctx):
    here = "src/shared/money.ts"
    head = import_lines(here, {"src/shared/rounding.ts": rounding_names(ctx, "nearest")})
    return render(ctx, '''
«imports»

/** Monetary amounts are integer minor units (cents) of the document currency. */
export type Cents = number;

export const ZERO: Cents = 0;

export function assertCents(value: unknown, label = 'amount'): asserts value is Cents {
  if (typeof value !== 'number' || !Number.isSafeInteger(value)) {
    throw new TypeError(`${label} must be an integer number of cents`);
  }
}

/** Multiplies and rounds half away from zero to whole cents. */
export function multiplyCents(amount: Cents, factor: number): Cents {
  const raw = amount * factor;
  const rounded = Math.sign(raw) * Math.round(Math.abs(raw) + 1e-9);
  return rounded === 0 ? 0 : rounded;
}

/** Applies a rate expressed in basis points (1/100 of a percent). */
export function applyBasisPoints(amount: Cents, basisPoints: number): Cents {
  return multiplyCents(amount, basisPoints / 10_000);
}

export function sumCents(values: Iterable<Cents>): Cents {
  let total = 0;
  for (const value of values) total += value;
  return total;
}

export function negate(amount: Cents): Cents {
  return amount === 0 ? 0 : -amount;
}

/**
 * Splits `total` proportionally to `weights` with the largest remainder method, so the parts
 * always add up to the total exactly.
 */
export function allocateCents(total: Cents, weights: readonly number[]): Cents[] {
  const weightSum = weights.reduce((sum, weight) => sum + weight, 0);
  if (weights.length === 0) return [];
  if (weightSum <= 0) {
    const result = weights.map(() => 0);
    result[0] = total;
    return result;
  }
  const exact = weights.map((weight) => (total * weight) / weightSum);
  const parts = exact.map((value) => Math.trunc(value));
  let remainder = total - parts.reduce((sum, value) => sum + value, 0);
  const order = exact
    .map((value, index) => ({ index, fraction: Math.abs(value - Math.trunc(value)) }))
    .sort((left, right) => right.fraction - left.fraction || left.index - right.index);
  const unit = remainder < 0 ? -1 : 1;
  for (let position = 0; remainder !== 0; position = (position + 1) % order.length) {
    parts[order[position].index] += unit;
    remainder -= unit;
  }
  return parts;
}

/** Rounds a payable total to the smallest coin of the currency (for example 5 cents). */
export function cashRound(amount: Cents, unit: Cents): Cents {
  if (unit <= 1) return amount;
  return «cash»;
}

export function formatCents(amount: Cents, currency: string): string {
  const sign = amount < 0 ? '-' : '';
  const absolute = Math.abs(amount);
  const whole = Math.floor(absolute / 100).toString().replace(/\\B(?=(\\d{3})+(?!\\d))/g, ',');
  const fraction = (absolute % 100).toString().padStart(2, '0');
  return `${sign}${currency} ${whole}.${fraction}`;
}

export function parseDecimalAmount(text: string): Cents {
  const match = /^(-)?(\\d+)(?:\\.(\\d{1,2}))?$/.exec(text.trim());
  if (!match) throw new TypeError(`Invalid decimal amount: ${text}`);
  const cents = Number(match[2]) * 100 + Number((match[3] ?? '0').padEnd(2, '0'));
  return match[1] ? -cents : cents;
}
''', imports=head, cash=rounding(ctx, "amount", "unit", "nearest"))


def time_module(ctx):
    return '''
/** UTC timestamps with minute precision, for example `2026-03-02T09:30:00Z`. */
export type Timestamp = string;
/** Calendar dates, for example `2026-03-02`. */
export type CalendarDate = string;

const MS_PER_MINUTE = 60_000;
export const MINUTES_PER_DAY = 1440;
const TIMESTAMP = /^(\\d{4})-(\\d{2})-(\\d{2})T(\\d{2}):(\\d{2})(?::00(?:\\.000)?)?Z$/;
const DATE = /^(\\d{4})-(\\d{2})-(\\d{2})$/;

export function isTimestamp(value: unknown): value is Timestamp {
  return typeof value === 'string' && TIMESTAMP.test(value) && !Number.isNaN(Date.parse(value));
}

export function isCalendarDate(value: unknown): value is CalendarDate {
  if (typeof value !== 'string' || !DATE.test(value)) return false;
  const parsed = new Date(`${value}T00:00:00Z`);
  return !Number.isNaN(parsed.getTime()) && parsed.toISOString().startsWith(value);
}

/** Minutes since the Unix epoch. */
export function toEpochMinutes(value: Timestamp): number {
  if (!isTimestamp(value)) throw new RangeError(`Invalid timestamp: ${value}`);
  return Math.floor(Date.parse(value) / MS_PER_MINUTE);
}

export function fromEpochMinutes(minutes: number): Timestamp {
  return new Date(minutes * MS_PER_MINUTE).toISOString().replace('.000Z', 'Z');
}

export function addMinutes(value: Timestamp, minutes: number): Timestamp {
  return fromEpochMinutes(toEpochMinutes(value) + minutes);
}

export function minutesBetween(from: Timestamp, to: Timestamp): number {
  return toEpochMinutes(to) - toEpochMinutes(from);
}

export function compareTimestamps(left: Timestamp, right: Timestamp): number {
  return toEpochMinutes(left) - toEpochMinutes(right);
}

export function isBefore(left: Timestamp, right: Timestamp): boolean {
  return compareTimestamps(left, right) < 0;
}

export function latest(...values: Timestamp[]): Timestamp {
  return values.reduce((best, value) => (compareTimestamps(value, best) > 0 ? value : best));
}

export function dateOf(value: Timestamp): CalendarDate {
  return fromEpochMinutes(toEpochMinutes(value)).slice(0, 10);
}

export function minuteOfDay(value: Timestamp): number {
  const minutes = toEpochMinutes(value);
  return ((minutes % MINUTES_PER_DAY) + MINUTES_PER_DAY) % MINUTES_PER_DAY;
}

/** 0 = Sunday … 6 = Saturday. */
export function weekdayOf(date: CalendarDate): number {
  return new Date(`${date}T00:00:00Z`).getUTCDay();
}

export function atMinute(date: CalendarDate, minute: number): Timestamp {
  if (!isCalendarDate(date)) throw new RangeError(`Invalid date: ${date}`);
  return fromEpochMinutes(Math.floor(Date.parse(`${date}T00:00:00Z`) / MS_PER_MINUTE) + minute);
}

export function addDays(date: CalendarDate, days: number): CalendarDate {
  return atMinute(date, days * MINUTES_PER_DAY).slice(0, 10);
}

export function daysBetween(from: CalendarDate, to: CalendarDate): number {
  return Math.round((Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / (MS_PER_MINUTE * MINUTES_PER_DAY));
}

export function formatClock(minute: number): string {
  const normalized = ((minute % MINUTES_PER_DAY) + MINUTES_PER_DAY) % MINUTES_PER_DAY;
  const hours = Math.floor(normalized / 60).toString().padStart(2, '0');
  return `${hours}:${(normalized % 60).toString().padStart(2, '0')}`;
}

export function parseClock(text: string): number {
  const match = /^([01]\\d|2[0-3]):([0-5]\\d)$/.exec(text);
  if (!match) throw new RangeError(`Invalid clock time: ${text}`);
  return Number(match[1]) * 60 + Number(match[2]);
}

export interface Interval {
  start: Timestamp;
  end: Timestamp;
}

export function overlapMinutes(left: Interval, right: Interval): number {
  const start = Math.max(toEpochMinutes(left.start), toEpochMinutes(right.start));
  const end = Math.min(toEpochMinutes(left.end), toEpochMinutes(right.end));
  return Math.max(0, end - start);
}

export function intervalsOverlap(left: Interval, right: Interval): boolean {
  return overlapMinutes(left, right) > 0;
}

export function durationOf(interval: Interval): number {
  return minutesBetween(interval.start, interval.end);
}
'''


def calendar_module(ctx):
    return '''
import {
  addDays,
  atMinute,
  dateOf,
  minuteOfDay,
  toEpochMinutes,
  weekdayOf,
} from './time.ts';
import type { CalendarDate, Timestamp } from './time.ts';

/** Working hours as minutes of the UTC day, applied on the listed weekdays except holidays. */
export interface BusinessCalendar {
  dayStartMinute: number;
  dayEndMinute: number;
  workingDays: readonly number[];
  holidays: readonly CalendarDate[];
}

export const ALWAYS_OPEN: BusinessCalendar = Object.freeze({
  dayStartMinute: 0,
  dayEndMinute: 1440,
  workingDays: Object.freeze([0, 1, 2, 3, 4, 5, 6]),
  holidays: Object.freeze([]),
});

export function isWorkingDay(calendar: BusinessCalendar, date: CalendarDate): boolean {
  return calendar.workingDays.includes(weekdayOf(date)) && !calendar.holidays.includes(date);
}

export function isWorkingTime(calendar: BusinessCalendar, at: Timestamp): boolean {
  const minute = minuteOfDay(at);
  return isWorkingDay(calendar, dateOf(at)) && minute >= calendar.dayStartMinute && minute < calendar.dayEndMinute;
}

function workingSpan(calendar: BusinessCalendar, date: CalendarDate): [number, number] | undefined {
  if (!isWorkingDay(calendar, date)) return undefined;
  const dayStart = toEpochMinutes(atMinute(date, 0));
  return [dayStart + calendar.dayStartMinute, dayStart + calendar.dayEndMinute];
}

/** Working minutes in `[from, to)`; zero when `to` is not after `from`. */
export function workingMinutesBetween(calendar: BusinessCalendar, from: Timestamp, to: Timestamp): number {
  const start = toEpochMinutes(from);
  const end = toEpochMinutes(to);
  if (end <= start) return 0;
  let total = 0;
  for (let date = dateOf(from); toEpochMinutes(atMinute(date, 0)) < end; date = addDays(date, 1)) {
    const span = workingSpan(calendar, date);
    if (!span) continue;
    total += Math.max(0, Math.min(end, span[1]) - Math.max(start, span[0]));
  }
  return total;
}

/** The first working minute at or after `at`. */
export function nextWorkingMinute(calendar: BusinessCalendar, at: Timestamp): Timestamp {
  const moment = toEpochMinutes(at);
  for (let offset = 0, date = dateOf(at); offset < 400; offset += 1, date = addDays(date, 1)) {
    const span = workingSpan(calendar, date);
    if (!span || span[1] <= moment) continue;
    return atMinute(date, Math.max(moment, span[0]) - toEpochMinutes(atMinute(date, 0)));
  }
  throw new RangeError('Calendar has no working time within 400 days');
}

/** Moves forward by `minutes` of working time. */
export function addWorkingMinutes(calendar: BusinessCalendar, from: Timestamp, minutes: number): Timestamp {
  if (minutes < 0) throw new RangeError('Cannot add negative working time');
  let cursor = toEpochMinutes(nextWorkingMinute(calendar, from));
  let remaining = minutes;
  for (let date = dateOf(nextWorkingMinute(calendar, from)), guard = 0; guard < 4000; guard += 1, date = addDays(date, 1)) {
    const span = workingSpan(calendar, date);
    if (!span) continue;
    const available = Math.max(0, span[1] - Math.max(cursor, span[0]));
    if (remaining <= available) {
      const result = Math.max(cursor, span[0]) + remaining;
      return atMinute(date, result - toEpochMinutes(atMinute(date, 0)));
    }
    remaining -= available;
    cursor = span[1];
  }
  throw new RangeError('Working time exceeds the supported horizon');
}

export function workingDaysBetween(calendar: BusinessCalendar, from: CalendarDate, to: CalendarDate): number {
  let count = 0;
  for (let date = from; date < to; date = addDays(date, 1)) {
    if (isWorkingDay(calendar, date)) count += 1;
  }
  return count;
}

export function withHolidays(calendar: BusinessCalendar, holidays: readonly CalendarDate[]): BusinessCalendar {
  return { ...calendar, holidays: [...new Set([...calendar.holidays, ...holidays])].sort() };
}
'''


def ids_module(ctx):
    return '''
/** Sequential, human-readable identifiers such as `CUS-000012`. */
export interface IdSource {
  next(prefix: string): string;
  peek(prefix: string): string;
  reset(): void;
}

const WIDTH = 6;

export function createIdSource(): IdSource {
  const counters = new Map<string, number>();
  const format = (prefix: string, value: number) => `${prefix}-${value.toString().padStart(WIDTH, '0')}`;
  return {
    next(prefix) {
      const value = (counters.get(prefix) ?? 0) + 1;
      counters.set(prefix, value);
      return format(prefix, value);
    },
    peek(prefix) {
      return format(prefix, (counters.get(prefix) ?? 0) + 1);
    },
    reset() {
      counters.clear();
    },
  };
}

export function sequenceOf(id: string): number {
  const match = /-(\\d+)$/.exec(id);
  return match ? Number(match[1]) : Number.NaN;
}

export function prefixOf(id: string): string {
  const index = id.lastIndexOf('-');
  return index < 0 ? '' : id.slice(0, index);
}
'''


def errors_module(ctx):
    return '''
export interface Issue {
  path: string;
  message: string;
}

/** Errors that carry an HTTP status and a stable machine-readable code. */
export class DomainError extends Error {
  readonly code: string;
  readonly status: number;
  readonly details: Record<string, unknown>;

  constructor(code: string, message: string, status = 422, details: Record<string, unknown> = {}) {
    super(message);
    this.name = 'DomainError';
    this.code = code;
    this.status = status;
    this.details = details;
  }
}

export class NotFoundError extends DomainError {
  constructor(entity: string, id: string) {
    super('not-found', `${entity} ${id} was not found`, 404, { entity, id });
    this.name = 'NotFoundError';
  }
}

export class ConflictError extends DomainError {
  constructor(code: string, message: string, details: Record<string, unknown> = {}) {
    super(code, message, 409, details);
    this.name = 'ConflictError';
  }
}

export class ForbiddenError extends DomainError {
  constructor(action: string, actorId: string) {
    super('forbidden', `${actorId} may not ${action}`, 403, { action, actorId });
    this.name = 'ForbiddenError';
  }
}

export class ValidationError extends DomainError {
  readonly issues: Issue[];

  constructor(issues: Issue[]) {
    super('invalid-input', issues.map((issue) => `${issue.path || 'input'}: ${issue.message}`).join('; '), 422, {
      issues,
    });
    this.name = 'ValidationError';
    this.issues = issues;
  }
}

export function isDomainError(error: unknown): error is DomainError {
  return error instanceof DomainError;
}

export function invariant(condition: unknown, code: string, message: string): asserts condition {
  if (!condition) throw new DomainError(code, message);
}
'''


def validate_module(ctx):
    return '''
import { ValidationError } from './errors.ts';
import type { Issue } from './errors.ts';
import { isCalendarDate, isTimestamp } from './time.ts';
import type { CalendarDate, Timestamp } from './time.ts';

/** A check returns the parsed value or records issues and returns `undefined`. */
export type Check<T> = (value: unknown, path: string, issues: Issue[]) => T | undefined;
export type Parsed<C> = C extends Check<infer T> ? T : never;
type Shape = Record<string, Check<unknown>>;
type ShapeValue<S extends Shape> = { [K in keyof S]: Parsed<S[K]> };

const fail = <T>(issues: Issue[], path: string, message: string): T | undefined => {
  issues.push({ path, message });
  return undefined;
};

export interface StringOptions {
  min?: number;
  max?: number;
  pattern?: RegExp;
  trim?: boolean;
}

export function string(options: StringOptions = {}): Check<string> {
  const { min = 0, max = 10_000, pattern, trim = true } = options;
  return (value, path, issues) => {
    if (typeof value !== 'string') return fail(issues, path, 'must be a string');
    const text = trim ? value.trim() : value;
    if (text.length < min) return fail(issues, path, min === 1 ? 'is required' : `must have at least ${min} characters`);
    if (text.length > max) return fail(issues, path, `must have at most ${max} characters`);
    if (pattern && !pattern.test(text)) return fail(issues, path, 'has an invalid format');
    return text;
  };
}

export interface NumberOptions {
  min?: number;
  max?: number;
}

export function number(options: NumberOptions = {}): Check<number> {
  return (value, path, issues) => {
    if (typeof value !== 'number' || !Number.isFinite(value)) return fail(issues, path, 'must be a number');
    if (options.min !== undefined && value < options.min) return fail(issues, path, `must be at least ${options.min}`);
    if (options.max !== undefined && value > options.max) return fail(issues, path, `must be at most ${options.max}`);
    return value;
  };
}

export function integer(options: NumberOptions = {}): Check<number> {
  const base = number(options);
  return (value, path, issues) => {
    if (typeof value === 'number' && !Number.isInteger(value)) return fail(issues, path, 'must be a whole number');
    return base(value, path, issues);
  };
}

export function boolean(): Check<boolean> {
  return (value, path, issues) => (typeof value === 'boolean' ? value : fail(issues, path, 'must be true or false'));
}

export function oneOf<const T extends string>(values: readonly T[]): Check<T> {
  return (value, path, issues) =>
    typeof value === 'string' && (values as readonly string[]).includes(value)
      ? (value as T)
      : fail(issues, path, `must be one of ${values.join(', ')}`);
}

export function timestamp(): Check<Timestamp> {
  return (value, path, issues) => (isTimestamp(value) ? value : fail(issues, path, 'must be a UTC timestamp'));
}

export function date(): Check<CalendarDate> {
  return (value, path, issues) => (isCalendarDate(value) ? value : fail(issues, path, 'must be a date (YYYY-MM-DD)'));
}

export interface ArrayOptions {
  min?: number;
  max?: number;
}

export function array<T>(item: Check<T>, options: ArrayOptions = {}): Check<T[]> {
  const { min = 0, max = 500 } = options;
  return (value, path, issues) => {
    if (!Array.isArray(value)) return fail(issues, path, 'must be a list');
    if (value.length < min) return fail(issues, path, `must contain at least ${min} item${min === 1 ? '' : 's'}`);
    if (value.length > max) return fail(issues, path, `must contain at most ${max} items`);
    const before = issues.length;
    const result = value.map((entry, index) => item(entry, `${path}[${index}]`, issues));
    return issues.length === before ? (result as T[]) : undefined;
  };
}

export function optional<T>(check: Check<T>): Check<T | undefined> {
  return (value, path, issues) => (value === undefined || value === null ? undefined : check(value, path, issues));
}

export function withDefault<T>(check: Check<T>, fallback: T): Check<T> {
  return (value, path, issues) => (value === undefined ? fallback : check(value, path, issues));
}

export function object<S extends Shape>(shape: S): Check<ShapeValue<S>> {
  return (value, path, issues) => {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) return fail(issues, path, 'must be an object');
    const record = value as Record<string, unknown>;
    const before = issues.length;
    const result: Record<string, unknown> = {};
    for (const [key, check] of Object.entries(shape)) {
      const parsed = check(record[key], path ? `${path}.${key}` : key, issues);
      if (parsed !== undefined) result[key] = parsed;
    }
    for (const key of Object.keys(record)) {
      if (!(key in shape)) issues.push({ path: path ? `${path}.${key}` : key, message: 'is not a recognized field' });
    }
    return issues.length === before ? (result as ShapeValue<S>) : undefined;
  };
}

export function refine<T>(check: Check<T>, predicate: (value: T) => boolean, message: string): Check<T> {
  return (value, path, issues) => {
    const parsed = check(value, path, issues);
    if (parsed === undefined) return undefined;
    return predicate(parsed) ? parsed : fail(issues, path, message);
  };
}

export function parseInput<T>(check: Check<T>, input: unknown): T {
  const issues: Issue[] = [];
  const result = check(input ?? {}, '', issues);
  if (issues.length > 0 || result === undefined) {
    throw new ValidationError(issues.length > 0 ? issues : [{ path: '', message: 'is invalid' }]);
  }
  return result;
}
'''


def collections_module(ctx):
    return '''
export function groupBy<T, K extends string | number>(items: Iterable<T>, key: (item: T) => K): Map<K, T[]> {
  const groups = new Map<K, T[]>();
  for (const item of items) {
    const group = key(item);
    const bucket = groups.get(group);
    if (bucket) bucket.push(item);
    else groups.set(group, [item]);
  }
  return groups;
}

export function indexBy<T>(items: Iterable<T>, key: (item: T) => string): Map<string, T> {
  const index = new Map<string, T>();
  for (const item of items) index.set(key(item), item);
  return index;
}

export function sumBy<T>(items: Iterable<T>, value: (item: T) => number): number {
  let total = 0;
  for (const item of items) total += value(item);
  return total;
}

export function countBy<T>(items: Iterable<T>, key: (item: T) => string): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const item of items) {
    const name = key(item);
    counts[name] = (counts[name] ?? 0) + 1;
  }
  return counts;
}

type SortKey<T> = (item: T) => string | number;

/** Stable sort by one or more keys; prefix a key with `desc` via `{ desc: key }`. */
export function sortBy<T>(items: readonly T[], ...keys: Array<SortKey<T> | { desc: SortKey<T> }>): T[] {
  return items
    .map((item, index) => ({ item, index }))
    .sort((left, right) => {
      for (const entry of keys) {
        const descending = typeof entry !== 'function';
        const key = descending ? entry.desc : entry;
        const a = key(left.item);
        const b = key(right.item);
        if (a < b) return descending ? 1 : -1;
        if (a > b) return descending ? -1 : 1;
      }
      return left.index - right.index;
    })
    .map((entry) => entry.item);
}

export function uniqueBy<T>(items: Iterable<T>, key: (item: T) => string): T[] {
  const seen = new Set<string>();
  const result: T[] = [];
  for (const item of items) {
    const name = key(item);
    if (seen.has(name)) continue;
    seen.add(name);
    result.push(item);
  }
  return result;
}

export function partition<T>(items: Iterable<T>, predicate: (item: T) => boolean): [T[], T[]] {
  const accepted: T[] = [];
  const rejected: T[] = [];
  for (const item of items) (predicate(item) ? accepted : rejected).push(item);
  return [accepted, rejected];
}

export function chunk<T>(items: readonly T[], size: number): T[][] {
  if (!Number.isInteger(size) || size <= 0) throw new RangeError('Chunk size must be a positive integer');
  const result: T[][] = [];
  for (let index = 0; index < items.length; index += size) result.push(items.slice(index, index + size));
  return result;
}

export function maxBy<T>(items: Iterable<T>, value: (item: T) => number): T | undefined {
  let best: T | undefined;
  let bestValue = Number.NEGATIVE_INFINITY;
  for (const item of items) {
    const current = value(item);
    if (current > bestValue) {
      best = item;
      bestValue = current;
    }
  }
  return best;
}

export function minBy<T>(items: Iterable<T>, value: (item: T) => number): T | undefined {
  return maxBy(items, (item) => -value(item));
}

export function compact<T>(items: Iterable<T | undefined | null>): T[] {
  const result: T[] = [];
  for (const item of items) if (item !== undefined && item !== null) result.push(item);
  return result;
}

export function range(start: number, end: number, step = 1): number[] {
  const result: number[] = [];
  for (let value = start; step > 0 ? value < end : value > end; value += step) result.push(value);
  return result;
}
'''


def text_module(ctx):
    return '''
export function slugify(text: string): string {
  return text
    .normalize('NFKD')
    .replace(/[\\u0300-\\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

export function titleCase(text: string): string {
  return text
    .split(/[\\s_-]+/)
    .filter(Boolean)
    .map((word) => word[0].toUpperCase() + word.slice(1).toLowerCase())
    .join(' ');
}

export function pluralize(count: number, singular: string, plural = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

export function truncate(text: string, length: number): string {
  if (text.length <= length) return text;
  return `${text.slice(0, Math.max(0, length - 1)).trimEnd()}…`;
}

/** Replaces `{{name}}` placeholders; unknown placeholders are left visible for review. */
export function renderTemplate(template: string, values: Record<string, string | number | undefined>): string {
  return template.replace(/\\{\\{\\s*([a-zA-Z][\\w.]*)\\s*\\}\\}/g, (match, name: string) => {
    const value = values[name];
    return value === undefined ? match : String(value);
  });
}

export function placeholdersOf(template: string): string[] {
  return [...template.matchAll(/\\{\\{\\s*([a-zA-Z][\\w.]*)\\s*\\}\\}/g)].map((match) => match[1]);
}

export function maskPhone(phone: string): string {
  const digits = phone.replace(/\\D/g, '');
  return digits.length <= 4 ? digits : `${'•'.repeat(digits.length - 4)}${digits.slice(-4)}`;
}

export function normalizePostcode(postcode: string): string {
  return postcode.replace(/\\s+/g, '').toUpperCase();
}

export function joinNonEmpty(parts: Array<string | undefined>, separator = ', '): string {
  return parts.filter((part): part is string => Boolean(part && part.trim())).join(separator);
}
'''


def events_module(ctx):
    return '''
import type { Timestamp } from './time.ts';

export interface DomainEvent {
  type: string;
  at: Timestamp;
  [field: string]: unknown;
}

export type EventHandler<E extends DomainEvent = DomainEvent> = (event: E) => void;

/**
 * Synchronous in-process publish/subscribe keyed by event type. Handlers run in subscription
 * order; publishing a type nobody subscribed to is not an error.
 */
export interface EventBus {
  on<E extends DomainEvent = DomainEvent>(type: string, handler: EventHandler<E>): () => void;
  emit(event: DomainEvent): number;
  subscribers(type: string): number;
  recent(limit?: number): DomainEvent[];
}

const RECENT_LIMIT = 200;

export function createEventBus(): EventBus {
  const handlers = new Map<string, EventHandler[]>();
  const journal: DomainEvent[] = [];
  return {
    on(type, handler) {
      const list = handlers.get(type) ?? [];
      list.push(handler as EventHandler);
      handlers.set(type, list);
      return () => {
        const current = handlers.get(type) ?? [];
        handlers.set(
          type,
          current.filter((entry) => entry !== handler),
        );
      };
    },
    emit(event) {
      journal.push(event);
      if (journal.length > RECENT_LIMIT) journal.shift();
      const list = [...(handlers.get(event.type) ?? [])];
      for (const handler of list) handler(event);
      return list.length;
    },
    subscribers(type) {
      return handlers.get(type)?.length ?? 0;
    },
    recent(limit = 50) {
      return journal.slice(-limit);
    },
  };
}
'''


def registry_module(ctx):
    return '''
/**
 * String-keyed lookup table for pluggable behaviour (work types, scoring rules, templates).
 * `resolve` falls back to the configured default when no entry matches.
 */
export interface Registry<T> {
  readonly label: string;
  register(key: string, value: T): void;
  lookup(key: string): T | undefined;
  resolve(key: string): T;
  has(key: string): boolean;
  keys(): string[];
  entries(): Array<[string, T]>;
}

export interface RegistryOptions<T> {
  fallback?: T;
  normalizeKey?: (key: string) => string;
}

export function createRegistry<T>(label: string, options: RegistryOptions<T> = {}): Registry<T> {
  const entries = new Map<string, T>();
  const normalize = options.normalizeKey ?? ((key: string) => key);
  return {
    label,
    register(key, value) {
      const name = normalize(key);
      if (!name) throw new Error(`${label}: empty registration key`);
      if (entries.has(name)) throw new Error(`${label}: duplicate registration for ${name}`);
      entries.set(name, value);
    },
    lookup(key) {
      return entries.get(normalize(key));
    },
    resolve(key) {
      const value = entries.get(normalize(key)) ?? options.fallback;
      if (value === undefined) throw new Error(`${label}: nothing registered for ${key}`);
      return value;
    },
    has(key) {
      return entries.has(normalize(key));
    },
    keys() {
      return [...entries.keys()].sort();
    },
    entries() {
      return [...entries.entries()].sort(([left], [right]) => left.localeCompare(right));
    },
  };
}
'''


def paging_module(ctx):
    return '''
import { integer, object, optional, parseInput } from './validate.ts';

export interface Page<T> {
  items: T[];
  page: number;
  pageSize: number;
  total: number;
  pages: number;
}

export interface PageRequest {
  page: number;
  pageSize: number;
}

const pageQuery = object({
  page: optional(integer({ min: 1, max: 10_000 })),
  pageSize: optional(integer({ min: 1, max: 200 })),
});

export function readPageRequest(query: Record<string, unknown>, defaultSize = 25): PageRequest {
  const parsed = parseInput(pageQuery, {
    page: query.page === undefined ? undefined : Number(query.page),
    pageSize: query.pageSize === undefined ? undefined : Number(query.pageSize),
  });
  return { page: parsed.page ?? 1, pageSize: parsed.pageSize ?? defaultSize };
}

export function paginate<T>(items: readonly T[], request: PageRequest): Page<T> {
  const total = items.length;
  const pages = Math.max(1, Math.ceil(total / request.pageSize));
  const start = (request.page - 1) * request.pageSize;
  return { items: items.slice(start, start + request.pageSize), page: request.page, pageSize: request.pageSize, total, pages };
}
'''


def geo_module(ctx):
    return '''
export interface Coordinates {
  lat: number;
  lon: number;
}

const EARTH_RADIUS_KM = 6371;

function radians(degrees: number): number {
  return (degrees * Math.PI) / 180;
}

export function isCoordinates(value: unknown): value is Coordinates {
  if (typeof value !== 'object' || value === null) return false;
  const { lat, lon } = value as Record<string, unknown>;
  return typeof lat === 'number' && typeof lon === 'number' && Math.abs(lat) <= 90 && Math.abs(lon) <= 180;
}

/** Great-circle distance in kilometres. */
export function distanceKm(from: Coordinates, to: Coordinates): number {
  const dLat = radians(to.lat - from.lat);
  const dLon = radians(to.lon - from.lon);
  const a = Math.sin(dLat / 2) ** 2 + Math.cos(radians(from.lat)) * Math.cos(radians(to.lat)) * Math.sin(dLon / 2) ** 2;
  return 2 * EARTH_RADIUS_KM * Math.asin(Math.min(1, Math.sqrt(a)));
}

/** Road distance estimate: straight-line distance with a detour factor. */
export function roadDistanceKm(from: Coordinates, to: Coordinates, detourFactor = 1.3): number {
  return distanceKm(from, to) * detourFactor;
}

export function centroid(points: readonly Coordinates[]): Coordinates | undefined {
  if (points.length === 0) return undefined;
  const lat = points.reduce((sum, point) => sum + point.lat, 0) / points.length;
  const lon = points.reduce((sum, point) => sum + point.lon, 0) / points.length;
  return { lat, lon };
}
'''


def clock_module(ctx):
    return '''
import { addMinutes, isTimestamp } from './time.ts';
import type { Timestamp } from './time.ts';

export interface Clock {
  now(): Timestamp;
}

export interface ManualClock extends Clock {
  set(at: Timestamp): void;
  advance(minutes: number): Timestamp;
}

export function createManualClock(start: Timestamp): ManualClock {
  if (!isTimestamp(start)) throw new RangeError(`Invalid clock start: ${start}`);
  let current = start;
  return {
    now: () => current,
    set(at) {
      if (!isTimestamp(at)) throw new RangeError(`Invalid clock time: ${at}`);
      current = at;
    },
    advance(minutes) {
      current = addMinutes(current, minutes);
      return current;
    },
  };
}

export const systemClock: Clock = {
  now: () => new Date(Math.floor(Date.now() / 60_000) * 60_000).toISOString().replace('.000Z', 'Z'),
};
'''


def log_module(ctx):
    return '''
export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

export interface LogRecord {
  level: LogLevel;
  message: string;
  fields: Record<string, unknown>;
}

export interface Logger {
  debug(message: string, fields?: Record<string, unknown>): void;
  info(message: string, fields?: Record<string, unknown>): void;
  warn(message: string, fields?: Record<string, unknown>): void;
  error(message: string, fields?: Record<string, unknown>): void;
  records(level?: LogLevel): LogRecord[];
}

const ORDER: Record<LogLevel, number> = { debug: 10, info: 20, warn: 30, error: 40 };
const RETAINED = 500;

/** Retains structured records in memory; the application never writes to the console. */
export function createLogger(threshold: LogLevel = 'info'): Logger {
  const sink: LogRecord[] = [];
  const write = (level: LogLevel) => (message: string, fields: Record<string, unknown> = {}) => {
    if (ORDER[level] < ORDER[threshold]) return;
    sink.push({ level, message, fields });
    if (sink.length > RETAINED) sink.shift();
  };
  return {
    debug: write('debug'),
    info: write('info'),
    warn: write('warn'),
    error: write('error'),
    records: (level) => (level ? sink.filter((record) => ORDER[record.level] >= ORDER[level]) : [...sink]),
  };
}
'''


def generate(ctx):
    ctx.add("src/shared/rounding.ts", rounding_module(ctx))
    ctx.add("src/shared/money.ts", money_module(ctx))
    ctx.add("src/shared/time.ts", time_module(ctx))
    ctx.add("src/shared/calendar.ts", calendar_module(ctx))
    ctx.add("src/shared/ids.ts", ids_module(ctx))
    ctx.add("src/shared/errors.ts", errors_module(ctx))
    ctx.add("src/shared/validate.ts", validate_module(ctx))
    ctx.add("src/shared/collections.ts", collections_module(ctx))
    ctx.add("src/shared/text.ts", text_module(ctx))
    ctx.add("src/shared/events.ts", events_module(ctx))
    ctx.add("src/shared/registry.ts", registry_module(ctx))
    ctx.add("src/shared/paging.ts", paging_module(ctx))
    ctx.add("src/shared/geo.ts", geo_module(ctx))
    ctx.add("src/shared/clock.ts", clock_module(ctx))
    ctx.add("src/shared/log.ts", log_module(ctx))
