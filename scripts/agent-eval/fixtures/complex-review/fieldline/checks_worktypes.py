"""Tests for the work-type registry, shared helpers and one test file per built-in handler."""

from .checks_kit import WITH_SETUP, Spec, register

WORLD = "tests/support/world.ts"
REGISTRY = "src/worktypes/registry.ts"
TYPES = "src/worktypes/types.ts"


def q(text):
    return "'" + text.replace("\\", "\\\\").replace("'", "\\'") + "'"


def arr(items):
    return "[" + ", ".join(q(item) for item in items) + "]"


# key, module file, exported name, label, skills, minutes, priority, checklist, requiresAsset,
# includedInPlans, labor category, asset kind used end to end
HANDLERS = [
    dict(key="general", file="general", name="generalWork", label="General maintenance", skills=[], minutes=60,
         priority="normal", checklist=["site left clean"], asset=False, plans=True, labor="standard", kind=None),
    dict(key="boiler-service", file="boiler-service", name="boilerService", label="Annual boiler service",
         skills=["gas-safe"], minutes=75, priority="low",
         checklist=["flue gas analysis", "pressure check", "condensate trap cleaned", "safety devices tested"],
         asset=True, plans=True, labor="standard", kind="boiler"),
    dict(key="boiler-repair", file="boiler-repair", name="boilerRepair", label="Boiler breakdown repair",
         skills=["gas-safe"], minutes=120, priority="high",
         checklist=["fault diagnosed", "repair completed", "flue gas analysis"],
         asset=True, plans=False, labor="standard", kind="boiler"),
    dict(key="heat-pump-service", file="heat-pump-service", name="heatPumpService", label="Heat pump service",
         skills=["refrigerant", "electrical"], minutes=90, priority="low",
         checklist=["refrigerant pressures logged", "coil cleaned", "defrost cycle tested", "filters replaced"],
         asset=True, plans=True, labor="specialist", kind="heat-pump"),
    dict(key="heat-pump-install", file="heat-pump-install", name="heatPumpInstall", label="Heat pump installation",
         skills=["refrigerant", "electrical", "commissioning"], minutes=960, priority="normal",
         checklist=["site survey confirmed", "unit mounted", "pipework pressure tested", "commissioned",
                    "customer handover"],
         asset=False, plans=False, labor="specialist", kind=None),
    dict(key="ac-repair", file="ac-repair", name="airConditioningRepair", label="Air conditioning repair",
         skills=["refrigerant"], minutes=90, priority="normal",
         checklist=["leak test", "refrigerant recovered or topped up"],
         asset=True, plans=False, labor="standard", kind="air-handler"),
    dict(key="chiller-maintenance", file="chiller-maintenance", name="chillerMaintenance",
         label="Chiller maintenance", skills=["refrigerant", "hvac-commercial"], minutes=240, priority="normal",
         checklist=["oil sample taken", "condenser cleaned", "controls calibrated", "log sheet completed"],
         asset=True, plans=True, labor="specialist", kind="chiller"),
    dict(key="water-heater-replace", file="water-heater-replace", name="waterHeaterReplacement",
         label="Water heater replacement", skills=["plumbing", "electrical"], minutes=240, priority="normal",
         checklist=["old unit drained", "new unit installed", "temperature relief valve tested"],
         asset=True, plans=False, labor="standard", kind="water-heater"),
    dict(key="generator-load-test", file="generator-load-test", name="generatorLoadTest",
         label="Generator load bank test", skills=["electrical", "generators"], minutes=180, priority="low",
         checklist=["load bank connected", "load steps recorded", "fuel level recorded", "transfer switch tested"],
         asset=True, plans=True, labor="specialist", kind="generator"),
    dict(key="fire-panel-inspection", file="fire-panel-inspection", name="firePanelInspection",
         label="Fire panel inspection", skills=["fire-safety"], minutes=120, priority="normal",
         checklist=["panel log reviewed", "detectors sampled", "sounders tested", "batteries load tested"],
         asset=True, plans=True, labor="standard", kind="fire-panel"),
    dict(key="elevator-inspection", file="elevator-inspection", name="elevatorInspection",
         label="Elevator inspection", skills=["lifts"], minutes=150, priority="normal",
         checklist=["brake test", "door interlocks", "alarm line", "pit inspection"],
         asset=True, plans=True, labor="specialist", kind="elevator"),
    dict(key="backflow-test", file="backflow-test", name="backflowTest", label="Backflow preventer test",
         skills=["plumbing", "backflow"], minutes=45, priority="low",
         checklist=["test kit calibrated", "differential pressure recorded"],
         asset=True, plans=True, labor="standard", kind="backflow-preventer"),
    dict(key="solar-inverter-repair", file="solar-inverter-repair", name="solarInverterRepair",
         label="Solar inverter repair", skills=["electrical", "solar"], minutes=120, priority="normal",
         checklist=["dc isolation confirmed", "fault codes recorded", "output verified"],
         asset=True, plans=False, labor="specialist", kind="solar-inverter"),
    dict(key="leak-investigation", file="leak-investigation", name="leakInvestigation",
         label="Leak investigation", skills=["plumbing"], minutes=90, priority="high",
         checklist=["moisture readings", "source located or ruled out"],
         asset=False, plans=False, labor="standard", kind=None),
    dict(key="drain-clearance", file="drain-clearance", name="drainClearance", label="Drain clearance",
         skills=["drainage"], minutes=60, priority="high", checklist=["flow test"],
         asset=False, plans=False, labor="apprentice", kind=None),
    dict(key="thermostat-install", file="thermostat-install", name="thermostatInstall",
         label="Smart thermostat installation", skills=["electrical"], minutes=60, priority="low",
         checklist=["wiring checked", "app paired", "schedule configured"],
         asset=False, plans=True, labor="apprentice", kind=None),
    dict(key="duct-cleaning", file="duct-cleaning", name="ductCleaning", label="Ductwork cleaning",
         skills=["hvac-commercial"], minutes=240, priority="low",
         checklist=["access panels fitted", "before photos", "after photos"],
         asset=False, plans=True, labor="apprentice", kind=None),
    dict(key="emergency-callout", file="emergency-callout", name="emergencyCallout", label="Emergency callout",
         skills=[], minutes=60, priority="emergency", checklist=["site made safe"],
         asset=False, plans=False, labor="standard", kind=None),
    dict(key="gas-safety-check", file="gas-safety-check", name="gasSafetyCheck",
         label="Landlord gas safety check", skills=["gas-safe"], minutes=45, priority="low",
         checklist=["appliances tested", "flues inspected", "certificate issued"],
         asset=False, plans=True, labor="standard", kind=None),
    dict(key="commissioning", file="commissioning", name="systemCommissioning", label="System commissioning",
         skills=["commissioning"], minutes=180, priority="normal",
         checklist=["design parameters confirmed", "performance recorded", "handover pack"],
         asset=True, plans=False, labor="specialist", kind="boiler"),
]

