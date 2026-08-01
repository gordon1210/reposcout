#!/bin/sh
set -eu

repo_root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
canonical="$repo_root/skills/reposcout"
mirror="$repo_root/.agents/skills/reposcout"
skill_lock="$repo_root/skills-lock.json"

usage() {
  echo "usage: $0 sync|check" >&2
  exit 2
}

assert_file() {
  if [ ! -f "$canonical/$1" ]; then
    echo "missing canonical skill file: $1" >&2
    exit 1
  fi
}

assert_route() {
  label=$1
  expected=$2
  row=$(grep -F "| $label |" "$canonical/SKILL.md" || true)
  count=$(printf '%s\n' "$row" | grep -c . || true)
  if [ "$count" -ne 1 ]; then
    echo "expected exactly one routing row for: $label" >&2
    exit 1
  fi
  references=$(printf '%s\n' "$row" | grep -o 'references/[a-z-]*\.md' || true)
  if [ "$references" != "references/$expected" ]; then
    echo "routing row '$label' must point only to references/$expected" >&2
    exit 1
  fi

  reference_count=$(grep -F -o "references/$expected" "$canonical/SKILL.md" | wc -l | tr -d ' ')
  if [ "$reference_count" -ne 1 ]; then
    echo "references/$expected must appear exactly once in the core skill" >&2
    exit 1
  fi
}

check_canonical() {
  assert_file SKILL.md
  assert_file agents/openai.yaml
  assert_file references/scouting.md
  assert_file references/context-planning.md
  assert_file references/change-analysis.md
  assert_file references/quality.md
  assert_file references/diagnostics.md

  assert_route "Repository scouting" scouting.md
  assert_route "Context planning" context-planning.md
  assert_route "Change analysis" change-analysis.md
  assert_route "Quality assessment" quality.md
  assert_route "Diagnostics and configuration" diagnostics.md

  reference_count=$(grep -o 'references/[a-z-]*\.md' "$canonical/SKILL.md" | wc -l | tr -d ' ')
  if [ "$reference_count" -ne 5 ]; then
    echo "the core skill must contain exactly five routed reference links" >&2
    exit 1
  fi

  if grep -Eq '^[[:space:]]*"reposcout"[[:space:]]*:' "$skill_lock"; then
    echo "reposcout must not be managed by skills-lock.json; the repository owns its mirror" >&2
    exit 1
  fi
}

mode=${1:-}
case "$mode" in
  sync)
    check_canonical
    rm -rf "$mirror"
    mkdir -p "$(dirname "$mirror")"
    cp -R "$canonical" "$mirror"
    ;;
  check)
    check_canonical
    if [ ! -d "$mirror" ]; then
      echo "RepoScout skill mirror is missing; run scripts/reposcout-skill.sh sync" >&2
      exit 1
    fi
    if ! diff -ru "$canonical" "$mirror"; then
      echo "RepoScout skill mirror is stale; run scripts/reposcout-skill.sh sync" >&2
      exit 1
    fi
    ;;
  *)
    usage
    ;;
esac
