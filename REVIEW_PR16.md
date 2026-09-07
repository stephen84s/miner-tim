# REVIEW_PR16 — round 1, independent

PR #16, "Record the eight-hour live pool run: 600 accepted, 1 rejected, 0 withheld".
Branch `docs/live-test` @ `a361247`, base `main` @ `ef40bea`.
Diff: `AUDIT.md` (+87), `CLAUDE.md` (+1), `LIVE8H_RUN.log` (+6041). No `src/`,
no `benches/`, no workflow, no build change.

**Verdict: MERGEABLE. 0 blockers, 0 majors, 7 minors, 2 nits — all ACTIONABLE.**

No blocker is reachable: nothing here can change an emitted instruction or a
submitted hash. What is under review is whether the record is accurate, and
seven figures/claims do not survive re-derivation as stated.

## Handoffs

None. Nothing in the diff belongs to `jit-reviewer` (no `src/randomx/jit/`,
no emitter, no `vm.rs`, no `benches/`, no speed-up or hashrate *benchmark*
claim — the 2226.8 H/s figure is an observed operating rate from a committed
log, not a measurement of the JIT against an alternative) or to `ci-reviewer`
(no `.github/workflows/`, `Makefile`, `scripts/` or `.cargo/config.toml`).

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Correctness of the change vs. what it claims | **done** — every figure re-derived from the committed log; see F1-F7 |
| 2 | Silent failure / swallowed error | **done, clear** — `0 ERROR` verified; withhold logs at `error!`, `SubmitVerifierUnavailable` at `warn!`; the single WARN is the rejection, so both silent-path candidates are excluded |
| 3 | Safety switches, fail-safe direction | **done, clear** — both defaults on, and *effective* (not just requested) on all four workers, log lines 42-45 |
| 4 | Tests / break-testing | **N/A** — the PR adds no test and changes no code a test guards. Nothing to mutate. Stated rather than silently ticked |
| 5 | Resource use | **done** — see N2 (928 KB log, precedent exists) |
| 6 | Documentation and audit accuracy | **done** — the bulk of the findings; append-only respected |
| 7 | Concurrency | **done, cleared as a positive** — see "Concurrency: checked and clear" |

## What reproduces exactly

Re-derived from `LIVE8H_RUN.log` (949,911 bytes, 6,041 lines):

- **600 accepted, 1 rejected, 601 found** — `grep -c` on all three. The single
  rejection is line 1655, `WARN … Share rejected: Invalid job id`.
- **0 withheld** — 601 found = 601 responses = 600 + 1, so every found share was
  submitted; and a withhold logs at `error!` (`miner.rs:764`), of which there are
  **0**. Sound, and stronger than the "grep -c" the entry cites.
- **0 `ERROR` lines, 1 `WARN`** (the rejection itself).
- **Verifier genuinely armed** — lines 42-45, `Worker N: native-loop JIT on |
  share verification on`, the *effective* line, not the requested one. So
  "0 withheld" is a real result, not a vacuous one. This is the point the repo's
  own history (`JitCompiler::new().ok()`) makes load-bearing, and it holds.
- **Range 2058.0-2345.3 H/s, n=2817** at `10m:` discarding the first 60 status
  lines (2,877 total). Reproduces exactly.
- **Discarding 60 is defensible, not convenient.** Status lines are every 10 s,
  so 60 = exactly the 600 s window of the `10m:` average; the retained minimum
  (2058.0) occurs at status index 793, deep in the run, not at the boundary — so
  the cut is not flattering the range.
- **Duration** 21:13:53Z → 05:14:00Z = 8h00m07s; clean SIGINT, all four workers
  stopped, final line `600 accepted, 1 rejected`.
- **0 unplanned disconnects** — 13 `Connecting to pool` lines, all 13 accounted
  for as 1 initial + 12 donation re-logins. No `Pool closed the connection`, no
  reconnect warning.
