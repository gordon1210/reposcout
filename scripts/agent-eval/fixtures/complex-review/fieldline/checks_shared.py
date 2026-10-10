"""Unit tests for the shared utilities."""

from .core import import_lines


def _head(here, groups, extra="import { describe, it } from 'node:test';"):
    return "import assert from 'node:assert/strict';\n" + extra + "\n" + import_lines(here, groups) + "\n"


def rounding_test(ctx):
    here = "tests/shared/rounding.test.ts"
    if ctx.quantize:
        return _head(here, {"src/shared/rounding.ts": ["quantize", "spreadInSteps", "stepsToCover"]}) + '''
describe('quantize', () => {
  const nearest: Array<[number, number, number]> = [
    [7, 5, 5],
    [7.5, 5, 10],
    [12.49, 1, 12],
    [12.5, 1, 13],
    [-12.5, 1, -13],
    [0.1 + 0.2, 0.1, 0.3],
    [44, 15, 45],
    [52, 15, 45],
    [52.5, 15, 60],
  ];
  for (const [value, step, expected] of nearest) {
    it(`rounds ${value} to the nearest ${step}`, () => {
      assert.equal(quantize(value, { step }), expected);
      assert.equal(quantize(value, { step, mode: 'nearest' }), expected);
    });
  }

  it('lifts to the next step in up mode, keeping exact multiples', () => {
    assert.equal(quantize(61, { step: 15, mode: 'up' }), 75);
    assert.equal(quantize(60, { step: 15, mode: 'up' }), 60);
    assert.equal(quantize(0.30000000000000004, { step: 0.1, mode: 'up' }), 0.3);
  });

  it('lowers to the previous step in down mode', () => {
    assert.equal(quantize(74, { step: 15, mode: 'down' }), 60);
    assert.equal(quantize(75, { step: 15, mode: 'down' }), 75);
    assert.equal(quantize(-1, { step: 15, mode: 'down' }), -15);
  });

  it('applies the minimum after rounding', () => {
    assert.equal(quantize(10, { step: 15, mode: 'up', minimum: 30 }), 30);
    assert.equal(quantize(40, { step: 15, mode: 'up', minimum: 30 }), 45);
  });

  it('rejects invalid steps and values', () => {
    assert.throws(() => quantize(10, { step: 0 }), RangeError);
    assert.throws(() => quantize(10, { step: -5 }), RangeError);
    assert.throws(() => quantize(Number.NaN, { step: 1 }), RangeError);
  });
});

describe('stepsToCover', () => {
  it('counts whole steps', () => {
    assert.equal(stepsToCover(0, 10), 0);
    assert.equal(stepsToCover(10, 10), 1);
    assert.equal(stepsToCover(10.5, 10), 2);
    assert.equal(stepsToCover(-3, 10), 0);
  });
});

describe('spreadInSteps', () => {
  it('spreads a total without losing steps', () => {
    assert.deepEqual(spreadInSteps(100, 3, 5), [35, 35, 30]);
    assert.deepEqual(spreadInSteps(90, 3, 15), [30, 30, 30]);
  });

  it('rejects an empty spread', () => {
    assert.throws(() => spreadInSteps(10, 0, 1), RangeError);
  });
});
'''
    return _head(here, {"src/shared/rounding.ts": ["floorToIncrement", "incrementsToCover", "roundToIncrement",
                                                   "spreadInIncrements"]}) + '''
describe('roundToIncrement', () => {
  const nearest: Array<[number, number, number]> = [
    [7, 5, 5],
    [7.5, 5, 10],
    [12.49, 1, 12],
    [12.5, 1, 13],
    [-12.5, 1, -13],
    [0.1 + 0.2, 0.1, 0.3],
    [44, 15, 45],
    [52, 15, 45],
    [52.5, 15, 60],
  ];
  for (const [value, increment, expected] of nearest) {
    it(`rounds ${value} to the nearest ${increment}`, () => {
      assert.equal(roundToIncrement(value, increment), expected);
      assert.equal(roundToIncrement(value, increment, false), expected);
    });
  }

  it('lifts to the next increment when rounding up, keeping exact multiples', () => {
    assert.equal(roundToIncrement(61, 15, true), 75);
    assert.equal(roundToIncrement(60, 15, true), 60);
    assert.equal(roundToIncrement(0.30000000000000004, 0.1, true), 0.3);
  });

  it('rejects invalid increments and values', () => {
    assert.throws(() => roundToIncrement(10, 0), RangeError);
    assert.throws(() => roundToIncrement(10, -5), RangeError);
    assert.throws(() => roundToIncrement(Number.NaN, 1), RangeError);
  });
});

describe('floorToIncrement', () => {
  it('lowers to the previous increment', () => {
    assert.equal(floorToIncrement(74, 15), 60);
    assert.equal(floorToIncrement(75, 15), 75);
    assert.equal(floorToIncrement(-1, 15), -15);
  });
});

describe('incrementsToCover', () => {
  it('counts whole increments', () => {
    assert.equal(incrementsToCover(0, 10), 0);
    assert.equal(incrementsToCover(10, 10), 1);
    assert.equal(incrementsToCover(10.5, 10), 2);
    assert.equal(incrementsToCover(-3, 10), 0);
  });
});

describe('spreadInIncrements', () => {
  it('spreads a total without losing increments', () => {
    assert.deepEqual(spreadInIncrements(100, 3, 5), [35, 35, 30]);
    assert.deepEqual(spreadInIncrements(90, 3, 15), [30, 30, 30]);
  });

  it('rejects an empty spread', () => {
    assert.throws(() => spreadInIncrements(10, 0, 1), RangeError);
  });
});
'''


