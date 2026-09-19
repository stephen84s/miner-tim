# REVIEW_PR28 — round 1, `ci/no-ledgers-gate` (issue #19, PROC-07)

Head reviewed: `f2394bc`. Reviewer: cold ci-reviewer. Worktree
`.claude/worktrees/ledger-gate`; primary checkout untouched.

## Verdict

**MERGEABLE.** No blockers, no majors. Six minors, all record accuracy — the
mechanism itself is sound and was made to fail on the case it exists for.
**ACTIONABLE:** yes, minors 1-5 are factual corrections to `AUDIT.md`, the PR
body and the `.gitignore` comment, plus one ordering warning for the lead.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Can the gate go red? | done — 10 cases, 4 red, 6 green, all as intended |
| 2 | Commands exist / mean what is claimed | done — step executed on GitHub, API-confirmed |
| 3 | Required-check names exact | done — live protection API |
| 4 | Path filters / conditional execution | done — no `paths:`, no `if:`, no `continue-on-error:` |
| 5 | Live config vs description | done |
| 6 | Platform assumptions | done — `ls`/glob only, no host assumptions |
| 7 | Resource limits | done — step cost 0 s, no parallelism change |
| 8 | Coverage given up | done — none; gate is additive |
| + | Crash recovery (priority 2) | done — full sequence performed, retrieval confirmed |
| + | GFM table render | done — one 36-row table, PROC-07 and Pending both in it |

## 1. Does the gate gate? (verified by execution)

Ran the committed step's shell verbatim under GitHub's own invocation
(`bash --noprofile --norc -e -o pipefail`), ten cases:

| Case | Result |
|---|---|
| clean tree | exit 0, no output |
| `REVIEW_PR28.md` at root | **exit 1**, names the file |
| `REVIEW_PR 28.md` (space) | **exit 1**, names the file |
| three ledgers at root | **exit 1**, names all three |
| `REVIEW_.md` | **exit 1** |
| `docs/REVIEW_PR28.md` | exit 0 — root-only (see minor 5) |
| `review_pr28.md` (lower) | exit 0 |
| `REVIEW_PR28.markdown` | exit 0 |
| `REVIEW_PR28.MD` | exit 0 |
| directory `REVIEW_dir.md` | exit 1, **empty** list (nit 7) |

`set -e` does not abort on the non-matching glob because the `ls` is the `if`
condition; the unmatched-glob path is the *pass* path and it exits 0 correctly.
This is not the repo's silent-green shape: the failure path is the one that
needs the glob to match, so a glob that matches nothing cannot produce a false
pass — it produces the true pass. I could not construct a root-level
`REVIEW_*.md` that the step failed to catch.

Snippet is the issue's suggested one verbatim. Provenance, not verification —
the ten cases above are the verification.

## 2-4. Execution, naming, conditionals (verified live)

`gh api repos/stephen84s/miner-tim/actions/jobs/105875464670` — head_sha
`f2394bc`, the PR head. Executed steps:

```
2 Run actions/checkout@v4          09:28:13 -> 09:28:14
3 Install Rust 1.97.1              09:28:14 -> 09:28:22
4 Cache cargo registry             09:28:22 -> 09:28:23
5 Cache target/                    09:28:23 -> 09:28:23
6 no review ledgers in the tree    09:28:23 -> 09:28:23   success
7 cargo clippy ... -D warnings     09:28:23 -> 09:28:28
```

The step **ran** — not skipped. No step-level `if:`, no `continue-on-error:`,
no `|| true`; `ci.yml` has no `paths:` filter, so the job always reports. Cost
0 s. Job `name:` is still the exact string `lint (clippy, x86_64 linux)`.

Live branch protection: the five required contexts are string-exact against the
check-run names this PR produced; `strict: true`, `enforce_admins: true`,
force-push and deletion blocked. With squash-merge plus `strict`, the tree the
gate sees is the tree that lands.

**Not established by me:** the step has never been observed *red* on GitHub.
Only the local reproduction above shows the red path. I did not push to force
it (out of scope).

## 3. Crash recovery — intact (verified by performing it)

Ran the sequence a reviewer agent would follow literally, in this worktree:

1. write `REVIEW_CRTEST.md` → invisible to `git status --short`.
2. plain `git add` → **refused**, with git's own `hint: Use -f`.
3. `git add -f` → staged; commit succeeded (`70afde2`).
4. edit again → shows as ` M` (tracked files ignore `.gitignore`); plain
   `git add`, `git commit -a` and `git add -A` all stage it. So `-f` is
   load-bearing only for the **first** add; recovery degrades gracefully rather
   than silently.
5. `git rm` → strips cleanly.
6. `git show 70afde2:REVIEW_CRTEST.md` → **content returned after the strip.**
   The `AUDIT.md` retrieval contract survives.

Worktree reset to `f2394bc`, clean. Also checked the three per-agent files
(`ci-reviewer.md`, `jit-reviewer.md`, `pr-reviewer.md`): none carries its own
`git add` instruction, so `_shared-context.md` rule 3 is the only place that
needed changing, and it was changed. No uncovered path found.

This review's own ledger is the live demonstration: plain `git add` was refused,
`git add -f` worked.

## 5. Collisions / cost

No tracked file matches `REVIEW_*` — the ignore rule collides with nothing.
Nothing in `Makefile`, `scripts/` or `make dist` consumes such a path. The step
adds 0 s and does not reorder anything that matters.

## Findings

**Minor 1 — `AUDIT.md` describes output the committed script cannot produce.**
"With no `REVIEW_*.md` files present: exits 0, prints `Check passed: no ledgers
found`." There is no such `echo` in the step; the clean path prints nothing
(case A above). The verification bullet describes a different script.

**Minor 2 — the step's position is misstated, and the rationale attached to it
is defeated.** `AUDIT.md`: "now contains 5 steps (was 4: Checkout, Install
Rust, Cache cargo registry, Cache target/, cargo clippy — now has the new
ledger-check step first)". The "was 4" list has **five** items; the job now has
**six** authored steps; and the new step is **fifth of six** (sixth of seven
executed), after rustup install and both cache restores. Both `AUDIT.md` and the
PR body justify the position as "so it fails fast" — it does not fail before
setup. Immaterial in cost (clippy took 5 s in the observed run), wrong as a
record. `CLAUDE.md`'s row repeats "5 steps in `lint` job"; "step placed before
clippy" in that row *is* true.

**Minor 3 — "rules 1-3 updated to specify `git add -f`"** (`AUDIT.md` twice,
PR body once). The diff touches **rule 3 only**; rules 1 and 2 are unchanged
context lines.

**Minor 4 — the "Not established" is stale in the direction that matters.**
`AUDIT.md`: "The CI check cannot be run on GitHub without pushing"; PR body:
"the step has never run on GitHub — that happens for the first time on this
PR." It has run, successfully, on this PR's head (job 105875464670, step 6).
The accurate residual gap is narrower and should replace it: *the step has never
been observed failing on GitHub.*

**Minor 5 — "`git add -A` can never sweep them in" is unconditional and is
true only while the ledger is untracked.** Stated in the `.gitignore` comment
and in `AUDIT.md`. Verified false after the first mandated force-add: from that
point the ledger is tracked and `git add -A` stages every subsequent edit
(step 4 above). The claim holds for the accidental path it was written about
(`mutants.out/` was untracked), so scope it — "can never sweep in an
un-force-added ledger" — rather than dropping it.

**Minor 6 — the two layers have different reach.** `.gitignore`'s `REVIEW_*.md`
matches at **any** depth (`git check-ignore` confirms `docs/REVIEW_X.md`); the
CI step checks the **repo root only**. A force-added
`docs/REVIEW_X.md` reaches `main` ungated. This matches the scope stated in
`CLAUDE.md` and in the issue ("at the repo root"), so it is an asymmetry to
record, not a defect to fix.

**Nit 7 —** a *directory* named `REVIEW_dir.md` makes the step exit 1 while
printing an empty file list. Fails in the safe direction; the message is
useless. `ls -d` would fix it. Not worth a push on its own.

## Ordering warning for the lead

Committing this ledger **arms the gate**. The PR is `CLEAN` right now only
because no `REVIEW_*.md` is present at the head. The next push of this branch
with this file in the tree turns `lint` red — correctly, by design. Strip the
ledger (`git rm REVIEW_PR28.md`) *before* pushing, and record `70afde2`'s
successor sha — this file's commit sha — in the `AUDIT.md` entry per LEDGER-01.

## What I did not verify

- The red path on GitHub itself (reproduced locally only).
- Whether a future lead actually runs `git rm` — unenforceable, and the entry
  is honest about it.