- **Job rotation ~20 s** — median inter-`New job` interval exactly 20.0 s.
- **Rejection not near a donation switch** — 30m09s after the 22:53:54 switch,
  1h04m before the 00:28:55 one. As stated.
- **Donation cadence** — Author 22:48:54→22:51:24 (2m30s), XMRig
  22:51:24→22:53:54 (2m30s); cycle starts 22:48:54 / 00:28:55 / 02:08:55 /
  03:48:55, i.e. 100m01s apart. 5 min per 100 = 5%, split 50/50. Correct.
- **Log integrity** — 6,027 timestamped lines, 0 non-monotonic, 0 status-line
  gaps > 12 s, mean tick 10.008 s. And the strong check: every one of the 2,877
  status lines' `Shares: a/r (found:f)` triple matches the running counts of the
  accepted / rejected / SHARE-FOUND lines above it. **2877/2877, 0 desyncs.**
  A deleted rejection or a doctored count desyncs this. The log is internally
  consistent in a way that would be hard to fake.

## Findings

### F1 (minor) — "1,865 vardiff adjustments" is 6× wrong; those are job pushes

`AUDIT.md`: "Pool difficulty | 50,000 -> 475,896 over 1,865 vardiff
adjustments". 1,865 = 1,852 `New job` + 13 `Initial job` lines. Those are job
*broadcasts*. The number of adjacent difficulty *changes* in that sequence is
**307**. A reader of an append-only authoritative ledger will later cite "the
pool adjusted difficulty 1,865 times"; it adjusted it 307 times and pushed
1,865 jobs.

Compounding it, the PR body says the count came "from the `New job` lines" while
the figure includes the 13 `Initial job` lines.

### F2 (minor) — the difficulty is a sawtooth, not a ramp; 475,896 is not the end

Difficulty resets to **50,000 on all 13 logins** — every donation re-login
re-enters vardiff from scratch. The run *ended* at 127,284. So:

- "50,000 -> 475,896" is min→max, not start→end.
- `CLAUDE.md`'s "a difficulty range widening 50k→476k" and `AUDIT.md`'s "across a
  difficulty range widening almost tenfold" both imply a monotone progression
  that did not happen. There were 12 re-ramps from 50,000.

The 12 vardiff re-ramps forced by the donation schedule are themselves an
unrecorded live observation the entry could have claimed and didn't.

### F3 (minor) — the rejection timeline is stated as fact; the log supports a second reading, and cannot settle it

**The conclusion "stale, not a wrong hash" survives. The specific timeline does
not.**

Three submissions were outstanding and were answered in the *same second*:

```
23:22:25  New job: OdNs0sPTHTyrpfXY (diff 132515)
23:22:27  Worker 2 SHARE FOUND! job_id=OdNs0sPTHTyrpfXY  nonce=12e20001   <- 2s into a live job
23:22:43  New job: Hy6Fp3CFgv0BI4m2                                       <- OdNs… dies
23:23:42  New job: nRnKGav31g3d17iB (diff 99386)
23:23:46  Worker 1 SHARE FOUND! job_id=nRnKGav31g3d17iB  nonce=75df0601   <- 4s in
23:23:48  Worker 0 SHARE FOUND! job_id=nRnKGav31g3d17iB  nonce=08020001   <- 6s in
23:24:02  New job: WenvUpDyx1EpocNJ                                       <- nRnK… dies
23:24:03  accepted  /  rejected: Invalid job id  /  accepted               <- THREE responses
```

The `Shares:` counter confirms three: 153/0 (found:156) → 155/1 (found:156).
The `AUDIT.md` entry and the PR body both quote only the two `nRnKGav…` shares
and omit the 96-second-old third.

Two readings, and the log distinguishes neither:

- **(A)** responses are FIFO ⇒ the reject is the 23:23:46 share, as the entry
  says. But then nothing explains why the *staler* 23:22:27 share (its job dead
  for 80 s) was **accepted**.