def money_test(ctx):
    here = "tests/shared/money.test.ts"
    return _head(here, {"src/shared/money.ts": [
        "allocateCents", "applyBasisPoints", "assertCents", "cashRound", "formatCents", "multiplyCents", "negate",
        "parseDecimalAmount", "sumCents"]}) + '''
describe('money arithmetic', () => {
  it('multiplies with half-away-from-zero rounding', () => {
    assert.equal(multiplyCents(1999, 1.5), 2999);
    assert.equal(multiplyCents(1001, 0.5), 501);
    assert.equal(multiplyCents(-1001, 0.5), -501);
    assert.equal(multiplyCents(-1, 0.4), 0);
  });

  it('applies basis points', () => {
    assert.equal(applyBasisPoints(10_000, 1900), 1900);
    assert.equal(applyBasisPoints(12_345, 810), 1000);
    assert.equal(applyBasisPoints(999, 50), 5);
  });

  it('sums and negates without producing negative zero', () => {
    assert.equal(sumCents([100, 250, -50]), 300);
    assert.equal(sumCents([]), 0);
    assert.ok(Object.is(negate(0), 0));
    assert.equal(negate(125), -125);
  });

  it('validates integer cents', () => {
    assert.doesNotThrow(() => assertCents(15));
    assert.throws(() => assertCents(1.5), TypeError);
    assert.throws(() => assertCents('15'), /amount must be an integer/);
  });
});

describe('allocateCents', () => {
  it('splits by weight and always adds up to the total', () => {
    assert.deepEqual(allocateCents(100, [1, 1, 1]), [34, 33, 33]);
    assert.deepEqual(allocateCents(1000, [3, 1]), [750, 250]);
    const parts = allocateCents(9_999, [2, 3, 5, 7]);
    assert.equal(parts.reduce((sum, part) => sum + part, 0), 9_999);
  });

  it('gives everything to the first part when weights are zero', () => {
    assert.deepEqual(allocateCents(500, [0, 0]), [500, 0]);
    assert.deepEqual(allocateCents(500, []), []);
  });

  it('handles negative totals', () => {
    assert.deepEqual(allocateCents(-100, [1, 1, 1]), [-34, -33, -33]);
  });
});

describe('cash rounding', () => {
  it('rounds to the smallest coin', () => {
    assert.equal(cashRound(1234, 5), 1235);
    assert.equal(cashRound(1232, 5), 1230);
    assert.equal(cashRound(1237, 5), 1235);
    assert.equal(cashRound(1238, 5), 1240);
  });

  it('leaves amounts alone when the coin is one cent', () => {
    assert.equal(cashRound(1234, 1), 1234);
    assert.equal(cashRound(1234, 0), 1234);
  });
});

describe('formatting and parsing', () => {
  it('formats with thousands separators', () => {
    assert.equal(formatCents(123_456_789, 'EUR'), 'EUR 1,234,567.89');
    assert.equal(formatCents(-5, 'CHF'), '-CHF 0.05');
    assert.equal(formatCents(0, 'EUR'), 'EUR 0.00');
  });

  it('parses decimal amounts', () => {
    assert.equal(parseDecimalAmount('12.5'), 1250);
    assert.equal(parseDecimalAmount(' 7 '), 700);
    assert.equal(parseDecimalAmount('-0.05'), -5);
    assert.throws(() => parseDecimalAmount('12.345'), TypeError);
    assert.throws(() => parseDecimalAmount('1,00'), TypeError);
  });
});
'''


