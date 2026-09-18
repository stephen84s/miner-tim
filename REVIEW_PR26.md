# REVIEW_PR26 — round 1, independent

PR #26 "test: pin `DonationSchedule::level()`" — branch `fix/donate-level-accessor`,
base `main`. Reviewed in worktree `.claude/worktrees/donate-level`.

## Scope

Diff is `src/donate.rs` (test module only, +24), `AUDIT.md` (+52), `CLAUDE.md`
(+1 task-board row). No `src/randomx/jit/`, no emitter, no `vm.rs` native loop,
no `benches/`, no `.github/workflows/`, no `Makefile`/`scripts/`/`.cargo/`.
**Nothing to hand off to `jit-reviewer` or `ci-reviewer`; all of it is mine.**

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Correctness of the change vs. its claims | pending |
| 2 | Silent failure / swallowed errors | pending |
| 3 | Safety switches & fail-safe direction | pending |
| 4 | Tests — break-tested, not read | pending |
| 5 | Resource use | pending |
| 6 | Documentation & audit accuracy | pending |
| 7 | Concurrency | pending |

## Findings

(in progress)

### Item 4 — break-testing (the priority-1 question): PASS

Method: `cp src/donate.rs` to scratchpad first; every mutation applied from that
copy and the copy restored at the end (`git diff --exit-code src/donate.rs`
clean — verified, tree left clean).

**(a) The gap the PR claims is real, reproduced.** Stubbed `level()` to `1` *and*
deleted `level_reports_what_was_configured`: `cargo test --lib donate::` →
`3 passed; 0 failed`. So pre-PR, a stuck `1` was genuinely invisible. Good.

**(b) The new test kills it, and kills more than the two "plausible constants".**
With the test restored and `level()` stubbed to each constant in turn:

| stub | result |
|---|---|
| `1` | FAILED (1 failed) — only the new test; `floor_enforced` still passes, exactly as the issue says |
| `5` (default) | FAILED (2 failed) |
| `0`, `2`, `3`, `10`, `50`, `100`, `255` | FAILED (2 failed each) |

The PR's claim ("neither the clamp floor nor the default can pass") holds, and is
in fact understated: **no** `u8` constant survives, because the array spans seven
distinct values. Note that of the nine, only `1` was ever missed pre-PR — the
others already tripped `floor_enforced`/`donated_fraction_matches_level`, which
slightly resizes the "2 mutants" framing (see item 6).

**Not vacuous, and no value is silently clamped.** `clamp_level` is
`level.clamp(1, 100)`; every array element is inside `[1, 100]`, so
`MAX_DONATE_LEVEL` (100) round-trips and that assertion is not asserting the
wrong thing. The specific worry in the brief does not materialise.

### Item 4b — neighbour sweep (`cargo-mutants` over the whole file)

`scripts/mutants.sh` does **not** exist on this branch (see F3), so I ran the raw
tool. I did not run the command the PR quotes; I ran its underlying equivalent.

```
# pre-PR (src/donate.rs from origin/main, test module and all)
cargo mutants --file src/donate.rs -- --lib 'donate::'
  25 mutants tested in 41s: 1 missed, 23 caught, 1 unviable
  MISSED  src/donate.rs:62:9: replace DonationSchedule::level -> u8 with 1

# this branch
cargo mutants --file src/donate.rs -- --lib 'donate::'
  25 mutants tested in 39s: 24 caught, 1 unviable
```

**Exactly one mutant was missed before, exactly the one the issue names, and it
is now caught. No other mutant in `donate.rs` survives** — `clamp_level`,
`beneficiary_at`, `new` and the constants are all covered. The fix does not close
one gap and leave a neighbour open. (The single `unviable` is a compile failure
of the mutated form, not a survivor; it is present identically on both sides.)

