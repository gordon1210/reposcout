"""Rendering helpers shared by the Fieldline fixture generator.

Every TypeScript file is produced from a template with `«name»` markers. Markers resolve to
per-file substitutions or to work-order API identifiers, whose names and modules depend on the
selected comparison variant.
"""

import hashlib
import posixpath
import re

VARIANTS = ("base", "api-migration", "module-split", "config-defaults", "refactor")
MARKER = re.compile(r"«([A-Za-z0-9_.:-]+)»")

# Exported work-order API: base identifier -> module-split identifier.
JOB_RENAMES = {
    "Job": "WorkOrder",
    "JobStatus": "WorkOrderStatus",
    "JobPriority": "WorkOrderPriority",
    "JobHold": "WorkOrderHold",
    "JobHistoryEntry": "WorkOrderHistoryEntry",
    "JobEvents": "WorkOrderEvents",
    "JobFilter": "WorkOrderFilter",
    "OpenJobInput": "OpenWorkOrderInput",
    "CompletionReport": "CompletionReport",
    "HoldReason": "HoldReason",
    "HOLD_REASONS": "HOLD_REASONS",
    "OPEN_STATUSES": "OPEN_STATUSES",
    "JOB_PRIORITIES": "WORK_ORDER_PRIORITIES",
    "isOpenStatus": "isOpenStatus",
    "canTransition": "canTransition",
    "openJob": "openWorkOrder",
    "getJob": "getWorkOrder",
    "requireJob": "requireWorkOrder",
    "listJobs": "listWorkOrders",
    "scheduleJob": "scheduleWorkOrder",
    "assignJob": "assignWorkOrder",
    "startJob": "startWorkOrder",
    "holdJob": "holdWorkOrder",
    "resumeJob": "resumeWorkOrder",
    "completeJob": "completeWorkOrder",
    "cancelJob": "cancelWorkOrder",
    "closeJob": "closeWorkOrder",
    "reopenJob": "reopenWorkOrder",
    "createFollowUp": "createFollowUp",
    "recordChecklistItem": "recordChecklistItem",
    "jobTimeline": "workOrderTimeline",
    "jobAge": "workOrderAge",
    "registerJobSubscriptions": "registerWorkOrderSubscriptions",
    "nextJobNumber": "nextWorkOrderNumber",
    "priorityRank": "priorityRank",
    "defaultPriorityFor": "defaultPriorityFor",
    "escalatePriority": "escalatePriority",
    "addJobNote": "addWorkOrderNote",
    "listJobNotes": "listWorkOrderNotes",
    "JobNote": "WorkOrderNote",
}

# Module that defines each exported work-order symbol in the module-split layout.
SPLIT_HOMES = {
    "src/workorders/model.ts": ("Job", "JobStatus", "JobPriority", "JobHold", "JobHistoryEntry", "JobEvents",
                                "JobFilter", "OpenJobInput", "CompletionReport", "HoldReason", "HOLD_REASONS",
                                "OPEN_STATUSES", "JOB_PRIORITIES", "isOpenStatus", "canTransition"),
    "src/workorders/lifecycle.ts": ("openJob", "scheduleJob", "assignJob", "startJob", "cancelJob", "closeJob",
                                    "reopenJob"),
    "src/workorders/holds.ts": ("holdJob", "resumeJob", "registerJobSubscriptions"),
    "src/workorders/completion.ts": ("completeJob", "recordChecklistItem"),
    "src/workorders/followups.ts": ("createFollowUp",),
    "src/workorders/queries.ts": ("getJob", "requireJob", "listJobs", "jobTimeline", "jobAge"),
    "src/workorders/numbering.ts": ("nextJobNumber",),
    "src/workorders/priorities.ts": ("priorityRank", "defaultPriorityFor", "escalatePriority"),
    "src/workorders/notes.ts": ("addJobNote", "listJobNotes", "JobNote"),
}
BASE_HOMES = {
    "src/jobs/numbering.ts": ("nextJobNumber",),
    "src/jobs/priorities.ts": ("priorityRank", "defaultPriorityFor", "escalatePriority"),
    "src/jobs/notes.ts": ("addJobNote", "listJobNotes", "JobNote"),
}
TYPE_SYMBOLS = {"Job", "JobStatus", "JobPriority", "JobHold", "JobHistoryEntry", "JobFilter", "OpenJobInput",
                "CompletionReport", "HoldReason", "JobNote"}