def time_test(ctx):
    here = "tests/shared/time.test.ts"
    return _head(here, {"src/shared/time.ts": [
        "addDays", "addMinutes", "atMinute", "compareTimestamps", "dateOf", "daysBetween", "durationOf",
        "formatClock", "fromEpochMinutes", "intervalsOverlap", "isCalendarDate", "isTimestamp", "latest",
        "minuteOfDay", "minutesBetween", "overlapMinutes", "parseClock", "toEpochMinutes", "weekdayOf"]}) + '''
describe('timestamps', () => {
  it('accepts minute-precision UTC timestamps only', () => {
    assert.ok(isTimestamp('2026-03-02T08:00:00Z'));
    assert.ok(isTimestamp('2026-03-02T08:00Z'));
    assert.ok(!isTimestamp('2026-03-02T08:00:30Z'));
    assert.ok(!isTimestamp('2026-03-02 08:00'));
    assert.ok(!isTimestamp(42));
  });

  it('round-trips through epoch minutes', () => {
    const minutes = toEpochMinutes('2026-03-02T08:00:00Z');
    assert.equal(fromEpochMinutes(minutes), '2026-03-02T08:00:00Z');
    assert.equal(fromEpochMinutes(minutes + 61), '2026-03-02T09:01:00Z');
  });

  it('adds minutes across midnight and month ends', () => {
    assert.equal(addMinutes('2026-02-28T23:30:00Z', 45), '2026-03-01T00:15:00Z');
    assert.equal(addMinutes('2026-03-02T08:00:00Z', -480), '2026-03-02T00:00:00Z');
  });

  it('measures and compares', () => {
    assert.equal(minutesBetween('2026-03-02T08:00:00Z', '2026-03-03T08:00:00Z'), 1440);
    assert.ok(compareTimestamps('2026-03-02T08:00:00Z', '2026-03-02T09:00:00Z') < 0);
    assert.equal(latest('2026-03-02T08:00:00Z', '2026-03-04T08:00:00Z', '2026-03-03T08:00:00Z'), '2026-03-04T08:00:00Z');
  });

  it('splits into date and minute of day', () => {
    assert.equal(dateOf('2026-03-02T23:59:00Z'), '2026-03-02');
    assert.equal(minuteOfDay('2026-03-02T07:30:00Z'), 450);
    assert.equal(atMinute('2026-03-02', 450), '2026-03-02T07:30:00Z');
    assert.equal(atMinute('2026-03-02', 1500), '2026-03-03T01:00:00Z');
  });
});

describe('calendar dates', () => {
  it('validates real dates', () => {
    assert.ok(isCalendarDate('2028-02-29'));
    assert.ok(!isCalendarDate('2026-02-29'));
    assert.ok(!isCalendarDate('2026-3-2'));
  });

  it('knows weekdays', () => {
    assert.equal(weekdayOf('2026-03-02'), 1);
    assert.equal(weekdayOf('2026-03-08'), 0);
  });

  it('adds and counts days', () => {
    assert.equal(addDays('2026-12-30', 3), '2027-01-02');
    assert.equal(addDays('2026-03-02', -2), '2026-02-28');
    assert.equal(daysBetween('2026-01-01', '2026-12-31'), 364);
    assert.equal(daysBetween('2026-03-10', '2026-03-02'), -8);
  });
});

describe('clock text', () => {
  it('formats and parses', () => {
    assert.equal(formatClock(0), '00:00');
    assert.equal(formatClock(1020), '17:00');
    assert.equal(formatClock(-30), '23:30');
    assert.equal(parseClock('07:45'), 465);
    assert.throws(() => parseClock('24:00'), RangeError);
  });
});

describe('intervals', () => {
  const morning = { start: '2026-03-02T08:00:00Z', end: '2026-03-02T12:00:00Z' };
  const lunch = { start: '2026-03-02T11:30:00Z', end: '2026-03-02T13:00:00Z' };
  const evening = { start: '2026-03-02T12:00:00Z', end: '2026-03-02T18:00:00Z' };

  it('measures overlap', () => {
    assert.equal(overlapMinutes(morning, lunch), 30);
    assert.equal(overlapMinutes(morning, evening), 0);
    assert.ok(intervalsOverlap(lunch, evening));
    assert.ok(!intervalsOverlap(morning, evening));
    assert.equal(durationOf(evening), 360);
  });
});
'''


