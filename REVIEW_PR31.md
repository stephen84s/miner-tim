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

**Reframing, on reflection (an advisor call caught that my first pass invited a
cheap rebuttal):** "the counts don't reproduce" is rebuttable by "you used a
different mutation" — and that rebuttal would be correct. Inverting the
comparison (`<` → `>=`) fires the assert on *every* call to the function,
which is a different, broader break than e.g. narrowing the bound by one. The
count is a function of *which* mutation you choose, and **the entry never
states which mutation produced its counts.** That is the actual defect, and it
is not fixable by finding "the right" mutation to match 8 and 3 — the entry's
own methodology is silent on this, so its counts are unreproducible **by
construction**, independent of whether mine happen to differ.

What does reproduce unconditionally: the qualitative claim. Both guards, once
broken by any input-widening mutation, take the debug half from 92/0 to
failing, the release half stays 92/0 (assert compiled out), and the gate's
overall verdict flips to `GATE FAILED`, exit 101/1. That is what issue #4
actually asked to be shown, and it holds.

**A structural check that supports this without a third probe.** `stp_fp_imm`
(line 190) and `ldp_fp_imm` (line 203) carry **byte-identical** assert bodies —
same `% 8 == 0` pre-check, same `(-512..=504).contains(&byte_offset)` range,
same message — on the same emit path. The entry claims 4 failures for one and
1 for the other. Two structurally identical guards should not diverge 4:1
under a mutation that fires on real inputs; a 4:1 split is itself evidence that
whatever mutation produced these four numbers was not applied uniformly, or
was chosen post hoc per guard. Observation, not a third measurement — I did not
probe these two.

**This does not overturn the core verdict on issue #4** — the guards genuinely
are reached in the debug profile, and the gate genuinely does fail when they
are violated, which is exactly what the issue asked to be shown (confirmed
against the issue text itself, see Finding 1a below). What's wrong is that the
entry states four precise integers as "proven, not asserted" evidence while
omitting the one fact (the mutation used) that would make them reproducible,
and two of the four I checked land 50%-267% higher under the standard
mutation.

### Finding 1a: issue #4's scope is correctly matched (checked, not just read)

`gh issue view 4` names exactly three guard categories: "the imm7 range checks
on `stp_fp_imm` / `ldp_fp_imm`", "the `subs_imm` imm12 check", and "the CBRANCH
forward-target check." It does **not** name the CBZ zero-iteration patch range
or the back-branch imm19 range that `scripts/verify-jit.sh`'s own header
mentions in a different, broader context (and that `_shared-context.md`
records a real historical defect in — the imm19 sign bug). So the entry's "all
four guards the issue names" is accurate: three named *categories* produce
four concrete assert *sites* because the imm7 bullet covers two functions. No
scoping error here — checked because it would have been a major if wrong.

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

**For whoever resolves the Finding 0 rebase**: once the conflict is resolved
correctly — keeping *both* `DOC-03` and `TEST-01` as separate rows — the
correct comparison is **37 rows against current `main`'s 36**, not "36 vs 35."
Recording this now so the corrected PR body does not repeat the same stale-base
error in a new form.

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
- **Major**: the entry states four precise break-test failure counts (4, 1, 3,
  8) without ever recording which mutation produced them — so they are
  unreproducible by construction, not merely "wrong." Confirmed the gap is
  real, not theoretical: under the standard boundary-inversion mutation, two
  of the four I checked came in far higher than claimed (CBRANCH: 12 vs 8;
  `subs_imm`: 11 vs 3), and the claimed 4:1 split between the two
  byte-identical imm7 guards (`stp_fp_imm`/`ldp_fp_imm`) is itself implausible.
  The qualitative verdict on issue #4 — confirmed correctly scoped against the
  issue text (Finding 1a) — is sound: the guards are reached in debug, the
  gate fails when they're violated, and both `jit-*` CI jobs run the gate in
  both profiles. But an entry whose entire framing is "proven, not asserted"
  should not itself carry integers nobody could reproduce without guessing the
  method.
- **Minor**: the `debug_assert!` per-file inventory split is off by one in two
  files (17/4 actual vs 16/5 claimed; totals still correct).
- **Minor**: the PR body's "36 rows against main's 35" is not true against
  current `main` (both are 36) — an artifact of comparing against the stale
  pre-rebase base rather than current `main`, same root cause as the blocker.
  After a correct rebase (keeping both `DOC-03` and `TEST-01`) the true
  comparison will be 37 vs 36, not 36 vs 35 — flagging so the fix doesn't
  repeat the error in a new form.

**ACTIONABLE**: yes, all findings have a concrete fix — rebase; either name the
mutation behind each count or drop the counts and keep the pass/fail evidence;
fix the inventory split; correct the row-count comparison to 37 vs 36 once
rebased.

**Not verified / left for a later round**: the `stp_fp_imm` (imm7, claimed 4)
and `ldp_fp_imm` (imm7, claimed 1) guards were not independently broken —
given the 2-for-2 pattern already found, I'd expect their counts to be wrong
too, but that is an inference, not a measurement. No probe was run on
`jit-linux-arm` (matches the entry's own disclosed gap). I did not verify
`cargo clippy --all-targets --release -- -D warnings` myself (the entry claims
clean); given no `.rs` file changed in the PR's own commits this is low-risk
and I did not spend the build time on it.
