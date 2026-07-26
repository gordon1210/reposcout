# Configuration and caching

← [Documentation index](README.md)

RepoScout layers built-in defaults, personal preferences, repository policy, and one-off CLI
overrides without hiding which source won.

## Precedence

From weakest to strongest:

1. built-in defaults;
2. the global configuration file;
3. the nearest project `reposcout.toml` or `.reposcout.toml`; and
4. command-line flags.

The global path follows the operating system:

- macOS: `~/Library/Application Support/reposcout/reposcout.toml`
- Linux: `${XDG_CONFIG_HOME:-~/.config}/reposcout/reposcout.toml`

Set `REPOSCOUT_GLOBAL_CONFIG` to an explicit path for hermetic automation and tests. Missing files
are normal; invalid or unknown keys fail with the source path and setting name.

Only defined fields override the lower layer. Nested `[context]` fields merge independently.
Arrays replace lower-layer arrays, while repeated CLI `--exclude` and `--health-include` values
extend the effective lists.

## Inspect the effective configuration

```sh
reposcout config .
reposcout config -f json path/to/subdirectory
```

The report includes:

- candidate global and project paths;
- whether each file was loaded, missing, or deliberately ignored;
- keys defined by each layer;
- precedence; and
- every effective file-configurable value.

Use `--no-project-config` when repository-owned policy should not be parsed or applied.

## Example `reposcout.toml`

```toml
encoding = "o200k_base"            # or "cl100k_base"
jobs = 8                            # worker threads
use_cache = true
top = 10                            # top-N projection length
max_complexity = 20                 # per-callable cyclomatic maximum

include_hidden = false
respect_gitignore = true
exclude_lockfiles = true
excludes = ["vendor/**", "*.min.js"]

markers = ["TODO", "FIXME", "HACK", "XXX", "BUG"]
health_scope = "source"             # source or all
health_includes = []                # html, css, scss, json, yaml, toml, markdown, xml, text

min_dup_tokens = 50
min_dup_lines = 3
near_dup_min_similarity = 0.85
duplication_mode = "mild"           # strict, mild, weak
duplication_format_scope = "exact"  # exact, compatible, all
duplication_report_snippets = false

churn_max_commits = 5000            # 0 means unlimited

[context]
enabled = false
budget = 32000
max_files = 25
```

## Execution profiles

Profiles change operational defaults while keeping every effective decision visible in the report.

### `full`

Runs the configured analyzer set. This is the default for human use and complete reports.

### `agent`

Disables whole-corpus duplication and Git churn unless an analyzer subcommand or `--only`
explicitly requests them:

```sh
reposcout -f json --summary --profile agent .
```

### `safe`

Starts from the agent profile and additionally:

- ignores the project configuration file;
- caps workers, top lists, Git history, context, and duplication work;
- requires normal ignore handling;
- excludes hidden files and lockfiles; and
- forces source-only health analysis without content includes.

```sh
reposcout -f json --summary --profile safe .
```

An explicit analyzer request may opt back into that analyzer under the safe limits. The
`execution.safety_limits` and `analysis_profile` blocks disclose the result.

## Discovery policy

### `.gitignore`

Normal scans respect Git ignore rules. `--no-ignore` disables them for one invocation.

### `.reposcoutignore`

RepoScout also reads hierarchical `.reposcoutignore` files using Git-ignore syntax. These rules
remain active even with `--no-ignore`, making them suitable for generated or vendored trees that
should never enter scouting.

### Lockfiles

Recognized dependency lockfiles are skipped by default to keep scans focused. Use
`--include-lockfiles` when their inventory or health facts are required.

## Cache location and behavior

RepoScout never writes analysis state into the scanned repository. Per-file reports and Git-history
events live under the operating system cache directory, for example:

- macOS: `~/Library/Caches/reposcout/`
- Linux: `~/.cache/reposcout/`

Per-file entries are keyed by canonical scan root, content hash, analyzer version, token encoding,
and every runtime setting that changes file facts. Subpath and diff scans merge refreshed entries;
only a complete root scan prunes files that disappeared.

Declaration outlines enrich lazily for context and symbol lookup. Graph source facts—import
specifiers, parse diagnostics, and type-relation declarations/references—enrich lazily for graph,
context, impact, and explain. Ordinary scans and daemon refreshes do not pay those query-only AST
traversals.

Git churn uses a separate cache of immutable per-commit path changes plus exact result views.
Unchanged history needs no new traversal; new commits reuse already indexed events.

Execution telemetry distinguishes:

- `cache_hits`;
- `cache_misses`;
- `cache_enrichments`; and
- `graph_fact_files`.

## Cache maintenance

Disable caching once:

```sh
reposcout --no-cache .
```

Clear one canonical repository cache:

```sh
reposcout cache clear .
reposcout cache clear path/inside/repository
```

Clear every RepoScout cache:

```sh
reposcout cache clear --all
```

Both forms are idempotent and report what was removed. Avoid clearing a repository cache while a
scan or daemon for that repository is actively writing it, because the running process may recreate
the cache immediately.
