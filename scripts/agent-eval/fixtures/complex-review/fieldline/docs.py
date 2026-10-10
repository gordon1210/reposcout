"""Business documentation shipped with the generated application."""


def readme(ctx):
    jobs = "/workorders" if ctx.split else "/jobs"
    return f'''
# Fieldline

Fieldline runs the operations of a building-services company: customer accounts and sites,
maintenance contracts, work orders, scheduling and dispatch, technician timesheets, van and
warehouse stock, invoicing, service levels and customer notifications. It is a TypeScript
service for Node.js without third-party packages; state lives in an in-memory store and all
requests go through one router (`src/app.ts`).

## Checks

Run every checked-in test with:

```sh
node tests/run.mjs
```

`node tests/run.mjs billing` runs only test files whose path contains `billing`.

## Documentation

| Topic | Document |
|---|---|
| Module layout, events and registries | [docs/architecture.md](docs/architecture.md) |
| Settings and their defaults | [docs/configuration.md](docs/configuration.md) |
| Roles, permissions and customer scope | [docs/access.md](docs/access.md) |
| Work-order lifecycle (`{jobs}`) | [docs/work-orders.md](docs/work-orders.md) |
| Scheduling, slots and arrival windows | [docs/scheduling.md](docs/scheduling.md) |
| Invoices, tariffs and payments | [docs/billing.md](docs/billing.md) |
| Service-level clocks and credits | [docs/service-levels.md](docs/service-levels.md) |
| Notifications and quiet hours | [docs/notifications.md](docs/notifications.md) |
| Reports | [docs/reports.md](docs/reports.md) |
| Webhooks and exports | [docs/integrations.md](docs/integrations.md) |
'''


def architecture(ctx):
    if ctx.split:
        orders = '''`src/workorders/` holds the work-order domain, split by concern: `model.ts` (types,
statuses and transitions), `lifecycle.ts`, `holds.ts`, `completion.ts`, `followups.ts`,
`queries.ts`, `numbering.ts`, `priorities.ts` and `notes.ts`. Other modules import from the
`src/workorders/index.ts` barrel.'''
    else:
        orders = '''`src/jobs/jobs.ts` holds the work-order domain: types, statuses, transitions, holds,
completion and follow-ups. Numbering, priorities and notes live beside it in `src/jobs/`.'''
    if ctx.refactor:
        routes = '''Each `src/api/routes/*.ts` module exports its routes as data (`RouteDefinition[]`): method,
path, the permission the caller needs and the handler. `src/api/routes/index.ts` concatenates
them and `router.mount` registers each one, checking the permission before the handler runs.
Handlers still check customer and technician scope themselves.'''
        stores = '''Repositories in `src/adapters/repositories/` are declared with `collectionAccessor(name)`,
which binds a collection name of the store. Collection names are persisted keys and must
not change.'''
        worktypes = '''Work types are declared with `defineWorkType`, which fills the common defaults
(`WORK_TYPE_DEFAULTS` in `src/worktypes/types.ts`) for omitted fields. Tariffs are declared with
`defineTariff` on top of `STANDARD_TARIFF_TERMS`.'''
    else:
        routes = '''Each `src/api/routes/*.ts` module registers its handlers on the router. Every handler
first calls `authorize` with the permission it needs and then checks customer and technician
scope where the resource belongs to one.'''
        stores = '''Repositories in `src/adapters/repositories/` wrap named collections of the store.
Collection names are persisted keys and must not change.'''
        worktypes = '''Work types are plain `WorkTypeHandler` objects in `src/worktypes/handlers/`; tariffs
are plain `Tariff` objects in `src/billing/tariffs/`.'''
    rounding = ('`quantize(value, { step, mode, minimum })` in `src/shared/rounding.ts`'
                if ctx.quantize else
                '`roundToIncrement`, `floorToIncrement` and `incrementsToCover` in `src/shared/rounding.ts`')
    return f'''
# Architecture

`createApp` (`src/app.ts`) resolves the configuration, creates the store, clock, event bus and
registries, registers the domain subscriptions and the HTTP routes, and returns an `App` whose
`request(method, path, {{ actor, body }})` method is the only entry point used by clients and
tests. Actors are identified by id; staff actors are seeded and customers and technicians are
registered when they are created.

## Modules

{orders}

Other domains: `customers`, `contracts`, `assets`, `technicians`, `scheduling`, `dispatch`,
`timesheets`, `inventory`, `quotes`, `billing`, `sla`, `notifications`, `reports` and
`integrations`. Shared utilities (time, money, rounding, validation, registries, events) live
in `src/shared/`.

{routes}

{stores}

## Events

Domain modules publish events on the in-process bus (`ctx.bus`) after a state change is stored.
Subscribers are registered by `createApp`: SLA clocks, notifications, inventory reservations,
billing and webhooks react to work-order and appointment events. Event types are plain
strings; each producer exports its names (for example the work-order event map and
`AppointmentEvents`), and subscribers that keep string-keyed tables must use the same names.

## Registries

Work types, notification templates, dispatch scoring rules and export formats are looked up by
key in registries created per application. {worktypes}

## Numbers

Money is integer cents. Quantities are rounded with {rounding}; rounding absorbs floating-point
noise so that values on a boundary stay on it.
'''