class Ctx:
    def __init__(self, variant):
        if variant not in VARIANTS:
            raise ValueError(f"unknown variant {variant}")
        self.variant = variant
        self.files = {}

    @property
    def split(self):
        return self.variant == "module-split"

    @property
    def quantize(self):
        return self.variant == "api-migration"

    @property
    def early(self):
        return self.variant == "config-defaults"

    @property
    def refactor(self):
        return self.variant == "refactor"

    def sym(self, name):
        if name not in JOB_RENAMES:
            raise KeyError(name)
        return JOB_RENAMES[name] if self.split else name

    def job_home(self, name):
        homes = SPLIT_HOMES if self.split else BASE_HOMES
        for module, names in homes.items():
            if name in names:
                return module
        return "src/jobs/jobs.ts"

    @property
    def jobs_dir(self):
        return "src/workorders" if self.split else "src/jobs"

    def add(self, path, text):
        if path in self.files:
            raise ValueError(f"duplicate generated file {path}")
        self.files[path] = normalize(text)

    def replace(self, path, text):
        if path not in self.files:
            raise KeyError(path)
        self.files[path] = normalize(text)

    def append(self, path, text):
        self.files[path] = normalize(self.files[path] + "\n" + text)

    def patch(self, path, old, new, count=1):
        """Replace `old` in a generated file; it must occur exactly `count` times."""
        text = self.files[path]
        if text.count(old) != count:
            raise ValueError(f"{path}: expected {count} occurrences of {old[:60]!r}, found {text.count(old)}")
        self.files[path] = normalize(text.replace(old, new))


def normalize(text):
    lines = [line.rstrip() for line in text.strip("\n").split("\n")]
    collapsed = []
    for line in lines:
        if line == "" and collapsed and collapsed[-1] == "":
            continue
        collapsed.append(line)
    return "\n".join(collapsed) + "\n"


def render(ctx, template, **subs):
    def replace(match):
        key = match.group(1)
        if key in subs:
            return str(subs[key])
        if key in JOB_RENAMES:
            return ctx.sym(key)
        raise KeyError(f"unresolved template marker {key}")
    return MARKER.sub(replace, template)


def relative(here, target):
    if target.startswith("node:"):
        return target
    path = posixpath.relpath(target, posixpath.dirname(here))
    return path if path.startswith(".") else "./" + path


def import_lines(here, groups):
    """`groups` maps target module paths to names; a `type ` prefix marks type-only names."""
    lines = []
    for target in sorted(groups, key=lambda item: (item.startswith("src/"), relative(here, item))):
        names = groups[target]
        if not names:
            continue
        values = sorted({name for name in names if not name.startswith("type ")}, key=str.lower)
        types = sorted({name[5:] for name in names if name.startswith("type ")}, key=str.lower)
        specifier = relative(here, target)
        if values:
            lines.append(_import_statement("import", values, specifier))
        if types:
            lines.append(_import_statement("import type", types, specifier))
    return "\n".join(lines)


def _import_statement(keyword, names, specifier):
    single = f"{keyword} {{ {', '.join(names)} }} from '{specifier}';"
    if len(single) <= 100:
        return single
    body = "".join(f"  {name},\n" for name in names)
    return f"{keyword} {{\n{body}}} from '{specifier}';"


def imports(ctx, here, groups, jobs=(), barrel=True):
    """Combine ordinary imports with work-order symbols resolved for the current variant."""
    merged = {target: list(names) for target, names in groups.items()}
    for name in jobs:
        is_type = name.startswith("type ")
        bare = name[5:] if is_type else name
        if ctx.split and barrel:
            target = "src/workorders/index.ts"
        else:
            target = ctx.job_home(bare)
        merged.setdefault(target, []).append(("type " if is_type else "") + ctx.sym(bare))
    return import_lines(here, merged)


def pick(key, options):
    digest = hashlib.sha256(key.encode()).digest()
    return options[int.from_bytes(digest[:4], "big") % len(options)]


ROUNDING_KINDS = ("nearest", "nearest-explicit", "up", "down", "wrong-down")


def rounding(ctx, value, step, kind, minimum=None):
    """A rounding call in the current variant's API; `wrong-down` is the misconverted site."""
    if kind not in ROUNDING_KINDS:
        raise ValueError(kind)
    if ctx.quantize:
        mode = {"nearest": None, "nearest-explicit": "nearest", "up": "up", "down": "down",
                "wrong-down": "down"}[kind]
        fields = ["step" if step == "step" else f"step: {step}"]
        if mode:
            fields.append(f"mode: '{mode}'")
        if minimum is not None:
            fields.append("minimum" if minimum == "minimum" else f"minimum: {minimum}")
        return f"quantize({value}, {{ {', '.join(fields)} }})"
    if kind == "down":
        call = f"floorToIncrement({value}, {step})"
    elif kind == "up":
        call = f"roundToIncrement({value}, {step}, true)"
    elif kind in ("nearest-explicit", "wrong-down"):
        call = f"roundToIncrement({value}, {step}, false)"
    else:
        call = f"roundToIncrement({value}, {step})"
    return call if minimum is None else f"Math.max({minimum}, {call})"


def rounding_names(ctx, *kinds):
    if ctx.quantize:
        return ["quantize"]
    names = set()
    for kind in kinds:
        names.add("floorToIncrement" if kind == "down" else "roundToIncrement")
    return sorted(names)