The PR's own figure reproduces **exactly**, including the wall clock:
```
cargo mutants --file src/donate.rs --re 'DonationSchedule::level' -- --lib 'donate::'
  2 mutants tested in 12s: 2 caught
```
and pre-PR the same filter is 1 missed / 1 caught — so of the "2 mutants" only
one was ever the gap; the `-> 0` sibling was already killed by `floor_enforced`.
The entry's separate "found it in 38 seconds" is consistent with the 39-41 s
whole-file sweeps.

### Items 1, 2, 3, 5, 7 — verified, nothing to report

- **1 Correctness vs. claims.** The diff adds one `#[test]` and two docs entries.
  No production code changes (`git diff origin/main...HEAD -- src/` touches only
  the `#[cfg(test)] mod tests` block). The PR claims nothing about behaviour.
- **2 Silent failure.** No error paths, fallbacks or `.ok()` added or removed.
- **3 Safety switches.** `--native-loop` / `--verify-shares` / `--tls-fingerprint`
  untouched. *Out-of-scope observation, not a finding against this PR:*
  `parse_donate_level` (`bin/minertim.rs:257`) silently ignores an unparseable
  `--donate-level` and keeps the default 5 — no warning, and 5 is the *higher*
  donation. Pre-existing, unchanged here; worth its own issue if anyone cares.
- **5 Resource use.** The test builds seven `DonationSchedule` values (one `u8`
  each) and runs in 0.00 s. Nothing allocated behind a `cfg`.
- **7 Concurrency.** No threads, locks or channels in the diff.

### Item 6 — documentation and audit accuracy

`AUDIT.md` is a **pure append** (`@@ -5768,3 +5768,55 @@`, 52 insertions, 0
deletions) — the append-only rule is respected. The new test's doc comment sits
directly above its own `#[test] fn`, and `donated_fraction_matches_level` keeps
its own attribute: **no doc comment orphaned** (the repo has done this twice).
The `CLAUDE.md` row is well-formed and inserted before the `Pending` row.

Findings below.

## Findings

**F1 — minor. The PR body's test count is the `--lib` figure labelled as the full
suite.** It says *"Full suite: 150 passed, 0 failed."* Measured here:

```
cargo test --release   ->  168 passed, 2 ignored (3 suites)
lib binary --list      ->  152 tests (150 pass + 2 ignored)
bin binary --list      ->  18 tests
```

150 is correct for the lib target alone; the full suite is **168 passed, 2
ignored**. Consistent with SEC-02's recorded 167 (149 lib + 18 bin) plus this
PR's one new test. `AUDIT.md` says only "Full suite green", which is true, so the
error is confined to the PR body — the artefact this repo has found un-updated in
three separate round-2 reviews.

**F2 — minor. `AUDIT.md` FIX-01's "Files changed" omits `CLAUDE.md`.** It reads
*"`src/donate.rs` (one test), `AUDIT.md` (this entry)"*; the diff also adds the
FIX-01 task-board row to `CLAUDE.md`. FIX-01 is on an **unmerged** branch, so per
`CLAUDE.md` step 0 this may be corrected **in place** — no append needed.

**F3 — minor, and the one with a consequence beyond wording. The PR and FIX-01
both cite `./scripts/mutants.sh` and `PROC-06`; neither exists on this branch or
on `main`.**

```
$ git ls-files scripts/            -> scripts/verify-jit.sh        (only)
$ git ls-tree origin/main scripts/ -> scripts/verify-jit.sh        (only)
$ grep -c PROC-06 CLAUDE.md        -> 0
$ grep -n PROC-06 AUDIT.md         -> 5791, 5822  (both inside FIX-01 itself)
```

Both land in PR **#24**, which is still **OPEN**. Two consequences:
1. The quoted verification command **cannot be run as written** on this branch. I
   substituted the raw `cargo mutants` invocation and say so above; the reviewer
   reading FIX-01 later will not be able to.
2. If #26 merges before #24, `main`'s `AUDIT.md` gains a dangling script path and
   a dangling process ID — and merged `AUDIT.md` is corrected only by appending,
   so the cheap fix stops being available.

