import type { DaemonGraphResponse, DaemonSnapshot } from "@/lib/types"

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}

function isNullableString(value: unknown): value is string | null {
  return typeof value === "string" || value === null
}

export function isDaemonSnapshot(value: unknown): value is DaemonSnapshot {
  if (!isRecord(value)) return false
  return (
    hasSnapshotIdentity(value) &&
    hasSnapshotState(value) &&
    (value.report === null || isRecord(value.report))
  )
}

function hasSnapshotIdentity(value: Record<string, unknown>): boolean {
  return (
    typeof value.target === "string" &&
    typeof value.profile === "string" &&
    typeof value.revision === "number"
  )
}

function hasSnapshotState(value: Record<string, unknown>): boolean {
  return (
    ["starting", "scanning", "ready", "error"].includes(
      typeof value.status === "string" ? value.status : ""
    ) &&
    isNullableString(value.scan_started_at) &&
    isNullableString(value.scan_finished_at) &&
    isNullableString(value.error)
  )
}

export function isDaemonGraphResponse(
  value: unknown
): value is DaemonGraphResponse {
  return (
    isRecord(value) &&
    typeof value.revision === "number" &&
    isRecord(value.graph)
  )
}
