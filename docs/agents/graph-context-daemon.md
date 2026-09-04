# Agent graph, context, and daemon contracts

This is a normative extension of the root [`AGENTS.md`](../../AGENTS.md). Read it completely before
changing graph resolution, impact, context planning, daemon graph generation, or dashboard graph
navigation. The root instructions remain in force.

## Dependency graph and impact

The dependency graph and impact analysis cover every first-class language heuristically.
`graph.rs` resolves:

- relative imports;
- JSONC `tsconfig.json` / `jsconfig.json` `baseUrl` and `paths`, following local project references
  and relative extends with cycle protection;
- deterministic local `package.json` exports, imports, entrypoints, and subpaths;
- JavaScript-runtime extensions to checked-in TypeScript and the fallback `@/` convention;
- relative and unambiguous repository-absolute or `src`-root Python imports;
- PHP namespaces through nearest Composer `autoload` / `autoload-dev` PSR-4 and PSR-0 mappings,
  conventional PHP source roots, and static include/require expressions;
- Rust external `mod` / `#[path]`, local `use`, and Cargo library paths;
- Go module and relative package imports from `go.mod`.

Go package imports target a deterministic representative file. This is package-level evidence, not
a claim of an exact file reference.

Full graph output contains deterministic adjacency and edge records with resolver provenance.
Focus, direction, and depth project a bounded subgraph without rebuilding topology. Keep unresolved
specifiers, syntax errors, invalid/ambiguous resolver configs, and unmatched focus paths as
diagnostics. Cycles use iterative Kosaraju SCC; orphans are files with `fan_in == 0` that are neither
entrypoints nor tests.

Symbol topology records only explicit syntax-proven `extends`, `implements`, trait, and embedding
relations. Qualified, same-file/scope, and globally unique short names may resolve; ambiguous names
stay unresolved. Keep symbol edges separate from import adjacency so fan-in and type reach retain
honest meanings.

Under diff scope, `--impact` reports changed graph files plus direct and transitive unchanged
importers with conservative `high`, `partial`, or `none` confidence.

## Context planning

The context plan is a bounded ranking, not a source pack. `context.rs` ranks existing `FileReport`
facts under hard aggregate-token and file-count limits.

- Explicit focus paths add same-directory siblings, direct dependencies, direct/transitive
  first-class-language dependents, and matching tests. Diff scopes automatically seed changed
  paths.
- Explicit focus and changed paths carry structured high-confidence distance-zero evidence.
  Direct configured or syntax evidence is distinct from heuristic/transitive confidence. Support
  files, entrypoints, graph centrality, risk, churn, and complexity are general ranking signals.
- Selected first-class files receive cached body-free declaration headers under separate bounds:
  16 symbols or 2 KiB per file, at most four private declarations, and 16 KiB total. Outlines do
  not enter ordinary `files[]`.
- Focus resolves against repository root and a nested scan target and reports misses or ambiguity.
  An oversized explicit focus/change seed may survive as `outline_only` without source content.
- Generated, minified, and vendored files are skipped unless focused or changed.
- The planner performs no source or network I/O. Keep it deterministic and honest when graph
  coverage is absent.
- Deleted diff paths may be virtual graph seeds without a `FileReport`.
  `planning_diagnostics` describes the separate full-tree universe while top-level diagnostics
  remain scoped to the primary scan.

## On-demand daemon graph

Normal watched scans keep `cfg.graph = false` and never build topology. They deliberately capture
and cache per-file graph source facts plus bounded resolver-config contents as immutable inputs of
the completed report revision. Opening the dashboard Graph tab calls `GET /api/graph?revision=N`,
which builds topology only from those revision-scoped inputs away from the async runtime and caches
one graph per report revision. Do not reread mutable live sources for an older revision or move
topology construction into every daemon scan.

The frontend creates deterministic mixed-language architecture scopes and performs bounded local
file-neighborhood projection:

- Collapse redundant single-child scope chains into selectable parent groups whose useful
  immediate children are visible. Never create empty intermediate drill-down screens.
- Single click highlights immediate relations; double click enters a child scope or file
  neighborhood.
- For files with syntax-proven symbol relations, the default neighborhood is a browser-side
  semantic projection: keep the focus type central, group incoming/outgoing
  `extends`/`implements`/embedding members separately, and include only bounded direct import
  context.
- Remain language-agnostic over `symbol_edges`, preserve visible and total counts, and provide an
  explicit route back to unrestricted direction/depth neighborhoods.
- Explicit type reach takes precedence over import fan-in when sizing prominent type-bearing files.
- Keep navigation URL-addressable: `/graph/scope/...` for architecture scopes and
  `/graph/file/...` for files. Canonical query parameters retain non-default presentation,
  direction, and depth. Preserve breadcrumbs and browser Back/Forward; transient single-click
  selection must not create history entries.
- Custom React Flow nodes retain explicit dimensions so `onlyRenderVisibleElements`, initial
  fitting, and the minimap work before DOM measurement. Dense views use readable minimum zoom,
  subdued idle edges, and a high-contrast minimap.
- The browser renders at most 100 graph nodes. Preserve that bound.
