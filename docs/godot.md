# Godot support

← [Documentation index](README.md)

RepoScout analyzes Godot 4 projects without starting Godot, importing assets, or executing scripts.
The support uses the same scan, cache, graph, context, query, and reporting pipeline as other
languages; no engine installation or project plugin is required.

## Formats and analysis

| Format | What RepoScout provides |
|---|---|
| `.gd` — GDScript | Syntax-aware lines/comments, per-callable complexity (including lambdas and property accessors), exact/Type-2 duplication, markers, class/method/signal/constant/property outlines, and local dependency facts |
| `.gdshader`, `.gdshaderinc` — Godot Shader | Syntax-aware code metrics, function outlines, duplication, and static `#include` dependencies |
| `.tscn`, `.escn` — Godot Scene | Inventory, syntax-aware lines, scene-node outlines, and external-resource dependencies |
| `.tres`, `plugin.cfg`, `export_presets.cfg` — Godot Resource | Inventory, syntax-aware lines and dependency-bearing external-resource/plugin fields |
| `project.godot` — Godot Project | Inventory, main-scene/autoload/enabled-plugin dependencies, and project-root boundaries |
| Script/shader `.uid` sidecars | Text inventory; bounded revision-scoped UID lookup for graph-capable scripts and shaders |

Scenes, resources, project files, and UID metadata do **not** enter code-health analysis by default.
To inspect duplication or comment markers in authored content, explicitly add
`--health-include godot-scene`, `godot-resource`, or `godot-project`, or use `--health-scope all`.
Data formats never receive code complexity, even when opted into health analysis.

`locate --kind` additionally accepts `signal`, `constant`, `property`, and `node`.
Scene-node names are scene paths, not filesystem paths; their outlines contain only the node header.
GDScript public/export counts use a public-name heuristic, not access-control guarantees.

## Project and resource graph

From the repository root, `--graph`, `--context`, `--impact`, `explain`, and the daemon graph can
connect project → main scene/autoload → scene/resource → script/shader → local dependencies.
Supported static evidence includes:

- Literal `load`/`preload`, `ResourceLoader.load`/`load_threaded_request`, path-based `extends`,
  shader includes, and scene/resource `[ext_resource]` entries.
- `res://` paths anchored to the nearest `project.godot`, plus normalized file-relative paths.
  References cannot escape the scanned root or cross into a different nested Godot project.
- Checked-in `uid://` identities from text scene/resource headers and `.uid` sidecars. A unique
  UID wins over an outdated external-resource path; an unknown UID may use the explicit path
  fallback. Ambiguous UIDs remain unresolved, even when a fallback path exists.
- Unambiguous project-local `class_name` references in inheritance, type annotations and member
  access, plus enabled singleton autoload names with `*res://` paths. File-local bindings suppress
  corresponding name-based guesses conservatively. Equal names in nested projects stay separate.

Edges retain `godot-resource`, `godot-uid`, or `godot-global` provenance. Resource/UID edges are
direct static evidence; name-based edges remain heuristic in context confidence. GDScript
inheritance contributes file dependency edges, not a separate symbol-inheritance topology.
Source facts and project/UID configuration are retained per daemon revision: a graph for an older
revision never rereads newer live files. Ordinary CLI scans still do not extract graph facts.

## Small, useful agent queries

```sh
# A bounded reading plan for one script and its scene/resource neighborhood.
reposcout --agent-summary --focus scripts/player.gd .

# Find a signal declaration without dumping source bodies.
reposcout locate damaged --kind signal --exact -f json .

# Show a small reverse dependency graph for a shared script.
reposcout --graph-focus scripts/player.gd --graph-direction dependents \
  --graph-depth 2 -f mermaid .

# Inspect only the complexity summary, retaining scan diagnostics.
reposcout complexity --summary -f json scripts/ \
  | jq -c '{diagnostics, complexity: .summary.complexity}'
```

For non-Git projects, use the Godot project directory as the scan root when you need `res://`
resolution and project-wide class discovery. Scanning a single script or narrow directory cannot
discover classes elsewhere in the project. A `--focus` on a full-project scan preserves that context.

## Tests, generated files, and limits

GUT and GdUnit4 test setup is detected from their enabled plugin entries in `project.godot`.
Within that project's evidence scope, GUT uses `test_*.gd`; GdUnit4 recognizes `test_*.gd`,
`*_test.gd`, and `*Test.gd`. These are conventional test-presence hints, not execution or measured
coverage; custom runner paths/discovery rules are not interpreted.

The generated `.godot/` directory is hidden and therefore omitted by default; daemon watcher
events below it do not trigger rescans. `--hidden` explicitly broadens inventory. RepoScout's
normal Git/RepoScout ignore and exclude rules apply; `.gdignore` is an engine import policy, not
an additional RepoScout exclusion rule. Use `--health-exclude 'addons/third_party/**'` when a
vendored addon should remain navigable without affecting health signals.

This is static repository analysis, not Godot's runtime/type checker. Binary `.scn`/`.res`, imported
assets, embedded shader/script source inside resources, GDExtension native-library loading,
runtime-generated paths, signal dispatch, engine-native classes, and complete C# semantics are
not reconstructed. Binary asset and generic C# references are outside the graph's analyzable
universe; their existence is not validated. Dynamic or undecodable resource paths and ambiguous
known classes/UIDs remain unresolved; native or otherwise unknown global names are not indexed.
Run Godot's own checks and the project's tests for authoritative correctness.

Format references: [Godot TSCN specification](https://docs.godotengine.org/en/stable/engine_details/file_formats/tscn.html),
[Godot UID sidecars](https://godotengine.org/article/uid-changes-coming-to-godot-4-4/),
[GDScript language reference](https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_basics.html).
