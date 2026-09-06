# REVIEW_PR15 — round 1, independent

PR #15, `perf/jit-fmov` @ `f1a0a61`, base `origin/main` @ `c337229`.
Reviewer: independent (jit-reviewer brief). Not the author of the change.

**Verdict: MERGEABLE.** No blockers. One **major** (a claim in `AUDIT.md` that
this repo's own prior data contradicts — worth fixing *before* merge, because
`AUDIT.md` is append-only and can afterwards only be corrected by a second
entry), four minors, one nit.

## Coverage

| # | Item | Result |
|---|---|---|
| 1 | Revert complete, nothing leaked | **Verified clean** |
| 2 | Criterion genuinely pre-registered; correction legitimate | **Verified** |
| 3 | Arithmetic and verdict internally consistent | Verified; see M1/m2 |
| 4 | Instruction-count claim 111→103 / 131,072 | **Reproduced** (one half run, one half derived) |
| 5 | Handling of the three discarded runs | **M1**, **m1**, **m2** |
| 6 | Over/under-statement in AUDIT / CLAUDE.md / PR body | **M1**, **m3**, **n1** |
| 7 | `make verify-jit` | Not re-run — see "Not verified" |

## 1. The revert is complete

`git diff origin/main -- src/` and `-- benches/` are both empty; `git diff
origin/main...HEAD --stat` is `AUDIT.md` +81, `CLAUDE.md` +1,
`PERF1_CRITERION.md` +94 and nothing else. `origin/main` is an ancestor of
`HEAD`, so the three-dot and two-dot diffs agree. Line counts are an exact
inverse: `2a0afc3` added 36 / removed 6 in `compiler.rs`, `3e61471` added 6 /
removed 36. No stray helper, no changed test, no altered emitter. Clean.

## 2. Pre-registration holds; the correction is legitimate

Commit order and timestamps: `2a0afc3` 20:12:20 (implementation +
`PERF1_CRITERION.md`), `bb3a469` 20:17:56 (correction), `3e61471` 20:36:17
(revert + results). The criterion and its correction both precede the results
commit. Author and committer dates are identical throughout, so no rebase
rewriting is hiding an earlier ordering.

The correction's justification is **sound**. The harness's paired diff is
native-loop vs body-JIT, already ~+6% (`AUDIT.md:1589`), so "CI excludes zero,
positive" was indeed trivially satisfied and measured nothing.

The load-bearing claim — `emit_iteration_pre` is called **only** from
`compile_native_loop` — is **true**. The only two references in the tree are
`compiler.rs:809` (inside `compile_native_loop`) and `compiler.rs:1089` (inside
`#[cfg(test)] mod tests`, from line 1047). I checked the stronger version too:
`2a0afc3` left `emit_cvt_packed_int` as a wrapper delegating to
`emit_cvt_packed_int_to(e, 25, 26)`, so the three body-JIT callers
(`emit_fadd_m`, `emit_fsub_m`, `emit_fdiv_m`, at 547/563/585) emit **byte-identical
words** after the change. The body-JIT arm is a genuinely untouched control at
the emitted-code level, not merely at the source level. The measurement design
stands.

## 3/4. The instruction-count claim is exact

`cargo test --release --lib native_loop_emitted_instruction_accounting` on this
branch prints `pre=111 post=55 +2`, `ADDED per hash: 2752512` — matching the
`main` column. `(111+55+2) × 16384 = 2,752,512` ✓.

The branch column is derived rather than re-run (see "Not verified"), but it is
arithmetically certain: `add_reg`, `add_imm`, `scvtf_dx` and `fmov_dd` each emit
exactly one word (`aarch64.rs:128/138/495/512`), so the old f-lane cost
1+5+1+1 = 8 words and the new one 1+5 = 6. 2 × 4 lanes = 8/iteration → 111−8 =
**103**, and `(103+55+2) × 16384 = 2,621,440` ✓, `2,752,512 − 2,621,440 =
131,072` ✓. Title, PR body, `AUDIT.md` and `CLAUDE.md` all agree.

