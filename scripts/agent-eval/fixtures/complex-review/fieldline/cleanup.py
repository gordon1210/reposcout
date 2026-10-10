"""Declarative work types, tariffs and repositories, plus shared work-type test builders."""

import re

WORK_TYPE_DEFAULTS = r'''

type WorkTypeDefaults = Omit<WorkTypeHandler, 'key' | 'label' | 'estimate'>;

/** Values a work type has unless its definition states otherwise. */
export const WORK_TYPE_DEFAULTS: Readonly<WorkTypeDefaults> = Object.freeze({
  skills: [],
  defaultDurationMinutes: 60,
  defaultPriority: 'normal',
  checklist: [],
  requiresAsset: false,
  includedInPlans: false,
  laborCategory: 'standard',
  validateCompletion: () => [],
});

export type WorkTypeDefinition = Pick<WorkTypeHandler, 'key' | 'label' | 'estimate'> & Partial<WorkTypeDefaults>;

/** Builds a handler from its definition; omitted fields come from `WORK_TYPE_DEFAULTS`. */
export function defineWorkType(definition: WorkTypeDefinition): WorkTypeHandler {
  const merged = { ...WORK_TYPE_DEFAULTS, ...definition };
  return {
    key: merged.key,
    label: merged.label,
    skills: [...merged.skills],
    defaultDurationMinutes: merged.defaultDurationMinutes,
    defaultPriority: merged.defaultPriority,
    checklist: [...merged.checklist],
    requiresAsset: merged.requiresAsset,
    includedInPlans: merged.includedInPlans,
    laborCategory: merged.laborCategory,
    estimate: merged.estimate,
    validateCompletion: merged.validateCompletion,
  };
}
'''

HANDLER_DEFAULT_LINES = (
    "  skills: [],\n",
    "  defaultDurationMinutes: 60,\n",
    "  defaultPriority: 'normal',\n",
    "  requiresAsset: false,\n",
    "  includedInPlans: false,\n",
    "  laborCategory: 'standard',\n",
    "  validateCompletion(_job, _report) {\n    return [];\n  },\n",
)

STANDARD_TERMS = r'''
import type { Tariff } from '../model.ts';

type StandardTerm =
  | 'currency'
  | 'laborIncrementMinutes'
  | 'minimumLaborMinutes'
  | 'afterHoursMultiplierBp'
  | 'travelFreeKm'
  | 'chargeTravelTime'
  | 'taxRateBp'
  | 'cashRoundingCents';

/** Terms shared by every region unless its tariff states otherwise. */
export const STANDARD_TARIFF_TERMS: Readonly<Pick<Tariff, StandardTerm>> = Object.freeze({
  currency: 'EUR',
  laborIncrementMinutes: 15,
  minimumLaborMinutes: 60,
  afterHoursMultiplierBp: 15_000,
  travelFreeKm: 0,
  chargeTravelTime: false,
  taxRateBp: 1_900,
  cashRoundingCents: 1,
});

export type RegionTariff = Omit<Tariff, StandardTerm> & Partial<Pick<Tariff, StandardTerm>>;

/** A region's tariff on top of `STANDARD_TARIFF_TERMS`. */
export function defineTariff(terms: RegionTariff): Tariff {
  const merged = { ...STANDARD_TARIFF_TERMS, ...terms };
  return {
    id: merged.id,
    region: merged.region,
    currency: merged.currency,
    laborRates: { ...merged.laborRates },
    laborIncrementMinutes: merged.laborIncrementMinutes,
    minimumLaborMinutes: merged.minimumLaborMinutes,
    calloutFeeCents: merged.calloutFeeCents,
    afterHoursMultiplierBp: merged.afterHoursMultiplierBp,
    travelRatePerKmCents: merged.travelRatePerKmCents,
    travelFreeKm: merged.travelFreeKm,
    chargeTravelTime: merged.chargeTravelTime,
    partsMarkupBp: merged.partsMarkupBp,
    taxRateBp: merged.taxRateBp,
    cashRoundingCents: merged.cashRoundingCents,
  };
}
'''

