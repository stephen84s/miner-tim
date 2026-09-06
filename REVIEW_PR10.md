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

Observed (full log in the run, both profiles executed):

```
verify-jit: FAIL — debug profile (debug_assert! live) ran '92' tests, expected 91.
verify-jit: FAIL — release profile (shipping profile) ran '92' tests, expected 91.
verify-jit: GATE FAILED on Darwin arm64
EXIT=1
```

The unmutated script on the same tree: `GATE PASSED on Darwin arm64 — 92 tests,
debug + release`, `EXIT=0`, and no `paste` line. **The gate can still go red,
and it goes red for the right reason** — the exact-count assertion that exists
because libtest reports an empty filter as success. Working tree left clean; the
mutation lived only in the scratchpad copy.

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

### F6 (minor) — DOC-02's closing "assumption stated rather than tested" is now stale, and the task-board row cites no ledger

Two accuracy items inside the audit entry itself, both fixable in place (the
entry is on an unmerged branch, so per `CLAUDE.md` it may be **edited**, not
appended to — do not "append a correction"):

1. The entry ends *"**Assumption stated rather than tested:** that no tooling
   parses `verify-jit.sh`'s final line."* It has now been tested — all three
   workflows, the `Makefile`, every script and two live runs of the script (see
   "Verified true" above). Leaving a disavowed-assumption sentence standing
   after the assumption was discharged is the exact shape round 3 of PR #8 and
   round 2 of PR #7 both caught.
2. The `CLAUDE.md` DOC-02 row ends "Closes #6; #2 closed separately." The two
   preceding rows end "Ledger: `REVIEW_PR7.md`." / "Ledger: `REVIEW_PR8.md`.".
   This row should cite `REVIEW_PR10.md` and its review outcome.

`bash -n scripts/verify-jit.sh` is clean and `make help` renders, so the rest of
the entry's Verification block reproduces.

## Note on the CI checks for this PR

Committing this ledger to the PR branch **restarts the JIT gate**: `jit.yml` has
`concurrency: jit-${{ github.ref }}` with
`cancel-in-progress: ${{ github.event_name == 'pull_request' }}`, so the run on
`cf7fb89` (33997841524) shows `cancelled` and a fresh ~14-minute run started on
the ledger commit. That is expected behaviour, not a failure — but a maintainer
seeing a cancelled required check on the PR should know a review commit caused
it. The authoritative run is the one on the head commit at merge time.

## What I could not verify

