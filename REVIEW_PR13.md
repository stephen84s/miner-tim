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
| 4 | Arithmetic and CI computation; strength of claims | done — F4, **F5 (major)**, F7 |
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

**F5 (MAJOR): the "+0.13 pp rise" is recorded in `AUDIT.md` as one of "three
things that reproduce", and an independent run does not reproduce it.**

My own barriered 11-thread run on the same host (12 pairs x 256 hashes, machine
in the same state — single-thread body JIT 572.9 H/s against the PR's 568.1-572.7,
11-thread body JIT 5020.7 H/s against the PR's 4982-5003) measured the aggregate
diff at **+7.28%**. That is *inside* the unbarriered pair's range [+7.21, +7.33]
and below both of the PR's barriered figures. Barriered observations are now
+7.34, +7.46, **+7.28**; unbarriered are +7.21, +7.33. The separation the PR
calls reproducible is gone with one more sample.

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

2. **But the sensitivity is not negligible.** For a roughly symmetric spread,
   `mean − min ≈ R/2`, so the sum-of-rates overstatement factor is `≈ 1 + R/2`
   and a **1 pp change in R moves the reported aggregate diff by about 0.5 pp**.
   My −1.10 pp asymmetry maps to about −0.56 pp; the PR's own −0.40/+0.60
   observations map to ∓0.20/0.30 pp. **That is larger than the +0.13 pp effect
   the PR attributes to de-dilution.** So the tail-idle check, read
   quantitatively rather than as a pass/fail, does not clear that attribution —
   it shows the candidate bias is the same order as the claimed effect. This is
   the strongest single argument for F5.

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
* The sensitivity coefficient in "Measurements I made" (2) is a first-order
  approximation from a symmetric-spread assumption, not a derivation from the
  raw per-thread times, which the harness does not retain.

---

## Verdict

**MERGEABLE.** The code change is correct and is a real improvement to the
instrument: the barrier is properly counted, properly placed outside every timed
region, inert on the single-thread path, and it removes a genuine variance
source. Nothing in `src/` changed, so there is no wrong-hash exposure. The PR is
also unusually honest about its own limits — it names the trap in its own fix,
implements both of the issue's options, and labels its n.

One **major** and eight **minors**, none of which block:

| ID | Sev | Summary |
|---|---|---|
| F5 | **major** | "+0.13 pp point-estimate rise reproduces" — my independent barriered run lands at +7.28%, inside the unbarriered range; also confounded with undocumented run order |
| F1 | minor | Barrier enforces a common start, not a common duration; the "*enforced*" comment overstates, and sum-of-rates still assumes equal windows |
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