def configuration(ctx):
    if ctx.early:
        scheduling = '''| `scheduling.dayStartMinute` | 420 (07:00) | First bookable minute of the scheduling day. |
| `scheduling.dayEndMinute` | 1020 (17:00) | End of the scheduling day. |
| `scheduling.shifts` | early 07:00–15:00, standard 08:00–17:00 | Shift patterns; every shift must lie inside the scheduling day and last at least one slot. |
| `scheduling.defaultShift` | `standard` | Shift given to technicians created without one. |'''
    else:
        scheduling = '''| `scheduling.dayStartMinute` | 480 (08:00) | First bookable minute of the scheduling day. |
| `scheduling.dayEndMinute` | 1020 (17:00) | End of the scheduling day. |'''
    return f'''
# Configuration

`resolveConfig(overrides)` merges deployment overrides into `DEFAULT_CONFIG`
(`src/config/defaults.ts`): objects merge key by key, arrays and scalars replace the default,
unknown keys are rejected. The merged result is validated by `configProblems` and frozen.
`createApp({{ config }})` passes its overrides through the same path.

Times of day are minutes after midnight UTC.

| Setting | Default | Meaning |
|---|---|---|
{scheduling}
| `scheduling.workingDays` | Monday–Friday | Weekdays (0 = Sunday) on which work is scheduled. |
| `scheduling.slotMinutes` | 30 | Granularity of bookable start times. |
| `scheduling.windowMinutes` | 120 | Length of the arrival window promised to customers. |
| `scheduling.maxDailyAppointments` | 6 | Appointments per technician and day unless the technician sets a lower limit. |
| `scheduling.bufferMinutes` | 15 | Gap kept between consecutive visits, in addition to driving time. |
| `scheduling.horizonDays` | 14 | How far ahead the first free slot is searched. |
| `scheduling.travelSpeedKmh` | 40 | Average driving speed used for travel estimates. |
| `holidays` | 2026 public holidays | Dates without scheduled work. |
| `billing.paymentTermsDays` | 30 | Days from issue until an invoice is due. |
| `billing.lateFeeBasisPoints` | 150 | Monthly late fee on overdue balances. |
| `sla.warningRatio` | 0.8 | Share of the resolution time after which a warning is sent. |
| `sla.creditCapBasisPoints` | 2000 | Maximum service credit per work order. |
| `timesheets.clockRoundingMinutes` | 5 | Rounding of clock-in and clock-out times. |
| `timesheets.overtimeAfterMinutes` | 480 | Daily worked minutes before overtime starts. |
| `timesheets.weeklyOvertimeAfterMinutes` | 2400 | Weekly worked minutes before overtime starts. |
| `notifications.quietStartMinute` / `quietEndMinute` | 21:00 / 07:00 | Quiet hours for non-urgent SMS. |
| `dispatch.maxDistanceKm` | 80 | Technicians farther away are not recommended. |
| `features.autoInvoiceOnCompletion` | false | Draft an invoice when a work order is completed. |
| `features.partsAutoResume` | true | Resume parts holds automatically when stock arrives. |
'''


def access(ctx):
    return '''
# Access

Every request names an actor. Unknown actors receive 401; a known actor without the required
permission receives 403.

| Role | Permissions |
|---|---|
| admin | everything, including `system:admin` |
| dispatcher | customers:read, jobs:read/open/schedule/cancel, technicians:write, inventory:read, timesheets:approve, reports:read, quotes:write, notifications:read |
| technician | jobs:read, jobs:work, inventory:read, timesheets:write |
| billing | customers:read, jobs:read, billing:read/write, reports:read, quotes:write |
| warehouse | inventory:read/write, jobs:read |
| customer | jobs:read, jobs:open, billing:read, notifications:read |

## Scope

- A customer actor only ever sees its own account: customer, site, work-order, invoice,
  statement, quote and notification requests for another customer are refused (403) or reported
  as missing (404), and lists are filtered to the actor's own records. Draft invoices are never
  shown to customers.
- A technician sees the work orders assigned to them and can only record work on those.
- Staff roles see every record their permissions cover.
'''


