#!/usr/bin/env bash
#
# worktree_targets_cmd for the example provider.
#
# Prints one JSON object per slot that exists:
# {"target": "<letter>", "label": "<current branch>"}.
#
# Only existing slots are offered. Listing a-z would hide slot creation behind a
# menu pick, which for a real provider is the expensive path the targets exist
# to avoid.
set -euo pipefail

source "$(dirname "$0")/pool.sh"

registry_init

shopt -s nullglob
for path in "$POOL_ROOT"/wt--pool-slot-?; do
  name="${path##*/}"
  [[ $name =~ ^wt--pool-slot-([a-z])$ ]] || continue
  letter="${BASH_REMATCH[1]}"

  if branch="$(git -C "$path" symbolic-ref --short --quiet HEAD 2>/dev/null)"; then
    label="$branch"
  else
    label="(detached)"
  fi

  printf '{"target":%s,"label":%s}\n' \
    "$(json_string "$letter")" "$(json_string "$label")"
done | sort
