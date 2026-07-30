import { describe, expect, it } from "vitest"

import {
  averageFileCyclomatic,
  markerTotal,
  rankedFiles,
  rankedFindings,
} from "@/lib/dashboard-data"
import { makeFile, makeReport } from "@/test/fixtures"

describe("dashboard data", () => {
  it("ranks every file by token count for client-side pagination", () => {
    const files = Array.from({ length: 105 }, (_, index) =>
      makeFile(`src/file-${index}.rs`, index)
    )
    const ranked = rankedFiles(makeReport({ files }))

    expect(ranked).toHaveLength(105)
    expect(ranked[0].tokens).toBe(104)
    expect(ranked.at(-1)?.tokens).toBe(0)
  })

  it("averages callable cyclomatic complexity within a file", () => {
    const file = makeFile("src/lib.rs")
    file.complexity!.functions = [
      { name: "first", line: 1, cyclomatic: 2, cognitive: 1, max_nesting: 1 },
      { name: "second", line: 10, cyclomatic: 7, cognitive: 4, max_nesting: 2 },
    ]

    expect(averageFileCyclomatic(file)).toBe(4.5)
    expect(
      averageFileCyclomatic(makeFile("src/no-callables.rs"))
    ).toBeUndefined()
  })

  it("orders findings by severity then stable location", () => {
    const report = makeReport()
    report.finding_catalog.findings = [
      {
        fingerprint: "warning",
        kind: "marker",
        severity: "warning",
        message: "warning",
        primary_location: { path: "src/z.rs", start_line: 2, end_line: 2 },
      },
      {
        fingerprint: "error",
        kind: "complexity",
        severity: "error",
        message: "error",
        primary_location: { path: "src/a.rs", start_line: 1, end_line: 1 },
      },
    ]

    expect(
      rankedFindings(report).map((finding) => finding.fingerprint)
    ).toEqual(["error", "warning"])
  })

  it("sums all configured marker counts", () => {
    expect(markerTotal({ TODO: 2, FIXME: 3, HACK: 1 })).toBe(6)
  })
})
