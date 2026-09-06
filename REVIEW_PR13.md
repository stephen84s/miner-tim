# REVIEW_PR13 — round 1, independent

**PR #13** "Barrier the multi-thread A/B phase, and measure the bias a barrier
could introduce" — `fix/bench-barrier`, base `main`. Closes #5.

Scope: `benches/nativeloop_ab.rs` (+105/−6), `AUDIT.md`, `CLAUDE.md`. No `src/`
change, confirmed by `git diff origin/main...HEAD --stat`. So no emitted ARM64
changed and no wrong-hash risk exists in this PR. Everything below is about the
instrument that produces this project's published numbers.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Barrier correctness (wait counts, timed regions, single-thread path) | done — correct |
| 2 | Does the barrier deliver the concurrency claim | done — **partly**, F1 |
| 3 | Tail-idle reasoning and `(max-min)/mean` | done — F2, F3 |
| 4 | Arithmetic and CI computation; strength of claims | done — F4, F5, F7 |
| 5 | Per-thread paired-diff reporting | done — computation right, F6 |
| 6 | Overstatement | done — F5, F6, F7, F8 |
| — | Ran the harness (3 threads and 11 threads) | done |
| — | Reproduced the PR's two paired 11-thread runs | **NOT done** — see "What I could not verify" |

---

## Item 1 — the barrier is correct

Verified by reading, and by two live runs.

* **Wait counts are identical on every thread.** `ab_phase` executes
  `sync()` twice before the two warm-up rounds, then exactly four per pair
  (lines 143, 145, 153, 155, 157, 159): `2 + 4·pairs` on every thread,
  independent of `tid` and of any timing. `Barrier::new(threads)` matches the
  `threads` spawned in `thread::scope`; the main thread does not participate.
  No deadlock, no desynchronisation.
* **Every wait is outside every timed region.** `round()` takes
  `Instant::now()` as its first statement (line 50) and every `sync()` is on the
  statement *before* a `round(...)` call. Nothing timed contains a wait.
* **The single-thread phase is unaffected.** `ab_phase(&dataset, 0, pairs,
  hashes, None)` (line 286) makes `sync` a no-op closure. Phase 1 numbers are
  comparable to `main`'s.
* **Aggregate indexing still lines up.** `base_rates` is pushed `[ta, td]` per
  pair and `nat_rates` `[tb, tc]`, so index `i` denotes the same global round
  number on every thread; summing across threads at index `i` sums the same
  round. Unchanged by this PR and still right.

No finding.

## Item 2 — what the barrier actually delivers

**F1 (minor): the barrier enforces a common *start*, not a common *duration*,
but the new comment says "enforced".**

Line 310–317 now reads "Round i of thread 0 is concurrent with round i of every
other thread — *enforced* by the barrier above". The barrier releases all
threads together; nothing makes them finish together. The aggregate then sums
per-thread rates measured over *unequal* windows:

    base_agg[i] = Σ_t h/T_t  =  n·h / HM(T)

whereas the true throughput over the barrier-to-barrier window is `n·h /
max(T)`. `HM(T) ≤ max(T)`, so the sum-of-rates overstates the concurrent
aggregate by a factor monotone in the across-thread spread of `T`. That residual
is exactly the tail-idle effect — it arrives through arithmetic before any
argument about memory pressure. The comment as written claims more than the
barrier gives.

The harness already holds the data for the correct statistic: `n·h/max(T)` per
round, equivalently `threads · min_t(rate_t)`. Recommending, not requiring.

## Item 3 — the tail-idle check

**F2 (minor): `(max-min)/mean` is a valid proxy but a low-power one, and the
harness throws away the power it does have.**

Rate spread is monotone in time spread, so a systematic tail-idle asymmetry
*would* show up in this statistic — it is the right family. But the range is the
least efficient dispersion estimator (2 of 11 points decide it), and
`spread_report` builds a per-round vector `cvs` of length `rounds` and then
discards the distribution, printing only its mean and median. A **within-run
paired CI across the ~24 rounds** is available for free and would replace two
point estimates with an actual test. The exact idle fraction is also directly
computable from the stored rates:

    idle_fraction(round) = 1 − (1/n)·Σ_t (rate_min / rate_t)

