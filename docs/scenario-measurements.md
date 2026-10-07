# Scenario CLI measurements

Run this opt-in tool to measure the same bounded CLI request with an empty and then populated
RepoScout cache. It records full outputs, exact command arguments, elapsed time and peak RAM
without a model, external repository, dependency installation, daemon or automatic benchmark.
The [development scenarios](development-scenarios.md) and their evidence budgets remain separate.

```sh
cargo build --release --locked
python3 scripts/measure-scenarios.py --repeats 3 --encoding o200k_base
python3 scripts/measure-scenarios.py --encoding cl100k_base --output /tmp/my-new-measurement-directory
```

Linux with kernel pidfd support (5.3+) and Python 3.9+ is required. The tool uses the already-built
`target/release/reposcout`; `--binary PATH` selects another explicitly trusted binary. It does not
build or download a binary. Record build commands, toolchain and enabled features alongside results
when comparing binaries. The binary hash/version and current checkout/configuration hashes are
recorded; the current checkout HEAD does not prove which sources produced a custom or stale binary.

`--output` must name a directory that does not exist, with no symlink parents. Without it, the tool
creates a fresh retained directory under `/tmp` and prints its exact path. `results.json`, each
query's `.stdout`, `.stderr`, `.argv` and `.process.json`, plus version/tokenization transcripts stay
there even after a query fails. An incomplete run exits nonzero and retains `complete: false`.
Temporary fixture/cache/token inputs are separate task-created directories, cleaned up on success,
failure or interrupt. No existing caches or output directories are removed.

## Fixed workload and comparison

The `invoice-known-source-ranges` workload copies the four authored Python files from the
[invoice efficiency case](../tests/development_scenarios/efficiency/invoice/packet.md). Each copied
file is limited to 64 KiB. Every invocation asks `read --range` for the same five spans: the active
invoice imports, Invoice fields, `net_due`, rounding helper and production regression. It requests
JSON under the agent profile with explicit encoding and a 4,096-token / 12,288-byte response limit.
The shared [test configuration](../tests/fixtures/test-global.toml) caps workers at two;
`--no-project-config` and a child-only environment whitelist isolate developer configuration.

These are authored known-source selectors. This workload measures source delivery and cache effects;
it does not measure discovery of the active import, a complete investigation, application repair or
provider/model savings. The independent [user efficiency cases](user-efficiency-cases.md) retain
their stronger evidence and interaction requirements.

The default is three serial cold→warm pairs; `--repeats` accepts 1–10. The fixture and full argv remain
unchanged. Each pair starts with its own empty private XDG cache directory; its successful cold query
populates the cache used by the warm query. The next pair gets a fresh cache. This is **RepoScout-cache
cold**, with no claim of cold OS page caches, filesystem caches or executable pages. Nothing clears
global caches. Cache inventories retain file paths, lengths and hashes before and after each call.
Warm runs require a populated cache, but `cache_hits` remains null because the `read` response does
not expose hit counters. A warm label alone does not establish that all lookups hit.

The tool checks every returned range against the authored LF-only fixture bytes, exact byte/line
spans, content hash and worktree snapshot identity, rejects missing/omitted/incomplete targets and
compares the complete parsed response across all pairs. JSON
object key order is irrelevant; all response fields, array order, hashes and source bytes must agree.
It keeps the original raw streams without normalization. A failure remains evidence and prevents a
successful comparison summary.

## Measurement units and boundaries

Each query's `wall_seconds` uses a monotonic clock from immediately before spawning the CLI until
the exact child is reaped. It includes process startup, output capture to local files and lightweight
watchdog setup; it excludes fixture creation, cache inspection, JSON validation, compilation and the
later token pass. Version probing and tokenization are retained separately and excluded from the
cold/warm summary. Disk and watchdog overhead, host scheduling, OS caches and tokenizer initialization
can affect small samples. Medians and min/max retain this variability; there are no host-sensitive
performance acceptance gates.

