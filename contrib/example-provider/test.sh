#!/usr/bin/env bash
#
# End-to-end round trip through the four provider commands, against a throwaway
# repo. Doubles as the executable spec for the seam's contract: what each
# command reads, what it must print, and what a targeted create costs.
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
  '{"path":"'"$pool"'/wt--pool-bork-1-fix-bug","name":"wt--pool-bork-1-fix-bug","label":"wt 0 · :8000"}' \
  "$created"
check "create.sh prints only the object on stdout" 1 \
  "$(printf '%s\n' "$created" | wc -l | tr -d ' ')"
check "create.sh checks out the composed branch" "bork-1-fix-bug" \
  "$(git -C "$pool/wt--pool-bork-1-fix-bug" symbolic-ref --short HEAD)"

# --- create.sh: a slot ----------------------------------------------------

slot="$(run create.sh BORK_ISSUE_ID=bork-2 BORK_TARGET=a 2>/dev/null)"

check "a targeted create lands on the slot" \
  '{"path":"'"$pool"'/wt--pool-slot-a","name":"wt--pool-slot-a","label":"slot a · :8010"}' \
  "$slot"

check "targets.sh reports the slot and its branch" \
  '{"target":"a","label":"bork-2"}' "$(run targets.sh)"

# --- create.sh: reusing a slot costs no index -----------------------------

# The pool holds two, and both are taken. A targeted create must still succeed:
# it swaps the branch of a worktree that already holds its index.
run create.sh BORK_ISSUE_ID=bork-3 BORK_TARGET=a >/dev/null 2>&1

check "reusing a slot swaps its branch" "bork-3" \
  "$(git -C "$pool/wt--pool-slot-a" symbolic-ref --short HEAD)"
check "reusing a slot claims no second index" 2 "$(run list.sh | wc -l | tr -d ' ')"

# --- the pool ceiling -----------------------------------------------------

if run create.sh BORK_ISSUE_ID=bork-4 >/dev/null 2>&1; then
  echo "FAIL - create.sh must refuse once the pool is full"
  failures=$((failures + 1))
else
  echo "ok - create.sh refuses once the pool is full"
fi

# --- list.sh --------------------------------------------------------------

check "list.sh reports both worktrees with their port blocks" \
  '{"path":"'"$pool"'/wt--pool-bork-1-fix-bug","name":"wt--pool-bork-1-fix-bug","label":"wt 0 · :8000"}
{"path":"'"$pool"'/wt--pool-slot-a","name":"wt--pool-slot-a","label":"slot a · :8010"}' \
  "$(run list.sh)"

check "list.sh resolves the main repo from inside a worktree" \
  "$(run list.sh)" "$(cd "$pool/wt--pool-slot-a" && "$here/list.sh")"

# --- remove.sh: a slot is detached, not destroyed -------------------------

run remove.sh BORK_WORKTREE_NAME=wt--pool-slot-a >/dev/null 2>&1

check "detaching a slot keeps it" "yes" \
  "$([[ -d $pool/wt--pool-slot-a ]] && echo yes || echo no)"
check "detaching a slot keeps its index" 2 "$(run list.sh | wc -l | tr -d ' ')"
check "a detached slot is still a target" '{"target":"a","label":"(detached)"}' \
  "$(run targets.sh)"
check "detaching a slot keeps its branch" "bork-3" \
  "$(git -C "$main" branch --list bork-3 --format='%(refname:short)')"

# --- remove.sh: refuses to discard uncommitted work -----------------------

git -C "$pool/wt--pool-slot-a" checkout -q -b dirty-branch
echo scratch >"$pool/wt--pool-slot-a/scratch.txt"

if run remove.sh BORK_WORKTREE_NAME=wt--pool-slot-a >/dev/null 2>&1; then
  echo "FAIL - remove.sh must refuse a dirty slot without force"
  failures=$((failures + 1))
else
  echo "ok - remove.sh refuses a dirty slot without force"
fi

run remove.sh BORK_WORKTREE_NAME=wt--pool-slot-a BORK_FORCE=1 >/dev/null 2>&1
check "force detaches a dirty slot" "" \
  "$(git -C "$pool/wt--pool-slot-a" symbolic-ref --short --quiet HEAD || true)"

# --- remove.sh: a worktree of its own frees its index ---------------------

run remove.sh BORK_WORKTREE_NAME=wt--pool-bork-1-fix-bug >/dev/null 2>&1

check "removing a worktree deletes it" "no" \
  "$([[ -d $pool/wt--pool-bork-1-fix-bug ]] && echo yes || echo no)"
check "removing a worktree frees its index" 1 "$(run list.sh | wc -l | tr -d ' ')"

# The freed index is reusable, which is the whole point of releasing it.
check "the freed index is handed to the next create" \
  '{"path":"'"$pool"'/wt--pool-bork-5","name":"wt--pool-bork-5","label":"wt 0 · :8000"}' \
  "$(run create.sh BORK_ISSUE_ID=bork-5 2>/dev/null)"

# --- remove.sh: idempotent -----------------------------------------------

run remove.sh BORK_WORKTREE_NAME=wt--pool-bork-5 >/dev/null 2>&1
run remove.sh BORK_WORKTREE_NAME=wt--pool-bork-5 >/dev/null 2>&1
check "removing an absent worktree is not an error" 1 \
  "$(run list.sh | wc -l | tr -d ' ')"

echo
if ((failures > 0)); then
  echo "$failures check(s) failed"
  exit 1
fi
echo "all checks passed"