Also: the function is named `arm_cv` and the printed quantity is range/mean, not
a coefficient of variation. Misleading name.

**F3 (minor): n=2 cannot support "there is no systematic tail-idle bias".**

The PR and AUDIT both conclude, from `−0.40 pp` then `+0.60 pp`, that "the check
passes … there is no systematic tail-idle bias". A sign flip in two observations
distinguishes nothing: it is equally consistent with a true mean of 0 and with a
true mean of +0.6 pp under noise, and the two values in fact average +0.10 pp.
What would suffice: the within-run paired test of F2 (n≈24 rounds, real power,
same runtime), and ≥5 runs before any between-run statement.

**F4 (minor): neither the PR nor AUDIT reports the *level* of the spread on
either arm — only the difference.** Tail-idle risk cannot be assessed from a
difference of two numbers that are never given. I measured it myself; see
"Measurements I made".

## Item 4 — the numbers

Arithmetic checked and correct: `(7.21+7.33)/2 = 7.27`, `(7.34+7.46)/2 = 7.40`,
rise `0.13`. Every quoted CI half-width matches its stated interval.

`mean_ci95` is correct: sample variance with `n−1`, `SE = sqrt(var/n)`, t from a
df table that deliberately rounds to the *lowest* df in each bucket so intervals
err wide. df=23 → 2.086 against the true 2.069 (conservative, as documented);
df=10 → 2.228, which is the exact value. No defect.

**F5 (minor): the "+0.13 pp rise" is recorded in `AUDIT.md` as one of "three
things that reproduce", and the quantity is too noisy between processes to
support any claim at that scale.**

