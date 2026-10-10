"""Tariffs, invoice assembly, payments and credit notes."""

from .core import imports, render, rounding, rounding_names


def step_field(ctx):
    return "laborStepMinutes" if ctx.quantize else "laborIncrementMinutes"


def billing_model(ctx):
    return render(ctx, '''
«imports»

export interface Tariff {
  id: string;
  region: string;
  currency: string;
  /** Hourly labour rates by labour category. */
  laborRates: Record<LaborCategory, Cents>;
  /** Labour is billed in whole blocks of this many minutes, always rounded up. */
  «step»: number;
  minimumLaborMinutes: number;
  calloutFeeCents: Cents;
  /** Multiplier for labour outside standard hours, in basis points (15000 = 1.5x). */
  afterHoursMultiplierBp: number;
  travelRatePerKmCents: Cents;
  travelFreeKm: number;
  chargeTravelTime: boolean;
  partsMarkupBp: number;
  taxRateBp: number;
  /** Smallest coin; totals are rounded to it. */
  cashRoundingCents: Cents;
}

export type LineKind = 'callout' | 'labor' | 'after-hours' | 'travel' | 'travel-time' | 'part' | 'discount' | 'credit' | 'rounding';

export interface InvoiceLine {
  kind: LineKind;
  description: string;
  quantity: number;
  unitCents: Cents;
  amountCents: Cents;
  taxable: boolean;
  jobId?: string;
}

export type InvoiceStatus = 'draft' | 'issued' | 'paid' | 'void';

export interface Invoice {
  id: string;
  number?: string;
  customerId: string;
  jobIds: string[];
  status: InvoiceStatus;
  currency: string;
  lines: InvoiceLine[];
  subtotalCents: Cents;
  taxCents: Cents;
  totalCents: Cents;
  paidCents: Cents;
  createdAt: Timestamp;
  issuedOn?: CalendarDate;
  dueOn?: CalendarDate;
  voidReason?: string;
}

export interface CreditNote {
  id: string;
  number: string;
  invoiceId: string;
  customerId: string;
  amountCents: Cents;
  reason: string;
  issuedOn: CalendarDate;
}

export interface Payment {
  id: string;
  invoiceId: string;
  amountCents: Cents;
  method: 'card' | 'transfer' | 'cash' | 'direct-debit';
  receivedOn: CalendarDate;
  reference?: string;
}

export function outstandingCents(invoice: Invoice, credits: Cents = 0): Cents {
  return Math.max(0, invoice.totalCents - invoice.paidCents - credits);
}
''', step=step_field(ctx), imports=imports(ctx, "src/billing/model.ts", {
        "src/shared/money.ts": ["type Cents"],
        "src/shared/time.ts": ["type CalendarDate", "type Timestamp"],
        "src/worktypes/types.ts": ["type LaborCategory"],
    }))


# region, tariff id, currency, apprentice, standard, specialist, increment, minimum, callout, after-hours bp,
# km rate, free km, travel time, markup bp, tax bp, cash rounding
TARIFFS = [
    ("metro-east", "T-ME-2026", "EUR", 4800, 7200, 9600, 15, 60, 4500, 15000, 95, 0, False, 2500, 1900, 1),
    ("metro-west", "T-MW-2026", "EUR", 4900, 7400, 9900, 15, 60, 4500, 15000, 95, 0, False, 2500, 1900, 1),
    ("northern-hills", "T-NH-2026", "EUR", 4200, 6400, 8800, 30, 60, 3500, 15000, 80, 10, True, 2000, 1900, 1),
    ("coastal", "T-CO-2026", "EUR", 4500, 6800, 9200, 15, 45, 4000, 17500, 85, 5, False, 2200, 1900, 1),
    ("lakeside", "T-LS-2026", "CHF", 7800, 11200, 14500, 15, 60, 9000, 15000, 140, 0, False, 1800, 810, 5),
    ("valley", "T-VA-2026", "EUR", 4300, 6500, 8900, 30, 60, 3500, 15000, 80, 15, True, 2000, 1900, 1),
    ("highlands", "T-HL-2026", "EUR", 4100, 6200, 8600, 30, 90, 3000, 20000, 75, 20, True, 2000, 1900, 1),
    ("capital", "T-CA-2026", "EUR", 5200, 7900, 10500, 15, 60, 5500, 17500, 110, 0, False, 3000, 1900, 1),
]


