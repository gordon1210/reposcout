"""Customers, sites, service contracts and installed assets."""

from .core import imports, render


def customer_model(ctx):
    return render(ctx, '''
«imports»

export type CustomerStatus = 'active' | 'suspended' | 'closed';
export type CustomerSegment = 'residential' | 'commercial' | 'public';

export interface Address {
  line1: string;
  line2?: string;
  city: string;
  postcode: string;
  region: string;
  location: Coordinates;
}

export interface Customer {
  id: string;
  accountNumber: string;
  name: string;
  segment: CustomerSegment;
  status: CustomerStatus;
  billingRegion: string;
  email?: string;
  phone?: string;
  taxExempt: boolean;
  creditLimitCents: Cents;
  createdAt: Timestamp;
  suspendedReason?: string;
}

export interface Site {
  id: string;
  customerId: string;
  label: string;
  address: Address;
  accessNotes?: string;
  hazards: string[];
  contactName?: string;
  contactPhone?: string;
}

export const SEGMENTS: readonly CustomerSegment[] = ['residential', 'commercial', 'public'];

/** Default credit limit per segment when an account is opened without one. */
export const DEFAULT_CREDIT_LIMITS: Record<CustomerSegment, Cents> = {
  residential: 150_000,
  commercial: 2_500_000,
  public: 5_000_000,
};
''', imports=imports(ctx, "src/customers/model.ts", {
        "src/shared/geo.ts": ["type Coordinates"],
        "src/shared/money.ts": ["type Cents"],
        "src/shared/time.ts": ["type Timestamp"],
    }))


def customer_accounts(ctx):
    return render(ctx, '''
«imports»

const LETTERS = 'ABCDEFGHJKLMNPQRSTUVWXYZ';

/**
 * Account numbers are `<segment letter><six digits><check letter>`, for example `C004211K`.
 * The check letter is a weighted modulus over the digits and catches transposed digits.
 */
export function checkLetter(digits: string): string {
  let total = 0;
  for (let index = 0; index < digits.length; index += 1) {
    total += Number(digits[index]) * (index % 2 === 0 ? 3 : 7);
  }
  return LETTERS[total % LETTERS.length];
}

export function formatAccountNumber(segment: CustomerSegment, sequence: number): string {
  const digits = sequence.toString().padStart(6, '0');
  if (digits.length > 6) throw new RangeError('Account sequence exhausted');
  return `${segment[0].toUpperCase()}${digits}${checkLetter(digits)}`;
}

export function isValidAccountNumber(value: string): boolean {
  const match = /^([RCP])(\\d{6})([A-Z])$/.exec(value.trim().toUpperCase());
  return match !== null && checkLetter(match[2]) === match[3];
}

export function nextAccountNumber(ctx: AppContext, segment: CustomerSegment): string {
  return formatAccountNumber(segment, sequenceOf(ctx.ids.next('ACCT')));
}
''', imports=imports(ctx, "src/customers/accounts.ts", {
        "src/context.ts": ["type AppContext"],
        "src/shared/ids.ts": ["sequenceOf"],
        "src/customers/model.ts": ["type CustomerSegment"],
    }))