def calendar_test(ctx):
    here = "tests/shared/calendar.test.ts"
    return _head(here, {"src/shared/calendar.ts": [
        "addWorkingMinutes", "ALWAYS_OPEN", "isWorkingDay", "isWorkingTime", "nextWorkingMinute",
        "withHolidays", "workingDaysBetween", "workingMinutesBetween", "type BusinessCalendar"]}) + '''
const OFFICE: BusinessCalendar = {
  dayStartMinute: 480,
  dayEndMinute: 1020,
  workingDays: [1, 2, 3, 4, 5],
  holidays: ['2026-04-03'],
};

describe('business calendar', () => {
  it('knows working days and hours', () => {
    assert.ok(isWorkingDay(OFFICE, '2026-03-02'));
    assert.ok(!isWorkingDay(OFFICE, '2026-03-07'));
    assert.ok(!isWorkingDay(OFFICE, '2026-04-03'));
    assert.ok(isWorkingTime(OFFICE, '2026-03-02T08:00:00Z'));
    assert.ok(!isWorkingTime(OFFICE, '2026-03-02T07:59:00Z'));
    assert.ok(!isWorkingTime(OFFICE, '2026-03-02T17:00:00Z'));
  });

  it('counts working minutes across nights and weekends', () => {
    assert.equal(workingMinutesBetween(OFFICE, '2026-03-02T16:00:00Z', '2026-03-03T09:00:00Z'), 120);
    assert.equal(workingMinutesBetween(OFFICE, '2026-03-06T16:00:00Z', '2026-03-09T09:00:00Z'), 120);
    assert.equal(workingMinutesBetween(OFFICE, '2026-03-02T08:00:00Z', '2026-03-09T08:00:00Z'), 2700);
    assert.equal(workingMinutesBetween(OFFICE, '2026-03-03T09:00:00Z', '2026-03-02T09:00:00Z'), 0);
  });

  it('skips holidays', () => {
    assert.equal(workingMinutesBetween(OFFICE, '2026-04-02T16:00:00Z', '2026-04-06T09:00:00Z'), 120);
  });

  it('finds the next working minute', () => {
    assert.equal(nextWorkingMinute(OFFICE, '2026-03-02T06:00:00Z'), '2026-03-02T08:00:00Z');
    assert.equal(nextWorkingMinute(OFFICE, '2026-03-06T17:30:00Z'), '2026-03-09T08:00:00Z');
    assert.equal(nextWorkingMinute(OFFICE, '2026-03-02T10:15:00Z'), '2026-03-02T10:15:00Z');
  });

  it('adds working time', () => {
    assert.equal(addWorkingMinutes(OFFICE, '2026-03-02T16:00:00Z', 120), '2026-03-03T09:00:00Z');
    assert.equal(addWorkingMinutes(OFFICE, '2026-03-06T16:30:00Z', 60), '2026-03-09T08:30:00Z');
    assert.equal(addWorkingMinutes(OFFICE, '2026-03-02T08:00:00Z', 0), '2026-03-02T08:00:00Z');
    assert.throws(() => addWorkingMinutes(OFFICE, '2026-03-02T08:00:00Z', -1), RangeError);
  });

  it('counts working days in a half-open range', () => {
    assert.equal(workingDaysBetween(OFFICE, '2026-03-02', '2026-03-09'), 5);
    assert.equal(workingDaysBetween(OFFICE, '2026-03-30', '2026-04-06'), 4);
  });

  it('merges holidays without duplicates', () => {
    const merged = withHolidays(OFFICE, ['2026-12-25', '2026-04-03']);
    assert.deepEqual(merged.holidays, ['2026-04-03', '2026-12-25']);
    assert.deepEqual(OFFICE.holidays, ['2026-04-03']);
  });

  it('treats the always-open calendar as continuous time', () => {
    assert.equal(workingMinutesBetween(ALWAYS_OPEN, '2026-03-07T22:00:00Z', '2026-03-08T02:00:00Z'), 240);
  });
});
'''


