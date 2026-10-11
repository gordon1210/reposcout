"""Building-services work types: new handlers, asset kinds, catalog tests and documentation."""

from . import checks_worktypes, worktypes
from .core import imports, render

VARIANT = "holdout-b-d"
HANDLER_DIR = "src/worktypes/handlers/"

OLDER = "older equipment: allow extra time"
TWO_PERSON = ("confined-space", "working-at-height", "live-electrical")

# Asset kinds added to the model: kind -> service interval in days.
NEW_ASSET_KINDS = {
    "ventilation-fan": 365,
    "cooling-tower": 90,
    "heat-exchanger": 180,
    "pump-set": 365,
    "pressure-vessel": 365,
    "ups": 182,
    "sprinkler-system": 91,
    "softener": 180,
}

# key, export name, label, skills, minutes, priority, checklist, asset kind, in plans, labor category,
# estimate {base, per, cap, age: (threshold, extra), hazard: (name, extra), crew, parts: (sku, per unit)},
# completion rules.
SPECS = [
    dict(key="air-handler-service", name="airHandlerService", label="Air handler service",
         skills=["hvac-commercial", "electrical"], minutes=120, priority="low",
         checklist=["filters replaced", "belts and bearings checked", "coil condition recorded"],
         kind="air-handler", plans=True, labor="standard",
         est=dict(base=90, per=30, age=(12, 20), crew=1, parts=("FLT-AHU-G4", True)),
         rules=[("signature", "the building manager must sign the service sheet")]),
    dict(key="ventilation-balancing", name="ventilationBalancing", label="Ventilation balancing",
         skills=["ventilation", "controls"], minutes=180, priority="normal",
         checklist=["design flow rates confirmed", "dampers set", "readings recorded"],
         kind="ventilation-fan", plans=True, labor="specialist",
         est=dict(base=120, per=20, cap=480, crew=1),
         rules=[("meter", "record the final fan speed reading")]),
    dict(key="cooling-tower-clean", name="coolingTowerClean", label="Cooling tower cleaning",
         skills=["water-treatment", "hvac-commercial"], minutes=360, priority="low",
         checklist=["basin drained", "fill pack cleaned", "biocide dosed", "water sample taken"],
         kind="cooling-tower", plans=True, labor="specialist",
         est=dict(base=300, per=60, crew=2, parts=("BIOCIDE-20L", False)),
         rules=[("sku", "BIOCIDE-", "record the biocide used"), ("signature", "the responsible person must sign the log book")]),
    dict(key="legionella-sampling", name="legionellaSampling", label="Legionella sampling",
         skills=["water-treatment"], minutes=60, priority="low",
         checklist=["outlets selected", "temperatures recorded", "samples sealed"],
         kind=None, plans=True, labor="standard",
         est=dict(base=45, per=10, cap=180, crew=1, parts=("SAMPLE-BOTTLE", True)),
         rules=[("sku", "SAMPLE-", "record the sample bottles used")]),
    dict(key="heat-exchanger-descale", name="heatExchangerDescale", label="Heat exchanger descaling",
         skills=["plumbing", "pressure-systems"], minutes=240, priority="normal",
         checklist=["circuit isolated", "descaler circulated", "plates inspected", "circuit refilled"],
         kind="heat-exchanger", plans=False, labor="standard",
         est=dict(base=210, per=45, age=(10, 30), crew=1, parts=("DESCALER-25L", False)),
         rules=[("asset", "descaling must reference the heat exchanger asset")]),
    dict(key="pump-set-overhaul", name="pumpSetOverhaul", label="Pump set overhaul",
         skills=["pumps", "electrical"], minutes=300, priority="normal",
         checklist=["pump isolated", "seals replaced", "alignment checked", "run test completed"],
         kind="pump-set", plans=False, labor="specialist",
         est=dict(base=240, per=90, hazard=("restricted-access", 30), crew=1, parts=("SEAL-MECH", True)),
         rules=[("sku", "SEAL-", "record the mechanical seals fitted"), ("meter", "record the running hours at handover")]),
    dict(key="pressure-vessel-inspection", name="pressureVesselInspection", label="Pressure vessel inspection",
         skills=["pressure-systems"], minutes=150, priority="normal",
         checklist=["written scheme reviewed", "external examination", "safety valve verified"],
         kind="pressure-vessel", plans=True, labor="specialist",
         est=dict(base=120, per=45, crew=1),
         rules=[("signature", "the competent person must countersign the report"), ("followup", "inspections cannot finish with open follow-up items")]),
    dict(key="ups-battery-test", name="upsBatteryTest", label="UPS battery test",
         skills=["electrical", "power-systems"], minutes=120, priority="low",
         checklist=["runtime test", "battery voltages logged", "bypass tested"],
         kind="ups", plans=True, labor="specialist",
         est=dict(base=90, per=30, age=(5, 30), crew=1),
         rules=[("meter", "record the measured runtime in minutes")]),
    dict(key="sprinkler-flow-test", name="sprinklerFlowTest", label="Sprinkler flow test",
         skills=["sprinklers", "fire-safety"], minutes=180, priority="normal",
         checklist=["main drain test", "alarm valve tripped", "flow switch verified"],
         kind="sprinkler-system", plans=True, labor="standard",
         est=dict(base=150, per=30, cap=420, crew=2),
         rules=[("signature", "the responsible person must sign the test certificate")]),
    dict(key="emergency-lighting-test", name="emergencyLightingTest", label="Emergency lighting test",
         skills=["lighting", "electrical"], minutes=120, priority="low",
         checklist=["duration test", "failed fittings listed", "certificate issued"],
         kind=None, plans=True, labor="standard",
         est=dict(base=60, per=5, cap=360, crew=1, parts=("LAMP-EM-3H", False)),
         rules=[("signature", "the responsible person must sign the lighting certificate")]),
    dict(key="fire-door-inspection", name="fireDoorInspection", label="Fire door inspection",
         skills=["fire-safety", "carpentry"], minutes=90, priority="low",
         checklist=["gaps measured", "seals checked", "closers tested"],
         kind=None, plans=True, labor="apprentice",
         est=dict(base=30, per=6, cap=240, crew=1),
         rules=[]),
    dict(key="smoke-extract-test", name="smokeExtractTest", label="Smoke extract test",
         skills=["fire-safety", "ventilation"], minutes=150, priority="normal",
         checklist=["fans started from the panel", "damper positions confirmed", "airflow measured"],
         kind=None, plans=True, labor="standard",
         est=dict(base=120, per=25, crew=2),
         rules=[("signature", "the responsible person must sign the test record")]),
    dict(key="door-entry-fault", name="doorEntryFault", label="Door entry fault",
         skills=["access-control", "electrical"], minutes=90, priority="high",
         checklist=["fault reproduced", "power and network checked", "access restored"],
         kind=None, plans=False, labor="standard",
         est=dict(base=75, per=20, cap=240, crew=1),
         rules=[]),
    dict(key="cctv-maintenance", name="cctvMaintenance", label="CCTV maintenance",
         skills=["security-systems"], minutes=120, priority="low",
         checklist=["camera views checked", "lenses cleaned", "recorder health verified"],
         kind=None, plans=True, labor="standard",
         est=dict(base=60, per=8, cap=300, crew=1),
         rules=[("signature", "the site contact must sign the maintenance sheet")]),
    dict(key="intruder-alarm-service", name="intruderAlarmService", label="Intruder alarm service",
         skills=["security-systems", "electrical"], minutes=90, priority="low",
         checklist=["detectors walk tested", "batteries load tested", "signalling path tested"],
         kind=None, plans=True, labor="standard",
         est=dict(base=60, per=10, cap=240, crew=1, parts=("BATT-ALARM-12V", False)),
         rules=[("sku", "BATT-", "record the alarm batteries replaced")]),
    dict(key="bms-commissioning", name="bmsCommissioning", label="Building management commissioning",
         skills=["building-management", "commissioning"], minutes=480, priority="normal",
         checklist=["points list verified", "control sequences tested", "alarms mapped", "handover pack"],
         kind=None, plans=False, labor="specialist",
         est=dict(base=420, per=60, crew=2),
         rules=[("signature", "commissioning requires a customer handover signature"), ("followup", "commissioning cannot finish with open follow-up items")]),
    dict(key="bms-fault-diagnosis", name="bmsFaultDiagnosis", label="Building management fault diagnosis",
         skills=["building-management", "controls"], minutes=120, priority="high",
         checklist=["alarm history reviewed", "fault isolated", "control restored"],
         kind=None, plans=False, labor="specialist",
         est=dict(base=90, per=30, cap=360, crew=1),
         rules=[]),
    dict(key="softener-service", name="softenerService", label="Water softener service",
         skills=["plumbing", "water-treatment"], minutes=60, priority="low",
         checklist=["resin bed checked", "salt level topped up", "hardness test recorded"],
         kind="softener", plans=True, labor="standard",
         est=dict(base=45, per=15, age=(8, 15), crew=1, parts=("SALT-25KG", True)),
         rules=[("sku", "SALT-", "record the salt supplied")]),
    dict(key="tank-cleaning", name="tankCleaning", label="Cold water tank cleaning",
         skills=["water-treatment", "plumbing"], minutes=240, priority="low",
         checklist=["tank isolated", "sludge removed", "surfaces disinfected", "refilled and flushed"],
         kind=None, plans=True, labor="apprentice",
         est=dict(base=180, per=60, crew=2),
         rules=[("signature", "the responsible person must sign the disinfection record")]),
    dict(key="grease-trap-clearance", name="greaseTrapClearance", label="Grease trap clearance",
         skills=["drainage"], minutes=90, priority="normal",
         checklist=["trap emptied", "baffles cleaned", "waste manifest issued"],
         kind=None, plans=False, labor="apprentice",
         est=dict(base=60, per=30, cap=240, crew=1),
         rules=[("signature", "the kitchen manager must sign the waste manifest")]),
    dict(key="gas-leak-response", name="gasLeakResponse", label="Gas leak response",
         skills=["gas-safe", "emergency-response"], minutes=90, priority="emergency",
         checklist=["supply isolated", "area ventilated", "leak located", "tightness test passed"],
         kind=None, plans=False, labor="specialist",
         est=dict(base=90, per=30, cap=240, crew=2),
         rules=[("signature", "the occupier must sign the gas safety notice"), ("followup", "a gas leak call cannot finish with open follow-up items")]),
    dict(key="electrical-testing", name="electricalTesting", label="Fixed wire electrical testing",
         skills=["electrical", "testing-inspection"], minutes=240, priority="normal",
         checklist=["circuits isolated in turn", "insulation resistance recorded", "earth fault loop recorded", "report issued"],
         kind=None, plans=True, labor="specialist",
         est=dict(base=180, per=15, cap=600, crew=1),
         rules=[("signature", "the duty holder must sign the condition report")]),
    dict(key="pat-testing", name="patTesting", label="Portable appliance testing",
         skills=["electrical"], minutes=60, priority="low",
         checklist=["appliances listed", "visual checks", "labels fitted"],
         kind=None, plans=True, labor="apprentice",
         est=dict(base=30, per=2, cap=480, crew=1, parts=("LABEL-PAT", False)),
         rules=[("sku", "LABEL-", "record the pass labels used")]),
    dict(key="thermal-imaging-survey", name="thermalImagingSurvey", label="Thermal imaging survey",
         skills=["thermography"], minutes=120, priority="low",
         checklist=["conditions recorded", "areas scanned", "images annotated"],
         kind=None, plans=False, labor="specialist",
         est=dict(base=90, per=30, cap=360, crew=1),
         rules=[]),
    dict(key="insulation-survey", name="insulationSurvey", label="Insulation survey",
         skills=["insulation"], minutes=90, priority="low",
         checklist=["loft and wall depths measured", "gaps photographed"],
         kind=None, plans=False, labor="apprentice",
         est=dict(base=60, per=15, cap=240, crew=1),
         rules=[]),
    dict(key="roof-unit-service", name="roofUnitService", label="Rooftop unit service",
         skills=["hvac-commercial", "rooftop"], minutes=150, priority="normal",
         checklist=["access secured", "filters and belts checked", "refrigerant circuit inspected"],
         kind="air-handler", plans=True, labor="standard",
         est=dict(base=120, per=45, crew=2),
         rules=[("signature", "the building manager must sign the service sheet")]),
    dict(key="telecoms-cabling", name="telecomsCabling", label="Telecoms cabling",
         skills=["telecoms"], minutes=180, priority="low",
         checklist=["route agreed", "cable pulled and labelled", "links tested"],
         kind=None, plans=False, labor="standard",
         est=dict(base=120, per=20, cap=600, crew=1, parts=("CAT6-DROP", True)),
         rules=[("sku", "CAT6-", "record the cable drops installed")]),
    dict(key="solar-panel-clean", name="solarPanelClean", label="Solar panel cleaning",
         skills=["solar", "rooftop"], minutes=120, priority="low",
         checklist=["array isolated", "panels washed", "output compared"],
         kind=None, plans=True, labor="apprentice",
         est=dict(base=60, per=3, cap=300, crew=1),
         rules=[]),
]

