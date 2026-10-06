#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'HELP'
Usage: scripts/test-scenarios.sh [--list] [--keep-failed] [FILTER]

Build the release CLI and run the opt-in development scenarios serially.
FILTER selects a test name or domain (inventory, navigation, revisions, boundaries, journeys, acceptance, efficiency).
--list         List scenarios without running them (compiles the test target).
--keep-failed  Retain synthetic repositories/caches when a scenario fails.
HELP
}

list=false
filter=
while (($#)); do
  case "$1" in
    --help|-h) usage; exit 0 ;;
    --list) list=true ;;
    --keep-failed) export REPOSCOUT_SCENARIO_KEEP_FAILED=1 ;;
    -*) usage >&2; exit 2 ;;
    *)
      if [[ -n "$filter" ]]; then usage >&2; exit 2; fi
      filter=$1
      ;;
  esac
  shift
done

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
unset CARGO_BIN_EXE_reposcout
cargo build --release --locked
listing=$(cargo test --release --locked --test development_scenarios "$filter" -- --ignored --list)
if [[ "$listing" != *": test"* ]]; then
  printf 'No development scenarios matched %q on this platform.\n' "$filter" >&2
  exit 2
fi
if "$list"; then
  printf '%s\n' "$listing"
  exit 0
fi
cargo test --release --locked --test development_scenarios "$filter" -- --ignored --nocapture