# Estimate scenarios: (input, minutes, crew, parts, notes) with the last two optional.
ESTIMATES = {
    "general": [
        ("{ hazards: [] }", 60, 1),
        ("{ hazards: [], quantity: 1 }", 60, 1),
        ("{ hazards: [], quantity: 4 }", 120, 1),
        ("{ hazards: [], quantity: 0 }", 60, 1),
        ("{ hazards: ['confined-space'], quantity: 2 }", 80, 2),
        ("{ hazards: ['dog', 'stairs'] }", 60, 1),
    ],
    "boiler-service": [
        ("{ hazards: [] }", 75, 1, "[{ sku: 'GSK-BOILER-KIT', quantity: 1 }]"),
        ("{ hazards: [], ageYears: 15 }", 75, 1, "[{ sku: 'GSK-BOILER-KIT', quantity: 1 }]"),
        ("{ hazards: [], ageYears: 16 }", 95, 1, "[{ sku: 'GSK-BOILER-KIT', quantity: 1 }]",
         "['older appliance: allow for seized fittings']"),
        ("{ hazards: ['restricted-access'] }", 90, 1, "[{ sku: 'GSK-BOILER-KIT', quantity: 1 }]"),
        ("{ hazards: ['restricted-access'], ageYears: 30 }", 110, 1, "[{ sku: 'GSK-BOILER-KIT', quantity: 1 }]",
         "['older appliance: allow for seized fittings']"),
        ("{ hazards: ['working-at-height'] }", 75, 2, "[{ sku: 'GSK-BOILER-KIT', quantity: 1 }]"),
    ],
    "boiler-repair": [
        ("{ hazards: [], assetKind: 'boiler' }", 120, 1, "[]", "['parts depend on the diagnosed fault']"),
        ("{ hazards: [] }", 150, 1, "[]", "['parts depend on the diagnosed fault']"),
        ("{ hazards: [], assetKind: 'heat-pump' }", 150, 1, "[]", "['parts depend on the diagnosed fault']"),
        ("{ hazards: ['live-electrical'], assetKind: 'boiler' }", 120, 2, "[]",
         "['parts depend on the diagnosed fault']"),
    ],
    "heat-pump-service": [
        ("{ hazards: [] }", 75, 1, "[{ sku: 'FLT-HP-STD', quantity: 1 }]"),
        ("{ hazards: [], quantity: 2 }", 90, 1, "[{ sku: 'FLT-HP-STD', quantity: 2 }]"),
        ("{ hazards: [], quantity: 3 }", 90, 1, "[{ sku: 'FLT-HP-STD', quantity: 3 }]"),
        ("{ hazards: [], quantity: 5 }", 120, 1, "[{ sku: 'FLT-HP-STD', quantity: 5 }]"),
        ("{ hazards: [], quantity: 0 }", 75, 1, "[{ sku: 'FLT-HP-STD', quantity: 1 }]"),
        ("{ hazards: ['confined-space'] }", 75, 2, "[{ sku: 'FLT-HP-STD', quantity: 1 }]"),
    ],
    "heat-pump-install": [
        ("{ hazards: [] }", 960, 2, "[{ sku: 'HP-UNIT-8KW', quantity: 1 }, { sku: 'PIPE-CU-15', quantity: 5 }]"),
        ("{ hazards: [], quantity: 3 }", 960, 2, "[{ sku: 'HP-UNIT-8KW', quantity: 1 }, { sku: 'PIPE-CU-15', quantity: 3 }]"),
        ("{ hazards: [], quantity: 10 }", 1020, 2, "[{ sku: 'HP-UNIT-8KW', quantity: 1 }, { sku: 'PIPE-CU-15', quantity: 10 }]"),
        ("{ hazards: [], quantity: 7.5 }", 990, 2, "[{ sku: 'HP-UNIT-8KW', quantity: 1 }, { sku: 'PIPE-CU-15', quantity: 8 }]"),
        ("{ hazards: [], quantity: 15 }", 1080, 2, "[{ sku: 'HP-UNIT-8KW', quantity: 1 }, { sku: 'PIPE-CU-15', quantity: 15 }]"),
        ("{ hazards: [], quantity: 16 }", 1092, 2, "[{ sku: 'HP-UNIT-8KW', quantity: 1 }, { sku: 'PIPE-CU-15', quantity: 16 }]",
         "['long pipe run: confirm refrigerant top-up']"),
    ],
    "ac-repair": [
        ("{ hazards: [] }", 90, 1),
        ("{ hazards: ['roof-unit'] }", 120, 2),
        ("{ hazards: ['confined-space'] }", 90, 2),
        ("{ hazards: ['roof-unit', 'confined-space'] }", 120, 2),
        ("{ hazards: ['dog'], assetKind: 'air-handler' }", 90, 1),
    ],
    "chiller-maintenance": [
        ("{ hazards: [] }", 240, 2, "[{ sku: 'OIL-SAMPLE-KIT', quantity: 1 }]"),
        ("{ hazards: [], quantity: 2 }", 270, 2, "[{ sku: 'OIL-SAMPLE-KIT', quantity: 2 }]"),
        ("{ hazards: [], quantity: 3 }", 330, 2, "[{ sku: 'OIL-SAMPLE-KIT', quantity: 3 }]"),
        ("{ hazards: [], quantity: 4 }", 360, 2, "[{ sku: 'OIL-SAMPLE-KIT', quantity: 4 }]"),
        ("{ hazards: [], quantity: 0 }", 240, 2, "[{ sku: 'OIL-SAMPLE-KIT', quantity: 1 }]"),
    ],
    "water-heater-replace": [
        ("{ hazards: [] }", 240, 1, "[{ sku: 'WH-200L', quantity: 1 }]"),
        ("{ hazards: [], quantity: 100 }", 240, 1, "[{ sku: 'WH-120L', quantity: 1 }]"),
        ("{ hazards: [], quantity: 120 }", 240, 1, "[{ sku: 'WH-120L', quantity: 1 }]"),
        ("{ hazards: [], quantity: 121 }", 240, 1, "[{ sku: 'WH-200L', quantity: 1 }]"),
        ("{ hazards: [], quantity: 200 }", 240, 1, "[{ sku: 'WH-200L', quantity: 1 }]"),
        ("{ hazards: [], quantity: 201 }", 300, 2, "[{ sku: 'WH-300L', quantity: 1 }]"),
    ],
    "generator-load-test": [
        ("{ hazards: [] }", 180, 2),
        ("{ hazards: [], quantity: 1 }", 120, 2),
        ("{ hazards: [], quantity: 4 }", 300, 2),
        ("{ hazards: [], quantity: 0 }", 120, 2),
        ("{ hazards: ['confined-space'], quantity: 3 }", 240, 2),
    ],
    "fire-panel-inspection": [
        ("{ hazards: [] }", 90, 1, "[]", "['sample 1 of 4 zones']"),
        ("{ hazards: [], quantity: 5 }", 120, 1, "[]", "['sample 2 of 5 zones']"),
        ("{ hazards: [], quantity: 12 }", 150, 1, "[]", "['sample 3 of 12 zones']"),
        ("{ hazards: [], quantity: 1 }", 90, 1, "[]", "['sample 1 of 1 zones']"),
        ("{ hazards: [], quantity: 0 }", 90, 1, "[]", "['sample 1 of 1 zones']"),
        ("{ hazards: [], quantity: 40 }", 360, 1, "[]", "['sample 10 of 40 zones']"),
    ],
    "elevator-inspection": [
        ("{ hazards: [] }", 150, 2),
        ("{ hazards: [], quantity: 1 }", 120, 2),
        ("{ hazards: [], quantity: 2 }", 120, 2),
        ("{ hazards: [], quantity: 10 }", 240, 2),
        ("{ hazards: ['confined-space'], quantity: 6 }", 180, 2),
    ],
    "backflow-test": [
        ("{ hazards: [] }", 45, 1),
        ("{ hazards: [], quantity: 3 }", 75, 1),
        ("{ hazards: [], quantity: 0 }", 45, 1),
        ("{ hazards: ['confined-space'], quantity: 2 }", 60, 1),
        ("{ hazards: [], quantity: 8 }", 150, 1),
    ],
    "solar-inverter-repair": [
        ("{ hazards: [] }", 120, 1),
        ("{ hazards: [], quantity: 8 }", 120, 2),
        ("{ hazards: [], quantity: 20 }", 120, 2),
        ("{ hazards: [], quantity: 21 }", 150, 2),
        ("{ hazards: ['live-electrical'] }", 120, 2),
        ("{ hazards: ['dog'], quantity: 0 }", 120, 1),
    ],
    "leak-investigation": [
        ("{ hazards: [] }", 90, 1),
        ("{ hazards: [], quantity: 2 }", 105, 1),
        ("{ hazards: [], quantity: 3 }", 120, 1),
        ("{ hazards: [], quantity: 4 }", 150, 1),
        ("{ hazards: [], quantity: 5 }", 165, 1, "[]", "['consider thermal imaging']"),
        ("{ hazards: ['confined-space'], quantity: 1 }", 90, 1),
    ],
    "drain-clearance": [
        ("{ hazards: [] }", 60, 1),
        ("{ hazards: [], quantity: 30 }", 60, 1),
        ("{ hazards: [], quantity: 31 }", 120, 2),
        ("{ hazards: [], quantity: 0 }", 60, 1),
        ("{ hazards: ['working-at-height'], quantity: 5 }", 60, 1),
    ],
    "thermostat-install": [
        ("{ hazards: [] }", 60, 1, "[{ sku: 'TSTAT-SMART', quantity: 1 }]"),
        ("{ hazards: [], quantity: 3 }", 90, 1, "[{ sku: 'TSTAT-SMART', quantity: 3 }]"),
        ("{ hazards: [], quantity: 0 }", 60, 1, "[{ sku: 'TSTAT-SMART', quantity: 1 }]"),
        ("{ hazards: [], quantity: 8 }", 165, 1, "[{ sku: 'TSTAT-SMART', quantity: 8 }]"),
    ],
    "duct-cleaning": [
        ("{ hazards: [] }", 180, 2),
        ("{ hazards: [], quantity: 75 }", 300, 2),
        ("{ hazards: [], quantity: 10 }", 60, 2),
        ("{ hazards: [], quantity: 5 }", 60, 2),
        ("{ hazards: [], quantity: 100 }", 420, 2),
        ("{ hazards: [], quantity: 150 }", 600, 2),
    ],
    "emergency-callout": [
        ("{ hazards: [] }", 60, 1, "[]", "['make safe first; repairs follow up']"),
        ("{ hazards: [], quantity: 9 }", 60, 1, "[]", "['make safe first; repairs follow up']"),
        ("{ hazards: ['confined-space'] }", 60, 2, "[]", "['make safe first; repairs follow up']"),
        ("{ hazards: ['live-electrical', 'dog'] }", 60, 2, "[]", "['make safe first; repairs follow up']"),
    ],
    "gas-safety-check": [
        ("{ hazards: [] }", 45, 1),
        ("{ hazards: [], quantity: 1 }", 30, 1),
        ("{ hazards: [], quantity: 5 }", 90, 1),
        ("{ hazards: [], quantity: 0 }", 30, 1),
        ("{ hazards: ['confined-space'], quantity: 3 }", 60, 1),
    ],
    "commissioning": [
        ("{ hazards: [] }", 180, 1),
        ("{ hazards: [], quantity: 3 }", 300, 1),
        ("{ hazards: [], quantity: 0 }", 180, 1),
        ("{ hazards: ['confined-space'], quantity: 2 }", 240, 1),
    ],
}