- **(B)** the reject belongs to the 23:22:27 share. This explains the pool's
  behaviour cleanly — under batch-time evaluation the 80-s-dead job fails and
  the 1-s-dead one passes — and means the entry names the wrong shares. Batched
  flushes are not obliged to preserve submission order, so (B) is live.

Note that all three shares were submitted while their job was **current**
(2 s, 4 s, 6 s in). If the pool evaluated on receipt, none should have drawn
`Invalid job id` at all; the entry's own story therefore already requires
batch-time evaluation, which is the assumption that makes (B) at least as likely
as (A).

`Duplicate share` is excluded: the two `nRnKGav…` nonces differ (`75df0601`,
`08020001`).

**Why it cannot be settled, and the follow-up.** The response carries a
JSON-RPC `id`, but `pool_connection.rs:551/558` logs bare `Share rejected: {}` /
`Share accepted by pool` with no id, job_id or nonce. Per-share attribution is
therefore impossible from any log this miner produces. Worth a follow-up issue
(not a fix in this PR): echo the id / job_id / nonce on the accept and reject
lines. That is the single change that would have made this entry's central claim
checkable rather than inferred.

### F4 (minor) — "`Invalid job id` means the hash was never evaluated" is uncited

The reasoning is almost certainly right — `Invalid job id` is a job-identity
error and standard Monero pool software (`nodejs-pool` lineage) does return
`Invalid result` / `Low difficulty share` for a bad hash. But as presented it is
an assumption dressed as a fact:

- The pool software and version behind `monerohash.com:2222` is identified
  nowhere.
- **Neither `Invalid result` nor `Low difficulty share` appears anywhere in 601
  responses**, so the claimed distinguisher is untested on this pool. The run
  gives no positive evidence of what monerohash returns for a wrong hash.

Grade it "consistent with the standard pool response set" rather than asserting
the mechanism. The wrong-hash conclusion is well-motivated; the certainty is not
earned.

### F5 (minor) — "eight donation rotations" undercounts the evidence by four

There were **12 re-login events**, not 8: Author → XMRig → **User** × 4. The
return-to-User is also a full disconnect + `login` + fresh session id
(`Connecting to pool` at 22:53:54, 00:33:55, 02:13:55, 03:53:55) with identical
in-flight-share risk. The entry's own framing — "the obvious place for an
in-flight share to be orphaned" — applies to all 12.

The claim itself holds and is stronger than written: 601 found = 601 responses,
so **not one share was orphaned across any of the 12 re-logins**. That balance
is the actual evidence and the entry doesn't state it.

### F6 (minor) — "600 opportunities" for the verifier should be 601

0 withheld ⇒ all 601 found shares were verified and submitted. The 601
denominator used in "zero wrong-result rejections in 601 submissions" is
correct; the verifier's opportunity count in the paragraph below it is not.

### F7 (minor) — median 2226.8 does not reproduce; it is 2226.9

Sorted, n=2817 (odd), the median is element 1409 = **2226.9**. 2226.8 is element
1408 — a 0-vs-1-indexed off-by-one. Confirmed twice, independently
(`sort -g | sed -n 1409p` and `statistics.median`). Magnitude is 0.1 H/s and
nothing turns on it; this repo's rule is that a quoted figure reproduces, and
this one doesn't.

### F8 (minor) — over-claims and a cherry-picked pair

- **"the first evidence that what this miner emits is what Monero accepts."**
  No block was found; the Monero network never saw anything. A *pool* accepted
  600 shares. The pool does recompute the RandomX hash, so the substance holds —
  say "the pool", not "Monero".
- **"ramping difficulty 132,515 → 335,439 in three minutes."** Endpoints are
  real (23:22:25 → 23:25:25) but the path oscillated: 132,515 → 99,386 →
  149,079 → 223,626 → 335,439, and the three minutes *before* that sat at
  206,141. At the moment of the rejection difficulty was **falling**. Cherry-
  picked endpoints on a non-monotone series, used to dramatise the context of
  the rejection.

### F9 (minor) — "what this run did not establish" is missing the one limitation a future reader needs

