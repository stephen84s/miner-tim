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

**Direction of the confound.** Main's retained 7.42–7.71 range is dominated by
its two *warm* runs. If a warm machine raises the native-vs-body diff — plausible,
since the body-JIT arm pays a per-iteration register reload and so degrades more
under thermal pressure — then gating on absolute baseline preferentially removed
main's *low*-diff observations and kept its high ones, which manufactures exactly
the "branch entirely below main" pattern criterion 3 reported, with no code effect
at all. I cannot confirm the direction: it needs the per-run pairing of baseline
to diff, which the branch does not record (m4). It is stated here as the specific
confound that would explain the observed sign, and as unresolvable from the record.

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

---

# Round 2 — review of the corrections (`5817552`)

Fresh reviewer, spawned cold. Scope: `git diff f1a0a61..HEAD`, i.e. the fixes
written for round 1's findings, plus the four documents they touch.

**Verdict: MERGEABLE on code grounds — `src/`, `benches/` and `scripts/` are
byte-identical to `main` (`git diff origin/main -- src/ benches/ scripts/` is
empty), so nothing ships and no blocker is possible. But one major is
ACTIONABLE, so this is not the terminating round.**

## Coverage

| item | verdict |
|---|---|
| Withdrawal complete — no surviving regression claim | **clean** (only `AUDIT.md:4790`/`:4872`, both explicit withdrawals) |
| BENCH-02 figures real, same statistic | **verified** (`AUDIT.md:4523`) — with one softness, m6 |
| "Unpassable criterion" framing correct, not under-claiming | **correct**, not under-claimed |
| `PERF1_RUNS.log` genuine and internally consistent | **verified** — provenance thin, m7 |
| Every quoted figure derivable from the log | **all but one** — 0.25 pp, m5 |
| Discard/gate corrections accurate and complete | **NO — M2** |
| Audit-requirement sections; issue-numbering convention | **clean** bar n2 |

## M2 (major, ACTIONABLE) — the discard correction names the defect, then repeats it, and misattributes the criterion

`PERF1_CRITERION.md`: "A run below **either** figure ... is thrown away without
being read as a result." Applied literally the retained set is branch r1, branch
r2, main r1. Then, from `PERF1_RUNS.log`:

| row | as written | applied literally |
|---|---|---|
| 1. phase-1 positive in every comparison | FAIL — +0.25, **−0.02**, +0.07 | one evaluable pair survives (branch r1 vs main r1) = **+0.25, positive**; the −0.02 is main r2, the worst run in the set at 5.5% below known-good |
| 2. min(branch) > max(main), phase 1 | FAIL — 6.12 vs 6.15 | 6.15 is main **r3**, a discarded run; retained max(main) = 6.05, so 6.12 > 6.05 |
| 3. phase-2 branch not entirely below main | FAIL — 7.19–7.24 vs 7.42–7.71 | branch 7.19/7.24 vs main 7.42 — still fails |

Three defects, in one paragraph:

1. **"All three fail" does not survive the discard rule the same paragraph
   endorses.** The correction discloses provenance taint for **row 3 only**
   ("criterion 3's ranges above were computed from all six runs"). Rows 1 and 2
   have exactly the same taint and are not flagged — and row 2's failing datum
   (6.15) *is* a discarded run.
2. **The defect is repeated one sentence after being named.** The text says "the
   write-up silently discarded a *phase*", then immediately does it again:
   "Phase 1's six runs all passed their gate (>= 560 H/s) and are the decisive
   evidence." Under the rule as written those runs are gone entirely.
3. **Criterion 1 is misattributed.** It is explicitly a *phase-1* criterion
   ("Phase 1 (1 thread) effect is positive ..., at least three of each"). The
   correction reports its "three of each" as "**unmet for phase 2**" — the one
   phase where it does not apply — and thereby exonerates phase 1, where it is
   in fact unmet (2 branch, 1 main). Round 1's m2 said the count requirement is
   unmet and "**the criteria are unevaluable**"; the correction weakened that to
   phase 2 only.

