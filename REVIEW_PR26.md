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