- Nothing material. The CI verdict was pending when the findings were written
  but has since landed: run **33998507095** on `98aa2f8` (this branch's ledger
  commit, which carries the PR's full diff) — `jit-macos` **success**,
  `jit-linux-arm` **success**, and `lint`/`audit`/`test` green on run
  33998507094. All five required contexts pass. I also ran the gate locally
  (Darwin arm64, 92/92, debug + release, exit 0) plus the mutation test above.
  Any run started after this commit is a re-run of an already-green tree.
- I did not re-derive CI-03's runner-minute figures; they are PR #8's and were
  re-derived there (REVIEW_PR8.md round 3).

## Verdict

**No blockers. No majors.** The mechanism is intact: the gate still fails and
still fails for the right reason, the required-check names are string-exact, no
workflow gained a `paths:`/`if:`/`continue-on-error:` escape hatch, the removed
`echo` is provably inert, and the PR's central factual claim — including the
precise "with no open PR" qualifier — is true.

So on the diff alone: **MERGEABLE**.

The one lever worth pulling first is `Closes #6`. F2 shows box 4 is half-met and
F1 shows six leftovers of exactly the kind this PR set out to remove, one of them
in the file it edited. If the issue closes on merge, that work closes with it.
Recommended: either (a) drop `Closes #6` from the PR body and leave the issue
open for the `verify-jit.sh` header, the workflow comments and step 6's wording,
or (b) fold F1 and the `CLAUDE.md` half of box 4 into this branch and then close
it. Merging as-is is not unsafe; it is a documented loss of follow-up.

F3, F4 and F5 are minors/nits that can land separately.

---

## Round 2

Fresh reviewer, spawned cold. Base `main` @ `9102bf3`, head `879ef08`. Scope: the
whole PR, with the four fix commits (`3b196a2`, `3ff2105`, `52617e4`, `879ef08`)
reviewed as new work rather than as accepted answers to round 1.

### Coverage ledger

| # | Item | Result |
|---|---|---|
| 1 | Can the gate still go red? | **re-verified independently** — mutation run, below |
| 2 | Commands/targets exist and mean what is claimed | verified (`make help` renders the new text; `bash -n` clean; 92/92 both profiles on this tree) |
| 3 | Required-check names match exactly | verified — 5 live contexts string-exact against this PR's own check runs |
| 4 | Path filters / conditional execution | verified — no `paths:`/`types:`/`if:`/`continue-on-error:` in any workflow; non-comment lines byte-identical to `main` |
| 5 | Live config vs description | verified via `gh api`: `strict: true`, `enforce_admins: true`, 5 contexts |
| 6 | Platform assumptions | n/a — no `.cargo/config.toml`, runner or toolchain change |
| 7 | Resource limits | n/a — no parallelism/dataset/`RUST_TEST_THREADS` change |
| 8 | Coverage given up | none by this PR |

### Break test (item 1, not inherited from round 1)

`scripts/verify-jit.sh` copied to the scratchpad with `EXPECTED_PASSES` 92 → 91,
run against this branch's tree with `CARGO_TARGET_DIR` redirected out of the
worktree:

```
verify-jit: FAIL — debug profile (debug_assert! live) ran '92' tests, expected 91.
verify-jit: FAIL — release profile (shipping profile) ran '92' tests, expected 91.
verify-jit: GATE FAILED on Darwin arm64
EXIT=1
```

Both profiles ran 92 passed / 0 failed, so the gate is red *for the mutated
reason only*. Working tree never touched (`git status --short` empty). The
stronger structural argument holds independently: the removed `echo` was the
file's last statement, below the `exit 1` branch, and the two workflows'
non-comment lines are byte-identical to `main`.

### Confirmed correct (the four fix commits)

- **F2 / issue #6 box 4 is genuinely met.** "must **pass the JIT gate** — but
  running it by hand is no longer your duty, and pasting its output into a PR is
  no longer evidence anyone needs" removes the human mandate and the paste
  requirement; the remaining obligation is CI's. The bare-branch warning — the
  reason the PR exists — survives intact and is now the load-bearing sentence.
  `Closes #6` is defensible.
- **F3's final mapping is right**, re-derived from `REVIEW_MR1.md`'s GitLab table
  against `gh issue list --state all`: GitLab 1→1, 2→2, 5→3, 6→4, 8→5, 9→6;
  GitLab 3, 4 and 7 were closed before the migration and never imported. So the
  debug/release gap is GitHub **#4**, the `MAP_JIT` fallback has no GitHub
  counterpart, and MIGRATE-01's issues are **#2 and #6**. (`#2 closed
  2026-09-06` is correct in local time: `2026-09-05T23:06:30Z` = 09:06 +10:00.)
- **F1's remapping is complete for `#9`** — zero live `#9` references remain
  outside `AUDIT.md`/`REVIEW_*`; the six identified now read GitHub #6.
- **Workflow edits are comments-only** and all three workflows parse.
- **DOC-02 vs the diff**: files-changed list matches, the "deleted rather than
  renumbered" claim is properly withdrawn, and the mutation-test claim now
  reproduces on my own run.
- **Sweep**: no live "every push" and no "run by a human" survives in `CLAUDE.md`,
  `README.md`, `RELEASING.md`, `Makefile`, `scripts/` or `.github/`. I re-examined
  `README.md` independently and agree it is more accurate under CI-03, not less.

### Findings

**R2-F1 (minor) — the stale-numbering class survives in the file `3ff2105`
rewrote, and one instance is now actively misleading.**
The new header declares GitHub numbering ("GitHub #2 … GitHub #6 … those were
GitLab #2 and #9"). Below it, still GitLab-numbered:

| Ref | Means | Resolves to on GitHub |
|---|---|---|
| `verify-jit.sh:50` `issue #4's shape` | GitLab #4, silent `MAP_JIT` fallback | #4 = the debug/release gap |
| `verify-jit.sh:129` `Issue #6 / issue #2 mitigation 2` | GitLab #6, debug/release gap | #6 = the migration issue **this PR closes** |
| `verify-jit.sh:141`, `ci.yml:78`, `ci.yml:239`, `jit.yml:91` `issue #7` | GitLab #7, test-suite RSS | PR #7 (branch protection) |

Line 129 is the identical defect `52617e4` fixed at `CLAUDE.md:157`, missed in
the file `3ff2105` was rewriting; and lines 50/129 are *swapped* relative to the
convention the header sets, so a reader gets both backwards. Correct fix is the
`GitLab #N` convention `CLAUDE.md:164` already uses — **not** a renumber: GitLab
#4 and #7 were never migrated and have no GitHub issue.

**R2-F2 (minor) — the same commit applied two standards inside one table.**
`52617e4` corrected MIGRATE-01's issue numbers but left `PLAT-02` ("debug_assert
guards … (issue #6)" = GitHub #4; "issue #9 is the GitHub-Actions plan"),
`MEM-01` ("#9's budget", "Unblocks #9", "issue #7") and `CI-02` ("issue #9,
workflows only") in the live task board an agent reads at session start.

**R2-F3 (minor) — `ci.yml`'s new "and is done" overstates the release
checklist item, which is inside the issue this PR closes.**
`release.yml` exists and is tag-triggered, and `RELEASING.md` is `gh`-based — so
far accurate. But `RELEASING.md` still says the CI release job *"does **not**
create releases in practice (no macOS runner, and it can't attach a binary it
can't build) … only creates an empty entry"*, which is false against
`release.yml` (it runs on `ubuntu-24.04` and calls `gh release create`); its tail
still proposes registering a self-hosted `macos-arm64` runner; and its opening
paragraph is a mangled GitLab-era leftover ("For now the shipping x86_64 Linux
and cannot build …"). The two halves also collide: step 4 `make release` pushes
the tag, `release.yml` creates the entry, then step 5 tells the operator to
`gh release create` the same tag — which I expect to fail as already-existing,
though I did not execute it. No `v*` tag has been pushed since the migration and
`gh release list` is empty, so `release.yml` has never run. `RELEASING.md`'s
defects pre-date this PR; asserting the item "is done" is new here.

**R2-F4 (minor) — "the macOS debug profile takes ~8 minutes in CI" does not
reproduce.** Measured from the `jit-macos` logs of the six most recent successful
runs: debug test phase 564–637 s (mean ≈ 600 s), i.e. **9.4–10.6 min** including
the build step. The figure is pre-existing on `main`, but `3b196a2` re-asserted
it inside rewritten text presenting it as a CI fact.

**R2-F5 (minor) — DOC-02 and the PR body record only round 1.** The entry ends
"**Review:** one round, `ci-reviewer`, verdict mergeable…"; the PR body's Review
section says the same. Both go stale the moment this section lands — the exact
shape PR #7 round 2 and PR #8 round 3 caught. Update both to name round 2 before
merging.

**R2-F6 (nit) — round 1's F4 is still open.** `DESIGN_JIT_NATIVE_LOOP.md:397-400`
("CI can never run any of this … a **mandatory local gate**, not a CI backstop")
remains the last live copy of the framing this PR retires. Dated design record,
so deferring is reasonable; recorded so it is not lost when #6 closes.

### What I could not verify

- That `gh release create` fails on a tag `release.yml` has already released
  (R2-F3) — inferred from `gh` semantics, not executed.
- Nothing about the aarch64 emitted code changed, so no JIT-correctness question
  arises in this diff.

### Verdict

**MERGEABLE — no blockers, no majors.** Six minors/nits, none of which weakens a
gate: the count assertion still fires, the required contexts are string-exact,
and the workflow diff is provably comments-only. R2-F5 should be fixed before
merge (it is one line in each place); R2-F1 and R2-F3 are the ones worth folding
in rather than deferring, because both live inside the scope of issue #6 and
close with it.

## Round 3

Fresh reviewer, spawned cold. Range reviewed as new work: `a8f9baa..HEAD`
(`f48dbb5`, `608b73d`, `8c0e6d0`, `0b4236e`, `fdf66cd`), the five commits that
answer round 2. Not a re-review of the PR.

### Coverage ledger

| # | Item | Result |
|---|---|---|
| 1 | Can the gate still go red? | **re-verified** — stub run, below |
| 2 | Commands/targets exist and mean what is claimed | verified — `bash -n` clean; non-comment lines of `verify-jit.sh`/`Makefile` untouched in this range |
| 3 | Required-check names match exactly | verified — the 5 live contexts are string-exact against the job names all three workflows parse to |
| 4 | Path filters / conditional execution | verified — workflow diff vs merge-base `9102bf3` is comments-only in all three files |
| 5 | Live config vs description | verified via `gh api`: `strict: true`, `enforce_admins: true`, 5 contexts |
| 6 | Platform assumptions | n/a — no `.cargo/config.toml`, runner or toolchain change |
| 7 | Resource limits | n/a — `RUST_TEST_THREADS=3` and the RSS figures untouched; only their issue label changed |
| 8 | Coverage given up | none |

### Break test (item 1, not inherited)

`cargo`/`rustc` stubbed on `PATH` to emit `test result: ok. 91 passed`, script
run unmodified from the branch tree:

```
verify-jit: FAIL — debug profile (debug_assert! live) ran '91' tests, expected 92.
verify-jit: FAIL — release profile (shipping profile) ran '91' tests, expected 92.
verify-jit: GATE FAILED on Darwin arm64
EXIT=1
```

The count assertion still fires in both profiles and the script exits 1. Stubs
lived outside the repo; working tree never touched.

### Findings

**R3-F1 (minor) — `f48dbb5` left two of the exact instances round 2 enumerated
in R2-F2, and the new convention note promotes them from stale to
authoritative.**

- `CLAUDE.md:103` (PLAT-02): "so the native loop's `debug_assert!` guards finally
  execute (issue #6)". That is GitLab #6 = GitHub **#4**. Under the note's rule a
  bare `#6` is GitHub #6 — the migration issue *this PR closes*, which is the
  same "resolves to the issue this PR closes" shape R2-F1 flagged in
  `verify-jit.sh:129` and which was fixed there. The identical claim is numbered
  `#4` at `CLAUDE.md:166`, so the file now contradicts itself in two places.
- `CLAUDE.md:104` (MEM-01): `"already over #9's budget"` — GitHub #9 is a PR.
  Its sibling in the same table cell was corrected to `Unblocks GitLab #9
  (GitHub #6)`. Two standards inside one row: precisely what R2-F2 said the
  previous commit did.

**R3-F2 (minor) — the note claims a scope the commit does not have.** "A bare
`#N` means the **GitHub** issue" reads as a repo-wide rule. Still bare and still
GitLab-numbered: `CLAUDE.md:100` JIT-01 `"filed as issues #3-#8"` (on GitHub #7
and #8 are PRs); `CLAUDE.md:101` VIS-01 `"(issues #4 + #3)"`, `"Closes #3 as a
side effect"`, `"#3 and #4 closed"` — four references to exactly the GitLab #3
and #4 the note names as never imported, now resolving to two *open* GitHub
issues. Beyond the task board there are ~26 more bare `issue #3/#4/#7` in
`src/miner.rs`, `src/randomx/vm.rs`, `src/bin/minertim.rs`,
`src/randomx/tests.rs`, `src/randomx/jit/memory.rs` and one in
`.claude/agents/jit-reviewer.md`. Renumbering source comments may not be worth
the churn — but then the note should say which files the convention has been
applied to rather than assert it globally.

**R3-F3 (minor) — "Six further `issue #7` references" is five, and the commit
message's own list names four.** Counted from the diff: `ci.yml` ×2, `jit.yml`
×1, `verify-jit.sh` ×1, `CLAUDE.md` MEM-01 ×1 = **five** conversions; the sixth
`GitLab #7` in the diff is newly *added* to CI-02, not a converted reference.
`f48dbb5`'s message says "Six more" while parenthesising only "(verify-jit.sh,
ci.yml x2, jit.yml)" = four. `0b4236e` carries "Six" into `AUDIT.md`. A miscount
inside the entry whose subject is miscounted references.

**R3-F4 (minor) — the design doc's most visible false line is left standing
directly above the new note.** `DESIGN_JIT_NATIVE_LOOP.md:3` still reads
"**Status:** proposed — implementation staged behind this document." The native
loop was merged as MR !1 (`365d288`) and is the default. The note corrects a
§6a bullet but not the Status line it is appended to. (Nit: the six new lines
are spliced between `Status:` and `Branch:`, breaking a four-line metadata
block.) Marking rather than rewriting was otherwise the right call, and the CI
correction itself is accurate: `jit.yml` does run the differential tests on
`macos-14` and `ubuntu-24.04-arm` on `pull_request`, and both job names are
string-exact required contexts.

**R3-F5 (minor) — DOC-02 and `AUDIT.md` now say "two rounds … both mergeable",
which is stale the moment this section lands.** Same shape as R2-F5. Record
round 3 in both before merging.

**R3-F6 (nit) — `ci.yml`'s quotation of `RELEASING.md` reorders it.**
`RELEASING.md` says "the CI job, if it runs at all, only creates an empty
entry"; `ci.yml:16-18` renders it `"only creates an empty entry ... if it runs
at all"`. An ellipsis marks omission, not reordering.

**R3-F7 (nit) — "No `v*` tag has been pushed since the migration" is true as
worded but load-bearing.** Three tags (`v0.1.0`–`v0.1.2`) *do* exist on
`origin`, pushed by the migration itself, and did not trigger `release.yml`
(`gh run list --workflow=release.yml` is empty, `gh release list` is empty).
Both #11 and `ci.yml` rest "the next release is when this bites" on that;
saying the tags pre-date `release.yml` reaching the default branch would make it
airtight.

### Confirmed correct

- **`608b73d`'s measurement reproduces exactly.** Re-derived from
  `actions/runs/<id>/jobs`. The 8 most recent successful `jit-macos` jobs as of
  the commit's timestamp (2026-09-06T01:49Z): 12.38, 15.27, 13.33, 14.75, 14.35,
  14.12, 14.90, 13.57 min → **mean 14.083**, range **12.38–15.27**. Sample is
  honestly described (named cut, mean called a mean, range given). PR #8's
  "13.94 over 12" is confirmed at `REVIEW_PR8.md:729`. The next run
  (`34004966560`, 14.68 min) started two minutes after the commit; including it
  gives 14.14, so the figure is not cut-sensitive. Substituting a whole-job
  figure for the old debug-profile figure is stated plainly in the text.
- **The GitLab→GitHub mapping in `f48dbb5` is right** where it was applied,
  re-derived independently from `REVIEW_MR1.md`'s table against `gh issue list
  --state all`: GitLab 1→1, 2→2, 5→3, 6→4, 8→5, 9→6; GitLab 3, 4 and 7 closed
  before the migration and never imported. So the note's central claim — that
  GitLab #3, #4 and #7 have no GitHub number — is **correct**. Every reference
  the commit rewrote (`verify-jit.sh` 50/129/142, `ci.yml` 84/245, `jit.yml` 91,
  PLAT-02, MEM-01 heading, CI-02) is now right.
- **Issue #11 is accurate.** All four quotations are verbatim against
  `RELEASING.md`; `release.yml` is `ubuntu-24.04`, `push: tags: v[0-9]*`, calls
  `gh release create`; the step-4/step-5 collision is explicitly labelled an
  inference not executed; no other claim is asserted without support.
  `ci.yml`'s comment matches it (modulo R3-F6/R3-F7).
- **`0b4236e`'s account of round 2** matches `REVIEW_PR10.md`'s Round 2
  everywhere except R3-F3's count: "six more [findings], four of which are
  defects introduced or missed by round 1's corrections" is right (R2-F1–F4 vs
  F5/F6), and the break-test description matches what round 2 recorded.

### What I could not verify

- I did not run the real 92-test gate on this tree (~12 min ×2 profiles); the
  gate's *logic* was exercised with a stub, and no executable line of
  `verify-jit.sh` changed in this range. CI's `jit-macos`/`jit-linux-arm` are
  green on the head.
- Whether `gh release create` fails on a tag `release.yml` already released —
  still inferred, as #11 itself says.

### Verdict

**MERGEABLE — no blockers, no majors.** Five minors and two nits, none touching
a gate: the count assertion still fires, the five required contexts are
string-exact, and the whole PR's workflow diff is provably comments-only.
R3-F1 is the one worth fixing before merge — it is the third recurrence of the
stale-numbering class, this time under a convention note that lends the wrong
numbers authority. R3-F5 is one line in each of two files.
