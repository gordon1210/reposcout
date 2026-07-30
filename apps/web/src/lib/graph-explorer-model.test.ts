import { describe, expect, it } from "vitest"

import {
  buildRepositoryGraphExplorer,
  classifyFile,
  groupExplorerFiles,
} from "@/lib/graph-explorer-model"
import type { DependencyGraph, FileReport } from "@/lib/types"
import { makeFile, makeReport } from "@/test/fixtures"

function file(path: string, language: string, tokens = 100): FileReport {
  return { ...makeFile(path, tokens), language }
}

function mixedFixture() {
  const report = makeReport()
  report.files = [
    file("apps/web/package.json", "JSON", 40),
    file("apps/web/src/main.ts", "TypeScript", 300),
    file("apps/web/src/api/client.ts", "TypeScript", 220),
    file("apps/web/src/legacy.php", "PHP", 180),
    file("crates/core/Cargo.toml", "TOML", 50),
    file("crates/core/src/lib.rs", "Rust", 260),
    file("crates/core/src/model.rs", "Rust", 210),
    file("services/worker/main.go", "Go", 190),
    file("services/worker/internal/job.go", "Go", 170),
    file("tools/run.py", "Python", 160),
    file("tools/helpers.py", "Python", 120),
  ]
  report.summary.top_risks = [
    {
      path: "apps/web/src/main.ts",
      score: 0.78,
      sloc: 16,
      cyclomatic: 4,
      churn_commits: 3,
      untested: true,
      reasons: ["no matching test"],
    },
  ]
  report.finding_catalog.findings = [
    {
      fingerprint: "main-risk",
      kind: "risk",
      severity: "warning",
      message: "Main is risky",
      primary_location: {
        path: "apps/web/src/main.ts",
        start_line: 1,
        end_line: 1,
      },
    },
  ]

  const edge_list = [
    {
      source: "apps/web/src/main.ts",
      target: "apps/web/src/api/client.ts",
      resolver: "relative",
    },
    {
      source: "apps/web/src/legacy.php",
      target: "apps/web/src/api/client.ts",
      resolver: "php-include",
    },
    {
      source: "crates/core/src/lib.rs",
      target: "crates/core/src/model.rs",
      resolver: "rust-mod",
    },
    {
      source: "services/worker/main.go",
      target: "services/worker/internal/job.go",
      resolver: "go-module",
    },
    {
      source: "tools/run.py",
      target: "tools/helpers.py",
      resolver: "python-absolute",
    },
  ]
  const graphPaths = report.files
    .filter((entry) =>
      ["TypeScript", "PHP", "Rust", "Go", "Python"].includes(entry.language)
    )
    .map((entry) => entry.path)
  const graph: DependencyGraph = {
    languages: ["Go", "PHP", "Python", "Rust", "TypeScript"],
    nodes: graphPaths.length,
    edges: edge_list.length,
    files: graphPaths.map((path) => ({
      path,
      language: report.files.find((entry) => entry.path === path)!.language,
      fan_in: edge_list.filter((edge) => edge.target === path).length,
      fan_out: edge_list.filter((edge) => edge.source === path).length,
    })),
    edge_list,
    cycles: [],
    orphans: [],
    top_depended: [],
    most_dependent: [],
    unresolved_imports: 0,
    config_files: [
      "apps/web/package.json",
      "crates/core/Cargo.toml",
      "services/worker/go.mod",
    ],
  }
  return { graph, report }
}