TARIFF_DEFAULT_LINES = (
    "  currency: 'EUR',\n",
    "  laborIncrementMinutes: 15,\n",
    "  minimumLaborMinutes: 60,\n",
    "  afterHoursMultiplierBp: 15_000,\n",
    "  travelFreeKm: 0,\n",
    "  chargeTravelTime: false,\n",
    "  taxRateBp: 1_900,\n",
    "  cashRoundingCents: 1,\n",
)

COLLECTIONS = r'''
import type { AppContext } from '../../context.ts';
import type { Collection, CollectionName, Entity } from './memory.ts';

/** An accessor for one named store collection. Collection names are persisted keys. */
export function collectionAccessor<T extends Entity>(name: CollectionName): (ctx: AppContext) => Collection<T> {
  return (ctx) => ctx.store.collection<T>(name);
}
'''

TEST_BUILDERS = r'''
import type { CompletionReport, Job } from '../../src/jobs/jobs.ts';

/** An in-progress work order of one work type, for handler-level tests. */
export const jobOfType = (workType: string, overrides: Partial<Job> = {}): Job => ({
  id: 'WO-000001',
  number: 'FL-2026-00001',
  customerId: 'CUS-000001',
  siteId: 'SITE-000001',
  assetId: 'AST-000001',
  workType,
  priority: 'normal',
  status: 'in_progress',
  summary: 'Scheduled attendance',
  description: '',
  source: 'phone',
  requestedAt: '2026-03-02T08:00:00Z',
  holds: [],
  history: [],
  checklist: {},
  tags: [],
  reopenCount: 0,
  ...overrides,
});

/** A complete report without parts or follow-up work. */
export const completionReport = (overrides: Partial<CompletionReport> = {}): CompletionReport => ({
  completedAt: '2026-03-02T11:00:00Z',
  resolution: 'Completed the scheduled work in full',
  partsUsed: [],
  followUpRequired: false,
  ...overrides,
});
'''

TEST_BLOCK = re.compile(
    r"const job = \(overrides: Partial<Job> = \{\}\): Job => \(\{\n"
    r"  id: 'WO-000001',\n  number: 'FL-2026-00001',\n  customerId: 'CUS-000001',\n  siteId: 'SITE-000001',\n"
    r"  assetId: 'AST-000001',\n  workType: '(?P<key>[a-z-]+)',\n  priority: 'normal',\n  status: 'in_progress',\n"
    r"  summary: 'Scheduled attendance',\n  description: '',\n  source: 'phone',\n"
    r"  requestedAt: '2026-03-02T08:00:00Z',\n  holds: \[\],\n  history: \[\],\n  checklist: \{\},\n  tags: \[\],\n"
    r"  reopenCount: 0,\n  \.\.\.overrides,\n\}\);\n\n"
    r"const report = \(overrides: Partial<CompletionReport> = \{\}\): CompletionReport => \(\{\n"
    r"  completedAt: '2026-03-02T11:00:00Z',\n  resolution: 'Completed the scheduled work in full',\n"
    r"  partsUsed: \[\],\n  followUpRequired: false,\n  \.\.\.overrides,\n\}\);\n"
)

REPOSITORY = re.compile(
    r"export function (?P<name>\w+)\(ctx: AppContext\): Collection<(?P<type>\w+)> \{\n"
    r"  return ctx\.store\.collection<(?P=type)>\('(?P<key>\w+)'\);\n\}\n"
)


def files_under(ctx, prefix, suffix=".ts"):
    return sorted(path for path in ctx.files if path.startswith(prefix) and path.endswith(suffix))


def add_value_import(text, module, name):
    """Adds `name` to the value import from `module`, creating the import when needed."""
    pattern = re.compile(r"^import \{ ([^}]+) \} from '" + re.escape(module) + r"';$", re.M)
    found = pattern.search(text)
    if found:
        names = sorted({*found.group(1).split(", "), name}, key=str.lower)
        return text[:found.start()] + f"import {{ {', '.join(names)} }} from '{module}';" + text[found.end():]
    return f"import {{ {name} }} from '{module}';\n" + text


def drop_unused_type_import(text, module, name):
    line = f"import type {{ {name} }} from '{module}';\n"
    if line in text and text.count(name) == 1:
        text = text.replace(line, "")
    return text


