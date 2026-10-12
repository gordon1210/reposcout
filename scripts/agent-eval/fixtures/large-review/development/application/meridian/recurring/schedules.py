"""Calendar arithmetic without clocks: callers supply the date being processed."""
from calendar import monthrange
from datetime import date, timedelta
from meridian.core.errors import require


def integer(value, field, minimum=0, maximum=10000):
    require(type(value) is int and minimum <= value <= maximum,
            "invalid_recurring_number", f"{field} must be an integer in {minimum}..{maximum}")
    return value


def boolean(value, field):
    require(type(value) is bool, "invalid_recurring_boolean", f"{field} must be boolean")
    return value


def day(value, field="date"):
    require(isinstance(value, str) and len(value) == 10,
            "invalid_recurring_date", f"{field} must be an ISO calendar date")
    try:
        result = date.fromisoformat(value)
    except ValueError:
        require(False, "invalid_recurring_date", f"{field} must be an ISO calendar date")
    require(result.isoformat() == value and 2000 <= result.year <= 2099,
            "invalid_recurring_date", f"{field} must be a date from 2000 through 2099")
    return result


def parse_schedule(value):
    require(isinstance(value, dict), "invalid_schedule", "schedule must be an object")
    require(set(value) <= {"unit", "every"}, "invalid_schedule", "Unknown schedule field")
    unit = value.get("unit")
    require(unit in ("day", "week", "month"), "invalid_schedule", "Choose day, week or month")
    every = integer(value.get("every", 1), "every", 1, 90 if unit == "day" else 12)
    return {"unit": unit, "every": every}


def occurrence(anchor, schedule, index):
    """Month-end clamping never drifts: every occurrence uses the original anchor."""
    start = day(anchor, "start_date")
    integer(index, "cycle index", 0, 10000)
    unit, every = schedule["unit"], schedule["every"]
    try:
        if unit == "month":
            absolute_month = start.year * 12 + start.month - 1 + index * every
            year, month_zero = divmod(absolute_month, 12)
            month = month_zero + 1
            result = date(year, month, min(start.day, monthrange(year, month)[1]))
        else:
            result = start + timedelta(days=index * every * (7 if unit == "week" else 1))
    except (ValueError, OverflowError):
        require(False, "schedule_exhausted", "Schedule exceeds supported calendar")
    require(result.year <= 2099, "schedule_exhausted", "Schedule exceeds supported calendar")
    return result.isoformat()


def after(value, days):
    result = day(value) + timedelta(days=integer(days, "days", 0, 365))
    require(result.year <= 2099, "schedule_exhausted", "Date exceeds supported calendar")
    return result.isoformat()


def window(value):
    require(isinstance(value, dict), "invalid_pause", "pause must be an object")
    start, end = day(value.get("from"), "from"), day(value.get("through"), "through")
    require(start <= end, "invalid_pause", "Pause end precedes its beginning")
    require((end - start).days <= 366, "invalid_pause", "Pause cannot exceed 367 dates")
    return {"from": start.isoformat(), "through": end.isoformat()}


def overlaps(left, right):
    return left["from"] <= right["through"] and right["from"] <= left["through"]


def contains(pause, value):
    return pause["from"] <= value <= pause["through"]