describe("repository graph explorer model", () => {
  it("builds stable architecture scopes for a mixed-language repository", () => {
    const { graph, report } = mixedFixture()
    const explorer = buildRepositoryGraphExplorer(graph, report)

    const root = explorer.view("")
    expect(root.groups?.map((group) => group.path)).toEqual([
      "apps/web",
      "crates/core",
      "services/worker",
      "tools",
    ])
    expect(
      root.groups
        ?.find((group) => group.path === "apps/web")
        ?.members.map((member) => member.path)
    ).toEqual(["apps/web/src", "apps/web/package.json"])
    expect(root.scope.languages.map((language) => language.name)).toEqual([
      "Go",
      "Python",
      "Rust",
      "TypeScript",
      "JSON",
      "PHP",
      "TOML",
    ])

    const web = explorer.inspectScope("apps/web")
    expect(web).toMatchObject({
      scopeKind: "package",
      files: 4,
      graphFiles: 3,
      findings: 1,
      riskFiles: 1,
    })
  })

  it("drills through directories and aggregates factual connection kinds", () => {
    const { graph, report } = mixedFixture()
    const explorer = buildRepositoryGraphExplorer(graph, report)

    const packageView = explorer.view("apps/web")
    expect(packageView.entities.map((entity) => entity.path)).toEqual([
      "apps/web/src",
      "apps/web/package.json",
    ])

    const sourceView = explorer.view("apps/web/src")
    expect(sourceView.breadcrumbs.map((crumb) => crumb.label)).toEqual([
      "Project",
      "apps",
      "web",
      "src",
    ])
    expect(sourceView.entities.map((entity) => entity.path)).toEqual([
      "apps/web/src/api",
      "apps/web/src/legacy.php",
      "apps/web/src/main.ts",
    ])
    expect(sourceView.connections).toHaveLength(2)
    expect(
      sourceView.connections.map((connection) => connection.relation).sort()
    ).toEqual(["imports", "includes"].sort())
  })

  it("exposes complete scope and file inspection without semantic invention", () => {
    const { graph, report } = mixedFixture()
    report.files.find(
      (entry) => entry.path === "apps/web/src/main.ts"
    )!.complexity!.functions = [
      {
        name: "main",
        line: 4,
        end_line: 18,
        cyclomatic: 4,
        cognitive: 3,
        max_nesting: 2,
      },
    ]
    const explorer = buildRepositoryGraphExplorer(graph, report)

    const scope = explorer.inspectScope("apps/web")
    expect(scope.allFiles).toHaveLength(4)
    expect(scope.configFiles).toEqual(["apps/web/package.json"])

    const inspected = explorer.inspectFile("apps/web/src/main.ts")
    expect(inspected?.file.category).toBe("entrypoint")
    expect(inspected?.graph?.roles).toContain("Top-level consumer")
    expect(inspected?.findings[0].message).toBe("Main is risky")
    expect(inspected?.risk?.score).toBe(0.78)
    expect(inspected?.file.report.complexity?.functions?.[0].name).toBe("main")
    expect(explorer.search("main").map((entry) => entry.path)).toEqual([
      "apps/web/src/main.ts",
      "services/worker/main.go",
    ])
  })

  it("classifies objective file roles across first-class languages", () => {
    expect(classifyFile(file("src/main.rs", "Rust"))).toBe("entrypoint")
    expect(classifyFile(file("pkg/service_test.go", "Go"))).toBe("test")
    expect(classifyFile(file("tests/ControllerTest.php", "PHP"))).toBe("test")
    expect(classifyFile(file("schema/events.proto", "Protocol Buffers"))).toBe(
      "schema"
    )
    expect(classifyFile(file("apps/web/tsconfig.json", "JSON"))).toBe("config")
    expect(
      classifyFile({
        ...file("generated/client.ts", "TypeScript"),
        skip_hint: "generated",
      })
    ).toBe("generated")
    expect(classifyFile(file("src/helpers.py", "Python"))).toBe("source")
  })

  it("retains every first-class language in one multilingual scope", () => {
    const languages = [
      "Rust",
      "Python",
      "JavaScript",
      "TypeScript",
      "TSX",
      "Go",
      "PHP",
    ]
    const report = makeReport()
    report.files = languages.map((language, index) =>
      file(`src/language-${index}.${index === 0 ? "rs" : "txt"}`, language)
    )
    const graph: DependencyGraph = {
      languages: [...languages].sort(),
      nodes: languages.length,
      edges: 0,
      files: report.files.map((entry) => ({
        path: entry.path,
        language: entry.language,
        fan_in: 0,
        fan_out: 0,
      })),
      edge_list: [],
      cycles: [],
      orphans: report.files.map((entry) => entry.path),
      top_depended: [],
      most_dependent: [],
      unresolved_imports: 0,
    }

    const scope = buildRepositoryGraphExplorer(graph, report).inspectScope(
      "src"
    )
    expect(scope.graphFiles).toBe(7)
    expect(scope.languages.map((language) => language.name).sort()).toEqual(
      [...languages].sort()
    )
  })

  it("labels path groups with honest language-aware scope semantics", () => {
    const { graph, report } = mixedFixture()
    const explorer = buildRepositoryGraphExplorer(graph, report)
    const summary = (path: string) => explorer.inspectFile(path)!.file

    expect(
      groupExplorerFiles([
        summary("crates/core/src/lib.rs"),
        summary("crates/core/src/model.rs"),
      ])[0].label
    ).toBe("Module scope")
    expect(
      groupExplorerFiles([
        summary("tools/run.py"),
        summary("tools/helpers.py"),
      ])[0].label
    ).toBe("Package scope")
    expect(
      groupExplorerFiles([summary("apps/web/src/legacy.php")])[0].label
    ).toBe("Namespace path")
    expect(groupExplorerFiles([summary("apps/web/src/main.ts")])[0].label).toBe(
      "Module directory"
    )
    expect(
      groupExplorerFiles([
        summary("apps/web/src/main.ts"),
        summary("apps/web/src/legacy.php"),
      ])[0].label
    ).toBe("Mixed-language scope")
  })

  it("projects a semantic type neighborhood before the noisy multi-hop file graph", () => {
    const report = makeReport()
    report.files = [
      file("src/HttpClient.php", "PHP"),
      file("src/GuzzleClient.php", "PHP"),
      file("src/CurlClient.php", "PHP"),
      file("src/Factory.php", "PHP"),
      file("src/Response.php", "PHP"),
      file("src/SecondHop.php", "PHP"),
    ]
    const edge_list = [
      {
        source: "src/GuzzleClient.php",
        target: "src/HttpClient.php",
        resolver: "composer-psr-4",
      },
      {
        source: "src/CurlClient.php",
        target: "src/HttpClient.php",
        resolver: "composer-psr-4",
      },
      {
        source: "src/Factory.php",
        target: "src/HttpClient.php",
        resolver: "composer-psr-4",
      },
      {
        source: "src/HttpClient.php",
        target: "src/Response.php",
        resolver: "composer-psr-4",
      },
      {
        source: "src/SecondHop.php",
        target: "src/Factory.php",
        resolver: "composer-psr-4",
      },
    ]
    const graph: DependencyGraph = {
      languages: ["PHP"],
      nodes: report.files.length,
      edges: edge_list.length,
      files: report.files.map((entry) => ({
        path: entry.path,
        language: entry.language,
        fan_in: edge_list.filter((edge) => edge.target === entry.path).length,
        fan_out: edge_list.filter((edge) => edge.source === entry.path).length,
        symbol_reach:
          entry.path === "src/HttpClient.php"
            ? {
                symbol_id: "http-client",
                name: "HttpClient",
                kind: "class",
                fan_in: 2,
                fan_out: 0,
                relation: "extends",
              }
            : undefined,
      })),
      edge_list,
      symbols: [
        {
          id: "http-client",
          name: "HttpClient",
          qualified_name: "App\\HttpClient",
          kind: "class",
          path: "src/HttpClient.php",
          language: "PHP",
          line: 4,
          fan_in: 2,
          fan_out: 0,
        },
        ...["GuzzleClient", "CurlClient"].map((name) => ({
          id: name.toLowerCase(),
          name,
          qualified_name: `App\\${name}`,
          kind: "class",
          path: `src/${name}.php`,
          language: "PHP",
          line: 3,
          fan_in: 0,
          fan_out: 1,
        })),
      ],
      symbol_edges: ["GuzzleClient", "CurlClient"].map((name) => ({
        source: name.toLowerCase(),
        target: "http-client",
        relation: "extends",
        resolver: "qualified",
      })),
      cycles: [],
      orphans: [],
      top_depended: [],
      most_dependent: [],
      unresolved_imports: 0,
    }
    const explorer = buildRepositoryGraphExplorer(graph, report)

    const semantic = explorer.neighborhood("src/HttpClient.php", "both", 2)
    expect(semantic.presentation).toBe("type")
    expect(semantic.focusPath).toBe("src/HttpClient.php")
    expect(
      semantic.groups?.map((group) => [group.label, group.members.length])
    ).toEqual([
      ["Extends", 2],
      ["Import dependents", 1],
      ["Import dependencies", 1],
    ])
    expect(
      semantic.groups?.every(
        (group) => group.totalMembers === group.members.length
      )
    ).toBe(true)
    expect(semantic.entities.map((entity) => entity.path)).not.toContain(
      "src/SecondHop.php"
    )

    const full = explorer.neighborhood("src/HttpClient.php", "both", 2, "full")
    expect(full.presentation).toBe("neighborhood")
    expect(full.entities.map((entity) => entity.path)).toContain(
      "src/SecondHop.php"
    )
  })

  it("collapses redundant entry chains into a selectable package containing useful children", () => {
    const report = makeReport()
    report.files = [
      file("paas/libraries/common/composer.json", "JSON"),
      file("paas/libraries/common/package.json", "JSON"),
      file("paas/libraries/common/src/php/HttpClient.php", "PHP"),
      file("paas/libraries/common/test/HttpClientTest.php", "PHP"),
    ]
    const graph: DependencyGraph = {
      languages: ["PHP"],
      nodes: 2,
      edges: 0,
      files: report.files
        .filter((entry) => entry.language !== "JSON")
        .map((entry) => ({
          path: entry.path,
          language: entry.language,
          fan_in: 0,
          fan_out: 0,
        })),
      edge_list: [],
      cycles: [],
      orphans: [],
      top_depended: [],
      most_dependent: [],
      unresolved_imports: 0,
      config_files: [
        "paas/libraries/common/composer.json",
        "paas/libraries/common/package.json",
      ],
    }

    const root = buildRepositoryGraphExplorer(graph, report).view("")

    expect(root.groups?.map((group) => group.path)).toEqual([
      "paas/libraries/common",
    ])
    expect(root.groups?.[0].label).toBe("Package")
    expect(root.groups?.[0].members.map((member) => member.path)).toEqual([
      "paas/libraries/common/src",
      "paas/libraries/common/test",
      "paas/libraries/common/composer.json",
      "paas/libraries/common/package.json",
    ])
    expect(root.entities.some((entity) => entity.path === "paas")).toBe(false)
  })
})