REGISTRY_GROUPS = (
    ("Heating, ventilation and water treatment", ("air-handler-service", "ventilation-balancing", "cooling-tower-clean",
                                                    "legionella-sampling", "heat-exchanger-descale", "pump-set-overhaul",
                                                    "pressure-vessel-inspection", "softener-service", "tank-cleaning",
                                                    "roof-unit-service")),
    ("Fire safety, security and electrical", ("ups-battery-test", "sprinkler-flow-test", "emergency-lighting-test",
                                              "fire-door-inspection", "smoke-extract-test", "door-entry-fault",
                                              "cctv-maintenance", "intruder-alarm-service", "electrical-testing",
                                              "pat-testing", "gas-leak-response")),
    ("Building controls and surveys", ("bms-commissioning", "bms-fault-diagnosis", "grease-trap-clearance",
                                       "thermal-imaging-survey", "insulation-survey", "telecoms-cabling",
                                       "solar-panel-clean")),
)

GOOD_RESOLUTION = "Work completed and verified on site"


def crew_of(hazards, minimum):
    return max(2, minimum) if any(h in TWO_PERSON for h in hazards) else minimum


def estimate_value(est, hazards=(), quantity=None, age=None):
    units = max(1, 1 if quantity is None else quantity)
    minutes = est["base"] + (units - 1) * est["per"]
    notes = []
    if "age" in est and (age or 0) > est["age"][0]:
        minutes += est["age"][1]
        notes = [OLDER]
    if "hazard" in est and est["hazard"][0] in hazards:
        minutes += est["hazard"][1]
    if "cap" in est:
        minutes = min(est["cap"], minutes)
    parts = []
    if "parts" in est:
        sku, per_unit = est["parts"]
        parts = [(sku, units if per_unit else 1)]
    return minutes, crew_of(hazards, est["crew"]), parts, notes


