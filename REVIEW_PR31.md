# Review: PR #31 (TEST-01 — issue #4 debug_assert! coverage)

Round 1, independent. Branch `docs/test-profile-coverage`, worktree
`.claude/worktrees/test-profile`. No `.rs` file touched in the PR's own
commits; the change is `AUDIT.md` + a `CLAUDE.md` task-board row.

## Coverage ledger

| # | Item | Status |
|---|---|---|
| 1 | Re-run >=2 break-test probes, CBRANCH mandatory | **Done** — CBRANCH + subs_imm |
| 2 | Scrutinise the CBRANCH reversal / Draft-2 filter explanation | **Done** — confirmed correct |
| 3 | Count `debug_assert!` inventory myself | **Done** — found a discrepancy |
| 4 | Judge the "fully closed" verdict / Not-established honesty | **Done** |
| 5 | Profile table vs real files | **Done** — accurate |
| 6 | AUDIT.md format + task-board GFM render | **Done** — found a discrepancy |
| — | Merge mechanics (not in the brief, found while orienting) | **Done** — blocker |

## Finding 0 (BLOCKER): PR is not mergeable — real conflicts with current `origin/main`

`docs/test-profile-coverage` branches from `b65fb86`, which predates PR #30
(`2fd056d`, "Help wording (#3)...", merged to `main`). PR #30 touched
`AUDIT.md`'s and `CLAUDE.md`'s task-board (added a `DOC-03` row in the same
slot this PR adds `TEST-01`) and also `src/bin/minertim.rs`. This branch was
never rebased.

Verified two ways:

1. **Live API**: `gh api repos/stephen84s/miner-tim/pulls/31 --jq '{mergeable,
   mergeable_state, rebaseable}'` → `{"mergeable":false,"mergeable_state":
   "dirty","rebaseable":false}`.
2. **Reproduced the merge**: `git worktree add --detach <scratch> origin/main`,
   then `git merge --squash docs/test-profile-coverage` →
   `CONFLICT (content): Merge conflict in AUDIT.md`,
   `CONFLICT (content): Merge conflict in CLAUDE.md`. Both are real
   `<<<<<<< HEAD` / `=======` / `>>>>>>>` conflicts on the task-board row
   (`DOC-03` vs `TEST-01` occupying the same line) and, in `CLAUDE.md`, on the
   "Runtime switches" paragraph PR #30 corrected.

**One thing this is *not*, checked and ruled out**: a naive
`git diff origin/main..HEAD` (which I ran first, before understanding the
history) makes it look like this PR reverts PR #30's `src/bin/minertim.rs`
wiring fix (the `native_loop_disabled_warning_for_target()` split and its two
tests, closing the "tested the helper not the wiring" defect DOC-03 fixed).
That is an artifact of diffing two trees with different bases, not something a
real merge does: PR #31's own three commits never touch `minertim.rs`, and the
squash-merge attempt above left that file with **zero** diff against `HEAD`
(no conflict, no change) — a proper merge/rebase will not resurrect the
pre-DOC-03 wiring bug. Flagging this explicitly so it is not mistaken for a
second blocker; it isn't one.

**What is real**: the branch cannot be merged today. It needs `git rebase
origin/main` (or an equivalent merge) with the task-board conflict resolved to
keep *both* `DOC-03` and `TEST-01` as separate rows, and the `CLAUDE.md`
"Runtime switches" section kept at PR #30's (corrected) text. This is exactly
the "Rebase on `main`, then merge on green" rule in `CLAUDE.md` step 0, and the
PR as it stands violates it.

## Finding 1 (MAJOR): the quoted break-test failure counts do not reproduce

Procedure per the brief: `cp` to `/tmp`, break one guard by inverting its
`debug_assert!` condition (the standard "invert a boundary comparison"
mutation), run `./scripts/verify-jit.sh` (or the exact same filtered
`cargo test --lib` the script uses, via `rtk proxy cargo ...` to avoid the
trailing-filter-mangling hook), capture exit code directly, restore from the
`/tmp` copy, confirm `cmp` byte-identical.