# Completion scenarios: (label, job overrides, report overrides, expected problems)
COMPLETIONS = {
    "general": [
        ("accepts a resolution of ten characters", "{}", "{ resolution: '0123456789' }", []),
        ("rejects nine characters", "{}", "{ resolution: '012345678' }", ["describe the work performed in at least 10 characters"]),
        ("rejects an empty resolution", "{}", "{ resolution: '' }", ["describe the work performed in at least 10 characters"]),
        ("does not ask for a signature", "{}", "{ resolution: 'Tightened the loose fixings' }", []),
    ],
    "boiler-service": [
        ("accepts a signed service of the boiler asset", "{}", "{ signatureName: 'Avery Collins' }", []),
        ("requires the asset", "{ assetId: undefined }", "{ signatureName: 'Avery Collins' }",
         ["boiler services must reference the boiler asset"]),
        ("requires the customer signature", "{}", "{}", ["the customer must sign the gas safety record"]),
        ("reports both problems in order", "{ assetId: undefined }", "{}",
         ["boiler services must reference the boiler asset", "the customer must sign the gas safety record"]),
    ],
    "boiler-repair": [
        ("accepts a repair with parts", "{}", "{ partsUsed: [{ sku: 'PCB-VAI-832', quantity: 1 }] }", []),
        ("accepts a flagged follow-up", "{}", "{ followUpRequired: true }", []),
        ("accepts a reset described in words", "{}", "{ resolution: 'Reset the lockout and cleared the fault' }", []),
        ("accepts an adjustment in any case", "{}", "{ resolution: 'ADJUSTED the gas valve pressure' }", []),
        ("rejects an unexplained repair", "{}", "{ resolution: 'Fixed the boiler' }",
         ["a repair without parts must explain the adjustment that fixed the fault"]),
    ],
    "heat-pump-service": [
        ("accepts recorded filters", "{}", "{ partsUsed: [{ sku: 'FLT-HP-STD', quantity: 2 }] }", []),
        ("counts any filter sku", "{}", "{ partsUsed: [{ sku: 'FLT-ALT-01', quantity: 1 }] }", []),
        ("rejects a service without filters", "{}", "{ partsUsed: [{ sku: 'SEAL-GEN', quantity: 1 }] }",
         ["a heat pump service must record the replaced filters"]),
        ("rejects filters with zero quantity", "{}", "{ partsUsed: [{ sku: 'FLT-HP-STD', quantity: 0 }] }",
         ["a heat pump service must record the replaced filters"]),
    ],
    "heat-pump-install": [
        ("accepts one unit and a signature", "{}", "{ partsUsed: [{ sku: 'HP-UNIT-8KW', quantity: 1 }], signatureName: 'Avery Collins' }", []),
        ("requires exactly one unit", "{}", "{ partsUsed: [{ sku: 'HP-UNIT-8KW', quantity: 2 }], signatureName: 'Avery Collins' }",
         ["exactly one heat pump unit must be recorded"]),
        ("counts units across lines", "{}",
         "{ partsUsed: [{ sku: 'HP-UNIT-8KW', quantity: 1 }, { sku: 'HP-UNIT-12KW', quantity: 1 }], signatureName: 'Avery Collins' }",
         ["exactly one heat pump unit must be recorded"]),
        ("requires the handover signature", "{}", "{ partsUsed: [{ sku: 'HP-UNIT-8KW', quantity: 1 }] }",
         ["installations require a customer handover signature"]),
        ("reports both problems in order", "{}", "{}",
         ["exactly one heat pump unit must be recorded", "installations require a customer handover signature"]),
    ],
    "ac-repair": [
        ("accepts any report", "{}", "{}", []),
        ("accepts a flagged follow-up", "{}", "{ followUpRequired: true }", []),
    ],
    "chiller-maintenance": [
        ("accepts recorded oil sample kits", "{}", "{ partsUsed: [{ sku: 'OIL-SAMPLE-KIT', quantity: 1 }] }", []),
        ("rejects a missing oil sample record", "{}", "{}", ["chiller maintenance must record the oil sample kits used"]),
        ("ignores other parts", "{}", "{ partsUsed: [{ sku: 'GSK-BOILER-KIT', quantity: 1 }] }",
         ["chiller maintenance must record the oil sample kits used"]),
    ],
    "water-heater-replace": [
        ("accepts exactly one heater", "{}", "{ partsUsed: [{ sku: 'WH-200L', quantity: 1 }] }", []),
        ("rejects no heater", "{}", "{}", ["record exactly one replacement water heater"]),
        ("rejects two heaters on one line", "{}", "{ partsUsed: [{ sku: 'WH-200L', quantity: 2 }] }",
         ["record exactly one replacement water heater"]),
        ("rejects two heaters on two lines", "{}",
         "{ partsUsed: [{ sku: 'WH-120L', quantity: 1 }, { sku: 'WH-300L', quantity: 1 }] }",
         ["record exactly one replacement water heater"]),
    ],
    "generator-load-test": [
        ("accepts a meter reading", "{}", "{ meterReading: 1450 }", []),
        ("accepts a zero meter reading", "{}", "{ meterReading: 0 }", []),
        ("requires the reading", "{}", "{}", ["record the generator run-hour meter after the test"]),
    ],
    "fire-panel-inspection": [
        ("accepts a signed certificate", "{}", "{ signatureName: 'Dana Whitfield' }", []),
        ("requires a signature", "{}", "{}", ["the responsible person must sign the inspection certificate"]),
        ("ignores empty signatures", "{}", "{ signatureName: undefined }", ["the responsible person must sign the inspection certificate"]),
    ],
    "elevator-inspection": [
        ("accepts a countersigned inspection", "{}", "{ signatureName: 'Dana Whitfield' }", []),
        ("accepts high priority work", "{ priority: 'high' }", "{ signatureName: 'Dana Whitfield' }", []),
        ("rejects low priority work", "{ priority: 'low' }", "{ signatureName: 'Dana Whitfield' }",
         ["elevator inspections cannot be completed as low priority work"]),
        ("requires the countersignature", "{}", "{}", ["the inspection must be countersigned"]),
        ("reports priority before signature", "{ priority: 'low' }", "{}",
         ["elevator inspections cannot be completed as low priority work", "the inspection must be countersigned"]),
    ],
    "backflow-test": [
        ("accepts psi readings", "{}", "{ resolution: 'Differential held at 4.2 psi' }", []),
        ("accepts kPa readings in any case", "{}", "{ resolution: 'Held 29 KPA over the test' }", []),
        ("accepts a space before the unit", "{}", "{ resolution: 'Reading 3 psi' }", []),
        ("rejects readings in other units", "{}", "{ resolution: 'Held 0.3 bar over the test' }",
         ["record the measured differential pressure with its unit"]),
        ("rejects a unit without a number", "{}", "{ resolution: 'Checked pressure in psi' }",
         ["record the measured differential pressure with its unit"]),
    ],
    "solar-inverter-repair": [
        ("accepts any report", "{}", "{}", []),
        ("accepts a meter reading", "{}", "{ meterReading: 500 }", []),
    ],
    "leak-investigation": [
        ("accepts any report", "{}", "{}", []),
        ("accepts a follow-up request", "{}", "{ followUpRequired: true }", []),
    ],
    "drain-clearance": [
        ("accepts any report", "{}", "{}", []),
        ("accepts parts being recorded", "{}", "{ partsUsed: [{ sku: 'ROD-DRAIN-10', quantity: 1 }] }", []),
    ],
    "thermostat-install": [
        ("accepts recorded thermostats", "{}", "{ partsUsed: [{ sku: 'TSTAT-SMART', quantity: 2 }] }", []),
        ("counts any thermostat sku", "{}", "{ partsUsed: [{ sku: 'TSTAT-BASIC', quantity: 1 }] }", []),
        ("rejects missing thermostats", "{}", "{}", ["record the thermostats fitted"]),
        ("rejects zero quantities", "{}", "{ partsUsed: [{ sku: 'TSTAT-SMART', quantity: 0 }] }", ["record the thermostats fitted"]),
    ],
    "duct-cleaning": [
        ("accepts any report", "{}", "{}", []),
        ("accepts a follow-up request", "{}", "{ followUpRequired: true }", []),
    ],
    "emergency-callout": [
        ("accepts a flagged follow-up", "{}", "{ resolution: 'Isolated the supply', followUpRequired: true }", []),
        ("accepts a resolved incident in any case", "{}", "{ resolution: 'Burst pipe RESOLVED on site' }", []),
        ("rejects an open incident", "{}", "{ resolution: 'Isolated the supply' }",
         ["emergency work must either be resolved or flagged for follow-up"]),
    ],
    "gas-safety-check": [
        ("accepts a named recipient", "{}", "{ signatureName: 'Avery Collins' }", []),
        ("requires the recipient", "{}", "{}", ["the certificate must name the person who received it"]),
    ],
    "commissioning": [
        ("accepts a closed-out commissioning", "{}", "{}", []),
        ("rejects open follow-up items", "{}", "{ followUpRequired: true }", ["commissioning cannot finish with open follow-up items"]),
    ],
}