def estimate_body(est):
    lines = []
    uses_units = est["per"] > 0 or ("parts" in est and est["parts"][1])
    if uses_units:
        lines.append("    const units = Math.max(1, input.quantity ?? 1);")
    base = f"{est['base']} + (units - 1) * {est['per']}" if est["per"] > 0 else str(est["base"])
    simple = not any(key in est for key in ("age", "hazard", "cap"))
    if simple:
        minutes = base
    else:
        lines.append(f"    let minutes = {base};")
        if "age" in est:
            lines.append(f"    if ((input.ageYears ?? 0) > {est['age'][0]}) minutes += {est['age'][1]};")
        if "hazard" in est and est["hazard"][1] > 0:
            lines.append(f"    if (input.hazards.includes('{est['hazard'][0]}')) minutes += {est['hazard'][1]};")
        if "cap" in est:
            lines.append(f"    minutes = Math.min({est['cap']}, minutes);")
        minutes = "minutes"
    crew = "crewFor(input.hazards)" if est["crew"] == 1 else f"crewFor(input.hazards, {est['crew']})"
    if "parts" in est:
        sku, per_unit = est["parts"]
        parts = f"[{{ sku: '{sku}', quantity: {'units' if per_unit else 1} }}]"
    else:
        parts = "[]"
    notes = f"(input.ageYears ?? 0) > {est['age'][0]} ? ['{OLDER}'] : []" if "age" in est else "[]"
    ret = f"    return {{ minutes{'' if minutes == 'minutes' else ': ' + minutes}, crew: {crew}, parts: {parts}, notes: {notes} }};"
    if len(ret) > 118:
        ret = (f"    return {{\n      minutes{'' if minutes == 'minutes' else ': ' + minutes},\n      crew: {crew},\n"
               f"      parts: {parts},\n      notes: {notes},\n    }};")
    lines.append(ret)
    return "\n".join(lines)