def validate_test(ctx):
    here = "tests/shared/validate.test.ts"
    return _head(here, {
        "src/shared/errors.ts": ["ValidationError"],
        "src/shared/validate.ts": ["array", "boolean", "date", "integer", "number", "object", "oneOf", "optional",
                                   "parseInput", "refine", "string", "timestamp", "withDefault"],
    }) + '''
const visit = object({
  title: string({ min: 3, max: 20 }),
  minutes: integer({ min: 15, max: 480 }),
  rate: optional(number({ min: 0 })),
  kind: oneOf(['repair', 'service'] as const),
  urgent: withDefault(boolean(), false),
  on: date(),
  start: optional(timestamp()),
  tags: withDefault(array(string({ min: 1 }), { max: 3 }), []),
});

function issuesOf(input: unknown): Array<{ path: string; message: string }> {
  try {
    parseInput(visit, input);
  } catch (error) {
    assert.ok(error instanceof ValidationError);
    return error.issues;
  }
  assert.fail('expected a validation error');
}

describe('input validation', () => {
  it('parses valid input, trims strings and applies defaults', () => {
    const parsed = parseInput(visit, { title: '  Boiler  ', minutes: 60, kind: 'service', on: '2026-03-02' });
    assert.deepEqual(parsed, { title: 'Boiler', minutes: 60, kind: 'service', urgent: false, on: '2026-03-02', tags: [] });
  });

  it('reports every problem with its path', () => {
    const issues = issuesOf({ title: 'ab', minutes: 30.5, kind: 'install', on: '2026-02-30', tags: ['a', '', 'c', 'd'] });
    assert.deepEqual(
      issues.map((issue) => issue.path),
      ['title', 'minutes', 'kind', 'on', 'tags'],
    );
  });

  it('reports nested array items', () => {
    const issues = issuesOf({ title: 'Valid', minutes: 30, kind: 'repair', on: '2026-03-02', tags: ['ok', ''] });
    assert.deepEqual(issues, [{ path: 'tags[1]', message: 'is required' }]);
  });

  it('rejects unknown fields', () => {
    const issues = issuesOf({ title: 'Valid', minutes: 30, kind: 'repair', on: '2026-03-02', colour: 'red' });
    assert.deepEqual(issues, [{ path: 'colour', message: 'is not a recognized field' }]);
  });

  it('treats null as missing for optional fields', () => {
    const parsed = parseInput(visit, { title: 'Valid', minutes: 30, kind: 'repair', on: '2026-03-02', rate: null });
    assert.equal(parsed.rate, undefined);
  });

  it('rejects non-objects', () => {
    assert.throws(() => parseInput(visit, 'visit'), ValidationError);
    assert.throws(() => parseInput(visit, [1, 2]), ValidationError);
  });

  it('refines values with custom predicates', () => {
    const even = refine(integer(), (value) => value % 2 === 0, 'must be even');
    assert.equal(parseInput(even, 4), 4);
    assert.throws(() => parseInput(even, 3), /must be even/);
  });

  it('enforces string patterns', () => {
    const sku = string({ pattern: /^[A-Z]{3}-\\d+$/ });
    assert.equal(parseInput(sku, 'GSK-12'), 'GSK-12');
    assert.throws(() => parseInput(sku, 'gsk-12'), /invalid format/);
  });
});
'''


