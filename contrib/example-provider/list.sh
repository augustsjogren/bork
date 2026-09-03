#!/usr/bin/env bash
#
# worktree_list_cmd for the example provider.
#
# Prints one JSON object per worktree holding a pool index:
# {"name": ..., "path": ...}.
#
# Index-holding is the thing that matters, not what `git worktree list` reports:
# a worktree created outside the pool shows up in git with no index, no ports
# and none of what a provider provisions, and counting it would refuse attaches
# the pool could serve.
set -euo pipefail

source "$(dirname "$0")/pool.sh"

registry_init

index=-1
while IFS= read -r name; do
  index=$((index + 1))
  [[ -n $name ]] || continue
  path="$POOL_ROOT/$name"
  [[ -d $path ]] || continue
  printf '{"path":%s,"name":%s}\n' \
    "$(json_string "$path")" \
    "$(json_string "$name")"
done <"$REGISTRY"