def rule_condition(rule):
    kind = rule[0]
    if kind == "asset":
        return "job.assetId === undefined"
    if kind == "signature":
        return "!report.signatureName"
    if kind == "sku":
        return f"usedSku(report, '{rule[1]}') === 0"
    if kind == "meter":
        return "report.meterReading === undefined"
    if kind == "followup":
        return "report.followUpRequired"
    raise ValueError(kind)


def rule_message(rule):
    return rule[-1]


def validate_body(rules):
    if not rules:
        return "    return [];"
    lines = ["    const problems: string[] = [];"]
    for rule in rules:
        lines.append(f"    if ({rule_condition(rule)}) problems.push('{rule_message(rule)}');")
    lines.append("    return problems;")
    return "\n".join(lines)


def violated(rule, has_asset, fields):
    kind = rule[0]
    if kind == "asset":
        return not has_asset
    if kind == "signature":
        return "signatureName" not in fields
    if kind == "sku":
        return not any(sku.startswith(rule[1]) for sku in fields.get("skus", []))
    if kind == "meter":
        return "meterReading" not in fields
    return bool(fields.get("followUpRequired"))


def report_ts(fields, resolution=None):
    parts = []
    if resolution is not None:
        parts.append(f"resolution: '{resolution}'")
    if "signatureName" in fields:
        parts.append("signatureName: 'Avery Collins'")
    if fields.get("skus"):
        used = ", ".join(f"{{ sku: '{sku}', quantity: 1 }}" for sku in fields["skus"])
        parts.append(f"partsUsed: [{used}]")
    if "meterReading" in fields:
        parts.append(f"meterReading: {fields['meterReading']}")
    if fields.get("followUpRequired"):
        parts.append("followUpRequired: true")
    return "{ " + ", ".join(parts) + " }" if parts else "{}"