def tariff_module(ctx, spec):
    (region, tariff_id, currency, apprentice, standard, specialist, increment, minimum, callout, after_bp, km_rate,
     free_km, travel_time, markup, tax, cash) = spec
    const = "".join(part.capitalize() for part in region.split("-"))
    const = const[0].lower() + const[1:] + "Tariff"
    return render(ctx, '''
«imports»

/** Rates for the «region» service region, valid from 1 January 2026. */
export const «const»: Tariff = {
  id: '«tid»',
  region: '«region»',
  currency: '«currency»',
  laborRates: { apprentice: «apprentice», standard: «standard», specialist: «specialist» },
  «step»: «increment»,
  minimumLaborMinutes: «minimum»,
  calloutFeeCents: «callout»,
  afterHoursMultiplierBp: «after»,
  travelRatePerKmCents: «km»,
  travelFreeKm: «free»,
  chargeTravelTime: «travel_time»,
  partsMarkupBp: «markup»,
  taxRateBp: «tax»,
  cashRoundingCents: «cash»,
};
''', imports=imports(ctx, f"src/billing/tariffs/{region}.ts", {"src/billing/model.ts": ["type Tariff"]}),
                  const=const, tid=tariff_id, region=region, currency=currency, apprentice=f"{apprentice:_}",
                  standard=f"{standard:_}", specialist=f"{specialist:_}", step=step_field(ctx), increment=increment,
                  minimum=minimum, callout=f"{callout:_}", after=f"{after_bp:_}", km=km_rate, free=free_km,
                  travel_time=str(travel_time).lower(), markup=f"{markup:_}", tax=f"{tax:_}", cash=cash)


def tariff_const(region):
    const = "".join(part.capitalize() for part in region.split("-"))
    return const[0].lower() + const[1:] + "Tariff"


def tariff_index(ctx):
    groups = {"src/billing/model.ts": ["type Tariff"], "src/shared/errors.ts": ["DomainError"]}
    for spec in TARIFFS:
        groups[f"src/billing/tariffs/{spec[0]}.ts"] = [tariff_const(spec[0])]
    entries = "".join(f"  [{tariff_const(spec[0])}.region, {tariff_const(spec[0])}],\n" for spec in TARIFFS)
    return render(ctx, '''
«imports»

const TARIFFS = new Map<string, Tariff>([
«entries»]);

export function tariffFor(region: string): Tariff {
  const tariff = TARIFFS.get(region);
  if (!tariff) throw new DomainError('no-tariff', `No tariff is defined for region ${region}`);
  return tariff;
}

export function tariffRegions(): string[] {
  return [...TARIFFS.keys()].sort();
}
''', imports=imports(ctx, "src/billing/tariffs/index.ts", groups), entries=entries)


def after_hours(ctx):
    return render(ctx, '''
«imports»

/**
 * Labour is charged at the after-hours rate when it starts outside the standard working day,
 * at a weekend or on a public holiday.
 */
export function isAfterHours(ctx: AppContext, at: Timestamp): boolean {
  return !isWorkingTime(businessCalendar(ctx), at);
}
''', imports=imports(ctx, "src/billing/after-hours.ts", {
        "src/context.ts": ["businessCalendar", "type AppContext"],
        "src/shared/calendar.ts": ["isWorkingTime"],
        "src/shared/time.ts": ["type Timestamp"],
    }))