def customer_validation(ctx):
    return render(ctx, '''
«imports»

const coordinates = object({
  lat: number({ min: -90, max: 90 }),
  lon: number({ min: -180, max: 180 }),
});

export const addressInput = object({
  line1: string({ min: 1, max: 120 }),
  line2: optional(string({ max: 120 })),
  city: string({ min: 1, max: 80 }),
  postcode: string({ min: 3, max: 12 }),
  region: string({ min: 1, max: 40 }),
  location: coordinates,
});

export const customerInput = object({
  name: string({ min: 2, max: 120 }),
  segment: oneOf(SEGMENTS),
  billingRegion: string({ min: 1, max: 40 }),
  email: optional(string({ pattern: /^[^@\\s]+@[^@\\s]+\\.[a-z]{2,}$/i })),
  phone: optional(string({ pattern: /^\\+?[0-9 ()-]{6,20}$/ })),
  taxExempt: withDefault(boolean(), false),
  creditLimitCents: optional(integer({ min: 0 })),
});

export const customerPatch = object({
  name: optional(string({ min: 2, max: 120 })),
  email: optional(string({ pattern: /^[^@\\s]+@[^@\\s]+\\.[a-z]{2,}$/i })),
  phone: optional(string({ pattern: /^\\+?[0-9 ()-]{6,20}$/ })),
  billingRegion: optional(string({ min: 1, max: 40 })),
  creditLimitCents: optional(integer({ min: 0 })),
});

export const siteInput = object({
  label: string({ min: 1, max: 80 }),
  address: addressInput,
  accessNotes: optional(string({ max: 500 })),
  hazards: withDefault(array(string({ min: 1, max: 60 }), { max: 20 }), []),
  contactName: optional(string({ max: 80 })),
  contactPhone: optional(string({ max: 20 })),
});

export type CustomerInput = Parsed<typeof customerInput>;
export type CustomerPatch = Parsed<typeof customerPatch>;
export type SiteInput = Parsed<typeof siteInput>;
''', imports=imports(ctx, "src/customers/validation.ts", {
        "src/shared/validate.ts": ["array", "boolean", "integer", "number", "object", "oneOf", "optional",
                                   "string", "withDefault", "type Parsed"],
        "src/customers/model.ts": ["SEGMENTS"],
    }))


