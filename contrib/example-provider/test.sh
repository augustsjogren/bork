#!/usr/bin/env bash
#
# End-to-end round trip through the three provider commands, against a
# throwaway repo. Doubles as the executable spec for the seam's contract: what
# each command reads and what it must print.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Resolved: on macOS mktemp hands back /var/... while git reports the
# /private/var/... realpath, which would make the path assertions fail.
tmp="$(cd "$(mktemp -d)" && pwd -P)"
trap 'rm -rf "$tmp"' EXIT

failures=0

check() {
  local name=$1 expected=$2 actual=$3
  if [[ $expected == "$actual" ]]; then
    echo "ok - $name"
  else
    echo "FAIL - $name"
    echo "  expected: $(printf %q "$expected")"
    echo "  actual:   $(printf %q "$actual")"
    failures=$((failures + 1))
  fi
}

main="$tmp/project/main"
mkdir -p "$main"
git -C "$main" init -q -b main
git -C "$main" -c user.email=t@t -c user.name=T commit -q --allow-empty -m init

pool="$tmp/pool"
export BORK_EXAMPLE_POOL_ROOT="$pool"
export BORK_EXAMPLE_POOL_SIZE=2

# Every provider command runs with the main checkout as its working directory.
run() {
  local script=$1
  shift
  (cd "$main" && env "$@" "$here/$script")
}

# --- create.sh: a worktree of its own ------------------------------------

created="$(run create.sh BORK_ISSUE_ID=bork-1 BORK_SLUG=fix-bug 2>/dev/null)"

check "create.sh prints one JSON object" \
  '{"path":"'"$pool"'/wt--pool-bork-1-fix-bug","name":"wt--pool-bork-1-fix-bug"}' \
  "$created"
check "create.sh prints only the object on stdout" 1 \
  "$(printf '%s\n' "$created" | wc -l | tr -d ' ')"
check "create.sh checks out the composed branch" "bork-1-fix-bug" \
  "$(git -C "$pool/wt--pool-bork-1-fix-bug" symbolic-ref --short HEAD)"

# --- the pool ceiling -----------------------------------------------------

run create.sh BORK_ISSUE_ID=bork-2 >/dev/null 2>&1

if run create.sh BORK_ISSUE_ID=bork-4 >/dev/null 2>&1; then
  echo "FAIL - create.sh must refuse once the pool is full"
  failures=$((failures + 1))
else
  echo "ok - create.sh refuses once the pool is full"
fi

# --- list.sh --------------------------------------------------------------

check "list.sh reports both worktrees" \
  '{"path":"'"$pool"'/wt--pool-bork-1-fix-bug","name":"wt--pool-bork-1-fix-bug"}
{"path":"'"$pool"'/wt--pool-bork-2","name":"wt--pool-bork-2"}' \
  "$(run list.sh)"

check "list.sh resolves the main repo from inside a worktree" \
  "$(run list.sh)" "$(cd "$pool/wt--pool-bork-2" && "$here/list.sh")"

# --- remove.sh: refuses to discard uncommitted work -----------------------

echo scratch >"$pool/wt--pool-bork-2/scratch.txt"

if run remove.sh BORK_WORKTREE_NAME=wt--pool-bork-2 >/dev/null 2>&1; then
  echo "FAIL - remove.sh must refuse a dirty worktree without force"
  failures=$((failures + 1))
else
  echo "ok - remove.sh refuses a dirty worktree without force"
fi

run remove.sh BORK_WORKTREE_NAME=wt--pool-bork-2 BORK_FORCE=1 >/dev/null 2>&1
check "force removes a dirty worktree" "no" \
  "$([[ -d $pool/wt--pool-bork-2 ]] && echo yes || echo no)"

# --- remove.sh: a worktree of its own frees its index ---------------------

run remove.sh BORK_WORKTREE_NAME=wt--pool-bork-1-fix-bug >/dev/null 2>&1

check "removing a worktree deletes it" "no" \
  "$([[ -d $pool/wt--pool-bork-1-fix-bug ]] && echo yes || echo no)"
check "removing a worktree frees its index" 0 "$(run list.sh | wc -l | tr -d ' ')"

# The freed index is reusable, which is the whole point of releasing it.
check "the freed index is handed to the next create" \
  '{"path":"'"$pool"'/wt--pool-bork-5","name":"wt--pool-bork-5"}' \
  "$(run create.sh BORK_ISSUE_ID=bork-5 2>/dev/null)"

# --- remove.sh: idempotent -----------------------------------------------

run remove.sh BORK_WORKTREE_NAME=wt--pool-bork-5 >/dev/null 2>&1
run remove.sh BORK_WORKTREE_NAME=wt--pool-bork-5 >/dev/null 2>&1
check "removing an absent worktree is not an error" 0 \
  "$(run list.sh | wc -l | tr -d ' ')"

echo
if ((failures > 0)); then
  echo "$failures check(s) failed"
  exit 1
fi
echo "all checks passed"