def labor(ctx):
    step = step_field(ctx)
    return render(ctx, '''
«imports»

function billable(entry: TimeEntry): boolean {
  return entry.status === 'submitted' || entry.status === 'approved';
}

/**
 * Labour lines for a work order. Standard and after-hours minutes are rounded up separately to
 * the tariff's billing block; the minimum charge tops up the standard block.
 */
export function laborLines(ctx: AppContext, job: «Job», entries: readonly TimeEntry[], tariff: Tariff): InvoiceLine[] {
  const handler = ctx.workTypes.resolve(job.workType);
  const rate = tariff.laborRates[handler.laborCategory];
  let standardMinutes = 0;
  let afterHoursMinutes = 0;
  for (const entry of entries) {
    if (entry.kind !== 'labor' || entry.jobId !== job.id || !billable(entry)) continue;
    if (isAfterHours(ctx, entry.start)) afterHoursMinutes += entry.minutes;
    else standardMinutes += entry.minutes;
  }
  if (standardMinutes === 0 && afterHoursMinutes === 0) return [];
  const block = tariff.«step»;
  let standard = standardMinutes > 0 ? «standard» : 0;
  const afterHours = afterHoursMinutes > 0 ? «after» : 0;
  if (standard + afterHours < tariff.minimumLaborMinutes) {
    standard = tariff.minimumLaborMinutes - afterHours;
  }
  const lines: InvoiceLine[] = [];
  if (standard > 0) {
    lines.push({
      kind: 'labor',
      description: `${handler.label} labour (${standard} min)`,
      quantity: standard / 60,
      unitCents: rate,
      amountCents: multiplyCents(rate, standard / 60),
      taxable: true,
      jobId: job.id,
    });
  }
  if (afterHours > 0) {
    const unit = applyBasisPoints(rate, tariff.afterHoursMultiplierBp);
    lines.push({
      kind: 'after-hours',
      description: `${handler.label} labour after hours (${afterHours} min)`,
      quantity: afterHours / 60,
      unitCents: unit,
      amountCents: multiplyCents(unit, afterHours / 60),
      taxable: true,
      jobId: job.id,
    });
  }
  return lines;
}

/** Labour included in a contract visit: the work type's standard duration at the standard rate. */
export function includedVisitAllowance(ctx: AppContext, job: «Job», tariff: Tariff): InvoiceLine | undefined {
  if (!job.tags.includes('included-visit')) return undefined;
  const handler = ctx.workTypes.resolve(job.workType);
  const rate = tariff.laborRates[handler.laborCategory];
  const minutes = handler.defaultDurationMinutes;
  return {
    kind: 'discount',
    description: `Included contract visit (${minutes} min)`,
    quantity: 1,
    unitCents: -multiplyCents(rate, minutes / 60),
    amountCents: -multiplyCents(rate, minutes / 60),
    taxable: true,
    jobId: job.id,
  };
}
''', step=step, standard=rounding(ctx, "standardMinutes", "block", "up"),
                  after=rounding(ctx, "afterHoursMinutes", "block", "up"),
                  imports=imports(ctx, "src/billing/labor.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/shared/money.ts": ["applyBasisPoints", "multiplyCents"],
                      "src/shared/rounding.ts": rounding_names(ctx, "up"),
                      "src/timesheets/model.ts": ["type TimeEntry"],
                      "src/billing/after-hours.ts": ["isAfterHours"],
                      "src/billing/model.ts": ["type InvoiceLine", "type Tariff"],
                  }, jobs=("type Job",)))


def travel(ctx):
    return render(ctx, '''
«imports»

/**
 * Travel for one work order: the kilometres of all its travel entries, rounded to the nearest
 * whole kilometre, less the tariff's free allowance. Regions that charge travel time bill the
 * driving minutes in quarter hours at the standard labour rate.
 */
export function travelLines(job: «Job», entries: readonly TimeEntry[], tariff: Tariff): InvoiceLine[] {
  const trips = entries.filter(
    (entry) => entry.kind === 'travel' && entry.jobId === job.id && (entry.status === 'submitted' || entry.status === 'approved'),
  );
  if (trips.length === 0) return [];
  const lines: InvoiceLine[] = [];
  const km = trips.reduce((sum, entry) => sum + (entry.km ?? 0), 0);
  const billedKm = Math.max(0, «km» - tariff.travelFreeKm);
  if (billedKm > 0) {
    lines.push({
      kind: 'travel',
      description: `Travel ${billedKm} km`,
      quantity: billedKm,
      unitCents: tariff.travelRatePerKmCents,
      amountCents: multiplyCents(tariff.travelRatePerKmCents, billedKm),
      taxable: true,
      jobId: job.id,
    });
  }
  if (tariff.chargeTravelTime) {
    const minutes = trips.reduce((sum, entry) => sum + entry.minutes, 0);
    const billed = «minutes»;
    if (billed > 0) {
      lines.push({
        kind: 'travel-time',
        description: `Travel time (${billed} min)`,
        quantity: billed / 60,
        unitCents: tariff.laborRates.standard,
        amountCents: multiplyCents(tariff.laborRates.standard, billed / 60),
        taxable: true,
        jobId: job.id,
      });
    }
  }
  return lines;
}
''', km=rounding(ctx, "km", 1, "wrong-down"), minutes=rounding(ctx, "minutes", 15, "up"),
                  imports=imports(ctx, "src/billing/travel.ts", {
                      "src/shared/money.ts": ["multiplyCents"],
                      "src/shared/rounding.ts": rounding_names(ctx, "nearest", "up"),
                      "src/timesheets/model.ts": ["type TimeEntry"],
                      "src/billing/model.ts": ["type InvoiceLine", "type Tariff"],
                  }, jobs=("type Job",)))