# Reports accepted by the handler, used by the end-to-end flow.
GOOD_REPORTS = {
    "general": "{ resolution: 'Replaced worn hinge and adjusted closer' }",
    "boiler-service": "{ resolution: 'Service completed, combustion within limits', signatureName: 'Avery Collins' }",
    "boiler-repair": "{ resolution: 'Adjusted the gas valve and cleared the fault' }",
    "heat-pump-service": "{ resolution: 'Serviced and filters changed', partsUsed: [{ sku: 'FLT-HP-STD', quantity: 1 }] }",
    "heat-pump-install": "{ resolution: 'Unit mounted and commissioned', partsUsed: [{ sku: 'HP-UNIT-8KW', quantity: 1 }], signatureName: 'Avery Collins' }",
    "ac-repair": "{ resolution: 'Recharged the circuit after repairing a flare joint' }",
    "chiller-maintenance": "{ resolution: 'Quarterly maintenance complete', partsUsed: [{ sku: 'OIL-SAMPLE-KIT', quantity: 1 }] }",
    "water-heater-replace": "{ resolution: 'Old cylinder removed, new unit running', partsUsed: [{ sku: 'WH-200L', quantity: 1 }] }",
    "generator-load-test": "{ resolution: 'Passed all load steps', meterReading: 1450 }",
    "fire-panel-inspection": "{ resolution: 'All zones tested and certificate signed', signatureName: 'Dana Whitfield' }",
    "elevator-inspection": "{ resolution: 'Inspection passed with no findings', signatureName: 'Dana Whitfield' }",
    "backflow-test": "{ resolution: 'Differential held at 4.2 psi' }",
    "solar-inverter-repair": "{ resolution: 'Replaced the DC isolator and verified output' }",
    "leak-investigation": "{ resolution: 'Located a weeping joint behind the basin' }",
    "drain-clearance": "{ resolution: 'Cleared the blockage and flushed the line' }",
    "thermostat-install": "{ resolution: 'Thermostat fitted and paired', partsUsed: [{ sku: 'TSTAT-SMART', quantity: 1 }] }",
    "duct-cleaning": "{ resolution: 'All ducts cleaned and photographed' }",
    "emergency-callout": "{ resolution: 'Leak isolated and resolved', followUpRequired: false }",
    "gas-safety-check": "{ resolution: 'All appliances passed', signatureName: 'Avery Collins' }",
    "commissioning": "{ resolution: 'Design parameters confirmed on site' }",
}