Zero-cost remedies: merge #24 first, or reword FIX-01 to quote
`cargo mutants --file src/donate.rs --re 'DonationSchedule::level' -- --lib 'donate::'`,
which is what the script wraps and which reproduces the figure exactly.

**F4 — nit. The test's doc comment states a reason that is false about the code
beneath it.** `src/donate.rs:115-116`:

> *"Values chosen to avoid both the clamp floor and the default, so neither a
> stuck `1` nor a stuck `5` can pass."*

The array is `[MIN_DONATE_LEVEL, 2, 3, 5, 10, 50, MAX_DONATE_LEVEL]` — it
**contains** the clamp floor (1) and the default (5) rather than avoiding them.
The claimed *effect* is real (the other five values kill any stuck constant, as
measured above), but the stated mechanism is not what the code does. The PR body
words it correctly ("chosen to defeat both plausible constants"); only the
in-code comment is wrong. Suggest "chosen so that no single constant can satisfy
them all".

**F5 — nit, context rather than defect.** "2 mutants, 2 caught" is an accurate
post-fix verification, but pre-PR that same filter reports **1 missed, 1 caught**
— the `-> 0` sibling was already killed by `floor_enforced`. So the fix closed
one mutant, not two. Nothing in the entry says otherwise; noting it so a later
reader does not infer a two-mutant gap.

### Verified clean — stated plainly

- **The PR's impact assessment is correct.** `.level()` has exactly one non-test
  caller in the whole tree: `src/pool_connection.rs:500`, inside
  `log::info!("Donation: mining to {:?} (donate-level {}%)", ...)`.
  `beneficiary_at` (`donate.rs:65-77`) reads `self.level`, the **field**. Grep
  covered `src/`, `src/bin/`, `benches/`, `scripts/`, `*.md`, `*.toml`. The
  donation split was never at risk; only the reported figure. `level()` is `pub`
  on a `pub` struct, so an out-of-tree consumer is theoretically possible, but
  there is none in this repository.
- **The test cannot pass vacuously** and no value is silently clamped —
  `clamp_level` is `clamp(1, 100)` and every element of the array is in range, so
  `MAX_DONATE_LEVEL` round-trips.
- **No neighbour left open.** 0 surviving mutants in `donate.rs` after the change.
- `cargo test --release` → **168 passed, 2 ignored, 0 failed** (exit 0).
- `cargo clippy --all-targets --release -- -D warnings` → **exit 0, clean**.
- Working tree left clean; every mutation restored from a scratchpad copy and
  confirmed with `git diff --exit-code src/donate.rs`. (`cargo mutants` leaves
  `mutants.out/` untracked and `.gitignore` has no entry for it on this branch —
  an artefact of my own run, removed; the ignore rule is #24's business.)

## Coverage ledger (final)

| # | Item | State |
|---|---|---|
| 1 | Correctness of the change vs. its claims | done — sound |
| 2 | Silent failure | done — N/A, nothing touched |
| 3 | Safety switches | done — untouched; one out-of-scope note |
| 4 | Tests — break-tested | done — gap reproduced, mutant killed, 9 constants |
| 5 | Resource use | done — negligible |
| 6 | Documentation & audit accuracy | done — F1-F5 |
| 7 | Concurrency | done — N/A |

## Verdict

**MERGEABLE.** No blockers, no majors. Three minors (F1, F2, F3) and two nits
(F4, F5), all documentation wording except F3's merge-ordering point.

The change is sound: it reproduces exactly the gap it claims, closes it, leaves
no neighbour open, and states its own impact at the right size rather than the
scariest one.

**ACTIONABLE:** F3 (decide the #24/#26 merge order, or reword the cited command
before this lands), F1 and F2 (one-line corrections; FIX-01 is unmerged so it may
be edited in place). F4 is a one-line comment fix worth taking while the file is
open.

**Not verified:** the JIT gate (`make verify-jit`) — not run, and not implicated:
the diff contains no `aarch64`, no `jit/`, no `vm.rs`. CI's five checks were not
observed; this is a local verification only.