def parts_lines(ctx):
    return render(ctx, '''
«imports»

/** Cost-plus price rounded to five cents, never below the catalogue list price. */
export function partUnitPrice(part: Part, tariff: Tariff): Cents {
  const costPlus = «price»;
  return Math.max(costPlus, part.listPriceCents);
}

/**
 * Part lines for the parts reported on completion. Parts fitted to an asset under warranty are
 * listed at zero so the customer still sees what was replaced.
 */
export function partLines(
  ctx: AppContext,
  job: «Job»,
  tariff: Tariff,
): InvoiceLine[] {
  const used = job.completion?.partsUsed ?? [];
  const asset = job.assetId ? assets(ctx).get(job.assetId) : undefined;
  const underWarranty =
    asset !== undefined && job.completion !== undefined && warrantyStatus(asset, dateOf(job.completion.completedAt)) !== 'expired' &&
    warrantyStatus(asset, dateOf(job.completion.completedAt)) !== 'none';
  return used.map((line) => {
    const part = parts(ctx).require(line.sku, 'part');
    const unit = underWarranty ? 0 : partUnitPrice(part, tariff);
    return {
      kind: 'part',
      description: underWarranty ? `${part.name} (warranty)` : part.name,
      quantity: line.quantity,
      unitCents: unit,
      amountCents: unit * line.quantity,
      taxable: true,
      jobId: job.id,
    };
  });
}
''', price=rounding(ctx, "applyBasisPoints(part.unitCostCents, 10_000 + tariff.partsMarkupBp)", 5, "nearest"),
                  imports=imports(ctx, "src/billing/parts.ts", {
                      "src/context.ts": ["type AppContext"],
                      "src/adapters/repositories/assets.ts": ["assets"],
                      "src/adapters/repositories/parts.ts": ["parts"],
                      "src/assets/service.ts": ["warrantyStatus"],
                      "src/inventory/model.ts": ["type Part"],
                      "src/shared/money.ts": ["applyBasisPoints", "type Cents"],
                      "src/shared/rounding.ts": rounding_names(ctx, "nearest"),
                      "src/shared/time.ts": ["dateOf"],
                      "src/billing/model.ts": ["type InvoiceLine", "type Tariff"],
                  }, jobs=("type Job",)))


def discounts(ctx):
    return render(ctx, '''
«imports»

/** Contract labour discount as a negative line; applies to standard and after-hours labour. */
export function contractDiscountLine(lines: readonly InvoiceLine[], contract: Contract | undefined): InvoiceLine | undefined {
  if (!contract || contract.laborDiscountBasisPoints <= 0) return undefined;
  const labor = sumCents(lines.filter((line) => line.kind === 'labor' || line.kind === 'after-hours').map((line) => line.amountCents));
  if (labor <= 0) return undefined;
  const discount = applyBasisPoints(labor, contract.laborDiscountBasisPoints);
  return {
    kind: 'discount',
    description: `${contract.plan} contract labour discount`,
    quantity: 1,
    unitCents: -discount,
    amountCents: -discount,
    taxable: true,
  };
}

/**
 * Volume discount for commercial accounts: 25 basis points per 1,000.00 invoiced in the last
 * twelve months, in steps of 50 basis points and capped at 10%.
 */
export function volumeDiscountBasisPoints(annualSpendCents: Cents): number {
  if (annualSpendCents <= 0) return 0;
  const raw = (annualSpendCents / 100_000) * 25;
  return Math.min(1000, «steps»);
}
''', steps=rounding(ctx, "raw", 50, "down"), imports=imports(ctx, "src/billing/discounts.ts", {
        "src/contracts/model.ts": ["type Contract"],
        "src/shared/money.ts": ["applyBasisPoints", "sumCents", "type Cents"],
        "src/shared/rounding.ts": rounding_names(ctx, "down"),
        "src/billing/model.ts": ["type InvoiceLine"],
    }))


def tax(ctx):
    return render(ctx, '''
«imports»

export interface Totals {
  subtotalCents: Cents;
  taxCents: Cents;
  totalCents: Cents;
}

/** Tax is computed once on the taxable subtotal; exempt customers pay none. */
export function computeTotals(lines: readonly InvoiceLine[], taxRateBp: number, exempt: boolean): Totals {
  const subtotalCents = sumCents(lines.map((line) => line.amountCents));
  const taxable = sumCents(lines.filter((line) => line.taxable).map((line) => line.amountCents));
  const taxCents = exempt || taxable <= 0 ? 0 : applyBasisPoints(taxable, taxRateBp);
  return { subtotalCents, taxCents, totalCents: subtotalCents + taxCents };
}
''', imports=imports(ctx, "src/billing/tax.ts", {
        "src/shared/money.ts": ["applyBasisPoints", "sumCents", "type Cents"],
        "src/billing/model.ts": ["type InvoiceLine"],
    }))