# A report the handler rejects, with the code the API answers.
BAD_REPORTS = {
    "general": "{ resolution: 'Done' }",
    "boiler-service": "{ resolution: 'Service completed, combustion within limits' }",
    "boiler-repair": "{ resolution: 'Fixed the boiler quickly' }",
    "heat-pump-service": "{ resolution: 'Serviced the heat pump' }",
    "heat-pump-install": "{ resolution: 'Unit mounted and commissioned' }",
    "chiller-maintenance": "{ resolution: 'Quarterly maintenance complete' }",
    "water-heater-replace": "{ resolution: 'Old cylinder removed, new unit running' }",
    "generator-load-test": "{ resolution: 'Passed all load steps' }",
    "fire-panel-inspection": "{ resolution: 'All zones tested and certificate signed' }",
    "elevator-inspection": "{ resolution: 'Inspection passed with no findings' }",
    "backflow-test": "{ resolution: 'Differential looked fine at the gauge' }",
    "thermostat-install": "{ resolution: 'Thermostat fitted and paired' }",
    "emergency-callout": "{ resolution: 'Leak isolated for now' }",
    "gas-safety-check": "{ resolution: 'All appliances passed' }",
    "commissioning": "{ resolution: 'Design parameters confirmed on site', followUpRequired: true }",
}

ESCALATED = {"low": "normal", "normal": "high", "high": "high", "emergency": "emergency"}

HANDLER_TEMPLATE = r'''
const handler = «NAME»;

const job = (overrides: Partial<«Job»> = {}): «Job» => ({
  id: 'WO-000001',
  number: 'FL-2026-00001',
  customerId: 'CUS-000001',
  siteId: 'SITE-000001',
  assetId: 'AST-000001',
  workType: '«KEY»',
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

const report = (overrides: Partial<CompletionReport> = {}): CompletionReport => ({
  completedAt: '2026-03-02T11:00:00Z',
  resolution: 'Completed the scheduled work in full',
  partsUsed: [],
  followUpRequired: false,
  ...overrides,
});

describe('«KEY» definition', () => {
  it('is registered under its key', () => {
    const registry = createWorkTypeRegistry();
    assert.equal(registry.lookup('«KEY»'), handler);
    assert.equal(registry.resolve('«UPPER»'), handler);
  });

  it('carries its label and priority', () => {
    assert.equal(handler.key, '«KEY»');
    assert.equal(handler.label, '«LABEL»');
    assert.equal(handler.defaultPriority, '«PRIORITY»');
  });

  it('declares its required skills', () => {
    assert.deepEqual(handler.skills, «SKILLS»);
  });

  it('plans a default visit of «MINUTES» minutes', () => {
    assert.equal(handler.defaultDurationMinutes, «MINUTES»);
  });

  it('lists its checklist in order', () => {
    assert.deepEqual(handler.checklist, «CHECKLIST»);
  });

  it('«ASSETWORD» an asset', () => {
    assert.equal(handler.requiresAsset, «ASSET»);
  });

  it('«PLANWORD» in service plans', () => {
    assert.equal(handler.includedInPlans, «PLANS»);
  });

  it('bills labor as «LABOR» work', () => {
    assert.equal(handler.laborCategory, '«LABOR»');
  });
});

describe('«KEY» estimates', () => {
«ESTIMATE_TESTS»
  it('never estimates fewer minutes than a quarter of an hour', () => {
    for (const quantity of [undefined, 0, 1, 2, 5, 12]) {
      assert.ok(handler.estimate({ hazards: [], quantity }).minutes >= 15);
    }
  });

  it('is deterministic for equal input', () => {
    const input = { hazards: ['dog'], quantity: 3, ageYears: 7 };
    assert.deepEqual(handler.estimate(input), handler.estimate({ ...input }));
  });

  it('reports at least one person for the crew', () => {
    assert.ok(handler.estimate({ hazards: [] }).crew >= 1);
  });
});

describe('«KEY» completion rules', () => {
«COMPLETION_TESTS»
});

describe('«KEY» in service', () => {
  let world: World;

  beforeEach(() => {
    world = createWorld();
  });

  const open = (extra: Record<string, unknown> = {}) => {
    const { app } = world;
    const asset = «ASSET_SEED»;
    return openWorkOrder(app, world.customer.id, world.site.id, '«KEY»', { ...(asset ? { assetId: asset.id } : {}), ...extra });
  };

  it('is listed by the work-type endpoint', () => {
    const entry = call(world.app, 'GET', '/work-types', 'dispatch').items.find((item: any) => item.key === '«KEY»');
    assert.deepEqual(entry, {
      key: '«KEY»',
      label: '«LABEL»',
      skills: «SKILLS»,
      defaultDurationMinutes: «MINUTES»,
      defaultPriority: '«PRIORITY»',
      requiresAsset: «ASSET»,
    });
  });

  it('opens work orders with the handler priority and checklist', () => {
    const created = open();
    assert.equal(created.workType, '«KEY»');
    assert.equal(created.workTypeLabel, '«LABEL»');
    assert.equal(created.priority, '«PRIORITY»');
    assert.deepEqual(Object.keys(created.checklist), «CHECKLIST»);
    assert.ok(Object.values(created.checklist).every((done) => done === false));
  });

  it('accepts the key in any letter case', () => {
    const { app } = world;
    const asset = «ASSET_SEED»;
    const created = openWorkOrder(app, world.customer.id, world.site.id, '«UPPER»', asset ? { assetId: asset.id } : {});
    assert.equal(created.workType, '«KEY»');
  });

  it('applies the plan priority rule under a premium contract', () => {
    const { app } = world;
    seedContract(app, world.customer.id, [world.site.id], 'premium', ['«KEY»']);
    const created = open();
    assert.equal(created.priority, '«ESCALATED»');
    assert.equal(created.contractId !== undefined, true);
  });

  it('tags included visits only for work types included in plans', () => {
    const { app } = world;
    seedContract(app, world.customer.id, [world.site.id], 'basic', ['«KEY»']);
    const created = open();
    assert.equal(call(app, 'GET', `${JOBS}/${created.id}`, 'dispatch').tags.includes('included-visit'), «PLANS»);
  });

  it('runs from request to completion with a qualified technician', () => {
    const { app } = world;
    const technician = seedTechnician(app, { skills: «SKILLS» });
    const created = open();
    call(app, 'POST', `${JOBS}/${created.id}/assign`, 'dispatch', { technicianId: technician.id });
    call(app, 'POST', `${JOBS}/${created.id}/start`, technician.id);
    completeChecklist(app, created.id, technician.id);
«BAD_FLOW»    const done = call(app, 'POST', `${JOBS}/${created.id}/complete`, technician.id, «GOOD»);
    assert.equal(done.status, 'completed');
    assert.equal(done.completion.completedAt, '2026-03-02T08:00:00Z');
  });

  it('turns away technicians without the required skills', () => {
    const { app } = world;
    «UNQUALIFIED»
  });
});
'''