def complete_fields(rules):
    fields = {}
    for rule in rules:
        if rule[0] == "signature":
            fields["signatureName"] = "Avery Collins"
        elif rule[0] == "sku":
            fields.setdefault("skus", []).append(rule[1] + "STD")
        elif rule[0] == "meter":
            fields["meterReading"] = 1200
    return fields


def parts_ts(parts):
    return "[" + ", ".join(f"{{ sku: '{sku}', quantity: {quantity} }}" for sku, quantity in parts) + "]"


def notes_ts(notes):
    return "[" + ", ".join(f"'{note}'" for note in notes) + "]"


def estimate_rows(spec):
    est = spec["est"]
    scenarios = [
        ("{ hazards: [] }", dict()),
        ("{ hazards: [], quantity: 3 }", dict(quantity=3)),
        ("{ hazards: [], quantity: 0 }", dict(quantity=0)),
        ("{ hazards: ['confined-space'] }", dict(hazards=("confined-space",))),
    ]
    if "age" in est:
        threshold = est["age"][0]
        scenarios.append((f"{{ hazards: [], ageYears: {threshold} }}", dict(age=threshold)))
        scenarios.append((f"{{ hazards: [], ageYears: {threshold + 1} }}", dict(age=threshold + 1)))
    if "hazard" in est:
        name = est["hazard"][0]
        scenarios.append((f"{{ hazards: ['{name}'] }}", dict(hazards=(name,))))
    if "cap" in est:
        scenarios.append(("{ hazards: [], quantity: 90 }", dict(quantity=90)))
    rows = []
    for input_ts, args in scenarios:
        minutes, crew, parts, notes = estimate_value(est, **args)
        rows.append((input_ts, minutes, crew, parts_ts(parts), notes_ts(notes)))
    return rows


