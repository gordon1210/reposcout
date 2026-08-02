#!/usr/bin/env bash

set -euo pipefail

# This check is deliberately advisory: oversized files emit warnings, while
# malformed configuration and operational failures still fail the command.
readonly DEFAULT_RUST_FILE_LINE_WARNING_THRESHOLD=1200
readonly MAX_RUST_FILE_LINE_WARNING_THRESHOLD=999999999

warning_threshold="${RUST_FILE_LINE_WARNING_THRESHOLD:-$DEFAULT_RUST_FILE_LINE_WARNING_THRESHOLD}"

if [[ ! "$warning_threshold" =~ ^[1-9][0-9]*$ ]] ||
  ((${#warning_threshold} > ${#MAX_RUST_FILE_LINE_WARNING_THRESHOLD})); then
  printf 'error: RUST_FILE_LINE_WARNING_THRESHOLD must be an integer from 1 through %s\n' \
    "$MAX_RUST_FILE_LINE_WARNING_THRESHOLD" >&2
  exit 2
fi

readonly warning_threshold

script_directory="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repository_root="$(cd -- "$script_directory/.." && pwd -P)"
readonly script_directory repository_root

if ! git -C "$repository_root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "error: check-rust-file-size.sh must live inside a Git repository" >&2
  exit 2
fi

check_rust_files() {
  local warning_count=0
  local file file_path line_count message annotation_file display_file

  while IFS= read -r -d '' file; do
    file_path="$repository_root/$file"

    # A tracked file can be absent in a working tree because it was deleted or
    # omitted by sparse checkout. Neither case has current contents to measure.
    if [[ ! -f "$file_path" ]]; then
      continue
    fi

    # awk counts the final physical line even when it has no trailing newline.
    line_count="$(awk 'END { print NR }' "$file_path")"

    if ((line_count < warning_threshold)); then
      continue
    fi

    warning_count=$((warning_count + 1))
    message="$line_count physical lines (advisory threshold: $warning_threshold); review whether this file should be split along module boundaries"

    if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
      # GitHub workflow-command properties require additional escaping beyond
      # the command payload. Escape '%' first to avoid double-encoding.
      annotation_file="$file"
      annotation_file=${annotation_file//'%'/'%25'}
      annotation_file=${annotation_file//$'\r'/'%0D'}
      annotation_file=${annotation_file//$'\n'/'%0A'}
      annotation_file=${annotation_file//':'/'%3A'}
      annotation_file=${annotation_file//','/'%2C'}
      printf '::warning file=%s,title=Oversized Rust source::%s\n' \
        "$annotation_file" "$message"
    else
      # %q prevents unusual Git filenames from writing terminal control data.
      printf -v display_file '%q' "$file"
      printf 'warning: %s has %s\n' "$display_file" "$message"
    fi
  done

  if ((warning_count == 0)); then
    printf 'Rust source-size advisory: no files reached %s physical lines.\n' \
      "$warning_threshold"
  else
    printf 'Rust source-size advisory: %s file(s) reached %s physical lines.\n' \
      "$warning_count" "$warning_threshold"
  fi
}

# Keeping the producer in the pipeline makes pipefail propagate a Git error;
# process substitution would silently turn that failure into an empty file set.
git -C "$repository_root" ls-files -z --cached --others --exclude-standard -- '*.rs' |
  check_rust_files