My own barriered 11-thread run on the same host (12 pairs x 256 hashes, machine
in a comparable state — single-thread body JIT 572.9 H/s against the PR's
568.1-572.7, 11-thread body JIT 5020.7 H/s against the PR's 4982-5003) measured
the aggregate diff at **+7.28%** — *inside* the unbarriered pair's range
[+7.21, +7.33] and below both barriered figures. Barriered observations are now
+7.34, +7.46, +7.28; unbarriered +7.21, +7.33.

**I am deliberately not calling this a falsification.** My run is a separate
process, hours later, in a different machine state, so it is exactly the kind of
unpaired between-process comparison this finding objects to — it cannot refute
the PR's figure any more than the PR's four runs can establish it. What the
third sample *does* establish is that the between-process spread of this
quantity is at least as wide as the 0.13 pp being attributed to the barrier.

It is also confounded with run order. Barriered − unbarriered is +0.13 pp in run 1 and +0.13 pp in run 2;
but run 2 − run 1 is +0.12 pp in the *unbarriered* arm and +0.12 pp in the
*barriered* arm. The claimed barrier effect is the same magnitude as an
uncontrolled between-run drift. The four runs are separate processes with no
pairing and — critically — **their order is not recorded anywhere in the PR or
AUDIT**. If they ran unbarriered₁, barriered₁, unbarriered₂, barriered₂, a
monotone drift reproduces this exact pattern with no barrier effect at all. The
harness itself alternates A-B-B-A precisely to cancel drift; the experiment
validating the harness was run without that protection. Either record the order
(and ideally interleave), or drop the sentence "the point estimate rises
slightly … the direction the issue predicts". As stated it asserts a mechanism
the data cannot separate from tail-idle inflation, which predicts the same sign.

Recommended: strike the point-estimate-rise sentence from the PR body, the
AUDIT entry and the CLAUDE.md task-board row, or restate it as "not separable
from run-to-run drift". The entry is on an unmerged branch, so per this repo's
protocol it may still be edited in place rather than appended to. **This does
not touch the headline** — every measurement, mine included, lands in +7.2..+7.5%.

**Not a finding — the "halves the CI" claim is fine, and my run supports it.**
My barriered aggregate CI was ±0.19, tighter than both the PR's barriered runs
(±0.24, ±0.29) and far tighter than either unbarriered run (±0.43, ±0.64). Third
independent observation of the narrowing. ±0.43→±0.24 and
±0.64→±0.29, twice, and it is explicitly hedged: "two paired runs establish that
the narrowing reproduces, not its exact size." That is the right amount of
claim for n=2.

**F7 (minor, accuracy): "still inside JIT-01's recorded +6.8%–7.4%" is false for
two of the four barriered point estimates** — the run-2 aggregate (+7.46%) and
both per-thread figures' run 2 (+7.50%) sit above 7.4%. True of the mean only.
The sentence appears in both the PR body and AUDIT.

## Item 5 — per-thread paired diffs

Computation is right: elementwise `zip` of `base_rates`/`nat_rates` reproduces
the same antithetic `(ta,tb)`,`(td,tc)` pairing that `report` uses, means the 24
per-round diffs within a thread, then takes a t-interval across the 11 thread
means with df=10 → t=2.228. Correct.

**F6 (minor): "n=11, the genuinely independent unit" is overstated, and the
barrier makes it less true, not more.** Thread-level diffs are positively
correlated through shared memory bandwidth, shared LLC and shared thermals. The
barrier *increases* that coupling: every thread's round window is now bounded
below by the slowest thread's, so a hiccup on one thread perturbs the idle tail
every other thread experiences. A change made partly in service of an
independence complaint has strengthened the dependence between the units it now
calls independent. "More nearly independent than 24 serially-adjacent rounds" is
defensible; "genuinely independent" is not.

## Item 6 — overstatement, and the failure-mode regression

**F8 (minor): "a barrier that corrupted the schedule would fail loudly rather
than quietly skewing a number" is a non-sequitur, and the failure is now a
hang.**

Two separate problems with that verification sentence (PR body and AUDIT):

1. *The divergence assert cannot detect barrier defects.* Each thread's hashes
   are a pure function of its own blob and its own nonce counter. Barrier
   placement changes *when* rounds run, never which nonces are hashed, so the
   checksums are invariant to barrier placement entirely. "Four runs, no
   divergence assert" is evidence about the JIT, not about the barrier.
2. *The assert's own failure mode regressed.* A panicking thread never reaches
   its next `sync()`, so all siblings block forever on `Barrier::wait()` (no
   timeout, no poisoning) and `thread::scope`'s join never returns.
   Reproduced with a standalone 30-line program using the same primitive: the
   panic message prints, then the process hangs until killed (SIGALRM, exit
   142). So the harness prints the correctness message and then deadlocks
   instead of exiting — degraded, though not silent.
   Cheap fix if the author wants it: accumulate the per-pair checksums and
   assert after the loop, so a mismatch cannot strand siblings at a barrier.

**F9 (minor): the headline number quoted in AUDIT and the CLAUDE.md task board
is the aggregate — the one the PR itself says overstates independence.** Keeping
both statistics is defensible and the PR's admission is honest; the defect is
that nothing tells a reader (or a future session quoting harness stdout, which
is how this project's numbers have historically travelled) which is
authoritative. Recommend the per-thread figure become the headline, or that the
harness output label one of the two.

---

## Measurements I made

Two runs of the branch's harness on this host (M2 Max, 12 cores), `caffeinate -i`.

**`cargo bench --bench nativeloop_ab -- 3 2 32`** — cheap barrier exercise.
Completed, no deadlock, no divergence assert. Spread 0.77% / 0.77%, asymmetry
−0.01 pp.

**`cargo bench --bench nativeloop_ab -- 11 12 256`** — the configuration the
PR's table uses:

```
=== across-thread spread within a round (barrier tail-idle check) ===
  body JIT     : (max-min)/mean  mean  5.64%   median  4.72%
  native loop  : (max-min)/mean  mean  4.54%   median  4.58%
  asymmetry    : -1.10 pp (native - body)
=== 11 threads (aggregate) ===
  paired diff  : +7.28%  (95% CI +7.09% .. +7.47%, n=24)
=== 11 threads (per-thread paired diffs) ===
  mean of per-thread means: +7.31%  (95% CI +7.03% .. +7.60%, n=11)
  per-thread   : +7.1% +6.9% +7.4% +7.4% +7.4% +7.7% +8.1% +7.8% +6.9% +6.8% +6.9%
```

Three things this settles.

1. **The absolute spread is a few percent, not tens.** That is the number that
   decides how much tail idle matters, and neither the PR nor AUDIT reports it
   (F4). With ~5% range the barrier leaves the machine only slightly
   underloaded in the tail, so the PR's conclusion — that the barrier is the
   right fix — is *correct*, for a reason it never gave. Had this come out at
   30–40% (11 threads on 8P+4E is a plausible way to get there) the aggregate
   should have been dropped rather than kept.

2. **The tail-idle artifact is empirically bounded well under 0.1 pp on this
   host.** In the *same* run, the aggregate (sum-of-rates, the estimator F1
   objects to) gave **+7.28%** and the per-thread paired estimator (no
   aggregation artifact at all — each thread compared against itself) gave
   **+7.31%**: a 0.03 pp gap, in the presence of a −1.10 pp spread asymmetry.
   Two estimators with quite different exposure to the tail bound the artifact
   at a few hundredths of a percentage point.

   For the record, the algebraic upper bound does *not* hold. `mean − min ≈ R/2`
   makes the sum-of-rates overstatement `≈ 1 + R/2` against true window
   throughput, and `F_nat/F_body − 1` for my R values is −0.56 pp — which would
   put the "true" figure near +7.84% and disagree with the per-thread estimator
   by half a point. It does not, because the overstatement is very nearly
   *common to both arms* and cancels in the ratio; what survives is second-order
   in the spread difference, not first-order as that model assumes. Trust the
   0.03 pp empirical gap, not the 0.56 pp derivation.

3. **The asymmetry is noisier than n=2 suggested.** Three observations are now
   −0.40, +0.60, **−1.10** — a 1.7 pp range around a mean of −0.30. The PR's two
   samples did not bracket the third. Direct support for F3.

Also worth recording: the 11 per-thread diffs do **not** cluster into two groups
(6.8–8.1%, no P-core/E-core bimodality), so the t-interval across threads is at
least applied to a plausibly exchangeable sample. That is the one part of the
"n=11" framing that survives; the independence half does not (F6).

## What I could not verify

* I did **not** reproduce the two paired 11-thread runs the PR's table comes
  from, and did not run the unbarriered `main` arm at all. The +0.13 pp and the
  CI-halving figures are unreproduced by me.
* The **order** of the PR's four runs is not recorded and cannot be recovered
  from the repository; part of F5 rests on that gap.
* I did not measure a **barriered vs unbarriered pair inside one process**,
  which is the experiment that would actually settle the +0.13 pp question. The
  harness cannot currently do it.
* The `1 + R/2` sensitivity model in "Measurements I made" (2) is a
  symmetric-spread approximation that the data contradicts; it is recorded as a
  rejected upper bound, not as a finding. The harness does not retain the raw
  per-thread round times, which is what a proper derivation would need.
* Phase 1 (single thread) measured +5.86% in my run, below JIT-01's band — but
  neither the PR nor AUDIT quotes a single-thread *diff*, only single-thread
  baseline H/s (which matched), so this is out of scope for PR #13 and I did not
  pursue it.

---

## Verdict

**MERGEABLE.** The code change is correct and is a real improvement to the
instrument: the barrier is properly counted, properly placed outside every timed
region, inert on the single-thread path, and it removes a genuine variance
source. Nothing in `src/` changed, so there is no wrong-hash exposure. The PR is
also unusually honest about its own limits — it names the trap in its own fix,
implements both of the issue's options, and labels its n.

**Nine minors. No blockers, no majors.**

| ID | Sev | Summary |
|---|---|---|
| F5 | minor | "+0.13 pp point-estimate rise reproduces" over-claims: unpaired between-process comparison at n=2, order undocumented, and a third barriered sample (+7.28%) lands inside the unbarriered range |
| F1 | minor | Barrier enforces a common start, not a common duration; the "*enforced*" comment overstates. Arithmetically real, empirically ≤0.03 pp here — a wording fix, not a bias claim |
| F2 | minor | `(max-min)/mean` is a low-power proxy, `arm_cv` is misnamed, and the per-round distribution is discarded when a within-run paired CI is free |
| F3 | minor | A sign flip at n=2 cannot support "no systematic tail-idle bias"; my third sample is outside both |
| F4 | minor | The *level* of the spread is never reported — only the difference — so the risk cannot be assessed from the write-up |
| F6 | minor | "n=11, the genuinely independent unit" overstates; the barrier *increases* cross-thread coupling |
| F7 | minor | "still inside +6.8–7.4%" is false for 2 of 4 barriered point estimates |
| F8 | minor | The divergence assert cannot detect barrier defects, and a panic now prints then **deadlocks** (reproduced) |
| F9 | minor | The headline quoted in AUDIT/CLAUDE.md is the aggregate — the statistic the PR itself says overstates independence |

Cheapest set of fixes: strike or restate the +0.13 pp sentence (F5, F7), print
the spread levels and a within-run paired CI over rounds instead of just the
mean (F2, F3, F4), soften two comments (F1, F6), correct the verification
sentence and move the checksum assert out of the barriered loop (F8).

---

# Round 2 — independent, fresh reviewer

Scope: commit `990786f` only (`git diff b315da1..HEAD`) — the fixes written for
round 1's nine minors. Still bench + docs only; `git diff b315da1..HEAD --stat`
shows `AUDIT.md`, `CLAUDE.md`, `benches/nativeloop_ab.rs`. No `src/`, so no
emitted ARM64 changed and there is no wrong-hash exposure in this PR.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Deadlock fix: panic paths, wait counts, per-pair/per-thread coverage, phase 1 | done — correct; R2-F4 (nit) |
| 2 | Rewritten spread statistic: pairing, `mean_ci95` input, printed note | done — pairing correct; **R2-F3** |
| 3 | Withdrawn claims: residue in AUDIT / CLAUDE / PR body / comments | done — clean; R2-F5 (nit) |
| 4 | New numbers: arithmetic, internal consistency, strength vs sample | done — **R2-F1, R2-F2** |
| 5 | `Checks`/`PhaseOut`, AUTHORITATIVE labelling, entry-vs-diff match | done — correct |
| — | Break test, sharper than the author's (last thread, last pair) | done — passes |
| — | clippy `--benches --release -D warnings` | done — clean |
| — | CI final state | see below |

## Item 1 — the deadlock fix is correct

* **No panic path remains between two `sync()` calls that the fix was meant to
  remove.** The barriered region contains only `round()`. Its `try_into().unwrap()`
  is on `chunks_exact(8)` (infallible), `blob[39..43]` is in range for the fixed
  76-byte blob, and `h / ta` yields `inf` rather than panicking. The assert was
  the realistic panic and it is gone from that region.
* **Wait counts unchanged and still `tid`-independent**: `2 + 4·pairs` on every
  thread. Removing the assert did not touch a `sync()`.
* **Coverage is per-pair and per-thread.** `assert_arms_agree` loops all pairs;
  `main` loops all threads *after* `thread::scope` returns, so a divergence on
  any thread at any pair is reached. (It stops at the first failure, which is
  reporting, not detection.)
* **The comparison is still the right one**: `(ca, cc) == (cb, cd)` is
  `ca==cb && cc==cd`, matching the A-B-B-A nonce ranges. Unchanged.
* **Phase 1 still asserts** (`assert_arms_agree("1 thread", &c1)`, before
  `report`).

**Break test, deliberately harder than the author's.** The author injected on
thread 1, pair 0 — which cannot distinguish "checks every thread and pair" from
"checks the first". I injected `cb ^ 1` on `tid == 2` (the **last** thread) at
`_p == pairs - 1` (the **last** pair) and ran `cargo bench --bench nativeloop_ab
-- 3 2 16`:

```
thread 'main' panicked at benches/nativeloop_ab.rs:192:9:
assertion `left == right` failed: thread 2: native loop and body JIT produced
different hashes in pair 1 — this is a correctness failure, not a benchmark result
  left: (1877151405702922549, 10378506918665234298)
 right: (1877151405702922548, 10378506918665234298)
error: bench failed
```

Phase 1 passed first (not injected), the process **exited** rather than hanging,
and the message names the last thread and the last pair. Mutation reverted;
`git status` clean.

## Item 4 — the numbers

Arithmetic checks out: barriered half-widths 0.19–0.41 all below 0.43; asymmetry
range 0.60−(−1.10) = 1.70 pp; "all eight point estimates in +7.05–7.50%" holds,
and holds even including the two unbarriered aggregates. Run 4's −0.94 pp
reconciles with the printed levels 8.03/7.08 under rounding.

**R2-F1 (minor): "every run's own paired CI includes zero" is asserted for three
runs that never computed one.** The paired CI across rounds is *new in this very
commit*. Runs 1–2 are the author's originals and run 3 is round 1's reviewer,
whose ledger quotes the old output verbatim — `asymmetry : -1.10 pp (native -
body)`, no interval. Only run 4 (−0.94) can have printed a CI. The sentence
appears in both `AUDIT.md` ("every run's own paired CI includes zero") and the
PR body. This is the same defect class as the +0.13 pp claim it replaces: a
statement quoted with a sample that cannot support it. Fix: "the one run
measured with the new statistic has a paired CI including zero", or re-run the
other three.

**R2-F2 (minor): the baseline-sanity ranges exclude the run the entry now counts
as one of its four.** `AUDIT.md` says "single-thread body JIT 568.1-572.7 H/s …
11-thread body JIT 4982-5007 H/s … **No run discarded**". Round 1's ledger
records its own run at **572.9 H/s** single-thread and **5020.7 H/s** at 11
threads — outside both ranges. The author widened 5003→5007 to admit run 4 but
never folded in run 3, whose figures are in the file being cited. Half-correction
of exactly the shape this repo keeps producing. The correct ranges are
568.1–572.9 and 4982–5020.7.

**R2-F2b (minor, quantitative): the surviving CI-narrowing claim is carried by
three observations, not four.** Run 4's ±0.41 against the tightest unbarriered
±0.43 is a 0.02 margin, and a half-width is itself a random variable: with n=24,
`SD(s)/σ ≈ 1/√(2·23) ≈ 15%`, so ±0.41 carries roughly ±0.06 of its own sampling
noise. That ordering is unresolvable. Also note the claim rests on the *same*
unpaired between-process design that the entry uses (correctly) to withdraw the
+0.13 pp claim two paragraphs earlier — the entry applies that caution to the
tail-idle paragraph but not to this one. It survives on effect size and on 3 of
4, which is worth saying rather than "reproduces across four observations".

## Item 2 — the rewritten statistic

**Correct where it matters.** `b[i]`/`n[i]` are the A-vs-B and D-vs-C antithetic
pairs — the identical convention `report()` uses — so the paired CI is over the
right pairing and `mean_ci95` is fed a genuine paired-difference sample (n=24 →
df=23 → t=2.086, the deliberately-wide bucket). `d_mean` equals `n_mean −
b_mean` exactly when the two vectors have equal length, so the four historical
asymmetry values remain comparable across code versions and the table is
coherent. The `if mean > 0.0` filter could in principle desynchronise `b` and
`n`, but rates are strictly positive here — unreachable, noted only for the
record. Rename away from `arm_cv` is complete (`grep arm_cv` → nothing) and the
"coefficient of variation" string survives only inside the comment that
disclaims it.

**R2-F3 (minor): the printed interpretation note is stale on arrival and asserts
an interval that does not exist.** Lines 265–271 hardcode "the asymmetry has
been observed at -1.10, -0.40 and +0.60 pp across separate runs" — omitting
**−0.94**, the observation produced by the run that exercised this very code and
recorded four lines away in `AUDIT.md`. It then states "it moves between runs by
more than any one run's interval", which is R2-F1 baked into program stdout: three
of those runs computed no interval. Beyond the two errors, hardcoding a run log
into a program's output guarantees permanent drift — this repo's numbers travel
by being pasted out of harness stdout.

## Item 3 — the withdrawals are complete

Grepped `AUDIT.md`, `CLAUDE.md`, `benches/`, `README.md` for `0.13`, `7.27`,
`7.40`, "still inside", "no divergence", "genuinely independent", "coefficient of
variation", `arm_cv`, `4982`, `5003`. Every surviving mention is inside an
explicit withdrawal. `7.27`/`7.40` and "genuinely independent" are gone
entirely. The PR body carries the same withdrawals. No half-correction here.

**R2-F5 (nit): one module-doc sentence went stale.** Line 34 still says the
harness "asserts this on every round"; the assert now runs after the phase. The
coverage is the same; the wording is not.

## Item 5 — types, labelling, entry-vs-diff

`Checks` / `PhaseOut` are honest aliases and are why `type_complexity` is
satisfied; the AUDIT entry's account of them matches. The AUTHORITATIVE label is
applied to the per-thread block in code, in the AUDIT table and in `CLAUDE.md`,
consistently. Every claim the AUDIT entry makes about the diff (assert moved
after join, level printed, paired CI, "common start not common duration",
"exchangeability not independence") is present in the code I read.

**R2-F4 (nit): the deadlock class is narrowed, not eliminated.** The write-ups
scope the fix correctly ("on a real divergence"), so this is not an over-claim.
For the record: any *other* panic inside `round()` — or in `new_full` /
`prepare_scratchpad` before a thread's first `sync()` — still strands every
sibling in `Barrier::wait()`, and `h.join()` on thread 0 never returns.
Unreachable in the bench profile (debug-assertions off), but a `cargo test
--benches` debug build would expose it.

## CI and compile evidence

`cargo clippy --benches --release -- -D warnings` clean locally. CI's `lint` job
runs `cargo clippy --all-targets --locked -- -D warnings`, which **does** compile
this bench on x86_64 — `lint` and `audit` passed on the current head; `test` and
both `jit-*` jobs were still in flight when I finished. Note that no CI job ever
*executes* this harness, and the JIT gate does not touch the changed file, so a
green `jit-macos` is not evidence about this diff.

## What I could not verify

* I did not reproduce any of the six 11-thread performance runs. The point
  estimates, CI half-widths, per-thread figures and asymmetry values in the
  tables are taken as reported except where round 1's ledger contradicts them
  (R2-F2).
* I could not confirm from the repository which code version produced runs 1, 2
  and 4; R2-F1 rests on run 3's output format as quoted in round 1's ledger plus
  the fact that the paired CI is new in `990786f`.
* The break test used 3 threads / 2 pairs / 16 hashes, not the 11×12×256
  configuration the tables use.

## Verdict — round 2

**MERGEABLE.** No blockers, no majors. The deadlock fix is correct and survives
a harder break test than the author's; the statistic's pairing is right; the
four withdrawals are complete with no residue.

**Three minors and two nits**, all in the write-ups or in printed prose:

| ID | Sev | Summary |
|---|---|---|
| R2-F1 | minor | "every run's own paired CI includes zero" — 3 of the 4 runs predate the paired CI and computed none (AUDIT + PR body) |
| R2-F2 | minor | Baseline-sanity ranges (568.1–572.7, 4982–5007) exclude round 1's run (572.9, 5020.7) while claiming "no run discarded" |
| R2-F2b | minor | The CI-narrowing claim is carried by 3 of 4 observations; ±0.41 vs ±0.43 is inside the half-width's own ~15% sampling noise |
| R2-F3 | nit→minor | The hardcoded stdout note omits the −0.94 observation and asserts intervals that were never computed |
| R2-F4 | nit | Non-assert panics inside the barriered region still deadlock |
| R2-F5 | nit | Module doc still says "asserts this on every round" |

**Operational point:** R2-F1, R2-F2 and R2-F3 are cheap to fix now, while the
entry is on an unmerged branch and may be edited in place. Once merged,
`AUDIT.md` is append-only and each becomes a permanent correction.
