"""Work-type handlers registered in the string-keyed work-type registry."""

from .core import imports, render, rounding, rounding_names


def types_module(ctx):
    return render(ctx, '''
«imports»

export type LaborCategory = 'apprentice' | 'standard' | 'specialist';

export interface EstimateInput {
  assetKind?: AssetKind;
  hazards: string[];
  /** Units of work, for example radiators, floors or metres of duct. */
  quantity?: number;
  ageYears?: number;
}

export interface PartLine {
  sku: string;
  quantity: number;
}

export interface Estimate {
  minutes: number;
  crew: number;
  parts: PartLine[];
  notes: string[];
}

/**
 * Behaviour for one kind of field work. Handlers are looked up by key from the work-type
 * registry; unknown keys resolve to the general handler.
 */
export interface WorkTypeHandler {
  key: string;
  label: string;
  skills: string[];
  defaultDurationMinutes: number;
  defaultPriority: «JobPriority»;
  checklist: string[];
  requiresAsset: boolean;
  includedInPlans: boolean;
  laborCategory: LaborCategory;
  estimate(input: EstimateInput): Estimate;
  validateCompletion(job: «Job», report: CompletionReport): string[];
}

/** Hazards that require a second person on site regardless of work type. */
export const TWO_PERSON_HAZARDS = ['confined-space', 'working-at-height', 'live-electrical'];

export function crewFor(hazards: readonly string[], minimum = 1): number {
  return hazards.some((hazard) => TWO_PERSON_HAZARDS.includes(hazard)) ? Math.max(2, minimum) : minimum;
}

export function usedSku(report: CompletionReport, prefix: string): number {
  return report.partsUsed.filter((line) => line.sku.startsWith(prefix)).reduce((sum, line) => sum + line.quantity, 0);
}
''', imports=imports(ctx, "src/worktypes/types.ts", {"src/assets/model.ts": ["type AssetKind"]},
                    jobs=("type Job", "type JobPriority", "type CompletionReport")))