def work_orders(ctx):
    prefix = "workorder" if ctx.split else "job"
    jobs = "/workorders" if ctx.split else "/jobs"
    legacy = ('''
Requests to the earlier `/jobs` paths are still served: a leading `/jobs` path segment is
rewritten to `/workorders` before routing. Webhook topics are unchanged (see
[integrations](integrations.md)).
''' if ctx.split else "")
    return f'''
# Work orders

Work orders are created with `POST {jobs}` and numbered `FL-<year>-<sequence>`; sequences
restart every calendar year. The usual path is:

```
new -> scheduled -> assigned -> in_progress -> completed -> closed
```

`on_hold` can be entered from any open status and returns to the status it interrupted.
`cancelled` ends an open work order. Completed and closed work orders can be reopened.
Transitions the model does not allow are rejected.
{legacy}
## Holds

| Reason | Caused by |
|---|---|
| `customer_unavailable` | customer |
| `access_denied` | customer |
| `awaiting_approval` | customer |
| `awaiting_parts` | provider |
| `weather` | provider |
| `safety` | provider |

Only one hold can be active at a time. With `features.partsAutoResume`, parts holds are
released automatically when the reserved parts arrive.

## Events

| Event | Published when |
|---|---|
| `{prefix}.opened` | a work order is created |
| `{prefix}.scheduled` | a start time is planned |
| `{prefix}.assigned` | a technician is assigned |
| `{prefix}.started` | the technician starts work |
| `{prefix}.held` | a hold starts; carries `reason` |
| `{prefix}.resumed` | a hold is released |
| `{prefix}.completed` | the technician completes the work |
| `{prefix}.cancelled` | the work order is cancelled |
| `{prefix}.closed` | the work order is closed |
| `{prefix}.reopened` | a closed work order is reopened |

Every work-order event carries `jobId`, `number`, `customerId`, `priority` and, for contract
work, `contractId`.

## Completion

Completion needs every checklist item ticked and a resolution; the work type can add its own
completion rules. Parts used are consumed from the technician's van stock.
'''


def scheduling(ctx):
    if ctx.early:
        day = '''Technicians work one of two shift patterns:

| Shift | Hours |
|---|---|
| `early` | 07:00–15:00 |
| `standard` | 08:00–17:00 |

The scheduling day runs from 07:00 to 17:00 so that early-shift technicians can be booked;
standard-shift technicians keep their 08:00 start. A technician's slots, daily capacity and
rostered hours follow their own shift. New technicians get `scheduling.defaultShift`
(`standard`) unless a shift is given, and `PUT /technicians/:id/shift` changes it. Shift
patterns only affect bookable time and rostered hours.'''
    else:
        day = '''Technicians are bookable during the scheduling day, 08:00–17:00 on working days, unless
they are absent.'''
    return f'''
# Scheduling

Times are UTC. Working days are Monday to Friday except configured public holidays.

{day}

## Slots

A slot is a start time, aligned to `slotMinutes`, at which the technician can begin the work
type's duration without overlapping another live appointment. Consecutive visits keep
`bufferMinutes` plus the estimated driving time between the two sites; the first visit of the
day may start at the beginning of the technician's working hours. A technician has at most
`maxDailyAppointments` appointments per day.

## Arrival windows

The customer is promised an arrival window: the planned start rounded down to the window size
(two hours), one window long and clipped to the scheduling day.

## Booking

Booking checks the slot, assigns the technician when needed, schedules the work order and
creates the appointment. If any step fails the appointment is not kept. A missed appointment
can be rebooked; a no-access report records that the technician could not get in.

## Dispatch

Recommendations rank qualified technicians in the region by skill match, distance, current
load and continuity with the site, using the configured weights; technicians farther than
`dispatch.maxDistanceKm` are not recommended.
'''


