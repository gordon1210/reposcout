import { describe, expect, it } from "vitest"
import {
  isDaemonGraphResponse,
  isDaemonSnapshot,
  parseDaemonGraphResponse,
  parseDaemonSnapshot,
} from "@/lib/api-validation"
import { makeSnapshot } from "@/test/fixtures"

function graphResponse() {
  return {
    revision: 3,
    graph: {
      languages: ["Rust"],
      nodes: 1,
      edges: 0,
      files: [{ path: "src/lib.rs", language: "Rust", fan_in: 0, fan_out: 0 }],
      edge_list: [],
      cycles: [],
      orphans: [],
      top_depended: [],
      most_dependent: [],
      unresolved_imports: 0,
    },
  }
}

describe("daemon response validation", () => {
  it("accepts schema 2.0, additive properties, and absent optional fields", () => {
    const snapshot = makeSnapshot()
    expect(isDaemonSnapshot(snapshot)).toBe(true)
    expect(isDaemonSnapshot({ ...snapshot, future: {} })).toBe(true)
    const report = snapshot.report!
    delete report.summary.complexity_violations
    delete report.summary.source
    delete report.analysis_profile
    expect(isDaemonSnapshot(snapshot)).toBe(true)
    expect(
      isDaemonSnapshot(makeSnapshot({ status: "starting", report: null }))
    ).toBe(true)
    expect(isDaemonGraphResponse(graphResponse())).toBe(true)
  })

  it.each([[], {}, { summary: {} }])(
    "rejects malformed non-null reports: %j",
    (report) => {
      expect(isDaemonSnapshot({ ...makeSnapshot(), report })).toBe(false)
    }
  )

  it("rejects unsupported schemas and invalid nested report values", () => {
    const snapshot = makeSnapshot()
    const report = snapshot.report!
    for (const replacement of [
      { ...report, schema_version: "3.0" },
      { ...report, files: [{}] },
      { ...report, summary: { ...report.summary, source: {} } },
      { ...report, summary: { ...report.summary, languages: [null] } },
      {
        ...report,
        summary: { ...report.summary, assessment: { reasons: "bad" } },
      },
      { ...report, finding_catalog: { version: 1, findings: [{}] } },
      { ...report, context: {} },
      { ...report, graph: {} },
      { ...report, diagnostics: [] },
    ]) {
      expect(isDaemonSnapshot({ ...snapshot, report: replacement })).toBe(false)
    }
  })

  it("normalizes collections omitted by Rust for empty graphs", () => {
    const response = graphResponse()
    const wireGraph = {
      ...response.graph,
      nodes: 0,
      files: undefined,
      edge_list: undefined,
    }
    expect(
      parseDaemonGraphResponse({ ...response, graph: wireGraph })?.graph.files
    ).toEqual([])
    expect(
      parseDaemonGraphResponse({ ...response, graph: wireGraph })?.graph
        .edge_list
    ).toEqual([])
    const snapshot = makeSnapshot()
    expect(
      parseDaemonSnapshot({
        ...snapshot,
        report: { ...snapshot.report, graph: wireGraph },
      })?.report?.graph?.files
    ).toEqual([])
    expect(
      parseDaemonGraphResponse({
        ...response,
        graph: { ...wireGraph, nodes: 2 },
      })
    ).toBeNull()
    expect(
      parseDaemonGraphResponse({
        ...response,
        graph: { ...wireGraph, files: null },
      })
    ).toBeNull()
  })

  it("rejects malformed graph collections and nested edges", () => {
    const response = graphResponse()
    for (const graph of [
      {},
      [],
      null,
      { ...response.graph, files: [{}] },
      {
        ...response.graph,
        edge_list: [{ source: "a", target: 1, resolver: "rust" }],
      },
      { ...response.graph, cycles: ["a"] },
      { ...response.graph, symbols: [{}] },
      { ...response.graph, nodes: Infinity },
    ]) {
      expect(isDaemonGraphResponse({ ...response, graph })).toBe(false)
    }
  })

  it.each([-1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, "1"])(
    "rejects invalid revisions: %j",
    (revision) => {
      expect(isDaemonSnapshot({ ...makeSnapshot(), revision })).toBe(false)
      expect(isDaemonGraphResponse({ ...graphResponse(), revision })).toBe(
        false
      )
    }
  )
})