# Each handler: (key, export name, label, skills, duration, priority, checklist, requires asset,
# included in plans, labor category, estimate body, completion body, extra imports, rounding kinds).
HANDLERS = [
    ("general", "generalWork", "General maintenance", [], 60, "normal", ["site left clean"], False, True, "standard",
     '''    const minutes = 60 + Math.max(0, (input.quantity ?? 1) - 1) * 20;
    return { minutes, crew: crewFor(input.hazards), parts: [], notes: [] };''',
     '''    return report.resolution.length < 10 ? ['describe the work performed in at least 10 characters'] : [];''',
     {}, ()),
    ("boiler-service", "boilerService", "Annual boiler service", ["gas-safe"], 75, "low",
     ["flue gas analysis", "pressure check", "condensate trap cleaned", "safety devices tested"], True, True, "standard",
     '''    let minutes = 75;
    if ((input.ageYears ?? 0) > 15) minutes += 20;
    if (input.hazards.includes('restricted-access')) minutes += 15;
    return {
      minutes,
      crew: crewFor(input.hazards),
      parts: [{ sku: 'GSK-BOILER-KIT', quantity: 1 }],
      notes: (input.ageYears ?? 0) > 15 ? ['older appliance: allow for seized fittings'] : [],
    };''',
     '''    const problems: string[] = [];
    if (job.assetId === undefined) problems.push('boiler services must reference the boiler asset');
    if (!report.signatureName) problems.push('the customer must sign the gas safety record');
    return problems;''',
     {}, ()),
    ("boiler-repair", "boilerRepair", "Boiler breakdown repair", ["gas-safe"], 120, "high",
     ["fault diagnosed", "repair completed", "flue gas analysis"], True, False, "standard",
     '''    const base = input.assetKind === 'boiler' ? 120 : 150;
    return { minutes: base, crew: crewFor(input.hazards), parts: [], notes: ['parts depend on the diagnosed fault'] };''',
     '''    return report.followUpRequired || report.partsUsed.length > 0 || /reset|adjust/i.test(report.resolution)
      ? []
      : ['a repair without parts must explain the adjustment that fixed the fault'];''',
     {}, ()),
    ("heat-pump-service", "heatPumpService", "Heat pump service", ["refrigerant", "electrical"], 90, "low",
     ["refrigerant pressures logged", "coil cleaned", "defrost cycle tested", "filters replaced"], True, True,
     "specialist",
     '''    const filters = Math.max(1, input.quantity ?? 1);
    return {
      minutes: «quarter»,
      crew: crewFor(input.hazards),
      parts: [{ sku: 'FLT-HP-STD', quantity: filters }],
      notes: [],
    };''',
     '''    return usedSku(report, 'FLT-') > 0 ? [] : ['a heat pump service must record the replaced filters'];''',
     {}, ("up",)),
    ("heat-pump-install", "heatPumpInstall", "Heat pump installation", ["refrigerant", "electrical", "commissioning"],
     960, "normal", ["site survey confirmed", "unit mounted", "pipework pressure tested", "commissioned",
                     "customer handover"], False, False, "specialist",
     '''    const metres = input.quantity ?? 5;
    const pipework = Math.max(0, metres - 5) * 12;
    return {
      minutes: 960 + pipework,
      crew: 2,
      parts: [
        { sku: 'HP-UNIT-8KW', quantity: 1 },
        { sku: 'PIPE-CU-15', quantity: Math.ceil(metres) },
      ],
      notes: metres > 15 ? ['long pipe run: confirm refrigerant top-up'] : [],
    };''',
     '''    const problems: string[] = [];
    if (usedSku(report, 'HP-UNIT') !== 1) problems.push('exactly one heat pump unit must be recorded');
    if (!report.signatureName) problems.push('installations require a customer handover signature');
    return problems;''',
     {}, ()),
    ("ac-repair", "airConditioningRepair", "Air conditioning repair", ["refrigerant"], 90, "normal",
     ["leak test", "refrigerant recovered or topped up"], True, False, "standard",
     '''    const summer = input.hazards.includes('roof-unit') ? 30 : 0;
    return { minutes: 90 + summer, crew: crewFor(input.hazards, input.hazards.includes('roof-unit') ? 2 : 1), parts: [], notes: [] };''',
     '''    return [];''',
     {}, ()),
    ("chiller-maintenance", "chillerMaintenance", "Chiller maintenance", ["refrigerant", "hvac-commercial"], 240,
     "normal", ["oil sample taken", "condenser cleaned", "controls calibrated", "log sheet completed"], True, True,
     "specialist",
     '''    const circuits = Math.max(1, input.quantity ?? 1);
    const raw = 180 + circuits * 45;
    return { minutes: «quarter_raw», crew: 2, parts: [{ sku: 'OIL-SAMPLE-KIT', quantity: circuits }], notes: [] };''',
     '''    return usedSku(report, 'OIL-SAMPLE') === 0 ? ['chiller maintenance must record the oil sample kits used'] : [];''',
     {}, ("up",)),
    ("water-heater-replace", "waterHeaterReplacement", "Water heater replacement", ["plumbing", "electrical"], 240,
     "normal", ["old unit drained", "new unit installed", "temperature relief valve tested"], True, False, "standard",
     '''    const capacity = input.quantity ?? 150;
    const sku = capacity > 200 ? 'WH-300L' : capacity > 120 ? 'WH-200L' : 'WH-120L';
    return { minutes: capacity > 200 ? 300 : 240, crew: capacity > 200 ? 2 : 1, parts: [{ sku, quantity: 1 }], notes: [] };''',
     '''    return usedSku(report, 'WH-') === 1 ? [] : ['record exactly one replacement water heater'];''',
     {}, ()),
    ("generator-load-test", "generatorLoadTest", "Generator load bank test", ["electrical", "generators"], 180, "low",
     ["load bank connected", "load steps recorded", "fuel level recorded", "transfer switch tested"], True, True,
     "specialist",
     '''    const hours = Math.max(1, input.quantity ?? 2);
    return { minutes: 60 + hours * 60, crew: crewFor(input.hazards, 2), parts: [], notes: [] };''',
     '''    return report.meterReading === undefined ? ['record the generator run-hour meter after the test'] : [];''',
     {}, ()),
    ("fire-panel-inspection", "firePanelInspection", "Fire panel inspection", ["fire-safety"], 120, "normal",
     ["panel log reviewed", "detectors sampled", "sounders tested", "batteries load tested"], True, True, "standard",
     '''    const zones = Math.max(1, input.quantity ?? 4);
    const sampled = Math.ceil(zones / 4);
    return { minutes: 60 + sampled * 30, crew: 1, parts: [], notes: [`sample ${sampled} of ${zones} zones`] };''',
     '''    return report.signatureName ? [] : ['the responsible person must sign the inspection certificate'];''',
     {}, ()),
    ("elevator-inspection", "elevatorInspection", "Elevator inspection", ["lifts"], 150, "normal",
     ["brake test", "door interlocks", "alarm line", "pit inspection"], True, True, "specialist",
     '''    const floors = Math.max(2, input.quantity ?? 4);
    return { minutes: 90 + floors * 15, crew: 2, parts: [], notes: [] };''',
     '''    const problems: string[] = [];
    if (job.priority === 'low') problems.push('elevator inspections cannot be completed as low priority work');
    if (!report.signatureName) problems.push('the inspection must be countersigned');
    return problems;''',
     {}, ()),
    ("backflow-test", "backflowTest", "Backflow preventer test", ["plumbing", "backflow"], 45, "low",
     ["test kit calibrated", "differential pressure recorded"], True, True, "standard",
     '''    const devices = Math.max(1, input.quantity ?? 1);
    return { minutes: 30 + devices * 15, crew: 1, parts: [], notes: [] };''',
     '''    return /\\d+(\\.\\d+)?\\s*(psi|kpa)/i.test(report.resolution)
      ? []
      : ['record the measured differential pressure with its unit'];''',
     {}, ()),
    ("solar-inverter-repair", "solarInverterRepair", "Solar inverter repair", ["electrical", "solar"], 120, "normal",
     ["dc isolation confirmed", "fault codes recorded", "output verified"], True, False, "specialist",
     '''    const panels = input.quantity ?? 0;
    return {
      minutes: 120 + (panels > 20 ? 30 : 0),
      crew: crewFor([...input.hazards, 'working-at-height'].filter((hazard) => hazard !== 'working-at-height' || panels > 0)),
      parts: [],
      notes: [],
    };''',
     '''    return [];''',
     {}, ()),
    ("leak-investigation", "leakInvestigation", "Leak investigation", ["plumbing"], 90, "high",
     ["moisture readings", "source located or ruled out"], False, False, "standard",
     '''    const rooms = Math.max(1, input.quantity ?? 1);
    return { minutes: «leak», crew: 1, parts: [], notes: rooms > 4 ? ['consider thermal imaging'] : [] };''',
     '''    return [];''',
     {}, ("up",)),
    ("drain-clearance", "drainClearance", "Drain clearance", ["drainage"], 60, "high", ["flow test"], False, False,
     "apprentice",
     '''    const metres = input.quantity ?? 10;
    return { minutes: metres > 30 ? 120 : 60, crew: metres > 30 ? 2 : 1, parts: [], notes: [] };''',
     '''    return [];''',
     {}, ()),
    ("thermostat-install", "thermostatInstall", "Smart thermostat installation", ["electrical"], 60, "low",
     ["wiring checked", "app paired", "schedule configured"], False, True, "apprentice",
     '''    const zones = Math.max(1, input.quantity ?? 1);
    return { minutes: 45 + zones * 15, crew: 1, parts: [{ sku: 'TSTAT-SMART', quantity: zones }], notes: [] };''',
     '''    const fitted = usedSku(report, 'TSTAT-');
    return fitted === 0 ? ['record the thermostats fitted'] : [];''',
     {}, ()),
    ("duct-cleaning", "ductCleaning", "Ductwork cleaning", ["hvac-commercial"], 240, "low",
     ["access panels fitted", "before photos", "after photos"], False, True, "apprentice",
     '''    const metres = Math.max(10, input.quantity ?? 40);
    return { minutes: «duct», crew: 2, parts: [], notes: [] };''',
     '''    return [];''',
     {}, ("up",)),
    ("emergency-callout", "emergencyCallout", "Emergency callout", [], 60, "emergency",
     ["site made safe"], False, False, "standard",
     '''    return { minutes: 60, crew: crewFor(input.hazards), parts: [], notes: ['make safe first; repairs follow up'] };''',
     '''    return report.followUpRequired || report.resolution.toLowerCase().includes('resolved')
      ? []
      : ['emergency work must either be resolved or flagged for follow-up'];''',
     {}, ()),
    ("gas-safety-check", "gasSafetyCheck", "Landlord gas safety check", ["gas-safe"], 45, "low",
     ["appliances tested", "flues inspected", "certificate issued"], False, True, "standard",
     '''    const appliances = Math.max(1, input.quantity ?? 2);
    return { minutes: 15 + appliances * 15, crew: 1, parts: [], notes: [] };''',
     '''    return report.signatureName ? [] : ['the certificate must name the person who received it'];''',
     {}, ()),
    ("commissioning", "systemCommissioning", "System commissioning", ["commissioning"], 180, "normal",
     ["design parameters confirmed", "performance recorded", "handover pack"], True, False, "specialist",
     '''    const systems = Math.max(1, input.quantity ?? 1);
    return { minutes: 120 + systems * 60, crew: 1, parts: [], notes: [] };''',
     '''    return report.followUpRequired ? ['commissioning cannot finish with open follow-up items'] : [];''',
     {}, ()),
]