**The fix target is "unevaluable at n=2 vs n=1", not "criterion 2 actually
passed".** Writing the latter would be the next round's over-claim. The revert
still stands — criterion 3 fails on retained data and "keep only if ALL hold"
cannot be met at any n — so the *outcome* is unaffected; the *record* is wrong.

Present in **three** documents: `AUDIT.md` (the "Two honesty corrections"
paragraph), `CLAUDE.md:162` ("phase 1's six all passed and are decisive"), and
the PR body (same two sentences). This repo has had stale cross-references
survive inside the commit fixing them; fix all three.

## m5 (minor) — the 0.25 pp "signal" is not derivable from the committed data

The withdrawal's headline arithmetic is "0.39 pp of spread ... *larger* than the
0.25 pp 'signal'". No phase-2 statistic in `PERF1_RUNS.log` is 0.25 pp: mean
separation 0.31 (7.2167 vs 7.5267), range gap 0.18, median difference 0.23.
`+0.25` does appear in the log — as the **phase-1** round-1 effect. So the
comparison quotes a phase-2 spread against a figure that is either rounded
loosely or borrowed from the other phase. Conclusion survives (0.39 > 0.31), but
in a PR whose deliverable is the record, a quoted number that cannot be
reproduced from the committed data is a finding. Also in `CLAUDE.md:162` and the
PR body.

## m6 (minor) — "same harness, same statistic, same configuration" is stated more strongly than BENCH-02 supports