def completion_rows(spec):
    rules = spec["rules"]
    ok = complete_fields(rules)
    rows = [("accepts a complete report", "{}", report_ts(ok), [])]
    for rule in rules:
        kind = rule[0]
        if kind == "asset":
            rows.append(("requires the asset", "{ assetId: undefined }", report_ts(ok), [rule_message(rule)]))
        elif kind == "followup":
            rows.append(("rejects open follow-up items", "{}", report_ts({**ok, "followUpRequired": True}),
                         [rule_message(rule)]))
        else:
            lacking = dict(ok)
            if kind == "signature":
                lacking.pop("signatureName")
            elif kind == "meter":
                lacking.pop("meterReading")
            else:
                lacking["skus"] = [sku for sku in ok["skus"] if not sku.startswith(rule[1])]
            label = {"signature": "signature", "sku": "parts", "meter": "meter reading"}[kind]
            rows.append((f"rejects a report without the {label}", "{}", report_ts(lacking), [rule_message(rule)]))
    if len(rules) > 1:
        has_asset = not any(rule[0] == "asset" for rule in rules)
        problems = [rule_message(rule) for rule in rules if violated(rule, has_asset, {})]
        rows.append(("reports every problem in order", "{}" if has_asset else "{ assetId: undefined }", "{}",
                     problems))
    return rows


def bad_report(spec):
    for rule in spec["rules"]:
        if rule[0] in ("signature", "sku", "meter"):
            fields = {key: value for key, value in complete_fields(spec["rules"]).items()}
            if rule[0] == "signature":
                fields.pop("signatureName", None)
            elif rule[0] == "meter":
                fields.pop("meterReading", None)
            else:
                fields["skus"] = [sku for sku in fields.get("skus", []) if not sku.startswith(rule[1])]
            return report_ts(fields, GOOD_RESOLUTION)
    return None


def handler_tuple(spec):
    return (spec["key"], spec["name"], spec["label"], spec["skills"], spec["minutes"], spec["priority"],
            spec["checklist"], spec["kind"] is not None, spec["plans"], spec["labor"],
            estimate_body(spec["est"]), validate_body(spec["rules"]), {}, ())


def test_spec(spec):
    return dict(key=spec["key"], file=spec["key"], name=spec["name"], label=spec["label"], skills=spec["skills"],
                minutes=spec["minutes"], priority=spec["priority"], checklist=spec["checklist"],
                asset=spec["kind"] is not None, plans=spec["plans"], labor=spec["labor"], kind=spec["kind"])


def register_test_data():
    """Adds the new handlers to the tables the shared per-handler test template reads."""
    for spec in SPECS:
        key = spec["key"]
        checks_worktypes.ESTIMATES[key] = estimate_rows(spec)
        checks_worktypes.COMPLETIONS[key] = completion_rows(spec)
        checks_worktypes.GOOD_REPORTS[key] = report_ts(complete_fields(spec["rules"]), GOOD_RESOLUTION)
        bad = bad_report(spec)
        if bad is not None:
            checks_worktypes.BAD_REPORTS[key] = bad