def handler_module(ctx, spec):
    (key, name, label, skills, duration, priority, checklist, requires_asset, included, category, estimate,
     validate, extra, kinds) = spec
    here = f"src/worktypes/handlers/{key}.ts"
    estimate = render(ctx, estimate,
                      quarter=rounding(ctx, "60 + filters * 10", 15, "up"),
                      quarter_raw=rounding(ctx, "raw", 30, "up"),
                      leak=rounding(ctx, "60 + rooms * 20", 15, "up", minimum=90),
                      duct=rounding(ctx, "metres * 4", 30, "up"))
    uses_types = ["type WorkTypeHandler"]
    body = estimate + validate
    for helper in ("crewFor", "usedSku"):
        if helper + "(" in body:
            uses_types.append(helper)
    groups = {"src/worktypes/types.ts": uses_types, **extra}
    if kinds:
        groups["src/shared/rounding.ts"] = rounding_names(ctx, *kinds)
    unused_input = "input" not in estimate.replace("Estimate", "")
    unused_job = "job." not in validate and "job " not in validate
    unused_report = "report" not in validate
    estimate_param = "_input" if unused_input else "input"
    job_param = "_job" if unused_job else "job"
    report_param = "_report" if unused_report else "report"
    skill_list = ", ".join(f"'{skill}'" for skill in skills)
    checklist_list = "".join(f"    '{item}',\n" for item in checklist)
    return render(ctx, '''
«imports»

export const «name»: WorkTypeHandler = {
  key: '«key»',
  label: '«label»',
  skills: [«skills»],
  defaultDurationMinutes: «duration»,
  defaultPriority: '«priority»',
  checklist: [
«checklist»  ],
  requiresAsset: «requires»,
  includedInPlans: «included»,
  laborCategory: '«category»',
  estimate(«ep») {
«estimate»
  },
  validateCompletion(«jp», «rp») {
«validate»
  },
};
''', imports=imports(ctx, here, groups), name=name, key=key, label=label, skills=skill_list, duration=duration,
                  priority=priority, checklist=checklist_list, requires=str(requires_asset).lower(),
                  included=str(included).lower(), category=category, ep=estimate_param, jp=job_param,
                  rp=report_param, estimate=estimate, validate=validate)