**CBRANCH forward-target guard** (`jit/compiler.rs:637`, `<` → `>=`):
ran the full `./scripts/verify-jit.sh`. Debug half: **12 failed**, 80 passed
(not the claimed **8**). Release half: 92/92 passed (as expected — the assert
compiles out). Overall: `verify-jit: FAIL — debug profile ... exited 101`,
`verify-jit: GATE FAILED on Darwin arm64` — so the *qualitative* claim (guard
reached, gate goes red) is correct, and both tests the entry names by name
(`full_hash_tests::test_native_loop_known_answer`,
`native_loop_diff_tests::native_loop_zero_iterations_terminates`) are indeed
among the 12 that failed. But the count itself, 8, is wrong by 50%. Full
12-test list committed nowhere but reproducible; the panic site in every
failure is `src/randomx/jit/compiler.rs:637:5`, confirming the same guard.
Restored, `cmp` byte-identical.

**`subs_imm` imm12 guard** (`jit/aarch64.rs:158`, `<` → `>=`): ran the same
filtered `cargo test --lib -- <JIT_FILTERS>` (the script's exact filter set,
via `rtk proxy`) in debug. Result: **11 failed**, 81 passed (not the claimed
**3**). Every failure panics at `src/randomx/jit/aarch64.rs:158:9`, the correct
site. Restored, `cmp` byte-identical.

Both mismatches go the same direction (actual >> claimed) and by a wide margin
(50% and 267%). This is the exact "reported figures do not reproduce" pattern
this repo has hit repeatedly (MEM-01's RSS figures, PERF-02's timing claims,
CI-03's first draft) and which this review was explicitly asked to check. The
entry's "Not established" section hedges on *correctness* of the guards and on
the *unprobed* 19 invocations, but not on whether its own quoted *counts* for
the four it did probe are right — they are two-for-four wrong. I did not
re-verify the `stp_fp_imm`/`ldp_fp_imm` pair (imm7 guards, claimed 4 and 1)
given the consistent pattern already found in the two I did check and the
~4-minute-per-probe cost (each full gate run is ~3-4 minutes for the debug
half alone); I'd expect them to be off too, but that's an inference, not a
measurement — noting it rather than claiming it.

**This does not overturn the core verdict on issue #4** — the guards genuinely
are reached in the debug profile, and the gate genuinely does fail when they
are violated, which is exactly what the issue asked to be shown. What's wrong
is the entry's specific evidentiary numbers, in an entry that opens by
insisting on "proven, not asserted."

## Finding 2 (MINOR): the debug_assert! inventory's per-file split is wrong

Entry claims: `jit/aarch64.rs` 16, `jit/compiler.rs` 5, `jit/memory.rs` 1,
`vm.rs` 1 (total 23 invocations, 24 matching lines, 1 a comment).

Counted with `grep -n "debug_assert!" <file> | wc -l` per file and cross-checked
against `grep -rn "debug_assert" src/` (24 lines) minus the one comment
(`vm.rs:43`, "a debug_assert would be absent..."):

- `jit/aarch64.rs`: **17** (not 16) — lines 139, 150, 158, 177, 179, 187, 190,
  200, 203, 395, 397, 409, 411, 417, 419, 425, 427.
- `jit/compiler.rs`: **4** (not 5) — lines 288, 637, 815, 827.
- `jit/memory.rs`: 1 — correct.
- `vm.rs`: 1 — correct.

The two headline totals (23 invocations, 24 matching lines with one a comment)
are exactly right — I reproduced them independently. Only the per-file
breakdown between `aarch64.rs` and `compiler.rs` is off by one in each
direction. Low-stakes on its own, but the entry explicitly says "An earlier
draft said 23 with a different distribution," flagging that the distribution
was already shaky once; this draft's distribution is still wrong, just
differently.

## Finding 3 (MINOR): the "36 rows against main's 35" GFM claim is stale, not false-comparison-proof

Rendered both `HEAD`'s `CLAUDE.md` and *current* `origin/main`'s `CLAUDE.md`
through `gh api --method POST /markdown -f mode=gfm`. Both produce exactly
**one** task-board table, and it has **36 `<tr>`** (35 data rows + 1 header) on
*both* — not 36 vs 35. Also confirmed 3 clean tables total on both (task
board, platform-coverage, versions), no stray blank-line table breakage — the
LEDGER-01/PROC-06/SEC-03 failure mode did **not** recur here, worth stating as
a clean pass rather than leaving it implicit.

The reason both are 36: `TEST-01` and `DOC-03` occupy the *same* row slot
relative to their common ancestor (`b65fb86`, which had 35 rows and no
`DOC-03`/`TEST-01`), so replacing one 35-row table's last row with a different
row keeps the row count at 36 either way — it is not additive. The entry's
"main's 35" was evidently computed against the stale pre-PR-30 base rather
than current `origin/main`, which is the same staleness as Finding 0's git
conflicts, showing up a second way.

## Finding 4 (confirmed correct): the CBRANCH reversal's explanation holds up

The entry says Draft 2's hand-built filter (`randomx::jit::` +
`randomx::vm::native_loop`, "69 tests") excluded `full_hash_tests` and
`native_loop_diff_tests`, and that is why it wrongly reported CBRANCH as
unreached. Verified: `cargo test --release --locked --lib --
randomx::jit:: randomx::vm::native_loop --list` reports exactly **69 tests,
0 benchmarks**, and by construction that filter string cannot match
`randomx::tests::full_hash_tests::*` or
`randomx::tests::native_loop_diff_tests::*` (different path prefixes). My own
CBRANCH break (Finding 1) shows those two suites are indeed where the failures
land. Draft 2's self-diagnosis is accurate, not merely plausible.