def registry_module(ctx):
    here = "src/worktypes/registry.ts"
    by_key = {spec["key"]: spec for spec in SPECS}
    groups = {
        "src/shared/registry.ts": ["createRegistry", "type Registry"],
        "src/worktypes/types.ts": ["type WorkTypeHandler"],
    }
    for spec in worktypes.HANDLERS:
        groups[f"{HANDLER_DIR}{spec[0]}.ts"] = [spec[1]]
    for spec in SPECS:
        groups[f"{HANDLER_DIR}{spec['key']}.ts"] = [spec["name"]]
    existing = "".join(f"  {spec[1]},\n" for spec in worktypes.HANDLERS if spec[0] != "general")
    sections = ""
    for title, keys in REGISTRY_GROUPS:
        sections += f"  // {title}\n" + "".join(f"  {by_key[key]['name']},\n" for key in keys)
    return render(ctx, '''
«imports»

export const BUILT_IN_WORK_TYPES: readonly WorkTypeHandler[] = [
  generalWork,
«existing»«sections»];

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
''', imports=imports(ctx, here, groups), existing=existing, sections=sections)


NUMBER_WORDS = {20: "twenty", 48: "forty-eight"}


def quoted(items, per_line=3):
    rows = [items[index:index + per_line] for index in range(0, len(items), per_line)]
    return "\n".join("      " + ", ".join(f"'{item}'" for item in row) + "," for row in rows)


WORK_TYPES_DOC = '''
## Work types

The work type of a work order selects its handler: default priority, planned duration,
required skills, checklist, estimate and completion rules. The catalogue covers heating,
ventilation, water treatment, fire safety, security, electrical and building-controls work in
addition to general maintenance. Equipment work types require an asset of the matching kind;
the asset kinds and their statutory or manufacturer service intervals are listed in
`src/assets/model.ts`. Only emergency work opens at emergency priority by default:
`emergency-callout` and `gas-leak-response`.
'''


