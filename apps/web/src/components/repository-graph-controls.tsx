import { ChevronRight } from "lucide-react"

import type { ExplorerMode } from "@/components/repository-graph-types"
import type { CanvasLegendData } from "@/components/repository-graph-legend"
import { languageColor } from "@/components/repository-graph-visuals"
import {
  Card,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import type { ExplorerView } from "@/lib/graph-explorer-model"
import { cn } from "@/lib/utils"

export function GraphBreadcrumbs({
  view,
  mode,
  focus,
  onOpen,
}: {
  view: ExplorerView
  mode: ExplorerMode
  focus: string | null
  onOpen: (path: string) => void
}) {
  return (
    <nav
      aria-label="Graph location"
      className="flex flex-wrap items-center gap-1 text-sm font-semibold"
    >
      {view.breadcrumbs.map((crumb, index) => (
        <span key={crumb.path || "."} className="flex items-center gap-1">
          {index > 0 ? (
            <ChevronRight className="size-3.5 text-muted-foreground" />
          ) : null}
          <button
            type="button"
            onClick={() => onOpen(crumb.path)}
            className="rounded px-1.5 py-1 hover:bg-accent focus-visible:bg-accent focus-visible:outline-none"
          >
            {crumb.label}
          </button>
        </span>
      ))}
      {mode === "neighborhood" && focus ? (
        <>
          <ChevronRight className="size-3.5 text-muted-foreground" />
          <span className="max-w-80 truncate rounded bg-muted px-2 py-1 font-mono text-xs">
            {focus}
          </span>
        </>
      ) : null}
      <span className="ml-2 text-xs font-normal text-muted-foreground">
        Esc to step back
      </span>
    </nav>
  )
}

export function GraphMetric({
  label,
  value,
  detail,
  tone,
}: {
  label: string
  value: number
  detail: string
  tone?: "attention"
}) {
  return (
    <Card className="gap-1 py-4">
      <CardHeader className="gap-1 px-4 sm:px-5">
        <CardDescription>{label}</CardDescription>
        <CardTitle
          className={cn(
            "text-2xl tabular-nums",
            tone === "attention" && value > 0 && "text-destructive"
          )}
        >
          {value.toLocaleString()}
        </CardTitle>
        <p className="truncate text-xs text-muted-foreground" title={detail}>
          {detail || "—"}
        </p>
      </CardHeader>
    </Card>
  )
}

function LegendEdgeSample({
  dashed,
  color,
}: {
  dashed?: boolean
  color: string
}) {
  return (
    <svg width="18" height="6" aria-hidden="true" className="shrink-0">
      <line
        x1="0"
        y1="3"
        x2="18"
        y2="3"
        stroke={color}
        strokeWidth="1.5"
        strokeDasharray={dashed ? "4 3" : undefined}
      />
    </svg>
  )
}

export function CanvasLegend({
  legend,
  typeFocused,
}: {
  legend: CanvasLegendData
  typeFocused: boolean
}) {
  return (
    <div className="pointer-events-none w-44 rounded-lg border bg-card/90 p-2.5 text-[10px] leading-relaxed text-muted-foreground shadow-sm backdrop-blur">
      <p className="font-semibold uppercase tracking-wide">Legend</p>
      <div className="mt-1.5 space-y-0.5">
        {legend.languages.map((language) => (
          <p key={language.name} className="flex items-center gap-1.5">
            <span
              className="size-2 shrink-0 rounded-full"
              style={{ background: languageColor(language.name) }}
            />
            <span className="min-w-0 flex-1 truncate text-foreground/85">
              {language.name}
            </span>
            <span className="tabular-nums">{language.files}</span>
          </p>
        ))}
        {legend.moreLanguages > 0 ? (
          <p>+{legend.moreLanguages} more languages</p>
        ) : null}
      </div>
      <LegendRelationships legend={legend} typeFocused={typeFocused} />
    </div>
  )
}

function LegendRelationships({
  legend,
  typeFocused,
}: {
  legend: CanvasLegendData
  typeFocused: boolean
}) {
  return (
    <div className="mt-2 space-y-1 border-t pt-2">
      <p className="flex items-center gap-1.5">
        <LegendEdgeSample
          color={
            typeFocused && legend.hasTypeRelations
              ? "var(--primary)"
              : "var(--muted-foreground)"
          }
        />
        <span className="min-w-0 flex-1">
          {typeFocused && legend.hasTypeRelations
            ? "declared type relation"
            : "arrow points at the dependency"}
        </span>
      </p>
      {legend.hasImportContext ? (
        <p className="flex items-center gap-1.5">
          <LegendEdgeSample dashed color="var(--muted-foreground)" />
          <span className="min-w-0 flex-1">direct import context</span>
        </p>
      ) : null}
      {legend.hasExternal ? (
        <p className="flex items-center gap-1.5">
          <span
            aria-hidden="true"
            className="h-3.5 w-[18px] shrink-0 rounded-sm border border-dashed border-muted-foreground/70"
          />
          <span className="min-w-0 flex-1">outside this scope</span>
        </p>
      ) : null}
      {legend.hasProminent ? (
        <p className="flex items-center gap-1.5">
          <span
            aria-hidden="true"
            className="flex w-[18px] shrink-0 items-end justify-between"
          >
            <span className="size-1.5 rounded-[2px] border border-muted-foreground/70" />
            <span className="size-2.5 rounded-[2px] border border-muted-foreground/70" />
          </span>
          <span className="min-w-0 flex-1">
            larger card = wider proven reach
          </span>
        </p>
      ) : null}
    </div>
  )
}

export function LabeledSelect<T extends string>({
  label,
  value,
  disabled,
  options,
  onChange,
}: {
  label: string
  value: string
  disabled: boolean
  options: Array<[T, string]>
  onChange: (value: T) => void
}) {
  return (
    <label className="grid grid-cols-[auto_1fr] items-center gap-2 rounded-md border bg-background px-3 text-xs text-muted-foreground shadow-xs has-[:focus-visible]:border-ring has-[:focus-visible]:ring-3 has-[:focus-visible]:ring-ring/50 has-[:disabled]:opacity-50">
      {label}
      <select
        aria-label={`Graph ${label.toLowerCase()}`}
        value={value}
        disabled={disabled}
        onChange={(event) => {
          const selected = options.find(
            ([option]) => option === event.target.value
          )
          if (selected) onChange(selected[0])
        }}
        className="h-8 bg-transparent font-medium text-foreground outline-none"
      >
        {options.map(([option, text]) => (
          <option key={option} value={option}>
            {text}
          </option>
        ))}
      </select>
    </label>
  )
}