def customer_service(ctx):
    if ctx.refactor:
        create_head = '''export function createCustomer(ctx: AppContext, actorId: string, input: unknown): Customer {
  const data = parseInput(customerInput, input);'''
        create_sig_type = "unknown"
    else:
        create_head = '''export function createCustomer(ctx: AppContext, actorId: string, data: CustomerInput): Customer {'''
        create_sig_type = "CustomerInput"
    del create_sig_type
    if ctx.refactor:
        site_head = '''export function addSite(ctx: AppContext, actorId: string, customerId: string, input: unknown): Site {
  const data = parseInput(siteInput, input);
  const customer = customers(ctx).require(customerId, 'customer');'''
        patch_head = '''export function updateCustomer(ctx: AppContext, actorId: string, customerId: string, input: unknown): Customer {
  const patch = parseInput(customerPatch, input);'''
    else:
        site_head = '''export function addSite(ctx: AppContext, actorId: string, customerId: string, data: SiteInput): Site {
  const customer = customers(ctx).require(customerId, 'customer');'''
        patch_head = '''export function updateCustomer(ctx: AppContext, actorId: string, customerId: string, patch: CustomerPatch): Customer {'''
    groups = {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/adapters/repositories/sites.ts": ["sites", "sitesForCustomer"],
        "src/adapters/store/audit.ts": ["recordAudit"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/text.ts": ["normalizePostcode"],
        "src/customers/accounts.ts": ["nextAccountNumber"],
        "src/customers/model.ts": ["DEFAULT_CREDIT_LIMITS", "type Customer", "type Site"],
    }
    if ctx.refactor:
        groups["src/shared/validate.ts"] = ["parseInput"]
        groups["src/customers/validation.ts"] = ["customerInput", "customerPatch", "siteInput"]
    else:
        groups["src/customers/validation.ts"] = ["type CustomerInput", "type CustomerPatch", "type SiteInput"]
    return render(ctx, '''
«imports»

«create_head»
  const customer: Customer = {
    id: ctx.ids.next('CUS'),
    accountNumber: nextAccountNumber(ctx, data.segment),
    name: data.name,
    segment: data.segment,
    status: 'active',
    billingRegion: data.billingRegion,
    email: data.email?.toLowerCase(),
    phone: data.phone,
    taxExempt: data.segment === 'public' ? true : data.taxExempt,
    creditLimitCents: data.creditLimitCents ?? DEFAULT_CREDIT_LIMITS[data.segment],
    createdAt: ctx.clock.now(),
  };
  customers(ctx).insert(customer);
  recordAudit(ctx, actorId, 'customer.created', customer.id, { segment: customer.segment });
  return customer;
}

«patch_head»
  const current = customers(ctx).require(customerId, 'customer');
  if (current.status === 'closed') {
    throw new ConflictError('customer-closed', `Customer ${customerId} is closed and cannot be changed`);
  }
  const updated = customers(ctx).update(customerId, {
    ...(patch.name !== undefined ? { name: patch.name } : {}),
    ...(patch.email !== undefined ? { email: patch.email.toLowerCase() } : {}),
    ...(patch.phone !== undefined ? { phone: patch.phone } : {}),
    ...(patch.billingRegion !== undefined ? { billingRegion: patch.billingRegion } : {}),
    ...(patch.creditLimitCents !== undefined ? { creditLimitCents: patch.creditLimitCents } : {}),
  });
  recordAudit(ctx, actorId, 'customer.updated', customerId, { fields: Object.keys(patch) });
  return updated;
}

export function suspendCustomer(ctx: AppContext, actorId: string, customerId: string, reason: string): Customer {
  const customer = customers(ctx).require(customerId, 'customer');
  if (customer.status !== 'active') {
    throw new ConflictError('customer-not-active', `Only active customers can be suspended (status ${customer.status})`);
  }
  const updated = customers(ctx).update(customerId, { status: 'suspended', suspendedReason: reason });
  recordAudit(ctx, actorId, 'customer.suspended', customerId, { reason });
  return updated;
}

export function reactivateCustomer(ctx: AppContext, actorId: string, customerId: string): Customer {
  const customer = customers(ctx).require(customerId, 'customer');
  if (customer.status !== 'suspended') {
    throw new ConflictError('customer-not-suspended', `Customer ${customerId} is not suspended`);
  }
  const updated = customers(ctx).update(customerId, (current) => {
    const { suspendedReason: _ignored, ...rest } = current;
    return { ...rest, status: 'active' };
  });
  recordAudit(ctx, actorId, 'customer.reactivated', customerId);
  return updated;
}

export function closeCustomer(ctx: AppContext, actorId: string, customerId: string, openWork: number): Customer {
  if (openWork > 0) {
    throw new ConflictError('customer-has-open-work', `Customer ${customerId} still has ${openWork} open work items`, {
      openWork,
    });
  }
  const updated = customers(ctx).update(customerId, { status: 'closed' });
  recordAudit(ctx, actorId, 'customer.closed', customerId);
  return updated;
}

«site_head»
  if (customer.status === 'closed') {
    throw new ConflictError('customer-closed', `Cannot add a site to closed customer ${customerId}`);
  }
  const postcode = normalizePostcode(data.address.postcode);
  const duplicate = sitesForCustomer(ctx, customerId).find(
    (site) => site.address.postcode === postcode && site.address.line1.toLowerCase() === data.address.line1.toLowerCase(),
  );
  if (duplicate) {
    throw new ConflictError('duplicate-site', `Customer ${customerId} already has this address as ${duplicate.id}`, {
      siteId: duplicate.id,
    });
  }
  const site: Site = {
    id: ctx.ids.next('SITE'),
    customerId,
    label: data.label,
    address: { ...data.address, postcode },
    accessNotes: data.accessNotes,
    hazards: [...new Set(data.hazards.map((hazard) => hazard.toLowerCase()))],
    contactName: data.contactName,
    contactPhone: data.contactPhone,
  };
  sites(ctx).insert(site);
  recordAudit(ctx, actorId, 'site.added', site.id, { customerId });
  return site;
}

export function updateSiteAccess(
  ctx: AppContext,
  actorId: string,
  siteId: string,
  accessNotes: string | undefined,
  hazards: string[],
): Site {
  if (hazards.length > 20) throw new DomainError('too-many-hazards', 'A site can list at most 20 hazards');
  const updated = sites(ctx).update(siteId, {
    accessNotes,
    hazards: [...new Set(hazards.map((hazard) => hazard.trim().toLowerCase()).filter(Boolean))],
  });
  recordAudit(ctx, actorId, 'site.access-updated', siteId, { hazards: updated.hazards.length });
  return updated;
}

/** Whether new work may be booked for the customer; suspended accounts only accept emergencies. */
export function acceptsNewWork(customer: Customer, emergency: boolean): boolean {
  if (customer.status === 'closed') return false;
  return customer.status === 'active' || emergency;
}
''', imports=imports(ctx, "src/customers/service.ts", groups), create_head=create_head,
                  patch_head=patch_head, site_head=site_head)


def contract_model(ctx):
    return render(ctx, '''
«imports»

export type ContractPlan = 'basic' | 'priority' | 'premium';
export type ContractStatus = 'draft' | 'active' | 'expired' | 'terminated';

export interface Contract {
  id: string;
  customerId: string;
  siteIds: string[];
  plan: ContractPlan;
  status: ContractStatus;
  startsOn: CalendarDate;
  endsOn?: CalendarDate;
  slaPolicyId: string;
  coveredWorkTypes: string[];
  laborDiscountBasisPoints: number;
  includedVisitsPerYear: number;
  renewal: 'auto' | 'manual';
  terminatedReason?: string;
}

export interface PlanTerms {
  slaPolicyId: string;
  laborDiscountBasisPoints: number;
  includedVisitsPerYear: number;
  waiveCalloutFee: boolean;
}

export const PLAN_TERMS: Record<ContractPlan, PlanTerms> = {
  basic: { slaPolicyId: 'standard', laborDiscountBasisPoints: 0, includedVisitsPerYear: 1, waiveCalloutFee: false },
  priority: { slaPolicyId: 'priority', laborDiscountBasisPoints: 500, includedVisitsPerYear: 2, waiveCalloutFee: false },
  premium: { slaPolicyId: 'premium', laborDiscountBasisPoints: 1000, includedVisitsPerYear: 4, waiveCalloutFee: true },
};

export interface Coverage {
  contract: Contract;
  terms: PlanTerms;
  includedVisitAvailable: boolean;
}
''', imports=imports(ctx, "src/contracts/model.ts", {"src/shared/time.ts": ["type CalendarDate"]}))


def contract_service(ctx):
    return render(ctx, '''
«imports»

const contractInput = object({
  customerId: string({ min: 1 }),
  siteIds: array(string({ min: 1 }), { min: 1, max: 50 }),
  plan: oneOf(['basic', 'priority', 'premium'] as const),
  startsOn: date(),
  endsOn: optional(date()),
  coveredWorkTypes: array(string({ min: 1 }), { min: 1, max: 40 }),
  renewal: withDefault(oneOf(['auto', 'manual'] as const), 'manual'),
});

export function createContract(ctx: AppContext, actorId: string, input: unknown): Contract {
  const data = parseInput(contractInput, input);
  const customer = customers(ctx).require(data.customerId, 'customer');
  if (customer.status !== 'active') {
    throw new ConflictError('customer-not-active', `Contracts require an active customer (${customer.status})`);
  }
  for (const siteId of data.siteIds) requireSiteOf(ctx, customer.id, siteId);
  if (data.endsOn !== undefined && data.endsOn <= data.startsOn) {
    throw new DomainError('invalid-term', 'A contract must end after it starts');
  }
  for (const key of data.coveredWorkTypes) {
    if (!ctx.workTypes.has(key)) throw new DomainError('unknown-work-type', `Unknown work type ${key}`);
  }
  const terms = PLAN_TERMS[data.plan];
  const contract: Contract = {
    id: ctx.ids.next('CON'),
    customerId: customer.id,
    siteIds: [...new Set(data.siteIds)],
    plan: data.plan,
    status: 'draft',
    startsOn: data.startsOn,
    endsOn: data.endsOn,
    slaPolicyId: terms.slaPolicyId,
    coveredWorkTypes: [...new Set(data.coveredWorkTypes)],
    laborDiscountBasisPoints: terms.laborDiscountBasisPoints,
    includedVisitsPerYear: terms.includedVisitsPerYear,
    renewal: data.renewal,
  };
  contracts(ctx).insert(contract);
  recordAudit(ctx, actorId, 'contract.created', contract.id, { plan: contract.plan });
  return contract;
}

export function activateContract(ctx: AppContext, actorId: string, contractId: string): Contract {
  const contract = contracts(ctx).require(contractId, 'contract');
  if (contract.status !== 'draft') {
    throw new ConflictError('contract-not-draft', `Contract ${contractId} is ${contract.status}`);
  }
  const today = dateOf(ctx.clock.now());
  for (const siteId of contract.siteIds) {
    const overlapping = contractForSite(ctx, siteId, today > contract.startsOn ? today : contract.startsOn);
    if (overlapping) {
      throw new ConflictError('overlapping-contract', `Site ${siteId} is already covered by ${overlapping.id}`, {
        contractId: overlapping.id,
      });
    }
  }
  const updated = contracts(ctx).update(contractId, { status: 'active' });
  recordAudit(ctx, actorId, 'contract.activated', contractId);
  return updated;
}

export function terminateContract(ctx: AppContext, actorId: string, contractId: string, reason: string): Contract {
  const contract = contracts(ctx).require(contractId, 'contract');
  if (contract.status === 'terminated' || contract.status === 'expired') {
    throw new ConflictError('contract-ended', `Contract ${contractId} has already ended`);
  }
  const today = dateOf(ctx.clock.now());
  const updated = contracts(ctx).update(contractId, {
    status: 'terminated',
    endsOn: contract.endsOn !== undefined && contract.endsOn < today ? contract.endsOn : today,
    terminatedReason: reason,
  });
  recordAudit(ctx, actorId, 'contract.terminated', contractId, { reason });
  return updated;
}

/** Expires active contracts whose end date has passed; auto-renewing contracts roll over one year. */
export function rollContracts(ctx: AppContext, today: CalendarDate): { expired: string[]; renewed: string[] } {
  const expired: string[] = [];
  const renewed: string[] = [];
  for (const contract of expiringContracts(ctx, today)) {
    if (contract.renewal === 'auto' && contract.endsOn !== undefined) {
      const length = daysBetween(contract.startsOn, contract.endsOn);
      const startsOn = addDays(contract.endsOn, 1);
      contracts(ctx).update(contract.id, { startsOn, endsOn: addDays(startsOn, length) });
      renewed.push(contract.id);
    } else {
      contracts(ctx).update(contract.id, { status: 'expired' });
      expired.push(contract.id);
    }
  }
  return { expired, renewed };
}
''', imports=imports(ctx, "src/contracts/service.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/contracts.ts": ["contractForSite", "contracts", "expiringContracts"],
        "src/adapters/repositories/customers.ts": ["customers"],
        "src/adapters/repositories/sites.ts": ["requireSiteOf"],
        "src/adapters/store/audit.ts": ["recordAudit"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/time.ts": ["addDays", "dateOf", "daysBetween", "type CalendarDate"],
        "src/shared/validate.ts": ["array", "date", "object", "oneOf", "optional", "parseInput", "string",
                                   "withDefault"],
        "src/contracts/model.ts": ["PLAN_TERMS", "type Contract"],
    }))


def contract_entitlements(ctx):
    loop = '''  const used = jobsForSite(ctx, siteId).filter(
    (job) =>
      job.contractId === contract.id &&
      job.status !== 'cancelled' &&
      job.requestedAt >= `${yearStart}T00:00Z` &&
      job.tags.includes('included-visit'),
  ).length;''' if ctx.refactor else '''  let used = 0;
  for (const job of jobsForSite(ctx, siteId)) {
    if (job.contractId !== contract.id || job.status === 'cancelled') continue;
    if (job.requestedAt < `${yearStart}T00:00Z`) continue;
    if (job.tags.includes('included-visit')) used += 1;
  }'''
    return render(ctx, '''
«imports»

/** Start of the contract year that contains `on`; contract years run from the start date. */
export function contractYearStart(contract: Contract, on: CalendarDate): CalendarDate {
  const [startYear, month, day] = contract.startsOn.split('-');
  let year = Number(on.slice(0, 4));
  let candidate = `${year}-${month}-${day}`;
  if (candidate > on) {
    year -= 1;
    candidate = `${year}-${month}-${day}`;
  }
  return Number(startYear) > year ? contract.startsOn : candidate;
}

export function includedVisitsUsed(ctx: AppContext, contract: Contract, siteId: string, on: CalendarDate): number {
  const yearStart = contractYearStart(contract, on);
«loop»
  return used;
}

/**
 * Coverage for a site and work type on a date. Work types outside the contract are billed as
 * ordinary work even when the site itself is covered.
 */
export function coverageFor(ctx: AppContext, siteId: string, workType: string, on: CalendarDate): Coverage | undefined {
  const contract = contractForSite(ctx, siteId, on);
  if (!contract || !contract.coveredWorkTypes.includes(workType)) return undefined;
  const used = includedVisitsUsed(ctx, contract, siteId, on);
  return { contract, terms: PLAN_TERMS[contract.plan], includedVisitAvailable: used < contract.includedVisitsPerYear };
}
''', loop=loop, imports=imports(ctx, "src/contracts/entitlements.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/contracts.ts": ["contractForSite"],
        "src/adapters/repositories/jobs.ts": ["jobsForSite"],
        "src/shared/time.ts": ["type CalendarDate"],
        "src/contracts/model.ts": ["PLAN_TERMS", "type Contract", "type Coverage"],
    }))