BENCH-02's own entry records that the harness was **under modification across
those four runs** ("the paired interval is new in this change set, so runs 1-3
printed a bare point estimate") and that one of the four was a reviewer's
separately executed run. The per-thread paired statistic is tabulated for all
four, so the load-bearing number is probably comparable — but "same harness" is
an assertion the record does not fully back.

**Why m5 and m6 stay minor:** the withdrawal does not need BENCH-02 at all.
PERF1's *own* three `main` runs span **7.42–7.71 = 0.29 pp** on unmodified code,
in one session, on this machine — already at or above the 0.18–0.31 pp
separation being called a signal. Saying that would make the withdrawal
self-contained and immune to any softness in the BENCH-02 comparison.

## m7 (minor) — `PERF1_RUNS.log` has no provenance

Genuineness is well supported: every `===` header matches a `println!` in
`benches/nativeloop_ab.rs`; all twelve paired diffs reproduce from the printed
means to ±0.02 pp; and the per-thread mean exceeds the aggregate in all six
blocks (7.19>7.11, 7.42>7.39, 7.24>7.22, 7.71>7.61, 7.22>7.15, 7.45>7.38), a
consistent sign that is a signature of real paired data.

What it lacks: no date, no commit SHA per arm, no host facts, no command line.
The arm labels are `##### round N / branch #####` wrapper echoes, not
self-identifying output. This repo retracted "+9.01%" precisely because an arm's
identity came from outside the measurement, so a header line per block giving
`git rev-parse HEAD` and the date is cheap insurance.

## n2 (nit) — the new "Files changed" section omits `REVIEW_PR15.md`

It lists `compiler.rs`, `PERF1_CRITERION.md`, `PERF1_RUNS.log`, `CLAUDE.md`,
`AUDIT.md`. `git diff origin/main...HEAD --stat` also shows `REVIEW_PR15.md`
(+214), a repo-tracked file this PR adds.

## Checked and clean

- **Withdrawal is complete.** No surviving assertion of a regression in
  `AUDIT.md`, `CLAUDE.md`, `PERF1_CRITERION.md` or the PR body; both remaining
  mentions are the explicit withdrawal. The register-pressure speculation is
  gone from all four.
- **BENCH-02's figures are real and are the right statistic.** `AUDIT.md:4523`,
  row "per-thread paired (authoritative)": +7.37, +7.50, +7.31, +7.11, at
  11 threads x 12 pairs x 256 hashes — the same statistic and configuration
  `PERF1_RUNS.log` prints. +7.11 is indeed below the branch's 7.19–7.24.
- **Arithmetic in the log checks out.** Phase-1 effects +0.25/−0.02/+0.07;
  criterion-2 pair 6.12/6.15; criterion-3 ranges 7.19–7.24 / 7.42–7.71; gate
  failures at 4714.8, 4495.8, 4649.1; all six 1-thread means >= 560. Every
  figure in the three write-ups traces to the log except m5.
- **The gate correction is arithmetically right, with one ambiguity.** Issue #1
  does say "~4756 H/s at 11 threads" and "Discard runs whose baseline is well
  below that". The three percentages (0.9%, 5.5%, 2.2%) are all relative to
  4756 — but the sentence's nearest antecedent for "0.9% under **it**" is the
  4900 gate, against which 4714.8 is 3.8% under. Reword or repeat the referent.
  Retention 2-of-3 vs 1-of-3 is correct, and "documents, does not excuse" is the
  right strength.
- **"Unpassable criterion" is right and is not under-claiming.** No reading of
  the data supports a real effect either way: phase 1 means +0.10 pp, phase 2
  means −0.31 pp — *opposite signs*, against 0.29–0.39 pp of spread on unchanged
  code. The hedge "no sub-1% effect could have cleared either" is the correct
  hedge; a 1–2% effect would have cleared it easily, so the criterion was not
  unconditionally unpassable. (Worth adding: the two phases pointing opposite
  ways is itself the cleanest evidence for "noise".)
- **Round 1's m4 anomaly is now answerable and unremarkable.** Branch r3's
  phase-2 diff (7.22) sits *between* r1's and r2's despite a 6.7% lower body-JIT
  baseline — the paired diff is robust to thermals, which cuts *for* the
  withdrawal.
- **Issue numbering.** `#1` bare = GitHub, per `CLAUDE.md`'s convention note;
  correct throughout. Audit-requirement sections now all present (goal, files
  changed, behaviour, verification, assumptions, plus a "could not verify"
  paragraph).

## Not verified — stated, not implied

- The benchmark was **not re-run**; the log was checked for internal consistency
  and against the harness's own format strings, not reproduced.
- The "92/92 on the modified code" claim remains uncheckable, as the entry
  itself now says.
- No JIT gate was run: `src/`, `benches/` and `scripts/` are byte-identical to
  `main`, so the outcome is `main`'s.

**Actionable this round: M2 (must), m5/m6/m7/n2 and the "under it" referent
(should).**

---

# Round 3 (fresh reviewer) — scope: the corrections in `e5ddfaa` only

`git diff 73b3853..HEAD` = `AUDIT.md` (+79/-26), `CLAUDE.md` (1 row), a
provenance header on `PERF1_RUNS.log`. No source, no bench, no script.

## Coverage

| item | verdict |
|---|---|
| New two-column table true against log + criterion as written | **verified** — all six figures reproduce; see below |
| Did the correction over-correct into "criterion 2 passed"? | **no** — trap avoided explicitly (C1) |
| Revert justification follows from the criterion as written | **verified** (C2) |
| Withdrawal's new self-contained footing (0.29 vs 0.31 pp) | **verified**, one disclosure gap — m10 |
| BENCH-02 caveat accurate against BENCH-02's own entry | **verified** (C3) |
| Round 2's four smaller findings landed | m5/m6/m7/n2 **all landed**; m7's fix carries a new error — m9 |
| Round 2's major closed everywhere it was named | **NO — M3** |
| `CLAUDE.md` row vs `AUDIT.md`, data, numbering convention | numbering **clean**; one inconsistency — m8 |
| JIT semantics / bounds / native-loop preconditions / ABI / FPCR / diff tests | **vacuous** — `git diff main...HEAD --stat` is 5 files, 0 in `src/` |
| `make verify-jit` | **not run, not required** — `src/`, `scripts/`, `benches/` byte-identical to `main` (confirmed); the gate can only reproduce `main`'s result. Round 1's note stands: "92/92 on the modified code" is permanently unverifiable, the code is gone and the output was not captured |

Arithmetic re-derived independently from `PERF1_RUNS.log`: gate-retained set =
branch r1, branch r2, `main` r1 (r2-main 4495.8, r3-branch 4714.8, r3-main
4649.1 all < 4900). Row 1 all-six +0.25/-0.02/+0.07; row 2 min-branch 6.12 vs
max-main 6.15 (all six) and 6.12 vs 6.05 (retained); row 3 branch 7.19-7.24 vs
main 7.42-7.71 (all six) and 7.19/7.24 vs 7.42 (retained). Every figure checks.
The known-good percentages also check: 4714.8/4495.8/4649.1 are 0.9/5.5/2.2%
under 4756 and 3.8/8.2/5.1% under 4900, exactly as written.

## C1 — the specific over-correction round 2 warned about did NOT happen

Stated as a finding, not as absence of one. The entry writes "would read 6.12 >
6.05 at n=2 vs n=1" and immediately: "**That is not a claim the change
passed.** ... a 'pass' drawn from n=2 against n=1 is noise with a verdict
attached." `CLAUDE.md` follows "would even read as passing" with "The revert
stands regardless." Nowhere does either document drift toward implying the
change might have worked; "bought nothing" survives in both. "Unevaluable" is
the right word for criteria 1 and 2 on the admissible set: criterion 1's "at
least **three** of each" is unmet at 2-vs-1, and criterion 2 reads the same
phase-1 run set.

## C2 — the revert justification is the text, not a post-hoc reading

`PERF1_CRITERION.md`'s heading is literally "**Keep only if ALL of these
hold**". "A change that cannot be shown to clear the bar is not kept" is that
sentence restated. The outcome is also over-determined: criterion 3 = FAIL on
the retained set triggers "**Revert if any fails**" directly, and criterion 3 =
unevaluable falls back on "keep only if ALL". Both paths reach revert.

## C3 — the withdrawal's new footing verified

`main`'s three phase-2 per-thread means are 7.42/7.71/7.45 → span 0.29 pp;
arm means 7.2167 vs 7.5267 → 0.31 pp separation. Both as written. The BENCH-02
caveat is accurate against BENCH-02's own entry: +7.37/+7.50/+7.31/+7.11
(`AUDIT.md:4523`), 0.39 pp; "the harness changing across them" is supported by
"the paired interval is new in this change set, so runs 1-3 printed a bare
point estimate" (`:4539`); "one was a reviewer's separate execution" by
"one barriered run is the reviewer's, independently executed" (`:4517`).
Demoting it from support to corroboration is the right call and closes m6.

## M3 (major, ACTIONABLE) — round 2's major is closed in two of the three documents it named

Round 2: "Present in **three** documents: `AUDIT.md` ..., `CLAUDE.md:162` ...,
and the PR body (same two sentences). ... fix all three." Two were fixed.

- **`AUDIT.md:4807`** — a bare, unqualified `All three fail.` survives five
  lines below the paragraph that withdraws it, and directly contradicts the new
  second column. In isolation this is an editing miss (it is refuted in situ).
- **The PR body** — untouched, and it carries the entire superseded framing
  *coherently and with no withdrawal anywhere*: the three-FAIL table, "0.39 pp
  ... larger than the 0.25 pp 'signal'" (m5), "same harness, same statistic,
  same config" (m6), "criterion 1's 'three of each' is **unmet for phase 2**"
  and "Phase 1's six all passed (≥ 560 H/s) and are the **decisive evidence**"
  (M2 verbatim). Its review section says "Round 1 review" only.

For a PR whose sole deliverable is the record, the public artifact presenting
every withdrawn claim as the verdict is the finding. Both loci, one push.

## m8 (minor) — "unevaluable" is over-generalised in the prose, and `CLAUDE.md` inherits it

`AUDIT.md:4844` says "Neither phase has enough admissible data to decide
anything", and `CLAUDE.md` says "on the admissible three runs the criteria are
**unevaluable**" (plural). Both contradict the table five lines up, which marks
criterion 3 **FAIL** on the same set.

The **table is the defensible one**. "At least three of each" sits textually
inside criterion 1, a phase-1 rule; criterion 2 reads that same set; criterion
3 is phase 2 and carries no count clause at all — only "the branch's range must
not sit entirely below `main`'s", which {7.19, 7.24} vs {7.42} mechanically
fails. So the fix is to narrow the two prose sentences ("criteria 1 and 2 are
unevaluable; criterion 3 still fails"), not to touch the table. **The verdict is
not at risk either way** (see C2) — this is a sentence, not a re-litigation.

## m9 (minor) — the new provenance header states the host wrong

`PERF1_RUNS.log`: "Apple M2 Max, macOS (Darwin arm64), **12 performance
cores**." `sysctl` on this host: `hw.perflevel0.logicalcpu` = **8**,
`hw.perflevel1.logicalcpu` = 4, `hw.logicalcpu` = 12. This repo's own words at
`AUDIT.md:679`: "12 threads (~4,600) beat **8 P-cores** (~3,300)".

Not cosmetic here: 11 threads on 8 P-cores means three threads run on E-cores,
which bears directly on why phase 2 is the phase that cannot resolve the effect.
`AUDIT.md:3265` carries the identical error pre-existing ("M2 Max (12 logical
P-cores)"), so this is a phrasing habit rather than one typo.

## m10 (minor) — the 0.29 pp noise figure comes from runs the same entry discards, undisclosed

Not a double standard — `PERF1_CRITERION.md` itself measures spread across
ungated runs ("~0.4 pp (5.89-6.30 across five runs)"), so the convention is
established. The gap is disclosure: 0.29 pp is driven by `main` r2 at **7.71**,
the most compromised run in the set (5.5% under known-good), and the entry never
says its noise estimate is drawn from runs it elsewhere declares "thrown away
without being read as a result". Restricted to admissible runs, `main` is n=1
and no spread is measurable at all.

Also omitted: the **direction** of the confound. Hotter runs produced higher
paired diffs (`main`'s hottest run is its highest at 7.71), and `main` was the
hot arm in 2 of 3 rounds — so the apparent `main` advantage is plausibly
thermal. One sentence closes both, and it makes the withdrawal *stronger*.

## m11 (minor) — "**Review:** one round" contradicts the same entry

`AUDIT.md:4892` says "one round, `jit-reviewer`", while `:4836` in the same
entry says "**Round 2** caught that". Introduced by this commit.

## Checked and clean

- Round 2's m5 closed: the withdrawal now quotes the derivable 0.31 pp mean
  separation; no "0.25 pp" survives outside table row 1, where it is the correct
  phase-1 figure.
- Round 2's m1/"under it" referent closed and re-derived above.
- n2 closed: "Files changed" now lists `REVIEW_PR15.md`; the list matches
  `git diff main...HEAD --stat` exactly (5 files).
- m7's substance closed, and the header does the hard part well: it discloses
  that the `##### round N / arm #####` separators come from an **uncommitted**
  driver script, that arm identity rests on that script's `cwd`, and cites the
  retracted +9.01% claim as the reason to say so. That is the right handling for
  an artifact that cannot be reproduced from itself, and it is the strongest
  single improvement in this commit.
- Commit refs verified: `2a0afc3`/`bb3a469` have identical `compiler.rs` (so the
  header's "the modified `compiler.rs` of `2a0afc3`" is exact for the whole
  measurement window), `3e61471`'s `compiler.rs` == `main`'s, and `main` is at
  `c337229`. Author and committer dates 20:12 / 20:17 / 20:36 on 2026-09-06 —
  the six runs fit that window at ~2 min/run.
- Issue-numbering convention: `CLAUDE.md`'s bare `#1` = GitHub issue #1, correct
  per the note at `CLAUDE.md:121` (GitLab 1→1).
- No superseded claim survives in `PERF1_CRITERION.md` (unmodified, correctly —
  it is the pre-registration) or `PERF1_RUNS.log`.

## Not verified — stated, not implied

- I did not re-run the benchmark; all six runs are taken as reported, checked
  only for internal consistency.
- The driver script is not committed, so the arm-to-worktree mapping cannot be
  verified from the repo. The header says so, which is the correct disclosure.

## Verdict

**MERGEABLE.** One major (M3), four minors (m8-m11). **ACTIONABLE: yes** — the
PR body must be updated and `AUDIT.md:4807` deleted before merge; m8 is a
two-sentence narrowing. Nothing here changes the outcome: the revert is
correctly justified, the arithmetic is right, and the entry did not over-correct
into claiming the change might have worked. Apart from the PR body, the record
is now substantially accurate.