def declarative_work_types(ctx):
    ctx.append("src/worktypes/types.ts", WORK_TYPE_DEFAULTS)
    for path in files_under(ctx, "src/worktypes/handlers/"):
        text = ctx.files[path]
        header = re.search(r"^export const (\w+): WorkTypeHandler = \{$", text, re.M)
        if not header or not text.endswith("\n};\n"):
            raise ValueError(f"{path}: unexpected handler layout")
        text = text[:header.start()] + f"export const {header.group(1)} = defineWorkType({{" + text[header.end():]
        text = text[:-len("};\n")] + "});\n"
        for line in HANDLER_DEFAULT_LINES:
            text = text.replace(line, "", 1) if text.count(line) == 1 else text
        text = add_value_import(text, "../types.ts", "defineWorkType")
        text = drop_unused_type_import(text, "../types.ts", "WorkTypeHandler")
        ctx.replace(path, text)


def declarative_tariffs(ctx):
    ctx.add("src/billing/tariffs/standard.ts", STANDARD_TERMS)
    for path in files_under(ctx, "src/billing/tariffs/"):
        if path.endswith(("/index.ts", "/standard.ts")):
            continue
        text = ctx.files[path]
        header = re.search(r"^export const (\w+): Tariff = \{$", text, re.M)
        if not header or not text.endswith("\n};\n"):
            raise ValueError(f"{path}: unexpected tariff layout")
        text = text[:header.start()] + f"export const {header.group(1)} = defineTariff({{" + text[header.end():]
        text = text[:-len("};\n")] + "});\n"
        for line in TARIFF_DEFAULT_LINES:
            text = text.replace(line, "", 1) if text.count(line) == 1 else text
        text = text.replace("import type { Tariff } from '../model.ts';\n", "import { defineTariff } from './standard.ts';\n")
        ctx.replace(path, text)


def repository_accessors(ctx):
    ctx.add("src/adapters/store/collections.ts", COLLECTIONS)
    for path in files_under(ctx, "src/adapters/repositories/"):
        text = ctx.files[path]
        text, count = REPOSITORY.subn(
            lambda match: f"export const {match['name']} = collectionAccessor<{match['type']}>('{match['key']}');\n", text)
        if count == 0:
            raise ValueError(f"{path}: no collection accessor")
        if text.count("AppContext") == 1:
            text = text.replace("import type { AppContext } from '../../context.ts';\n", "")
        memory = "import type { Collection } from '../store/memory.ts';\n"
        if text.count("Collection") == 1:
            text = text.replace(memory, "")
        accessor = "import { collectionAccessor } from '../store/collections.ts';\n"
        if memory in text:
            text = text.replace(memory, accessor + memory)
        else:
            text = re.sub(r"((?:^import [^\n]*\n)+)", lambda block: block.group(1) + accessor, text, count=1, flags=re.M)
        ctx.replace(path, text)


def work_type_test_builders(ctx):
    ctx.add("tests/support/worktypes.ts", TEST_BUILDERS)
    rewritten = 0
    for path in files_under(ctx, "tests/worktypes/", ".test.ts"):
        text = ctx.files[path]
        found = TEST_BLOCK.search(text)
        if not found:
            continue
        text = (text[:found.start()]
                + f"const job = (overrides: Partial<Job> = {{}}) => jobOfType('{found['key']}', overrides);\n\n"
                + "const report = completionReport;\n" + text[found.end():])
        text = text.replace("import { beforeEach, describe, it } from 'node:test';\n",
                            "import { beforeEach, describe, it } from 'node:test';\n"
                            "import { completionReport, jobOfType } from '../support/worktypes.ts';\n")
        if text.count("CompletionReport") == 1:
            text = text.replace("import type { CompletionReport, Job } from '../../src/jobs/jobs.ts';\n",
                                "import type { Job } from '../../src/jobs/jobs.ts';\n")
        ctx.replace(path, text)
        rewritten += 1
    if rewritten != 20:
        raise ValueError(f"expected 20 work-type tests with local builders, rewrote {rewritten}")


def generate(ctx):
    if not ctx.refactor:
        return
    declarative_work_types(ctx)
    declarative_tariffs(ctx)
    repository_accessors(ctx)
    work_type_test_builders(ctx)