The reported timing figures are also internally consistent: effects
{+0.25, −0.02, +0.07} pp reconcile with min(branch) 6.12 and max(main) 6.15 under
index-wise pairing (e.g. main 6.15/−0.02 → branch 6.13; main 6.05/+0.07 → branch
6.12). Each criterion's stated verdict follows from the criterion as written in
`PERF1_CRITERION.md`; no criterion was reworded between the file and the
write-up. The revert verdict is correct.

## M1 (major) — criterion 3's "regression signal" is contradicted by BENCH-02

`AUDIT.md` PERF-02 and the PR body both say: "at 11 threads the branch sat
**consistently below** `main` by ~0.25 pp, in all three runs, across widely
varying absolute baselines. That is a weak signal of a small *regression* …
plausible if removing the FMOVs shifted register pressure or scheduling, and
equally plausible as noise."

`AUDIT.md`'s own BENCH-02 entry records four barriered runs of **unmodified
`main`**, same harness, same statistic (per-thread paired diffs), same config
(11 threads × 12 pairs × 256 hashes): **+7.37, +7.50, +7.31, +7.11**. That is
0.39 pp of run-to-run spread on *identical code* — larger than the 0.25 pp
"signal" — and the lowest of them, **+7.11, sits below the branch's entire
7.19–7.24 range**.

