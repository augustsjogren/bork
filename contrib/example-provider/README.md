# Example worktree provider

A working, self-contained provider for `worktree_create_cmd` and friends. It is
here to document the seam and to be copied: swap the pool for whatever your
project actually provisions.

Real providers exist because `git worktree add` alone is not enough. A worktree
of a large project often needs its own port allocation, databases, containers or
search indices, and provisioning those can take minutes — long enough that a
board wants to reuse a small pool of pre-provisioned worktrees rather than
create one per card. Everything below is bork's side of that arrangement.

What this example does: it keeps a fixed pool of worktrees **outside the project
root**, each holding an index in a registry file that gives it a port block. A
real provider would allocate databases and services against that same index.

```sh
contrib/example-provider/test.sh
```

The test needs nothing but git, and is the executable version of the contract
below.

## Configuration

```toml
worktree_create_cmd = "$HOME/dev/bork/contrib/example-provider/create.sh"
worktree_remove_cmd = "$HOME/dev/bork/contrib/example-provider/remove.sh"
worktree_list_cmd   = "$HOME/dev/bork/contrib/example-provider/list.sh"

# Must match the provider's own ceiling — BORK_EXAMPLE_POOL_SIZE here — so bork
# refuses an attach at the limit instead of letting create fail mid-launch.
worktree_pool_size = 4
```

**With these keys unset, bork behaves exactly as it does without a provider.**
Every behaviour they reach is gated on `worktree_create_cmd` being set.

`main/` must point at the checkout. A symlink is enough: once creation is
delegated, bork only runs read-only `git` and `gh` commands there.

Two knobs are read from the environment rather than bork: pool location
(`BORK_EXAMPLE_POOL_ROOT`, default `~/.bork-example-pool`) and pool size
(`BORK_EXAMPLE_POOL_SIZE`, default 4).

On a large repo also raise the polling gap — one `git status` per worktree costs
about a second there, which saturates the default 5s poller:

```toml
git_poll_interval_secs = 15
git_full_sweep_every   = 4   # attached worktrees every round, the rest every 4th
```

## Script contract

Everything crossing the boundary is JSON — one object, or one object per line —
so a new field never breaks an existing parse.

| Script | Input | Output |
| --- | --- | --- |
| `create.sh` | `BORK_ISSUE_ID`, `BORK_SLUG`, `BORK_BASE`, `BORK_BRANCH` | one object, last stdout line |
| `remove.sh` | `BORK_WORKTREE_NAME`, `BORK_WORKTREE_PATH`, `BORK_FORCE` | — |
| `list.sh` | — | one object per pool worktree |

```jsonc
// create.sh, and each line of list.sh. Only "path" is required; "name"
// defaults to its basename.
{"name": "wt--pool-bork-1-fix-bug",
 "path": "/Users/me/.bork-example-pool/wt--pool-bork-1-fix-bug"}
```

All three run with `main/` as their working directory, and every invocation
also carries `BORK_PROVIDER_API` (currently `1`) and `BORK_PROJECT_ROOT`.
Commands are killed at a deadline (minutes for create/remove, seconds for
list), so a provider that can hang should do its own retrying. `pool.sh` is shared state,
sourced by the others; bork never calls it.

A malformed line in `list.sh` is skipped rather than failing the
whole enumeration — these feed the poll loop, and one bad line must not blank the
board. `create.sh` is the opposite: it returns a single object, so anything
unparseable is a hard error.

bork **captures** both streams rather than inheriting them: a child writing to
the terminal scribbles over the alternate screen the TUI draws on. Progress is
therefore not live during a create or a remove, and failures surface as the last
stderr line in bork's error message — so log progress to stderr, and put nothing
but the object on stdout.

`BORK_BRANCH`, when set, is an existing branch to use verbatim — a pull request's
head branch, so a review card lands on the PR's own branch rather than a newly
invented one. Otherwise the branch is composed from the issue id and slug.

## Why the registry, and not `git worktree list`

`list.sh` enumerates the registry, not git. A worktree created outside the pool
appears in `git worktree list` with no index, no ports and none of what a
provider provisions. Counting those would refuse attaches the pool could serve —
on one real project git reported 15 worktrees where the pool held 11.

The registry is positional: one line per index, a blank line is a vacant slot.
An index is stable for as long as a worktree holds it, which is what lets a port
block or database name be derived from it.