def collections_test(ctx):
    here = "tests/shared/collections.test.ts"
    return _head(here, {"src/shared/collections.ts": [
        "chunk", "compact", "countBy", "groupBy", "indexBy", "maxBy", "minBy", "partition", "range", "sortBy",
        "sumBy", "uniqueBy"]}) + '''
interface Visit {
  id: string;
  region: string;
  minutes: number;
}

const visits: Visit[] = [
  { id: 'a', region: 'north', minutes: 45 },
  { id: 'b', region: 'south', minutes: 90 },
  { id: 'c', region: 'north', minutes: 30 },
  { id: 'd', region: 'east', minutes: 90 },
];

describe('collection helpers', () => {
  it('groups, indexes and counts', () => {
    const groups = groupBy(visits, (visit) => visit.region);
    assert.deepEqual([...groups.keys()], ['north', 'south', 'east']);
    assert.deepEqual(groups.get('north')?.map((visit) => visit.id), ['a', 'c']);
    assert.equal(indexBy(visits, (visit) => visit.id).get('d')?.region, 'east');
    assert.deepEqual(countBy(visits, (visit) => visit.region), { north: 2, south: 1, east: 1 });
    assert.equal(sumBy(visits, (visit) => visit.minutes), 255);
  });

  it('sorts stably by several keys', () => {
    const sorted = sortBy(visits, { desc: (visit) => visit.minutes }, (visit) => visit.region);
    assert.deepEqual(sorted.map((visit) => visit.id), ['d', 'b', 'a', 'c']);
    const stable = sortBy(visits, (visit) => (visit.minutes > 40 ? 1 : 0));
    assert.deepEqual(stable.map((visit) => visit.id), ['c', 'a', 'b', 'd']);
  });

  it('deduplicates and partitions', () => {
    assert.deepEqual(uniqueBy(visits, (visit) => visit.region).map((visit) => visit.id), ['a', 'b', 'd']);
    const [long, short] = partition(visits, (visit) => visit.minutes >= 60);
    assert.deepEqual(long.map((visit) => visit.id), ['b', 'd']);
    assert.deepEqual(short.map((visit) => visit.id), ['a', 'c']);
  });

  it('chunks and ranges', () => {
    assert.deepEqual(chunk([1, 2, 3, 4, 5], 2), [[1, 2], [3, 4], [5]]);
    assert.throws(() => chunk([1], 0), RangeError);
    assert.deepEqual(range(0, 10, 3), [0, 3, 6, 9]);
    assert.deepEqual(range(5, 0, -2), [5, 3, 1]);
    assert.deepEqual(range(0, 0), []);
  });

  it('finds extremes, preferring the first on ties', () => {
    assert.equal(maxBy(visits, (visit) => visit.minutes)?.id, 'b');
    assert.equal(minBy(visits, (visit) => visit.minutes)?.id, 'c');
    assert.equal(maxBy([], () => 1), undefined);
  });

  it('drops missing values', () => {
    assert.deepEqual(compact([1, undefined, 0, null, 2]), [1, 0, 2]);
  });
});
'''


def text_test(ctx):
    here = "tests/shared/text.test.ts"
    return _head(here, {"src/shared/text.ts": [
        "joinNonEmpty", "maskPhone", "normalizePostcode", "placeholdersOf", "pluralize", "renderTemplate",
        "slugify", "titleCase", "truncate"]}) + '''
describe('text helpers', () => {
  it('slugifies with accents removed', () => {
    assert.equal(slugify('Café Müller & Söhne'), 'cafe-muller-sohne');
    assert.equal(slugify('  --Boiler  Room-- '), 'boiler-room');
  });

  it('title-cases words separated by spaces, dashes or underscores', () => {
    assert.equal(titleCase('awaiting_parts'), 'Awaiting Parts');
    assert.equal(titleCase('heat-pump SERVICE'), 'Heat Pump Service');
  });

  it('pluralizes and truncates', () => {
    assert.equal(pluralize(1, 'visit'), '1 visit');
    assert.equal(pluralize(3, 'visit'), '3 visits');
    assert.equal(pluralize(2, 'battery', 'batteries'), '2 batteries');
    assert.equal(truncate('Replace expansion vessel', 12), 'Replace exp…');
    assert.equal(truncate('Short', 12), 'Short');
  });

  it('renders templates and leaves unknown placeholders visible', () => {
    const template = 'Hello {{ name }}, your visit is {{window}} ({{missing}})';
    assert.equal(
      renderTemplate(template, { name: 'Sam', window: 'between 10:00 and 12:00' }),
      'Hello Sam, your visit is between 10:00 and 12:00 ({{missing}})',
    );
    assert.deepEqual(placeholdersOf(template), ['name', 'window', 'missing']);
  });

  it('masks phone numbers and normalizes postcodes', () => {
    assert.equal(maskPhone('+44 20 7946 0958'), '••••••••0958');
    assert.equal(maskPhone('123'), '123');
    assert.equal(normalizePostcode(' ep1 4qs '), 'EP14QS');
  });

  it('joins only non-empty parts', () => {
    assert.equal(joinNonEmpty(['12 Quay Street', undefined, '  ', 'Eastport']), '12 Quay Street, Eastport');
  });
});
'''