`peak_rss_bytes` is **one owned CLI child's** kernel high-water RSS from
[`os.wait4(pid, 0)`](https://docs.python.org/3/library/os.html#os.wait4).
[Linux reports `ru_maxrss` in KiB](https://man7.org/linux/man-pages/man2/getrusage.2.html), so the tool
multiplies by 1,024. It includes the child's threads and entire lifetime; it is not live heap bytes,
the running Python parent's RSS, cumulative `RUSAGE_CHILDREN` or the simultaneous peak of a process tree.
The [kernel retains the old address-space peak across `exec`](https://github.com/torvalds/linux/blob/master/fs/exec.c),
so memory inherited briefly before executing the CLI can establish a startup floor. This can matter
for very small commands; the tool does not subtract that floor or claim a peak limited to post-exec work.
The fixed CLI query does not launch another RepoScout process. Linux-only support avoids silently
mixing platform RSS units.

A joined watchdog owns a pidfd for the exact launched child. It can stop only that child after
30 seconds, after the whole run exceeds 180 seconds, when sampled live RSS exceeds 1 GiB or when
combined output exceeds 1 MiB. The final kernel peak also stops subsequent work if it exceeds 1 GiB.
The 20 ms `/proc` samples serve only the resource guard and can miss short spikes; they never supply
the reported peak. Guards are development safety bounds, not performance expectations. No broad PID,
name, port or process-group termination is used.

## Complete stream and argument costs

After all queries, one separate `tokens --no-cache` call counts bounded temporary Markdown files
containing the complete UTF-8 stdout, stderr and actual argv of every query. It uses the same
explicit encoding and shared worker configuration. The token pass and its outputs are recorded
under `tokenization`, outside query timings; it has the same resource guards. Invalid UTF-8 or an
omitted token input fails accounting instead of silently counting replacement text.

`.argv` is the complete actual argument vector, including the executable path, represented as
compact UTF-8 JSON with no trailing newline. Its bytes/tokens describe that representation, not
shell syntax or the operating system's NUL-separated argv storage. The temporary absolute fixture
path may appear in actual stdout and affects its cost across independent runs; no metadata is
stripped. `response_tokens` is stdout tokens plus stderr tokens, and `interaction_tokens` adds argv
tokens. Repeated responses and arguments count on every call. These are CLI text costs, without
provider message envelopes, context retention, reasoning or billing claims.

Check the small harness independently with:

```sh
python3 -m unittest discover -s scripts -p 'test_measure_scenarios.py'
```

Nothing invokes the measurements from default tests, CI, or `test-scenarios.sh`. Run measurements
serially with other builds/scans, and retain the exact artifact path and reproduction command with
any published observation. Scenario semantic failures stay separate from noisy timing differences.

## Recorded local observation: 2026-10-07

Three pairs with `--repeats 3 --encoding o200k_base` completed on the shared Linux development host
after all four harness checks passed. The current release-profile binary identifies as 0.4.1;
its SHA-256 is `353cfb610ca61c524d207494ceffbf655ab4e7d8f5252bbee00e7f32a7dc7bcf`.
It includes the unreleased source selectors. Local raw evidence is retained at
`/tmp/reposcout-measurements-gj9kzsuo/results.json` and the adjacent transcripts, not committed.

| RepoScout cache | CLI seconds, median (min–max) | Individual peak RSS, median (min–max), MiB |
| --- | ---: | ---: |
| Cold | 0.255 (0.253–0.256) | 61.18 (61.10–61.21) |
| Warm | 0.237 (0.201–0.246) | 60.33 (60.05–60.77) |

Every response used 1,070 tokens; actual JSON argv added 126, for 1,196 interaction tokens per call.
All returned source, hashes, spans and revision identities matched. A preceding three-pair run
had cold/warm medians of 0.197/0.204 seconds, illustrating shared-host noise. These few samples
establish a bounded working measurement, not a reliable cache speedup or model-session saving.