def asset_model(ctx):
    return render(ctx, '''
«imports»

export type AssetKind =
  | 'boiler'
  | 'heat-pump'
  | 'air-handler'
  | 'chiller'
  | 'water-heater'
  | 'generator'
  | 'fire-panel'
  | 'elevator'
  | 'solar-inverter'
  | 'backflow-preventer';

export interface Asset {
  id: string;
  siteId: string;
  kind: AssetKind;
  manufacturer: string;
  model: string;
  serial: string;
  installedOn: CalendarDate;
  warrantyUntil?: CalendarDate;
  retiredOn?: CalendarDate;
  serviceIntervalDays: number;
  lastServicedOn?: CalendarDate;
  meterReading?: number;
}

/** Statutory or manufacturer service intervals in days. */
export const SERVICE_INTERVALS: Record<AssetKind, number> = {
  boiler: 365,
  'heat-pump': 365,
  'air-handler': 180,
  chiller: 90,
  'water-heater': 365,
  generator: 182,
  'fire-panel': 182,
  elevator: 30,
  'solar-inverter': 730,
  'backflow-preventer': 365,
};

export const ASSET_KINDS = Object.keys(SERVICE_INTERVALS) as AssetKind[];
''', imports=imports(ctx, "src/assets/model.ts", {"src/shared/time.ts": ["type CalendarDate"]}))