def events_test(ctx):
    here = "tests/shared/events.test.ts"
    return _head(here, {"src/shared/events.ts": ["createEventBus", "type DomainEvent"]}) + '''
const event = (type: string, extra: Record<string, unknown> = {}): DomainEvent => ({ type, at: '2026-03-02T08:00:00Z', ...extra });

describe('event bus', () => {
  it('delivers events to subscribers in subscription order', () => {
    const bus = createEventBus();
    const seen: string[] = [];
    bus.on('visit.booked', () => seen.push('first'));
    bus.on('visit.booked', () => seen.push('second'));
    bus.on('visit.cancelled', () => seen.push('other'));
    assert.equal(bus.emit(event('visit.booked')), 2);
    assert.deepEqual(seen, ['first', 'second']);
  });

  it('does not fail without subscribers', () => {
    const bus = createEventBus();
    assert.equal(bus.emit(event('nobody.listens')), 0);
    assert.equal(bus.subscribers('nobody.listens'), 0);
  });

  it('unsubscribes', () => {
    const bus = createEventBus();
    let calls = 0;
    const off = bus.on('tick', () => {
      calls += 1;
    });
    bus.emit(event('tick'));
    off();
    bus.emit(event('tick'));
    assert.equal(calls, 1);
    assert.equal(bus.subscribers('tick'), 0);
  });

  it('lets handlers subscribe during delivery without affecting the current event', () => {
    const bus = createEventBus();
    let late = 0;
    bus.on('tick', () => {
      bus.on('tick', () => {
        late += 1;
      });
    });
    bus.emit(event('tick'));
    assert.equal(late, 0);
    bus.emit(event('tick'));
    assert.equal(late, 1);
  });

  it('keeps a journal of recent events', () => {
    const bus = createEventBus();
    for (let index = 0; index < 5; index += 1) bus.emit(event('tick', { index }));
    assert.deepEqual(bus.recent(2).map((entry) => entry.index), [3, 4]);
    assert.equal(bus.recent().length, 5);
  });
});
'''


def registry_test(ctx):
    here = "tests/shared/registry.test.ts"
    return _head(here, {"src/shared/registry.ts": ["createRegistry"]}) + '''
describe('registry', () => {
  it('registers and looks up by normalized key', () => {
    const registry = createRegistry<number>('sizes', { normalizeKey: (key) => key.trim().toLowerCase() });
    registry.register('Large', 3);
    registry.register('small', 1);
    assert.equal(registry.lookup(' LARGE '), 3);
    assert.ok(registry.has('SMALL'));
    assert.deepEqual(registry.keys(), ['large', 'small']);
    assert.deepEqual(registry.entries(), [['large', 3], ['small', 1]]);
  });

  it('refuses duplicates and empty keys', () => {
    const registry = createRegistry<string>('names');
    registry.register('a', 'first');
    assert.throws(() => registry.register('a', 'second'), /duplicate registration for a/);
    assert.throws(() => registry.register('', 'empty'), /empty registration key/);
  });

  it('resolves to the fallback for unknown keys', () => {
    const registry = createRegistry<string>('colours', { fallback: 'grey' });
    registry.register('red', '#f00');
    assert.equal(registry.resolve('red'), '#f00');
    assert.equal(registry.resolve('mauve'), 'grey');
    assert.equal(registry.lookup('mauve'), undefined);
  });

  it('fails to resolve without a fallback', () => {
    const registry = createRegistry<string>('colours');
    assert.throws(() => registry.resolve('mauve'), /colours: nothing registered for mauve/);
  });
});
'''