def billing(ctx):
    jobs = "/workorders" if ctx.split else "/jobs"
    return f'''
# Billing

Invoices are drafted from completed work orders (`POST {jobs}/:id/invoice`), issued, paid,
credited or voided. The tariff of the customer's billing region prices every line.

## Invoice lines

1. **Callout fee** once per work order, unless the visit is an included visit or the contract
   plan waives callout fees.
2. **Labour.** Submitted and approved labour entries of the work order are added up and billed
   at the hourly rate of the work type's labour category, in whole blocks of the tariff's
   labour increment, always rounded up. The tariff's minimum labour time is topped up on the
   standard line.
3. **After-hours labour.** Labour that starts before 08:00 or at or after 17:00 on a working
   day, or at any time on a weekend or public holiday, is billed on a separate after-hours line
   at the tariff's after-hours multiplier. Standard and after-hours minutes are rounded up
   separately. These hours are part of the published tariff terms and do not depend on how
   technicians are rostered.
4. **Contract discount.** The contract's labour discount reduces standard and after-hours
   labour.
5. **Travel.** The kilometres of all travel entries of the work order are added up and rounded
   to the nearest whole kilometre (halves round up); the tariff's free kilometres are deducted
   and the rest is billed per kilometre. Regions that charge travel time bill the driving
   minutes in quarter hours, rounded up, at the standard labour rate.
6. **Parts** at catalogue price with the tariff's markup; parts fitted to an asset under
   warranty are not charged.
7. **Service credit** for a breached service level (see [service levels](service-levels.md)).

Tax is calculated on taxable lines at the tariff's rate unless the customer is tax exempt.
Totals are cash-rounded to the tariff's rounding unit.

## Payments and statements

Invoices are due `billing.paymentTermsDays` after issue. Overdue balances accrue the monthly
late fee. Payments and credit notes reduce the outstanding balance; neither can exceed it.
Statements group outstanding invoices by how long they are overdue.
'''


def service_levels(ctx):
    return '''
# Service levels

Contract work orders run an SLA clock under the contract's policy; emergencies use the
emergency policy when it is stricter.

| Policy | Response | Resolution | Coverage |
|---|---|---|---|
| standard | 20 h | 50 h | business |
| priority | 4 h | 16 h | extended |
| premium | 2 h | 8 h | always |
| emergency | 1 h | 4 h | always |

Coverage calendars: `business` is 08:00–18:00 Monday to Friday, `extended` is 07:00–21:00
Monday to Saturday, `always` counts every minute. Public holidays are excluded from business
and extended coverage. Only covered minutes count towards the targets.

## Clock

- The clock starts when a contract work order is opened. Response is met when work starts.
- **Customer-caused delays stop the clock.** A hold for `customer_unavailable`,
  `access_denied` or `awaiting_approval` pauses the resolution clock, and so does a no-access
  report on an appointment. Provider-side holds (`awaiting_parts`, `weather`, `safety`) do not
  pause it.
- A paused clock resumes when the hold is released or the visit is rebooked; the paused covered
  minutes are excluded from the elapsed time.
- Completion stops the clock. Resolution beyond the target is a breach; the customer earns a
  service credit per started hour of breach, capped at `sla.creditCapBasisPoints`. A breach
  publishes `sla.breached`.
- Cancelling stops the clock without a result; reopening restarts it.
'''


def notifications(ctx):
    return '''
# Notifications

Templates are looked up by key and rendered with the work order's values. Customers can opt
out of channels; urgent messages ignore opt-outs for SMS.

Non-urgent SMS that would be sent during quiet hours (21:00–07:00) are deferred to the end of
the quiet period, on the next quarter hour. Email and portal messages are never deferred.
'''


def reports(ctx):
    if ctx.early:
        available = '''**Utilisation** compares each technician's submitted and approved labour and travel time
with their rostered working time in the period: the hours of their shift on every working day
that is not a public holiday.'''
    else:
        available = '''**Utilisation** compares each technician's submitted and approved labour and travel time
with their rostered working time in the period: the scheduling day on every working day that
is not a public holiday.'''
    return f'''
# Reports

All reports require `reports:read`.

{available} Percentages are whole numbers.

**Backlog** lists open work orders by age and priority. **Revenue** sums issued invoices by
billing region and month. **KPIs** cover first-time fix, SLA attainment and average response.
**Activity** counts recent work-order events for the operations dashboard.
'''


def integrations(ctx):
    return '''
# Integrations

## Webhooks

Webhooks subscribe to public topics: `job.created`, `job.scheduled`, `job.started`,
`job.completed`, `job.cancelled`, `appointment.booked` and `sla.breached`. Public topic names
are part of the integration contract and do not change when internal event names change.
Deliveries are signed with the webhook secret (HMAC-SHA-256). Customer webhooks only receive
events about their own account.

## Exports

Export formats are registered by key (`csv`, `json` and the accounting journal). The accounting
export lists issued invoices and credit notes for a period.
'''


DOCS = {
    "README.md": readme,
    "docs/architecture.md": architecture,
    "docs/configuration.md": configuration,
    "docs/access.md": access,
    "docs/work-orders.md": work_orders,
    "docs/scheduling.md": scheduling,
    "docs/billing.md": billing,
    "docs/service-levels.md": service_levels,
    "docs/notifications.md": notifications,
    "docs/reports.md": reports,
    "docs/integrations.md": integrations,
}


def generate(ctx):
    for path, build in DOCS.items():
        ctx.add(path, build(ctx))