def asset_service(ctx):
    due = '''    .filter((asset) => {
      const last = asset.lastServicedOn ?? asset.installedOn;
      return daysBetween(last, on) >= asset.serviceIntervalDays - withinDays;
    })''' if not ctx.refactor else '''    .filter((asset) => daysBetween(asset.lastServicedOn ?? asset.installedOn, on) + withinDays >= asset.serviceIntervalDays)'''
    return render(ctx, '''
«imports»

const assetInput = object({
  siteId: string({ min: 1 }),
  kind: oneOf(ASSET_KINDS),
  manufacturer: string({ min: 1, max: 60 }),
  model: string({ min: 1, max: 60 }),
  serial: string({ min: 3, max: 40 }),
  installedOn: date(),
  warrantyUntil: optional(date()),
  serviceIntervalDays: optional(integer({ min: 7, max: 3650 })),
});

export function registerAsset(ctx: AppContext, actorId: string, input: unknown): Asset {
  const data = parseInput(assetInput, input);
  sites(ctx).require(data.siteId, 'site');
  const serial = data.serial.toUpperCase();
  const existing = findAssetBySerial(ctx, serial);
  if (existing) {
    throw new ConflictError('duplicate-serial', `Serial ${serial} is already registered as ${existing.id}`, {
      assetId: existing.id,
    });
  }
  if (data.warrantyUntil !== undefined && data.warrantyUntil < data.installedOn) {
    throw new DomainError('invalid-warranty', 'Warranty cannot end before installation');
  }
  const asset: Asset = {
    id: ctx.ids.next('AST'),
    siteId: data.siteId,
    kind: data.kind,
    manufacturer: data.manufacturer,
    model: data.model,
    serial,
    installedOn: data.installedOn,
    warrantyUntil: data.warrantyUntil,
    serviceIntervalDays: data.serviceIntervalDays ?? SERVICE_INTERVALS[data.kind],
  };
  assets(ctx).insert(asset);
  recordAudit(ctx, actorId, 'asset.registered', asset.id, { siteId: asset.siteId, kind: asset.kind });
  return asset;
}

export function retireAsset(ctx: AppContext, actorId: string, assetId: string, on: CalendarDate): Asset {
  const asset = assets(ctx).require(assetId, 'asset');
  if (asset.retiredOn !== undefined) throw new ConflictError('asset-retired', `Asset ${assetId} is already retired`);
  if (on < asset.installedOn) throw new DomainError('invalid-date', 'An asset cannot be retired before installation');
  const updated = assets(ctx).update(assetId, { retiredOn: on });
  recordAudit(ctx, actorId, 'asset.retired', assetId);
  return updated;
}

/** Records a service visit; meter readings may never decrease. */
export function recordService(ctx: AppContext, assetId: string, on: CalendarDate, meterReading?: number): Asset {
  const asset = assets(ctx).require(assetId, 'asset');
  if (meterReading !== undefined && asset.meterReading !== undefined && meterReading < asset.meterReading) {
    throw new DomainError('meter-decreased', `Meter reading ${meterReading} is below the last reading ${asset.meterReading}`);
  }
  return assets(ctx).update(assetId, {
    lastServicedOn: asset.lastServicedOn !== undefined && asset.lastServicedOn > on ? asset.lastServicedOn : on,
    ...(meterReading !== undefined ? { meterReading } : {}),
  });
}

export type WarrantyStatus = 'in-warranty' | 'expiring' | 'expired' | 'none';

export function warrantyStatus(asset: Asset, on: CalendarDate): WarrantyStatus {
  if (asset.warrantyUntil === undefined) return 'none';
  if (asset.warrantyUntil < on) return 'expired';
  return daysBetween(on, asset.warrantyUntil) <= 30 ? 'expiring' : 'in-warranty';
}

/** Assets whose next service falls within `withinDays` of `on`, most overdue first. */
export function assetsDueForService(ctx: AppContext, on: CalendarDate, withinDays = 0): Asset[] {
  return assets(ctx)
    .find((asset) => asset.retiredOn === undefined)
«due»
    .sort((left, right) => nextServiceDate(left).localeCompare(nextServiceDate(right)));
}

export function nextServiceDate(asset: Asset): CalendarDate {
  return addDays(asset.lastServicedOn ?? asset.installedOn, asset.serviceIntervalDays);
}
''', due=due, imports=imports(ctx, "src/assets/service.ts", {
        "src/context.ts": ["type AppContext"],
        "src/adapters/repositories/assets.ts": ["assets", "findAssetBySerial"],
        "src/adapters/repositories/sites.ts": ["sites"],
        "src/adapters/store/audit.ts": ["recordAudit"],
        "src/shared/errors.ts": ["ConflictError", "DomainError"],
        "src/shared/time.ts": ["addDays", "daysBetween", "type CalendarDate"],
        "src/shared/validate.ts": ["date", "integer", "object", "oneOf", "optional", "parseInput", "string"],
        "src/assets/model.ts": ["ASSET_KINDS", "SERVICE_INTERVALS", "type Asset"],
    }))


def generate(ctx):
    ctx.add("src/customers/model.ts", customer_model(ctx))
    ctx.add("src/customers/accounts.ts", customer_accounts(ctx))
    ctx.add("src/customers/validation.ts", customer_validation(ctx))
    ctx.add("src/customers/service.ts", customer_service(ctx))
    ctx.add("src/contracts/model.ts", contract_model(ctx))
    ctx.add("src/contracts/service.ts", contract_service(ctx))
    ctx.add("src/contracts/entitlements.ts", contract_entitlements(ctx))
    ctx.add("src/assets/model.ts", asset_model(ctx))
    ctx.add("src/assets/service.ts", asset_service(ctx))