def misc_test(ctx):
    here = "tests/shared/support.test.ts"
    return _head(here, {
        "src/shared/clock.ts": ["createManualClock", "systemClock"],
        "src/shared/errors.ts": ["ConflictError", "DomainError", "ForbiddenError", "invariant", "isDomainError",
                                 "NotFoundError"],
        "src/shared/geo.ts": ["centroid", "distanceKm", "isCoordinates", "roadDistanceKm"],
        "src/shared/ids.ts": ["createIdSource", "prefixOf", "sequenceOf"],
        "src/shared/log.ts": ["createLogger"],
        "src/shared/paging.ts": ["paginate", "readPageRequest"],
        "src/shared/time.ts": ["isTimestamp"],
    }) + '''
describe('identifiers', () => {
  it('counts per prefix', () => {
    const ids = createIdSource();
    assert.equal(ids.next('CUS'), 'CUS-000001');
    assert.equal(ids.next('CUS'), 'CUS-000002');
    assert.equal(ids.next('WO'), 'WO-000001');
    assert.equal(ids.peek('CUS'), 'CUS-000003');
    ids.reset();
    assert.equal(ids.next('CUS'), 'CUS-000001');
  });

  it('parses identifiers', () => {
    assert.equal(sequenceOf('INV-000042'), 42);
    assert.ok(Number.isNaN(sequenceOf('INV')));
    assert.equal(prefixOf('SLA-WO-000001'), 'SLA-WO');
  });
});

describe('errors', () => {
  it('carries status codes', () => {
    assert.equal(new NotFoundError('customer', 'CUS-1').status, 404);
    assert.equal(new ConflictError('x', 'y').status, 409);
    assert.equal(new ForbiddenError('approve', 'tech').status, 403);
    assert.equal(new DomainError('x', 'y').status, 422);
    assert.ok(isDomainError(new NotFoundError('a', 'b')));
    assert.ok(!isDomainError(new Error('plain')));
  });

  it('asserts invariants', () => {
    assert.doesNotThrow(() => invariant(true, 'never', 'never'));
    assert.throws(() => invariant(0, 'zero', 'must not be zero'), /must not be zero/);
  });
});

describe('geography', () => {
  it('measures great-circle and road distance', () => {
    const london = { lat: 51.5074, lon: -0.1278 };
    const paris = { lat: 48.8566, lon: 2.3522 };
    assert.equal(Math.round(distanceKm(london, paris)), 344);
    assert.equal(Math.round(roadDistanceKm(london, paris)), 447);
    assert.equal(distanceKm(london, london), 0);
  });

  it('validates coordinates and finds centroids', () => {
    assert.ok(isCoordinates({ lat: 10, lon: 20 }));
    assert.ok(!isCoordinates({ lat: 100, lon: 20 }));
    assert.ok(!isCoordinates(null));
    assert.deepEqual(centroid([{ lat: 0, lon: 0 }, { lat: 10, lon: 20 }]), { lat: 5, lon: 10 });
    assert.equal(centroid([]), undefined);
  });
});

describe('clocks', () => {
  it('advances a manual clock', () => {
    const clock = createManualClock('2026-03-02T08:00:00Z');
    assert.equal(clock.advance(90), '2026-03-02T09:30:00Z');
    clock.set('2026-03-03T07:00:00Z');
    assert.equal(clock.now(), '2026-03-03T07:00:00Z');
    assert.throws(() => clock.set('tomorrow'), RangeError);
  });

  it('reports system time at minute precision', () => {
    assert.ok(isTimestamp(systemClock.now()));
  });
});

describe('logging', () => {
  it('filters by threshold and level', () => {
    const log = createLogger('info');
    log.debug('hidden');
    log.info('booked', { jobId: 'WO-1' });
    log.error('failed');
    assert.equal(log.records().length, 2);
    assert.deepEqual(log.records('warn').map((record) => record.message), ['failed']);
  });
});

describe('paging', () => {
  it('reads page requests with defaults', () => {
    assert.deepEqual(readPageRequest({}), { page: 1, pageSize: 25 });
    assert.deepEqual(readPageRequest({ page: '3', pageSize: '10' }), { page: 3, pageSize: 10 });
    assert.throws(() => readPageRequest({ pageSize: '500' }), /at most 200/);
  });

  it('paginates', () => {
    const page = paginate([1, 2, 3, 4, 5], { page: 2, pageSize: 2 });
    assert.deepEqual(page, { items: [3, 4], page: 2, pageSize: 2, total: 5, pages: 3 });
    assert.equal(paginate([], { page: 1, pageSize: 10 }).pages, 1);
  });
});
'''


TESTS = {
    "tests/shared/rounding.test.ts": rounding_test,
    "tests/shared/money.test.ts": money_test,
    "tests/shared/time.test.ts": time_test,
    "tests/shared/calendar.test.ts": calendar_test,
    "tests/shared/validate.test.ts": validate_test,
    "tests/shared/collections.test.ts": collections_test,
    "tests/shared/text.test.ts": text_test,
    "tests/shared/events.test.ts": events_test,
    "tests/shared/registry.test.ts": registry_test,
    "tests/shared/support.test.ts": misc_test,
}


def generate(ctx):
    for path, builder in TESTS.items():
        ctx.add(path, builder(ctx))