BAD_FLOW = r'''    const rejected = expectStatus(app, 422, 'POST', `${JOBS}/${created.id}/complete`, technician.id, «BAD»);
    assert.equal(rejected.error.code, 'completion-rejected');
    assert.ok(rejected.error.details.problems.length >= 1);
    assert.equal(call(app, 'GET', `${JOBS}/${created.id}`, 'dispatch').status, 'in_progress');
'''

UNQUALIFIED_NONE = "const created = open();\n    const technician = seedTechnician(app, { skills: [] });\n    assert.equal(call(app, 'POST', `${JOBS}/${created.id}/assign`, 'dispatch', { technicianId: technician.id }).status, 'assigned');"
UNQUALIFIED_SOME = (
    "const created = open();\n"
    "    const technician = seedTechnician(app, { skills: [] });\n"
    "    const body = expectStatus(app, 409, 'POST', `${JOBS}/${created.id}/assign`, 'dispatch', { technicianId: technician.id });\n"
    "    assert.equal(body.error.code, 'technician-not-qualified');\n"
    "    assert.deepEqual(body.error.details.missing, «SKILLS»);"
)


def estimate_tests(spec):
    lines = []
    for index, row in enumerate(ESTIMATES[spec["key"]], start=1):
        input_ts, minutes, crew = row[0], row[1], row[2]
        parts = row[3] if len(row) > 3 else "[]"
        notes = row[4] if len(row) > 4 else "[]"
        lines.append(f"""  it('estimates scenario {index}', () => {{
    assert.deepEqual(handler.estimate({input_ts}), {{ minutes: {minutes}, crew: {crew}, parts: {parts}, notes: {notes} }});
  }});
""")
    return "\n".join(lines)


def completion_tests(spec):
    lines = []
    for label, job_ts, report_ts, problems in COMPLETIONS[spec["key"]]:
        lines.append(f"""  it({q(label)}, () => {{
    assert.deepEqual(handler.validateCompletion(job({job_ts}), report({report_ts})), {arr(problems)});
  }});
""")
    return "\n".join(lines)


def handler_body(spec):
    key = spec["key"]
    bad = BAD_REPORTS.get(key)
    values = {
        "NAME": spec["name"],
        "KEY": key,
        "UPPER": key.upper(),
        "LABEL": spec["label"],
        "PRIORITY": spec["priority"],
        "SKILLS": arr(spec["skills"]),
        "MINUTES": str(spec["minutes"]),
        "CHECKLIST": arr(spec["checklist"]),
        "ASSETWORD": "requires" if spec["asset"] else "does not require",
        "ASSET": "true" if spec["asset"] else "false",
        "PLANWORD": "is included" if spec["plans"] else "is not included",
        "PLANS": "true" if spec["plans"] else "false",
        "LABOR": spec["labor"],
        "ESCALATED": ESCALATED[spec["priority"]],
        "ASSET_SEED": f"seedAsset(app, world.site.id, {q(spec['kind'])})" if spec["kind"] else "(undefined as { id: string } | undefined)",
        "GOOD": GOOD_REPORTS[key],
        "BAD_FLOW": BAD_FLOW.replace("«BAD»", bad) if bad else "",
        "UNQUALIFIED": UNQUALIFIED_SOME if spec["skills"] else UNQUALIFIED_NONE,
        "ESTIMATE_TESTS": estimate_tests(spec),
        "COMPLETION_TESTS": completion_tests(spec),
    }
    text = HANDLER_TEMPLATE
    for _ in range(2):
        for name, value in values.items():
            text = text.replace(f"«{name}»", value)
    return text


def handler_spec(spec):
    groups = {
        WORLD: ["call", "completeChecklist", "createWorld", "expectStatus", "JOBS", "openWorkOrder", "seedAsset",
                "seedContract", "seedTechnician", "type World"],
        REGISTRY: ["createWorkTypeRegistry"],
        f"src/worktypes/handlers/{spec['file']}.ts": [spec["name"]],
    }
    return Spec(groups, handler_body(spec), ("type Job", "type CompletionReport"), node=WITH_SETUP)


