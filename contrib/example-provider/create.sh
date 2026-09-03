#!/usr/bin/env bash
#
# worktree_create_cmd for the example provider.
#
# Reads BORK_ISSUE_ID / BORK_SLUG / BORK_BASE / BORK_TARGET / BORK_BRANCH,
# creates (or re-attaches) a worktree under the pool root, and prints it as a
# JSON object on the last line of stdout.
#
# BORK_TARGET, when set, is a slot letter: a persistent worktree whose branch is
# swapped in place. A real provider does this to skip whatever a fresh worktree
# spends minutes provisioning; here it only demonstrates that a targeted create
# reuses a worktree and costs no pool index.
set -euo pipefail

source "$(dirname "$0")/pool.sh"

[[ -n ${BORK_ISSUE_ID:-} ]] || { echo "BORK_ISSUE_ID is not set" >&2; exit 1; }

main="$(main_repo)"
registry_init

# BORK_BRANCH is an existing branch the card already refers to — a pull
# request's head branch, say — and is used verbatim. Otherwise the branch name
# is composed from the issue id and slug.
if [[ -n ${BORK_BRANCH:-} ]]; then
  branch="$BORK_BRANCH"
else
  branch="$BORK_ISSUE_ID"
  [[ -n ${BORK_SLUG:-} ]] && branch="$BORK_ISSUE_ID-$BORK_SLUG"
fi

base="${BORK_BASE:-HEAD}"

branch_exists() {
  git -C "$main" show-ref --verify --quiet "refs/heads/$1"
}

if [[ -n ${BORK_TARGET:-} ]]; then
  if [[ ! $BORK_TARGET =~ ^[a-z]$ ]]; then
    echo "BORK_TARGET must be a single a-z slot letter, got '$BORK_TARGET'" >&2
    exit 1
  fi
  name="wt--pool-slot-$BORK_TARGET"
else
  name="wt--pool-$(sanitise "$branch")"
fi
path="$POOL_ROOT/$name"

if [[ -d $path ]]; then
  # Swap the branch in place. Cheap, and the reason a card is pointed at a slot
  # rather than given a worktree of its own.
  echo "Reusing $name" >&2
  if branch_exists "$branch"; then
    git -C "$path" checkout "$branch" >&2
  else
    git -C "$path" checkout -b "$branch" "$base" >&2
  fi
else
  index="$(registry_claim "$name")"
  echo "Allocated index $index (ports :$(port_for "$index")+)" >&2
  if branch_exists "$branch"; then
    git -C "$main" worktree add "$path" "$branch" >&2
  else
    git -C "$main" worktree add -b "$branch" "$path" "$base" >&2
  fi
fi

# A directory that exists without a registry entry was made outside the pool;
# claim one for it rather than reporting it with no port block.
index="$(registry_index_of "$name")"
[[ -n $index ]] || index="$(registry_claim "$name")"

printf '{"path":%s,"name":%s,"label":%s}\n' \
  "$(json_string "$path")" \
  "$(json_string "$name")" \
  "$(json_string "$(label_for "$name" "$index")")"
