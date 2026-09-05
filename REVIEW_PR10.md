# Independent review — PR #10 "Retire the manual JIT gate; fix the 'every push' claim CI-03 falsified"

Round 1. Base `main` @ `9102bf3`, head `cf7fb89`. Diff: `CLAUDE.md`, `Makefile`,
`scripts/verify-jit.sh`, `AUDIT.md`. Documentation plus one removed `echo`; no
Rust, no workflow, no build config.

## Coverage ledger

| # | Item | Result |
|---|---|---|
| 1 | Can the gate still go red? | **verified by mutation** — see "Break test" |
| 2 | Commands/targets exist and mean what is claimed | verified (`make help` renders; `verify-jit` target unchanged; 92/92 both profiles) |
| 3 | Required-check names match exactly | verified against the live API and this PR's own check runs |
| 4 | Path filters / conditional execution | verified — **no** `paths:`, `paths-ignore:`, `types:`, `if:` or `continue-on-error:` in any of the three workflows |
| 5 | Live config vs description | verified via `gh api` (`strict: true`, `enforce_admins: true`, 5 contexts) |
| 6 | Platform assumptions | n/a — no `.cargo/config.toml`, runner or toolchain change |
| 7 | Resource limits | n/a — no parallelism, dataset or `RUST_TEST_THREADS` change |
| 8 | Coverage given up | none by this PR; it *documents* coverage CI-03 gave up |

## Verified true (the PR's central claims hold)

- **Trigger keys, read directly.** `ci.yml` and `jit.yml`: `pull_request:`
  (bare) + `workflow_dispatch:`. `release.yml`: `push: tags: ['v[0-9]*']`,
  untouched. The PR's premise is correct.
- **"A push to a branch with no open PR is checked by nothing at all" is
  precise, not over-broad.** A bare `pull_request:` defaults to
  `opened, synchronize, reopened`; `synchronize` fires on every push to an
  *open* PR's head branch, so those pushes *are* checked. The new step 6 names
  exactly the uncovered case. Demonstrated empirically: this PR's own head push
  produced all five check runs (`gh pr checks 10`).
- **"Both are required checks on a protected `main`, so a failure blocks the
  merge."** Live API: contexts are string-exact against the check-run names a
  `pull_request` run produces —
  `jit-macos (aarch64 darwin, make verify-jit)`,
  `jit-linux-arm (aarch64 linux, scripts/verify-jit.sh)`, plus the three x86_64
  ones — with `strict: true` and `enforce_admins: true`.
- **"Three places" is the right count.** On `main` the phrase appears in step 6
  (line-wrapped across two lines, so a naive `grep "every push"` misses it) and
  the two platform-coverage rows. A whitespace-normalised sweep of `CLAUDE.md`,
  `README.md`, `RELEASING.md`, `Makefile`, `scripts/` and the three workflows
  finds no fourth live instance.
- **The removed `echo` cannot change the gate's behaviour — tested, not
  assumed.** `jit.yml` invokes the gate as plain `run:` steps (`make verify-jit`
  and `./scripts/verify-jit.sh`) with no `id:`, no output capture, no
  `grep`/`tee`, no `$GITHUB_STEP_SUMMARY`; nothing anywhere in the repo greps
  `GATE PASSED`. The removed line was the last statement in the file, below the
  `exit 1` fail branch. Under `set -uo pipefail` (no `set -e`) the old and new
  final statements are both `echo`s exiting 0. Confirmed by running the script:
  exit 0, one `GATE PASSED` line, no `paste` line.
- **`README.md` left unchanged is a defensible call.** "Automatically, on every
  change … a failure blocks the change" reads, for a user-facing document, as
  changes that *land*; branch protection makes that more true, not less. I
  examined it and accept the judgement. (Pre-existing nit, not this PR's: the
  row `Linux/macOS on Intel … Automatically, on every change` conflates
  platform with code path — there is no macOS-x86_64 job; `ubuntu-24.04` covers
  the fallback path only.)
- **`Closes #6` maps correctly.** The Makefile's deleted "Issue #9" is GitLab
  numbering for the migration issue, which is GitHub **#6** ("Migrate to GitHub
  completely"). GitHub #9 is an unrelated merged PR. Confirmed against the live
  issue list.
- **Issue #2 is CLOSED** (`2026-09-05T23:06Z`, one minute before this branch's
  commit), so "closed separately, before this branch" is accurate.

## Break test (item 1)

The diff sits immediately below the gate's verdict, so the question is whether
the exact-count assertion and the `exit 1` path still fire with the trailing
`echo` gone.

Mutation (on a copy in the scratchpad; **the working tree was never modified**):
`scripts/verify-jit.sh` copied to `$SCRATCH/vj-mutant.sh` with
`EXPECTED_PASSES=92` → `91`.

RESULT_PLACEHOLDER

## Findings

### F1 (minor) — "three leftovers" is incomplete, and the file the PR edited still contradicts the file it fixed

`scripts/verify-jit.sh` is edited by this PR (one `echo` removed at line 156),
but its header — lines 3–14, untouched — still says:

```
# The aarch64 JIT gate — the tests CI structurally cannot run.
#
# GitLab's shared runners are x86_64 Linux, ...
# This script is that missing coverage, run by a human:
...
# Issue #2 tracks the gap; issue #9 tracks moving it into GitHub Actions, which
# gives public repos free `macos-14` and `ubuntu-24.04-arm` runners.
```

That is the manual-gate framing the PR exists to retire ("run by a human", "the
tests CI structurally cannot run"), plus the *identical* stale sentence the PR
deleted from the `Makefile`, surviving verbatim in the very file the PR touched.
`AUDIT.md`'s "the stale 'Issue #9 tracks replacing this' deleted rather than
renumbered" is true of the `Makefile` and false of the repo:

| File | Line | Text |
|---|---|---|
| `scripts/verify-jit.sh` | 13 | `issue #9 tracks moving it into GitHub Actions` |
| `.github/workflows/ci.yml` | 8, 14 | `(issue #9)`, `a separate checklist item in issue #9` |
| `.github/workflows/jit.yml` | 9, 27, 219 | `Issues #2 and #9.`, `(issue #9)`, `(issue #9)` |

Six live references to an issue number that, post-migration, resolves to an
unrelated merged PR. Documentation-only, so no gate weakens — but it is the
PR's own stated cleanup class, left half-done in the artefact the PR is named
after.

### F2 (minor, and the reason to think twice about `Closes #6`) — issue #6's box 4 is half-met

Box 4, verbatim: *"Once both are green in CI, remove 'mandatory before merge'
from **`CLAUDE.md`'s agent protocol** and the requirement to paste gate output
into MR descriptions."*

- The paste half: **done** (Makefile comment + the script's `echo`).
- The `CLAUDE.md` half: **not done.** Step 6 still opens "Any change touching
  `src/randomx/jit/`, or `vm.rs`'s native-loop path, **must pass**
  `make verify-jit`", and the rewrite *strengthens* the local-run instruction
  ("Run the gate locally while a change is still on a bare branch").

The new wording is defensible on its merits — the bare-branch window is real and
worth naming. But it is not the demotion box 4 asks for, and the PR claims box 4
as done while closing the issue. Either reword step 6 so the mandate reads as
CI's rather than the agent's, or drop `Closes #6` and tick the boxes that are
genuinely done. This is the one finding that turns an otherwise-minor pile into
a durable loss: once #6 closes, F1's leftovers and this box close with it.

(For completeness, the other four boxes in that section *are* met: both `jit-*`
jobs exist on the right runners, both hard-fail the workflow, and both make
targets are kept and demoted in the `Makefile` help text. The Scope checklist
higher up the issue — history, issues, MRs, workflows, release flow, `glab`→`gh`,
README links, GitLab disposition — I spot-checked as done by MIGRATE-01: zero
live `glab` references, zero GitLab URLs in `README.md`/`RELEASING.md`, and
`release.yml` in place. The boxes are simply never ticked in the issue body.)

### F3 (minor) — stale GitLab issue numbers survive inside the file this PR edits, including one that this PR is about to make actively misleading

`CLAUDE.md` uses an explicit disambiguating convention at line 281
(`(issue GitLab #4)`), which two nearby references violate:

- line 152 — `(issue #6)` for the debug/release `debug_assert!` gap. That is
  GitLab #6; on GitHub it is **#4** (open). After this PR merges, `#6` resolves
  to the *closed migration issue*.
- line 159 — `(issue #4)` for the inert-JIT/`native_loop_effective` point. That
  is GitLab #4; GitHub #4 is a different, open issue.
- line 92 — the MIGRATE-01 row's "Closes issues #2 and #4". GitHub #4 is open
  and unrelated; GitLab #4 was the one closed.

Same class as the `Makefile`'s deleted "#9", one paragraph away from the text
this PR rewrote.

### F4 (nit) — `DESIGN_JIT_NATIVE_LOOP.md:397-400`

*"CI can never run any of this … the GitLab runners are x86_64 Linux … The
differential tests are therefore a **mandatory local gate**, not a CI backstop."*
Flatly false since the migration, and it carries both the "mandatory" wording
and the manual-gate framing. Rated a nit rather than a finding because the
document is explicitly a dated design record — its header reads
`**Status:** proposed — implementation staged behind this document` /
`**Branch:** feat/jit-native-loop` — not standing documentation. Worth a
"superseded" note some day; not this PR's job.

### F5 (nit) — the new `Makefile` comment drops `workflow_dispatch`

"the workflows trigger on pull requests only" (Makefile:67-69) versus
`CLAUDE.md`'s more careful "`pull_request` and `workflow_dispatch` only". The
conclusion is unaffected — a manual dispatch is not automatic coverage — but the
two texts landed in the same commit and disagree on the fact.

## What I could not verify

- The `jit-macos` and `jit-linux-arm` check runs on PR #10 were still `pending`
  when this ledger was written. I ran the gate locally instead (Darwin arm64,
  92/92, debug + release, exit 0) and the mutation test above; I did not wait
  for the CI verdict.
- I did not re-derive CI-03's runner-minute figures; they are PR #8's and were
  re-derived there (REVIEW_PR8.md round 3).

## Verdict

VERDICT_PLACEHOLDER