def generate(ctx):
    if ctx.variant != VARIANT:
        return
    register_test_data()

    # Asset kinds.
    union = "".join(f"\n  | '{kind}'" for kind in NEW_ASSET_KINDS)
    ctx.patch("src/assets/model.ts", "  | 'backflow-preventer';", "  | 'backflow-preventer'" + union + ";")
    intervals = "".join(f"  {_prop(kind)}: {days},\n" for kind, days in NEW_ASSET_KINDS.items())
    ctx.patch("src/assets/model.ts", "  'backflow-preventer': 365,\n", "  'backflow-preventer': 365,\n" + intervals)

    # Handlers and registry.
    for spec in SPECS:
        ctx.add(f"{HANDLER_DIR}{spec['key']}.ts", worktypes.handler_module(ctx, handler_tuple(spec)))
    ctx.replace("src/worktypes/registry.ts", registry_module(ctx))

    # One test file per handler, from the shared template.
    for spec in SPECS:
        ctx.add(f"tests/worktypes/{spec['key']}.test.ts", _handler_test(ctx, spec))

    # Catalogue-wide tests.
    total = len(worktypes.HANDLERS) + len(SPECS)
    old_total = len(worktypes.HANDLERS)
    path = "tests/worktypes/catalog.test.ts"
    ctx.patch(path, "  it('ships twenty handlers', () => {\n    assert.equal(handlers.length, 20);\n    assert.equal(registry.keys().length, 20);",
              f"  it('ships {NUMBER_WORDS[total]} handlers', () => {{\n    assert.equal(handlers.length, {total});\n    assert.equal(registry.keys().length, {total});")
    ordered = [spec[0] for spec in worktypes.HANDLERS] + [key for _, keys in REGISTRY_GROUPS for key in keys]
    by_key = {spec["key"]: spec for spec in SPECS}
    old_required = [item["key"] for item in checks_worktypes.HANDLERS if item["asset"]]
    required = sorted(old_required + [spec["key"] for spec in SPECS if spec["kind"] is not None])
    old_required_text = ("    assert.deepEqual(required, [\n"
                         "      'ac-repair', 'backflow-test', 'boiler-repair', 'boiler-service', 'chiller-maintenance', 'commissioning',\n"
                         "      'elevator-inspection', 'fire-panel-inspection', 'generator-load-test', 'heat-pump-service',\n"
                         "      'solar-inverter-repair', 'water-heater-replace',\n    ]);")
    ctx.patch(path, old_required_text, "    assert.deepEqual(required, [\n" + quoted(required) + "\n    ]);")
    old_excluded = [item["key"] for item in checks_worktypes.HANDLERS if not item["plans"]]
    excluded = sorted(old_excluded + [spec["key"] for spec in SPECS if not spec["plans"]])
    old_excluded_text = ("    assert.deepEqual(excluded, [\n"
                         "      'ac-repair', 'boiler-repair', 'commissioning', 'drain-clearance', 'emergency-callout', 'heat-pump-install',\n"
                         "      'leak-investigation', 'solar-inverter-repair', 'water-heater-replace',\n    ]);")
    ctx.patch(path, old_excluded_text, "    assert.deepEqual(excluded, [\n" + quoted(excluded) + "\n    ]);")
    emergencies = [key for key in ordered if (by_key[key]["priority"] if key in by_key else
                                              next(item["priority"] for item in checks_worktypes.HANDLERS if item["key"] == key)) == "emergency"]
    ctx.patch(path, "    assert.deepEqual(emergencies, ['emergency-callout']);",
              "    assert.deepEqual(emergencies, [" + ", ".join(f"'{key}'" for key in emergencies) + "]);")

    path = "tests/worktypes/registry.test.ts"
    ctx.patch(path, "    assert.equal(registry.keys().length, 21);", f"    assert.equal(registry.keys().length, {total + 1});")
    ctx.patch(path, "workTypesCoveredBy(registry, [...everything]).length, 20);",
              f"workTypesCoveredBy(registry, [...everything]).length, {total});")

    path = "tests/api/work-types.test.ts"
    ctx.patch(path, "    assert.equal(keys.length, 20);", f"    assert.equal(keys.length, {total});")
    ctx.patch(path, "call(world.app, 'GET', '/work-types', actor).items.length, 20, actor);",
              f"call(world.app, 'GET', '/work-types', actor).items.length, {total}, actor);")
    ctx.patch(path, "call(world.app, 'GET', '/work-types', portal.id).items.length, 20);",
              f"call(world.app, 'GET', '/work-types', portal.id).items.length, {total});")
    ctx.patch(path, "    assert.equal(required.length, 12);", f"    assert.equal(required.length, {len(required)});")

    ctx.patch("tests/assets/maintenance.test.ts", "    assert.equal(ASSET_KINDS.length, 10);",
              f"    assert.equal(ASSET_KINDS.length, {10 + len(NEW_ASSET_KINDS)});")
    del old_total

    path = "tests/workorders/priorities.test.ts"
    ctx.patch(path, "it('never defaults to emergency except for the emergency work type', () => {",
              "it('never defaults to emergency except for the emergency work types', () => {")
    ctx.patch(path, "=== 'emergency', key === 'emergency-callout', key);",
              "=== 'emergency', ['emergency-callout', 'gas-leak-response'].includes(key), key);")

    ctx.append("docs/work-orders.md", WORK_TYPES_DOC)


def _prop(kind):
    return f"'{kind}'" if "-" in kind else kind


def _handler_test(ctx, spec):
    from .checks_kit import test_file
    return test_file(ctx, f"tests/worktypes/{spec['key']}.test.ts", checks_worktypes.handler_spec(test_spec(spec)))