def registry_module(ctx):
    here = "src/worktypes/registry.ts"
    groups = {
        "src/shared/registry.ts": ["createRegistry", "type Registry"],
        "src/worktypes/types.ts": ["type WorkTypeHandler"],
    }
    for spec in HANDLERS:
        groups[f"src/worktypes/handlers/{spec[0]}.ts"] = [spec[1]]
    entries = "".join(f"  {spec[1]},\n" for spec in HANDLERS if spec[0] != "general")
    return render(ctx, '''
«imports»

export const BUILT_IN_WORK_TYPES: readonly WorkTypeHandler[] = [
  generalWork,
«entries»];

/**
 * Work types are addressed by their key in API payloads, contracts and tariffs. Keys are
 * case-insensitive; anything unknown resolves to general maintenance.
 */
export function createWorkTypeRegistry(extra: readonly WorkTypeHandler[] = []): Registry<WorkTypeHandler> {
  const registry = createRegistry<WorkTypeHandler>('work types', {
    fallback: generalWork,
    normalizeKey: (key) => key.trim().toLowerCase(),
  });
  for (const handler of [...BUILT_IN_WORK_TYPES, ...extra]) registry.register(handler.key, handler);
  return registry;
}

/** Work types whose skills are all within `skills`. */
export function workTypesCoveredBy(registry: Registry<WorkTypeHandler>, skills: readonly string[]): string[] {
  return registry
    .entries()
    .filter(([, handler]) => handler.skills.every((skill) => skills.includes(skill)))
    .map(([key]) => key);
}
''', imports=imports(ctx, here, groups), entries=entries)


def generate(ctx):
    ctx.add("src/worktypes/types.ts", types_module(ctx))
    for spec in HANDLERS:
        ctx.add(f"src/worktypes/handlers/{spec[0]}.ts", handler_module(ctx, spec))
    ctx.add("src/worktypes/registry.ts", registry_module(ctx))
