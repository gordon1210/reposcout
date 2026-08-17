import type { DaemonSnapshot, FileReport, ScanReport } from "@/lib/types"

export function makeFile(path: string, tokens = 100): FileReport {
  return {
    path,
    language: "Rust",
    bytes: tokens * 4,
    tokens,
    loc: 20,
    sloc: 16,
    comment_lines: 2,
    comment_ratio: 0.1,
    complexity: {
      cyclomatic: 4,
      cognitive: 3,
      max_nesting: 2,
      maintainability_index: 82,
    },
    churn: { commits: 3, authors: 2 },
    approximate: false,
  }
}

export function makeReport(overrides: Partial<ScanReport> = {}): ScanReport {
  const report: ScanReport = {
    schema_version: "2.0",
    root: "/workspace/repo",
    target: "/workspace/repo",
    generated_at: "2026-07-14T08:00:00Z",
    analysis_profile: {
      analyzers: {
        tokens: true,
        complexity: true,
        imports: true,
        markers: true,
        duplication: false,
        churn: false,
      },
      health: { scope: "source" },
    },
    summary: {
      files: 2,
      bytes: 8_000,
      tokens: 1_234,
      loc: 100,
      sloc: 80,
      comment_lines: 10,
      comment_ratio: 0.1,
      source: {
        files: 2,
        bytes: 8_000,
        tokens: 1_234,
        loc: 100,
        sloc: 80,
        comment_lines: 10,
      },
      languages: [
        {
          name: "Rust",
          source: true,
          files: 2,
          bytes: 8_000,
          loc: 100,
          sloc: 80,
          comment_lines: 10,
          tokens: 1_234,
        },
      ],
      complexity: {
        cyclomatic_total: 12,
        cyclomatic_avg: 3,
        cyclomatic_max: 7,
        cognitive_total: 9,
        cognitive_avg: 2.25,
        cognitive_max: 5,
        mi_avg: 81.5,
        mi_min: 72,
        functions: 4,
        cyclomatic_threshold: 20,
        functions_over_threshold: 0,
        approximate_files: 0,
      },
      duplication: {
        exact_groups: 1,
        near_groups: 0,
        duplicated_lines: 6,
        duplicated_pct: 7.5,
        analyzed_lines: 80,
        duplicated_tokens: 20,
        analyzed_tokens: 1_000,
        duplicated_tokens_pct: 2,
      },
      markers: { TODO: 1 },
      top_token_files: [{ path: "src/lib.rs", tokens: 900 }],
      top_source_token_files: [{ path: "src/lib.rs", tokens: 900 }],
      top_hotspots: [
        { path: "src/lib.rs", commits: 3, cyclomatic: 7, score: 21 },
      ],
      top_functions: [
        {
          path: "src/lib.rs",
          name: "analyze",
          line: 42,
          cyclomatic: 7,
          cognitive: 5,
          max_nesting: 2,
        },
      ],
      complexity_violations: [],
      top_duplicates: [],
      symbols: { functions: 4, types: 2, exports: 1 },
      test_presence: {
        frameworks: [{ name: "cargo-test", evidence: "Cargo.toml" }],
        test_files: 1,
      },
      top_risks: [
        {
          path: "src/lib.rs",
          score: 0.62,
          sloc: 64,
          cyclomatic: 7,
          churn_commits: 3,
          untested: false,
          reasons: ["high complexity"],
        },
      ],
      assessment: {
        fits_context: true,
        token_budget: 200_000,
        cleanup_worth: "low",
        reasons: [],
      },
    },
    files: [makeFile("src/lib.rs", 900), makeFile("src/main.rs", 334)],
    finding_catalog: {
      version: 1,
      findings: [
        {
          fingerprint: "finding-1",
          kind: "marker",
          severity: "warning",
          message: "TODO marker",
          primary_location: {
            path: "src/lib.rs",
            start_line: 12,
            end_line: 12,
          },
        },
      ],
    },
    diagnostics: {
      discovered_files: 2,
      analyzed_files: 2,
      unsupported_files: 0,
      unreadable_files: 0,
      walker_errors: 0,
    },
  }
  return { ...report, ...overrides }
}

export function makeSnapshot(
  overrides: Partial<DaemonSnapshot> = {}
): DaemonSnapshot {
  return {
    target: "/workspace/repo",
    profile: "lite",
    revision: 1,
    status: "ready",
    scan_started_at: "2026-07-14T07:59:55Z",
    scan_finished_at: "2026-07-14T08:00:00Z",
    error: null,
    report: makeReport(),
    ...overrides,
  }
}