def invoices(ctx):
    return render(ctx, '''
«imports»

function calloutLine(job: «Job», tariff: Tariff, contract: Contract | undefined): InvoiceLine | undefined {
  if (job.tags.includes('included-visit')) return undefined;
  if (contract && PLAN_TERMS[contract.plan].waiveCalloutFee) return undefined;
  return {
    kind: 'callout',
    description: 'Callout fee',
    quantity: 1,
    unitCents: tariff.calloutFeeCents,
    amountCents: tariff.calloutFeeCents,
    taxable: true,
    jobId: job.id,
  };
}

function slaCreditLine(ctx: AppContext, job: «Job», chargeable: Cents): InvoiceLine | undefined {
  const clock = clockForJob(ctx, job.id);
  if (!clock || clock.status !== 'breached' || !clock.creditBasisPoints) return undefined;
  const credit = applyBasisPoints(chargeable, clock.creditBasisPoints);
  if (credit <= 0) return undefined;
  return {
    kind: 'credit',
    description: `Service level credit (${clock.creditBasisPoints / 100}%)`,
    quantity: 1,
    unitCents: -credit,
    amountCents: -credit,
    taxable: true,
    jobId: job.id,
  };
}

/**
 * Builds a draft invoice for completed work: callout, labour, travel, parts, contract and
 * service-level reductions, tax and cash rounding.
 */
export function draftInvoiceForJob(ctx: AppContext, actorId: string, jobId: string): Invoice {
  const job = «requireJob»(ctx, jobId);
  if (job.status !== 'completed' && job.status !== 'closed') {
    throw new ConflictError('not-completed', `Work order ${job.number} has not been completed`);
  }
  const existing = invoiceForJob(ctx, job.id);
  if (existing) throw new ConflictError('already-invoiced', `Work order ${job.number} is on invoice ${existing.id}`);
  const customer = customers(ctx).require(job.customerId, 'customer');
  const tariff = tariffFor(customer.billingRegion);
  const contract = job.contractId ? contracts(ctx).get(job.contractId) : undefined;
  const entries = entriesForJob(ctx, job.id);
  const lines: InvoiceLine[] = [];
  const callout = calloutLine(job, tariff, contract);
  if (callout) lines.push(callout);
  lines.push(...laborLines(ctx, job, entries, tariff));
  const allowance = includedVisitAllowance(ctx, job, tariff);
  const laborTotal = sumCents(lines.filter((line) => line.kind === 'labor').map((line) => line.amountCents));
  if (allowance) lines.push({ ...allowance, unitCents: Math.max(allowance.unitCents, -laborTotal), amountCents: Math.max(allowance.amountCents, -laborTotal) });
  const discount = contractDiscountLine(lines, contract);
  if (discount) lines.push(discount);
  lines.push(...travelLines(job, entries, tariff));
  lines.push(...partLines(ctx, job, tariff));
  const credit = slaCreditLine(ctx, job, sumCents(lines.map((line) => line.amountCents)));
  if (credit) lines.push(credit);
  const totals = computeTotals(lines, tariff.taxRateBp, customer.taxExempt);
  const rounded = cashRound(totals.totalCents, tariff.cashRoundingCents);
  if (rounded !== totals.totalCents) {
    lines.push({
      kind: 'rounding',
      description: 'Cash rounding',
      quantity: 1,
      unitCents: rounded - totals.totalCents,
      amountCents: rounded - totals.totalCents,
      taxable: false,
    });
  }
  const invoice: Invoice = {
    id: ctx.ids.next('INVD'),
    customerId: customer.id,
    jobIds: [job.id],
    status: 'draft',
    currency: tariff.currency,
    lines,
    subtotalCents: totals.subtotalCents + (rounded - totals.totalCents),
    taxCents: totals.taxCents,
    totalCents: rounded,
    paidCents: 0,
    createdAt: ctx.clock.now(),
  };
  invoicesRepo(ctx).insert(invoice);
  recordAudit(ctx, actorId, 'invoice.drafted', invoice.id, { jobId: job.id, totalCents: invoice.totalCents });
  return invoice;
}

export function issueInvoice(ctx: AppContext, actorId: string, invoiceId: string): Invoice {
  const invoice = invoicesRepo(ctx).require(invoiceId, 'invoice');
  if (invoice.status !== 'draft') throw new ConflictError('not-draft', `Invoice ${invoiceId} is ${invoice.status}`);
  if (invoice.totalCents < ctx.config.billing.minimumInvoiceCents) {
    throw new ConflictError('below-minimum', `Invoices below ${ctx.config.billing.minimumInvoiceCents} cents are not issued`);
  }
  const customer = customers(ctx).require(invoice.customerId, 'customer');
  const total = invoice.totalCents;
  const exposure = invoicesForCustomer(ctx, customer.id)
    .filter((other) => other.status === 'issued')
    .reduce((sum, other) => sum + other.totalCents - other.paidCents, 0);
  if (exposure + total > customer.creditLimitCents) {
    throw new ConflictError('credit-limit', `Issuing ${invoiceId} would exceed the credit limit of ${customer.id}`, {
      exposure,
      limit: customer.creditLimitCents,
    });
  }
  const issuedOn = dateOf(ctx.clock.now());
  const updated = invoicesRepo(ctx).update(invoiceId, {
    status: total === 0 ? 'paid' : 'issued',
    number: `${ctx.config.billing.invoicePrefix}-${ctx.ids.next('INVNO').slice(6)}`,
    issuedOn,
    dueOn: addDays(issuedOn, ctx.config.billing.paymentTermsDays),
  });
  recordAudit(ctx, actorId, 'invoice.issued', invoiceId, { number: updated.number });
  return updated;
}

export function voidInvoice(ctx: AppContext, actorId: string, invoiceId: string, reason: string): Invoice {
  const invoice = invoicesRepo(ctx).require(invoiceId, 'invoice');
  if (invoice.status === 'void') throw new ConflictError('already-void', `Invoice ${invoiceId} is already void`);
  if (invoice.paidCents > 0) throw new ConflictError('has-payments', 'Invoices with payments must be credited instead');
  const updated = invoicesRepo(ctx).update(invoiceId, { status: 'void', voidReason: reason });
  recordAudit(ctx, actorId, 'invoice.voided', invoiceId, { reason });
  return updated;
}
''', imports=imports(ctx, "src/billing/invoices.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/contracts.ts": ["contracts"],
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/adapters/repositories/invoices.ts": ["invoiceForJob", "invoicesForCustomer", "invoices as invoicesRepo"],
        "src/adapters/repositories/sla-clocks.ts": ["clockForJob"],
        "src/adapters/repositories/time-entries.ts": ["entriesForJob"],
        "src/adapters/store/audit.ts": ["recordAudit"],
        "src/contracts/model.ts": ["PLAN_TERMS", "type Contract"],
        "src/shared/errors.ts": ["ConflictError"],
        "src/shared/money.ts": ["applyBasisPoints", "cashRound", "sumCents", "type Cents"],
        "src/shared/time.ts": ["addDays", "dateOf"],
        "src/billing/discounts.ts": ["contractDiscountLine"],
        "src/billing/labor.ts": ["includedVisitAllowance", "laborLines"],
        "src/billing/model.ts": ["type Invoice", "type InvoiceLine", "type Tariff"],
        "src/billing/parts.ts": ["partLines"],
        "src/billing/tariffs/index.ts": ["tariffFor"],
        "src/billing/tax.ts": ["computeTotals"],
        "src/billing/travel.ts": ["travelLines"],
    }, jobs=("type Job", "requireJob")))