So the observation is not "equally plausible as noise"; on the repo's own prior
data for this exact statistic it is **indistinguishable from noise**, and the
register-pressure mechanism is offered without anything to support it. The
"across widely varying absolute baselines" phrase compounds this: it is
presented as strengthening the signal, when varying baselines are precisely the
confound (main's high range is dominated by its two warm runs — see m1).

Suggested fix, before merge: withdraw the regression reading, cite BENCH-02's
7.11–7.50 unmodified spread as the reason, and state the observation as "inside
the harness's documented run-to-run spread". One sentence, not another hedge.
Raised as major only because `AUDIT.md` is append-only once this lands.

## m1 (minor) — the ≥4900 gate is stricter than the source it cites, and its effect was asymmetric

Issue #1's verification section names ~4756 H/s at 11 threads as known-good and
says to "discard runs whose baseline is *well below* that". `PERF1_CRITERION.md`
sets the gate at **≥4900**, justified from BENCH-02's recent observed band
(4982–5020.7) — which BENCH-02 itself describes as "against a recorded ~4756".

Consequence: branch run 3 at **4714.8** is 0.9% below the figure issue #1 calls
known-good — not "well below" it — and was discarded. Main's two (4495.8, 4649.1)
are 5.5% and 2.2% below and are more defensibly warm. The retention split is
therefore asymmetric: branch 2/3, main 1/3, and criterion 3 compares arms
measured under different machine conditions.

Mitigating and material: the gate was **pre-registered**, so no threshold was
picked after seeing which runs it would cut. This is a calibration question, not
evidence of gaming. It belongs in the record because it is the mechanism behind
M1.

## m2 (minor) — the discard rule was applied per *phase*; the criterion says per *run*

`PERF1_CRITERION.md`: "A run below **either** figure was thermally compromised
and is thrown away without being read as a result." The write-up instead keeps
the phase-1 data from runs that failed only the 11-thread gate ("Phase 1's six
runs all passed their gate … phase 1 alone is decisive"), and computes criterion
3's ranges from **all six** runs — the reported `main` 7.42–7.71 must include the
two discarded runs, since only one main run survived. That sits badly beside the
same entry's "They are discarded rather than interpreted, which is what the gate
is for."

Two consequences worth stating rather than leaving implicit:
- Under the literal rule, only 2 branch and 1 main run survive, so criterion 1's
  "at least three of each" is **not met** and the criteria are unevaluable — the
  protocol-faithful action would have been three more runs on a cool machine, not
  a verdict. The PR pre-empts this ("nothing here suggests a positive effect more
  data would reveal"), which is a reasonable judgement but is not a call the
  pre-registered criterion authorises.
- It does **not** change the outcome. "Keep only if ALL hold" cannot be satisfied
  either way, so the deviation runs in the conservative direction.

## m3 (minor) — the pre-registered rule set had no reachable path to "keep"

Criterion 2 requires min(branch) > max(main) against a spread the same document
puts at ~0.4 pp and calls "larger than any effect this change can plausibly
produce". Criterion 3 ("range must not sit entirely below") is near a coin flip
at that spread with three runs per arm. Taken together the criteria could not
have been satisfied by any outcome this change could produce.

Pre-registration did its main job — the threshold could not be chosen after the
data, which is real and worth the credit the entry gives it — but it did not
pre-register a decision the data could influence. The honest one-line framing is
**"the change is smaller than this harness can resolve"**, not "the change failed
three criteria". Method observation, not misconduct; the outcome errs safe.

## m4 (minor) — no primary data for a PR whose deliverable *is* the record

Nothing in the branch carries the six runs' harness output — no log, no
appendix, no untracked file. What survives is min/max ranges and three rounded
deltas. Criterion 2 cannot be rechecked, the effects cannot be recomputed, and
the mapping from each effect to its run (which m1/m2 turn on) is unrecoverable.

Related and unexplained: the branch's three phase-2 means span **0.05 pp**
(7.19–7.24) against main's 0.29 pp in the same session and 0.39 pp on unmodified
code in BENCH-02. Either those runs were implausibly consistent for this machine
or they are not three independent run-level means. Unanswerable from the record,
which is the point. This repo has already had reported figures fail to reproduce
(MEM-01); for a PR that ships *only* a record, the raw output belongs in it.

## n1 (nit) — PERF-02 has no "Files changed" line

`CLAUDE.md`'s audit requirement lists files changed as a required element. PERF-02
covers it only implicitly ("byte-identical to `main`"); the 2026-08-15
`emit_mem_addr` entry it cites as precedent has an explicit `### Files Changed`
section.

## Checked and clean

- Issue-numbering convention: `#1` = GitHub issue 1, correct per `CLAUDE.md:121`.
- `emit_mem_addr` precedent as summarised ("0.35% fewer, measured slower, then
  null, reverted") matches `AUDIT.md:878-897`.
- PERF-02 is appended at the end of `AUDIT.md`, not edited in place.
- Harness arm identity is set **explicitly** on both arms
  (`benches/nativeloop_ab.rs:134,136`: `set_native_loop(false)` / `(true)`), so
  the defect that produced the retracted +9.01% is not present here.
- No single-run interval is presented as reproducibility anywhere in this PR.

## Not verified — stated, not implied

- **`make verify-jit` was not re-run.** `src/`, `scripts/` and `benches/` are
  byte-identical to `main`, so the result is fully determined by `main`'s. This
  is a reason, not a check.
- **"92/92 debug and release on the modified code" is unverifiable from the
  record** — no log was committed, and the code no longer exists in the tree. I
  did not rebuild `2a0afc3`; nothing ships from it. Corroboration does exist and
  is worth noting: `assert_arms_agree` (`nativeloop_ab.rs:195,345,372`) compares
  the modified native arm against the *unmodified* body JIT every round, and six
  runs completed without a panic — so the modified emitted ARM64 produced hashes
  identical to the body JIT across thousands of programs. That is real
  differential evidence, but it is corroboration, not confirmation of the 92/92
  figure.
- **The branch's `iter_pre = 103`** is derived from the emitter encodings, not
  observed from a build of `2a0afc3`.
- **The six runs themselves** were not reproduced (instructed not to; ~4 min per
  run) and cannot be, from what the branch records — see m4.
