#!/usr/bin/env bash
#
# worktree_create_cmd for the example provider.
#
# Reads BORK_ISSUE_ID / BORK_SLUG / BORK_BASE / BORK_BRANCH, creates a
# worktree under the pool root, and prints it as a JSON object on the last
# line of stdout.
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

name="wt--pool-$(sanitise "$branch")"
path="$POOL_ROOT/$name"

index="$(registry_claim "$name")"
echo "Allocated index $index (ports :$(port_for "$index")+)" >&2
if branch_exists "$branch"; then
  git -C "$main" worktree add "$path" "$branch" >&2
else
  git -C "$main" worktree add -b "$branch" "$path" "$base" >&2
fi

printf '{"path":%s,"name":%s}\n' \
  "$(json_string "$path")" \
  "$(json_string "$name")"