def payments(ctx):
    return render(ctx, '''
«imports»

export function recordPayment(
  ctx: AppContext,
  actorId: string,
  invoiceId: string,
  amountCents: Cents,
  method: Payment['method'],
  reference?: string,
): Payment {
  const invoice = invoices(ctx).require(invoiceId, 'invoice');
  if (invoice.status !== 'issued') throw new ConflictError('not-payable', `Invoice ${invoiceId} is ${invoice.status}`);
  if (!Number.isInteger(amountCents) || amountCents <= 0) throw new DomainError('invalid-amount', 'Payments must be positive');
  const credited = creditedCents(ctx, invoiceId);
  const outstanding = outstandingCents(invoice, credited);
  if (amountCents > outstanding) {
    throw new ConflictError('overpayment', `Payment exceeds the outstanding ${outstanding} cents`, { outstanding });
  }
  const payment: Payment = { id: ctx.ids.next('PAY'), invoiceId, amountCents, method, receivedOn: dateOf(ctx.clock.now()), reference };
  paymentsRepo(ctx).insert(payment);
  const paidCents = invoice.paidCents + amountCents;
  invoices(ctx).update(invoiceId, { paidCents, status: paidCents + credited >= invoice.totalCents ? 'paid' : 'issued' });
  recordAudit(ctx, actorId, 'payment.recorded', payment.id, { invoiceId, amountCents });
  return payment;
}

export function creditedCents(ctx: AppContext, invoiceId: string): Cents {
  return sumCents(creditNotes(ctx).find((note) => note.invoiceId === invoiceId).map((note) => note.amountCents));
}

/**
 * Late fee for an overdue invoice: the configured monthly rate per started 30 days overdue on
 * the outstanding amount, rounded to whole currency units.
 */
export function lateFee(ctx: AppContext, invoice: Invoice, today: CalendarDate): Cents {
  if (invoice.status !== 'issued' || invoice.dueOn === undefined || today <= invoice.dueOn) return 0;
  const months = Math.ceil(daysBetween(invoice.dueOn, today) / 30);
  const outstanding = outstandingCents(invoice, creditedCents(ctx, invoice.id));
  const fee = applyBasisPoints(outstanding, ctx.config.billing.lateFeeBasisPoints * months);
  return «fee»;
}

export interface OverdueInvoice {
  invoiceId: string;
  customerId: string;
  daysOverdue: number;
  outstandingCents: Cents;
  lateFeeCents: Cents;
}

export function overdueInvoices(ctx: AppContext, today: CalendarDate): OverdueInvoice[] {
  return invoices(ctx)
    .find((invoice) => invoice.status === 'issued' && invoice.dueOn !== undefined && invoice.dueOn < today)
    .map((invoice) => ({
      invoiceId: invoice.id,
      customerId: invoice.customerId,
      daysOverdue: daysBetween(invoice.dueOn as CalendarDate, today),
      outstandingCents: outstandingCents(invoice, creditedCents(ctx, invoice.id)),
      lateFeeCents: lateFee(ctx, invoice, today),
    }))
    .sort((left, right) => right.daysOverdue - left.daysOverdue || left.invoiceId.localeCompare(right.invoiceId));
}
''', fee=rounding(ctx, "fee", 100, "nearest-explicit"), imports=imports(ctx, "src/billing/payments.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/invoices.ts": ["creditNotes", "invoices", "payments as paymentsRepo"],
        "src/adapters/store/audit.ts": ["recordAudit"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/money.ts": ["applyBasisPoints", "sumCents", "type Cents"],
        "src/shared/rounding.ts": rounding_names(ctx, "nearest"),
        "src/shared/time.ts": ["dateOf", "daysBetween", "type CalendarDate"],
        "src/billing/model.ts": ["outstandingCents", "type Invoice", "type Payment"],
    }))