## Finding 5 (confirmed correct): the profile table matches the real files

Checked `Makefile`, `.github/workflows/ci.yml`, `.github/workflows/jit.yml`
directly:

- `make test` → `cargo test` (debug, whole `--lib`+bin suite, explicitly
  commented "NOT the JIT gate").
- CI `test` job → `cargo test --release --locked` on `ubuntu-24.04`
  (x86_64 — `randomx::jit` is `cfg`'d out there, confirmed via `mod.rs`
  elsewhere in this repo's history, not re-checked here since unchanged).
- `jit-macos` → `make verify-jit` → `scripts/verify-jit.sh`, which runs the
  debug group then the `--release` group, each via the same `JIT_FILTERS`.
- `jit-linux-arm` → `scripts/verify-jit.sh` directly (same two groups).
- `EXPECTED_PASSES=92` in the script matches an actual unmutated run in this
  session: debug 92 passed / 0 failed / 1 ignored; release 92 passed / 0
  failed / 1 ignored.

The entry's profile table is accurate.

## Verdict

**NOT MERGEABLE.**

- **Blocker**: the PR literally cannot be merged right now (`mergeable: false,
  mergeable_state: "dirty"`, confirmed live and by reproducing the conflict).
  Needs a rebase onto current `origin/main` with the `AUDIT.md`/`CLAUDE.md`
  task-board conflicts resolved (both `DOC-03` and `TEST-01` must survive as
  separate rows) and the "Runtime switches" section kept at PR #30's corrected
  text. This is mechanical, not a design problem — but it is real and it is
  not optional per CLAUDE.md's own rebase-before-merge rule.
- **Major**: two of the four quoted break-test failure counts (CBRANCH: 8
  claimed vs 12 actual; `subs_imm`: 3 claimed vs 11 actual) do not reproduce
  with the standard boundary-inversion mutation. The qualitative verdict on
  issue #4 (guards are reached in debug, gate fails when they're violated,
  gate is run in both profiles by both `jit-*` CI jobs) is sound and I
  reproduced it independently — but an entry whose entire point is "proven,
  not asserted" should not itself contain unverified numbers, and these are
  wrong by 50-267%.
- **Minor**: the `debug_assert!` per-file inventory split is off by one in two
  files (17/4 actual vs 16/5 claimed; totals still correct).
- **Minor**: the PR body's "36 rows against main's 35" is not true against
  current `main` (both are 36) — an artifact of comparing against the stale
  pre-rebase base rather than current `main`, same root cause as the blocker.

**ACTIONABLE**: yes, all four findings have a concrete fix — rebase, correct
the two counts (or explicitly caveat them as mutation-dependent and give the
actual reproduced numbers), fix the inventory split, and drop or correct the
row-count comparison.

**Not verified / left for a later round**: the `stp_fp_imm` (imm7, claimed 4)
and `ldp_fp_imm` (imm7, claimed 1) guards were not independently broken —
given the 2-for-2 pattern already found, I'd expect their counts to be wrong
too, but that is an inference, not a measurement. No probe was run on
`jit-linux-arm` (matches the entry's own disclosed gap). I did not verify
`cargo clippy --all-targets --release -- -D warnings` myself (the entry claims
clean); given no `.rs` file changed in the PR's own commits this is low-risk
and I did not spend the build time on it.
