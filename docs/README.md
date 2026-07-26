# RepoScout documentation

RepoScout is intentionally simple to start: install one binary and run `reposcout`.
The guides below explain the deeper workflows without turning the project README into
a reference manual.

| Guide | Use it when you want to… |
|---|---|
| [Getting started](getting-started.md) | Install RepoScout, run the first scan, and understand its defaults |
| [CLI reference](cli-reference.md) | Find commands, flags, examples, gates, and debugging options |
| [Agent workflows](agent-workflows.md) | Produce compact scouting, context, impact, review, and symbol-query results |
| [Metrics and interpretation](metrics.md) | Understand complexity, duplication, risk, test matching, and analysis limits |
| [Configuration and caching](configuration.md) | Set personal or project defaults and inspect what RepoScout resolved |
| [Reports and machine formats](report-formats.md) | Consume JSON, NDJSON, SARIF, Markdown, DOT, or Mermaid output |
| [Daemon and web dashboard](daemon-and-web.md) | Run the local watcher and explore live results in the browser |
| [Development](development.md) | Build, test, and work on RepoScout itself |

## Useful project links

- [README](../README.md) — concise product overview and quick start
- [Contributing](../CONTRIBUTING.md) — development expectations and pull-request guidance
- [Security policy](../SECURITY.md) — supported versions and private vulnerability reporting
- [Changelog](../CHANGELOG.md) — released and upcoming user-visible changes
- [Roadmap](../ROADMAP.md) — product direction, delivered work, and explicit non-goals
- [License](../LICENSE) — MIT
- [Third-party notices](../THIRD_PARTY_NOTICES.md) — bundled and adapted material

## Discover the installed contract

Agents and integrations should prefer the binary's own capability report over assumptions:

```sh
reposcout capabilities -f json
```

It describes the installed commands, formats, execution profiles, supported languages,
health scopes, and hard analysis bounds without scanning a repository.