def credits(ctx):
    return render(ctx, '''
«imports»

/** Credit notes reduce what the customer owes; they can never exceed the outstanding amount. */
export function issueCreditNote(ctx: AppContext, actorId: string, invoiceId: string, amountCents: Cents, reason: string): CreditNote {
  const invoice = invoices(ctx).require(invoiceId, 'invoice');
  if (invoice.status !== 'issued' && invoice.status !== 'paid') {
    throw new ConflictError('not-issued', `Credit notes require an issued invoice (${invoice.status})`);
  }
  if (!Number.isInteger(amountCents) || amountCents <= 0) throw new DomainError('invalid-amount', 'Credits must be positive');
  const already = creditedCents(ctx, invoiceId);
  if (already + amountCents > invoice.totalCents) {
    throw new ConflictError('credit-exceeds-invoice', `Credits would exceed invoice ${invoiceId}`, {
      credited: already,
      total: invoice.totalCents,
    });
  }
  const note: CreditNote = {
    id: ctx.ids.next('CRN'),
    number: `${ctx.config.billing.creditNotePrefix}-${ctx.ids.next('CRNNO').slice(6)}`,
    invoiceId,
    customerId: invoice.customerId,
    amountCents,
    reason,
    issuedOn: dateOf(ctx.clock.now()),
  };
  creditNotes(ctx).insert(note);
  if (invoice.status === 'issued' && invoice.paidCents + already + amountCents >= invoice.totalCents) {
    invoices(ctx).update(invoiceId, { status: 'paid' });
  }
  recordAudit(ctx, actorId, 'credit-note.issued', note.id, { invoiceId, amountCents });
  return note;
}
''', imports=imports(ctx, "src/billing/credits.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/invoices.ts": ["creditNotes", "invoices"],
        "src/adapters/store/audit.ts": ["recordAudit"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/money.ts": ["type Cents"],
        "src/shared/time.ts": ["dateOf"],
        "src/billing/model.ts": ["type CreditNote"],
        "src/billing/payments.ts": ["creditedCents"],
    }))