CATALOG = r'''
const registry = createWorkTypeRegistry();
const handlers = BUILT_IN_WORK_TYPES;

describe('built-in work types', () => {
  it('ships twenty handlers', () => {
    assert.equal(handlers.length, 20);
    assert.equal(registry.keys().length, 20);
  });

  it('uses unique lower-case kebab-case keys', () => {
    const keys = handlers.map((handler) => handler.key);
    assert.equal(new Set(keys).size, keys.length);
    for (const key of keys) assert.match(key, /^[a-z]+(-[a-z]+)*$/, key);
  });

  it('uses unique labels', () => {
    const labels = handlers.map((handler) => handler.label);
    assert.equal(new Set(labels).size, labels.length);
  });

  it('starts with general maintenance, the fallback handler', () => {
    assert.equal(handlers[0].key, 'general');
    assert.deepEqual(handlers[0].skills, []);
  });

  it('plans visits in whole quarter hours', () => {
    for (const handler of handlers) assert.equal(handler.defaultDurationMinutes % 15, 0, handler.key);
  });

  it('gives every handler a non-empty, duplicate-free checklist', () => {
    for (const handler of handlers) {
      assert.ok(handler.checklist.length >= 1, handler.key);
      assert.equal(new Set(handler.checklist).size, handler.checklist.length, handler.key);
    }
  });

  it('lists skills in lower case without duplicates', () => {
    for (const handler of handlers) {
      assert.equal(new Set(handler.skills).size, handler.skills.length, handler.key);
      for (const skill of handler.skills) assert.equal(skill, skill.toLowerCase(), handler.key);
    }
  });

  it('only uses the three labor categories', () => {
    const categories = new Set(handlers.map((handler) => handler.laborCategory));
    assert.deepEqual([...categories].sort(), ['apprentice', 'specialist', 'standard']);
  });

  it('only uses defined priorities and never defaults to a surprise emergency', () => {
    const emergencies = handlers.filter((handler) => handler.defaultPriority === 'emergency').map((handler) => handler.key);
    assert.deepEqual(emergencies, ['emergency-callout']);
    for (const handler of handlers) assert.ok(['low', 'normal', 'high', 'emergency'].includes(handler.defaultPriority), handler.key);
  });

  it('requires assets exactly for the equipment work types', () => {
    const required = handlers.filter((handler) => handler.requiresAsset).map((handler) => handler.key).sort();
    assert.deepEqual(required, [
      'ac-repair', 'backflow-test', 'boiler-repair', 'boiler-service', 'chiller-maintenance', 'commissioning',
      'elevator-inspection', 'fire-panel-inspection', 'generator-load-test', 'heat-pump-service',
      'solar-inverter-repair', 'water-heater-replace',
    ]);
  });

  it('keeps emergency and repair work out of service plans', () => {
    const excluded = handlers.filter((handler) => !handler.includedInPlans).map((handler) => handler.key).sort();
    assert.deepEqual(excluded, [
      'ac-repair', 'boiler-repair', 'commissioning', 'drain-clearance', 'emergency-callout', 'heat-pump-install',
      'leak-investigation', 'solar-inverter-repair', 'water-heater-replace',
    ]);
  });

  it('estimates a positive duration and crew for empty input', () => {
    for (const handler of handlers) {
      const estimate = handler.estimate({ hazards: [] });
      assert.ok(estimate.minutes > 0, handler.key);
      assert.ok(estimate.crew >= 1, handler.key);
      assert.ok(Array.isArray(estimate.parts) && Array.isArray(estimate.notes), handler.key);
    }
  });

  it('adds a second person for confined-space work in most estimates', () => {
    const single = handlers
      .filter((handler) => handler.estimate({ hazards: ['confined-space'] }).crew < 2)
      .map((handler) => handler.key)
      .sort();
    assert.deepEqual(single, [
      'backflow-test', 'commissioning', 'drain-clearance', 'fire-panel-inspection', 'gas-safety-check',
      'leak-investigation', 'thermostat-install', 'water-heater-replace',
    ]);
  });
});
'''

REGISTRY_BODY = r'''
describe('work type registry', () => {
  it('normalises keys on lookup', () => {
    const registry = createWorkTypeRegistry();
    assert.equal(registry.lookup('  Boiler-Service ')?.key, 'boiler-service');
    assert.equal(registry.has('LEAK-INVESTIGATION'), true);
  });

  it('reports unknown keys on lookup and falls back on resolve', () => {
    const registry = createWorkTypeRegistry();
    assert.equal(registry.lookup('teleportation'), undefined);
    assert.equal(registry.has('teleportation'), false);
    assert.equal(registry.resolve('teleportation').key, 'general');
    assert.equal(registry.resolve('').key, 'general');
  });

  it('lists keys alphabetically', () => {
    const keys = createWorkTypeRegistry().keys();
    assert.deepEqual(keys, [...keys].sort());
    assert.equal(keys[0], 'ac-repair');
    assert.equal(keys.at(-1), 'water-heater-replace');
  });

  it('returns entries in key order', () => {
    const entries = createWorkTypeRegistry().entries();
    assert.deepEqual(entries.map(([key]) => key), createWorkTypeRegistry().keys());
    for (const [key, handler] of entries) assert.equal(handler.key, key);
  });

  it('accepts extra handlers', () => {
    const custom: WorkTypeHandler = {
      ...BUILT_IN_WORK_TYPES[0],
      key: 'window-cleaning',
      label: 'Window cleaning',
      defaultDurationMinutes: 30,
    };
    const registry = createWorkTypeRegistry([custom]);
    assert.equal(registry.keys().length, 21);
    assert.equal(registry.resolve('Window-Cleaning').label, 'Window cleaning');
  });

  it('rejects an extra handler with a duplicate key', () => {
    assert.throws(() => createWorkTypeRegistry([{ ...BUILT_IN_WORK_TYPES[0], key: 'GENERAL' }]), /duplicate registration for general/);
  });

  it('rejects an extra handler with a blank key', () => {
    assert.throws(() => createWorkTypeRegistry([{ ...BUILT_IN_WORK_TYPES[0], key: '   ' }]), /empty registration key/);
  });

  it('creates independent registries', () => {
    const first = createWorkTypeRegistry();
    const second = createWorkTypeRegistry([{ ...BUILT_IN_WORK_TYPES[0], key: 'window-cleaning' }]);
    assert.equal(first.has('window-cleaning'), false);
    assert.equal(second.has('window-cleaning'), true);
  });
});

describe('workTypesCoveredBy', () => {
  const registry = createWorkTypeRegistry();

  it('returns only skill-free work types without skills', () => {
    assert.deepEqual(workTypesCoveredBy(registry, []), ['emergency-callout', 'general']);
  });

  it('adds the work types a skill unlocks', () => {
    assert.deepEqual(workTypesCoveredBy(registry, ['gas-safe']), ['boiler-repair', 'boiler-service', 'emergency-callout', 'gas-safety-check', 'general']);
  });

  it('needs every skill of a work type', () => {
    const partial = workTypesCoveredBy(registry, ['refrigerant']);
    assert.ok(partial.includes('ac-repair'));
    assert.equal(partial.includes('heat-pump-service'), false);
    const full = workTypesCoveredBy(registry, ['refrigerant', 'electrical']);
    assert.ok(full.includes('heat-pump-service'));
    assert.equal(full.includes('heat-pump-install'), false);
  });

  it('covers every work type for a fully certified technician', () => {
    const everything = new Set(BUILT_IN_WORK_TYPES.flatMap((handler) => handler.skills));
    assert.equal(workTypesCoveredBy(registry, [...everything]).length, 20);
  });

  it('ignores unrelated skills', () => {
    assert.deepEqual(workTypesCoveredBy(registry, ['juggling']), ['emergency-callout', 'general']);
  });
});
'''

