const compactNumber = new Intl.NumberFormat(undefined, {
  notation: "compact",
  maximumFractionDigits: 1,
})

const fullNumber = new Intl.NumberFormat()

export function formatCompact(value: number): string {
  return compactNumber.format(value)
}

export function formatNumber(value: number): string {
  return fullNumber.format(value)
}

export function formatPercent(value: number): string {
  return `${value.toFixed(1)}%`
}

export function formatRatio(value: number): string {
  return formatPercent(value * 100)
}

export function formatScore(value: number): string {
  return value.toFixed(2)
}

export function formatDateTime(value: string | null | undefined): string {
  if (!value) return "Never"
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "medium",
  }).format(new Date(value))
}

export function formatElapsed(startedAt: string | null, now: number): string | null {
  if (!startedAt) return null
  const seconds = Math.max(0, Math.floor((now - new Date(startedAt).getTime()) / 1000))
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  const remainder = seconds % 60
  return `${minutes}m ${remainder}s`
}