def statements(ctx):
    buckets = '''  const buckets: Record<AgingBucket, Cents> = { current: 0, '1-30': 0, '31-60': 0, '61-90': 0, '90+': 0 };
  for (const line of lines) buckets[line.bucket] += line.outstandingCents;''' if not ctx.refactor else '''  const buckets = lines.reduce<Record<AgingBucket, Cents>>(
    (totals, line) => ({ ...totals, [line.bucket]: totals[line.bucket] + line.outstandingCents }),
    { current: 0, '1-30': 0, '31-60': 0, '61-90': 0, '90+': 0 },
  );'''
    bucket_fn = '''export function agingBucket(daysOverdue: number): AgingBucket {
  if (daysOverdue <= 0) return 'current';
  if (daysOverdue <= 30) return '1-30';
  if (daysOverdue <= 60) return '31-60';
  if (daysOverdue <= 90) return '61-90';
  return '90+';
}''' if not ctx.refactor else '''const BUCKET_LIMITS: ReadonlyArray<[number, AgingBucket]> = [
  [0, 'current'],
  [30, '1-30'],
  [60, '31-60'],
  [90, '61-90'],
];

export function agingBucket(daysOverdue: number): AgingBucket {
  return BUCKET_LIMITS.find(([limit]) => daysOverdue <= limit)?.[1] ?? '90+';
}'''
    return render(ctx, '''
«imports»

export type AgingBucket = 'current' | '1-30' | '31-60' | '61-90' | '90+';

«bucket_fn»

export interface StatementLine {
  invoiceId: string;
  number?: string;
  issuedOn?: CalendarDate;
  dueOn?: CalendarDate;
  totalCents: Cents;
  outstandingCents: Cents;
  bucket: AgingBucket;
}

export interface Statement {
  customerId: string;
  asOf: CalendarDate;
  lines: StatementLine[];
  buckets: Record<AgingBucket, Cents>;
  totalOutstandingCents: Cents;
}

export function customerStatement(ctx: AppContext, customerId: string, asOf: CalendarDate): Statement {
  const lines = invoicesForCustomer(ctx, customerId)
    .filter((invoice) => invoice.status === 'issued')
    .map((invoice) => {
      const outstanding = outstandingCents(invoice, creditedCents(ctx, invoice.id));
      const overdue = invoice.dueOn === undefined ? 0 : daysBetween(invoice.dueOn, asOf);
      return {
        invoiceId: invoice.id,
        number: invoice.number,
        issuedOn: invoice.issuedOn,
        dueOn: invoice.dueOn,
        totalCents: invoice.totalCents,
        outstandingCents: outstanding,
        bucket: agingBucket(overdue),
      };
    })
    .filter((line) => line.outstandingCents > 0);
«buckets»
  return {
    customerId,
    asOf,
    lines,
    buckets,
    totalOutstandingCents: lines.reduce((sum, line) => sum + line.outstandingCents, 0),
  };
}
''', bucket_fn=bucket_fn, buckets=buckets, imports=imports(ctx, "src/billing/statements.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/invoices.ts": ["invoicesForCustomer"],
        "src/shared/money.ts": ["type Cents"],
        "src/shared/time.ts": ["daysBetween", "type CalendarDate"],
        "src/billing/model.ts": ["outstandingCents"],
        "src/billing/payments.ts": ["creditedCents"],
    }))


def subscriptions(ctx):
    return render(ctx, '''
«imports»

/** Drafts an invoice when work is completed, if `features.autoInvoiceOnCompletion` is enabled. */
export function registerBillingSubscriptions(ctx: AppContext): void {
  ctx.bus.on(«JobEvents».completed, (event) => {
    if (!ctx.config.features.autoInvoiceOnCompletion) return;
    try {
      draftInvoiceForJob(ctx, SYSTEM_ACTOR.id, String(event.jobId));
    } catch (error) {
      ctx.log.warn('automatic invoice failed', { jobId: event.jobId, error: error instanceof Error ? error.message : String(error) });
    }
  });
}
''', imports=imports(ctx, "src/billing/subscriptions.ts", {
        "src/context.ts": ["type AppContext"],
        "src/auth/roles.ts": ["SYSTEM_ACTOR"],
        "src/billing/invoices.ts": ["draftInvoiceForJob"],
    }, jobs=("JobEvents",)))


def generate(ctx):
    ctx.add("src/billing/model.ts", billing_model(ctx))
    for spec in TARIFFS:
        ctx.add(f"src/billing/tariffs/{spec[0]}.ts", tariff_module(ctx, spec))
    ctx.add("src/billing/tariffs/index.ts", tariff_index(ctx))
    ctx.add("src/billing/after-hours.ts", after_hours(ctx))
    ctx.add("src/billing/labor.ts", labor(ctx))
    ctx.add("src/billing/travel.ts", travel(ctx))
    ctx.add("src/billing/parts.ts", parts_lines(ctx))
    ctx.add("src/billing/discounts.ts", discounts(ctx))
    ctx.add("src/billing/tax.ts", tax(ctx))
    ctx.add("src/billing/invoices.ts", invoices(ctx))
    ctx.add("src/billing/payments.ts", payments(ctx))
    ctx.add("src/billing/credits.ts", credits(ctx))
    ctx.add("src/billing/statements.ts", statements(ctx))
    ctx.add("src/billing/subscriptions.ts", subscriptions(ctx))