TYPES_BODY = r'''
describe('crewFor', () => {
  it('lists the hazards that need a second person', () => {
    assert.deepEqual([...TWO_PERSON_HAZARDS], ['confined-space', 'working-at-height', 'live-electrical']);
  });

  it('defaults to one person', () => {
    assert.equal(crewFor([]), 1);
    assert.equal(crewFor(['dog', 'asbestos', 'restricted-access']), 1);
  });

  for (const hazard of ['confined-space', 'working-at-height', 'live-electrical']) {
    it(`sends two people for ${hazard}`, () => {
      assert.equal(crewFor([hazard]), 2);
      assert.equal(crewFor(['dog', hazard]), 2);
    });
  }

  it('respects a larger minimum crew', () => {
    assert.equal(crewFor([], 3), 3);
    assert.equal(crewFor(['confined-space'], 3), 3);
    assert.equal(crewFor([], 2), 2);
  });

  it('never goes below two when a hazard applies', () => {
    assert.equal(crewFor(['confined-space'], 1), 2);
    assert.equal(crewFor(['confined-space'], 0), 2);
  });

  it('matches hazard names exactly', () => {
    assert.equal(crewFor(['Confined-Space']), 1);
    assert.equal(crewFor(['confined space']), 1);
  });
});

describe('usedSku', () => {
  const report = (partsUsed: Array<{ sku: string; quantity: number }>): CompletionReport => ({
    completedAt: '2026-03-02T11:00:00Z',
    resolution: 'Completed the scheduled work in full',
    partsUsed,
    followUpRequired: false,
  });

  it('sums the quantities of matching skus', () => {
    const parts = [
      { sku: 'FLT-HP-STD', quantity: 2 },
      { sku: 'FLT-HP-XL', quantity: 3 },
      { sku: 'SEAL-GEN', quantity: 9 },
    ];
    assert.equal(usedSku(report(parts), 'FLT-'), 5);
    assert.equal(usedSku(report(parts), 'FLT-HP-STD'), 2);
    assert.equal(usedSku(report(parts), 'SEAL'), 9);
  });

  it('returns zero when nothing matches', () => {
    assert.equal(usedSku(report([]), 'FLT-'), 0);
    assert.equal(usedSku(report([{ sku: 'SEAL-GEN', quantity: 1 }]), 'FLT-'), 0);
  });

  it('matches prefixes case-sensitively', () => {
    assert.equal(usedSku(report([{ sku: 'flt-hp-std', quantity: 1 }]), 'FLT-'), 0);
  });

  it('counts repeated lines of the same sku', () => {
    assert.equal(usedSku(report([{ sku: 'WH-200L', quantity: 1 }, { sku: 'WH-200L', quantity: 1 }]), 'WH-'), 2);
  });

  it('treats an empty prefix as every line', () => {
    assert.equal(usedSku(report([{ sku: 'A', quantity: 1 }, { sku: 'B', quantity: 4 }]), ''), 5);
  });
});
'''

API_WORK_TYPES = r'''
describe('GET /work-types', () => {
  let world: World;

  beforeEach(() => {
    world = createWorld();
  });

  it('lists every registered work type alphabetically', () => {
    const { items } = call(world.app, 'GET', '/work-types', 'dispatch');
    const keys = items.map((item: any) => item.key);
    assert.equal(keys.length, 20);
    assert.deepEqual(keys, [...keys].sort());
  });

  it('exposes the planning fields only', () => {
    const [item] = call(world.app, 'GET', '/work-types', 'dispatch').items;
    assert.deepEqual(Object.keys(item).sort(), ['defaultDurationMinutes', 'defaultPriority', 'key', 'label', 'requiresAsset', 'skills']);
  });

  it('is readable by everyone who may read work orders', () => {
    for (const actor of ['admin', 'dispatch', 'billing', 'warehouse', world.technician.id]) {
      assert.equal(call(world.app, 'GET', '/work-types', actor).items.length, 20, actor);
    }
  });

  it('is readable by customer portal users', () => {
    const portal = call(world.app, 'POST', `/customers/${world.customer.id}/portal-users`, 'admin', { name: 'Avery Collins' });
    assert.equal(call(world.app, 'GET', '/work-types', portal.id).items.length, 20);
  });

  it('rejects unknown actors', () => {
    expectStatus(world.app, 401, 'GET', '/work-types', 'nobody');
  });

  it('only answers to GET', () => {
    const response = world.app.request('POST', '/work-types', { actor: 'admin', body: {} });
    assert.equal(response.status, 405);
    assert.match(String(response.headers?.allow), /GET/);
  });

  it('marks the equipment work types as requiring an asset', () => {
    const { items } = call(world.app, 'GET', '/work-types', 'dispatch');
    const required = items.filter((item: any) => item.requiresAsset).map((item: any) => item.key);
    assert.equal(required.length, 12);
    assert.ok(required.includes('boiler-service'));
    assert.equal(required.includes('general'), false);
  });

  it('refuses work orders for equipment work types without an asset', () => {
    for (const key of ['boiler-service', 'chiller-maintenance', 'elevator-inspection']) {
      const body = expectStatus(world.app, 422, 'POST', JOBS, 'dispatch', {
        customerId: world.customer.id,
        siteId: world.site.id,
        workType: key,
        summary: 'Scheduled attendance',
      });
      assert.equal(body.error.code, 'asset-required', key);
    }
  });
});
'''

FILES = {
    "tests/worktypes/catalog.test.ts": Spec(
        {REGISTRY: ["BUILT_IN_WORK_TYPES", "createWorkTypeRegistry"]}, CATALOG),
    "tests/worktypes/registry.test.ts": Spec(
        {REGISTRY: ["BUILT_IN_WORK_TYPES", "createWorkTypeRegistry", "workTypesCoveredBy"],
         TYPES: ["type WorkTypeHandler"]}, REGISTRY_BODY),
    "tests/worktypes/helpers.test.ts": Spec(
        {TYPES: ["crewFor", "TWO_PERSON_HAZARDS", "usedSku"]}, TYPES_BODY, ("type CompletionReport",)),
    "tests/api/work-types.test.ts": Spec(
        {WORLD: ["call", "createWorld", "expectStatus", "JOBS", "type World"]}, API_WORK_TYPES, node=WITH_SETUP),
}
for _spec in HANDLERS:
    FILES[f"tests/worktypes/{_spec['key']}.test.ts"] = handler_spec(_spec)


def generate(ctx):
    register(ctx, FILES)