The section is otherwise good (4 threads, plain TCP, defaults only, 8 h). What
it omits is precisely what made F3 undecidable, and precisely what a future
reader would need to re-derive the central claim:

- **The pool batches submit responses onto its ~20 s job tick.** 578 of 601
  responses land within 1 s of a `New job` line. Observed submit→response
  latency (FIFO-matched): median **8 s**, p90 18 s, p99 20 s, max **96 s**.
- **The client cannot attribute a response to a submission** (F3).
- Consequently a share's fate is not observable promptly, and per-share
  forensics are not possible from this log.

Also worth one clause: the run says nothing about pool software other than
monerohash's, and nothing about a seed-hash change (none occurred).

### N1 (nit) — n=2817 is not 2817 independent samples

They are 600-second trailing averages sampled every 10 s; the effective sample
size is ~48. Harmless as used (median and range only), but it becomes an error
the instant anyone quotes a confidence interval off n=2817. One clause prevents
that.

### N2 (nit) — 928 KB log in the repo

`LIVE8H_RUN.log` is 949,911 bytes, not the ~500 KB assumed. Precedent exists
(`PERF1_RUNS.log` is already tracked, though only 5.5 KB), `.gitignore` has no
`*.log` rule so no force-add was needed, and the file is text that compresses
well and will never change. Committing it is the right call given the entry's
figures are otherwise unverifiable. Flagged only so the pattern is a deliberate
choice: a policy on run-log retention is worth having before the third one.

## Concurrency: checked and clear

The repo's known ~15%-stale-reject failure mode (starved pool receiver) is
**foreclosed for this run**, empirically:

- `RECV_POLL_INTERVAL` is 50 ms (`pool_connection.rs:20`); the receiver holds
  the stream lock for one read at a time so worker submits interleave.
- Job pushes were logged on schedule (median 20.0 s) throughout, including
  across the 96-second response gap — the receiver was reading, not blocked.
- 578/601 responses arrive within 1 s of a job push. The 8 s median latency is
  **pool-side batching**, not client-side delay.

That is a real positive result and the entry could claim it.

## Process / consistency

- `AUDIT.md` entry is **appended** at the end. Correct.
- `CLAUDE.md` row is inserted immediately above `Pending`, matches the entry, and
  cites no `#N`, so the issue-numbering convention at line 121 is not engaged.
- The entry carries all five required sections (goal, files changed, behaviour,
  verification, assumptions/limits). Behaviour/API changes: none, correctly.
- **The branch is unmerged**, so per `CLAUDE.md`'s own rule the LIVE-01 entry may
  be **edited in place**. Do not append a correction to an entry that is not yet
  part of the record.

## What I could not verify

- **Which submission the rejection belongs to.** Not determinable from any log
  this miner produces (F3). My FIFO matching is an assumption, and I give
  reasons to doubt it.
- **That monerohash returns a different message for a wrong hash.** No such
  response exists in 601 samples (F4).
- **That the log is unedited.** I established internal consistency
  (2877/2877 counter agreement, monotonic timestamps, no gaps) — strong, but it
  is not provenance.
- **Break-testing: N/A.** The PR adds no test and changes no code under test.
- I did not run `make check` / `make test` / `make verify-jit`: the diff touches
  no compiled file. CI's five jobs exercise nothing this PR changes.

## Bottom line

**MERGEABLE — and everything above is ACTIONABLE**, by editing the LIVE-01
`AUDIT.md` entry and the `CLAUDE.md` row before merge.

The run is real, the log is genuine and complete, the headline result (600
accepted, 0 withheld, verifier provably armed) holds, and the central conclusion
— the one rejection was not a wrong hash — survives every reading I could
construct. What needs fixing is that the entry states an inferred timeline as
fact (F3), gets a job-push count and a difficulty progression wrong (F1, F2),
undercounts its own donation evidence (F5, F6), quotes a median that is off by
one order statistic (F7), and omits the response-batching limitation that a
future reader needs most (F9).
