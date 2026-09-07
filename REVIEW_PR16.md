# REVIEW_PR16 — round 1, independent

PR #16, "Record the eight-hour live pool run: 600 accepted, 1 rejected, 0 withheld".
Branch `docs/live-test` @ `a361247`, base `main` @ `ef40bea`.
Diff: `AUDIT.md` (+87), `CLAUDE.md` (+1), `LIVE8H_RUN.log` (+6041). No `src/`,
no `benches/`, no workflow, no build change.

**Verdict: MERGEABLE. 0 blockers, 0 majors, 9 minors (F1-F9), 2 nits — all
ACTIONABLE.** (The first commit of this ledger said "7 minors"; that was a
miscount taken from the bottom-line paragraph, which had omitted F4 and F8.
Corrected here rather than amended away.)

F1 and F3 carry the most weight. They are *minors* only because this PR changes
no behaviour — on `_shared-context.md`'s scale a documentation-accuracy defect
cannot exceed minor. F3 is the entry's load-bearing claim.

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

Also in this paragraph: **the run donated 4.17%, not 5%.** The first switch is
94m59s in; cycles land at 95 / 195 / 295 / 395 min and the run ends at 480 min
mid-cycle, so 4 x 5 = 20 min of 480. "5 minutes donated per 100 ... exactly the
documented 5%" is correct *as a statement about the schedule*, but
"Donation accounting verified against a live pool" invites the reader to take
5% as the realised figure. One clause separating schedule from realisation.

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
- **That the binary that produced the log is `ef40bea`.** The log header asserts
  `commit : ef40bea` and `binary : 2208992 bytes`; nothing ties the two. Distinct
  from the point above. The converse *is* sound and is what licenses my reading
  of `error!` levels and `RECV_POLL_INTERVAL`: this diff is docs-only, so the
  `miner.rs` and `pool_connection.rs` in this worktree are byte-for-byte
  `ef40bea`'s source.
- **Break-testing: N/A.** The PR adds no test and changes no code under test.
- I did not run `make check` / `make test` / `make verify-jit`: the diff touches
  no compiled file. CI's five jobs exercise nothing this PR changes.

## Bottom line

**MERGEABLE — and everything above is ACTIONABLE**, by editing the LIVE-01
`AUDIT.md` entry and the `CLAUDE.md` row before merge.

The run is real, the log is genuine and complete, the headline result (600
accepted, 0 withheld, verifier provably armed) holds, and the central conclusion
— the one rejection was not a wrong hash — survives every reading I could
construct. What needs fixing, across all nine minors: the entry states an inferred timeline
as fact (**F3**) and presents an uncited pool-behaviour assumption as established
(**F4**); it gets a job-push count and a difficulty progression wrong (**F1**,
**F2**); it undercounts its own donation and verifier evidence (**F5**, **F6**);
it quotes a median that is off by one order statistic (**F7**); it over-claims
"Monero" for "the pool" and dramatises with a cherry-picked difficulty pair
(**F8**); and it omits the response-batching limitation a future reader needs
most (**F9**).

---

## Round 2

Fresh reviewer, cold. Scope: commit `7cf5e95` (the corrections) reviewed as new
work — `git diff a361247..HEAD -- AUDIT.md CLAUDE.md` — plus the PR body. Round
1's findings were **not** inherited; every figure was re-derived from
`LIVE8H_RUN.log`.

**Scope check / handoff.** The diff touches `AUDIT.md`, `CLAUDE.md` and this
ledger only. No `src/`, no `src/randomx/jit/`, no `vm.rs`, no `benches/`, no
`.github/workflows/`, `Makefile`, `scripts/` or `.cargo/config.toml`. Nothing to
hand to `jit-reviewer` or `ci-reviewer`. The one performance number in the entry
(median 2226.9 H/s) is an observation read off the committed run log, not a
benchmark claim from `benches/`, and I verified it from the log; an opinion on
"median of overlapping 10-minute averages" as a *methodology* would be
`jit-reviewer`'s.

**Append-only.** LIVE-01 was added on this unmerged branch (`a361247`), so
`CLAUDE.md`'s protocol permits editing it in place. The heavy in-place rewrite is
legitimate and the entry never claims to have appended. Not a finding.

### Re-derivation — every figure in the corrected entry

All from `LIVE8H_RUN.log`, independently:

| Claim | Derived | ✓ |
|---|---|---|
| 601 found / 600 accepted / 1 rejected / 0 withheld | 601 / 600 / 1 / 0 | ✓ |
| 0 `ERROR` lines, 0 unplanned disconnects | 0 ERROR, 1 WARN (the rejection); 13 connects all accounted for | ✓ |
| 1,865 job pushes | 1852 `New job` + 13 `Initial job` | ✓ |
| 307 adjacent difficulty changes | 307 | ✓ |
| first 50,000 / last 127,284 / min 50,000 / max 475,896 | all four | ✓ |
| resets to 50,000 at all 13 logins | all 13 `Initial job` lines are difficulty 50000 | ✓ |
| 13 logins, 12 rotations (4+4+4) | 13 `Login successful`; 4 Author + 4 Xmrig + 4 User | ✓ |
| stints 2.5 min on a ~100 min cycle | 150 s each; 22:48:54 → 00:28:55 = 100m01s | ✓ |
| realised donation 4.17% | 8 × 150 s = 1200 s / 28800 s = 4.167% | ✓ |
| median 2226.9 H/s, range 2058.0–2345.3, n=2817 | median 2226.9 (n odd), min 2058.0, max 2345.3, 2877−60 = 2817 | ✓ |
| latency median 8 s / p90 18 s / max 96 s | 8.0 / 18.0 (nearest-rank and linear agree) / 96.0 | ✓ |
| 578 of 601 within 1 s of a job push | 578 at a ±1 s window | ✓ |
| rejection 30 min after a switch, 1h04m before the next | 30m09s / 1h04m52s | ✓ |
| the three-outstanding table (2 s / 4 s / 6 s job ages) | exact, lines 1638–1648 | ✓ |
| `RECV_POLL_INTERVAL` 50 ms | `pool_connection.rs:20` | ✓ |

Nothing in the table above is wrong. The defects are elsewhere.

### The three things round 1 said were understated — verified independently

1. **Verifier provably armed.** `LIVE8H_RUN.log:42-45`: all four workers log
   `Worker N: native-loop JIT on | share verification on`. `miner.rs:623-633`
   derives that text from `native_loop_effective()`, so it is effective, not
   requested. Confirmed. *(I first thought this claim was false — `grep
   effective` misses it, because the emitted text does not contain the word.)*
2. **Receiver cleared.** `RECV_POLL_INTERVAL` = 50 ms; six `New job` pushes fall
   inside the 96 s gap (23:22:43, :23:02, :23:22, :23:25, :23:42, :24:02), on
   the ~20 s tick throughout. Confirmed.
3. **2877/2877.** `7cf5e95` touches only the three `.md` files, so
   `LIVE8H_RUN.log` is byte-identical to what round 1 checked and the evidence
   base is fixed across rounds. Recomputed from scratch with a running tally of
   accepted/rejected/found: **2877 status lines, 2877 matching, 0 mismatched.**
   Confirmed.

### Findings

**R2-F1 (major) — the PR body still tells the withdrawn story as fact.**
`AUDIT.md` and `CLAUDE.md` are clean: `grep` across the branch finds the
withdrawn strings only inside explicit withdrawals. **The PR body is not.** It
still carries, verbatim and unqualified: the `23:23:46 / 23:23:48 / 23:24:02 /
23:24:03` timeline; "The one rejection was stale, not wrong"; "A *wrong* hash
returns `Invalid result` or `Low difficulty share`" — which the entry now says
appears nowhere in 601 responses; "600 accepted shares is the first evidence that
what this miner emits is what Monero accepts"; and all five corrected figures
(2226.8, "1,865 vardiff adjustments", "50,000 → 475,896", "Eight donation
rotations", "600 opportunities"). Issue #17 says "Found during round 1 of PR
#16", so a future reader following #17 back lands on exactly this page. This is
the repo's document-that-argues-with-itself failure spread across artifacts. One
`gh pr edit` fixes it.

**R2-F2 (major) — "Twelve re-logins produced zero orphaned shares" is vacuous;
the correction made a true weak claim into an untrue strong one.** Round 1's
entry said "Eight donation rotations produced zero rejections". The correction
raised it to "Twelve re-logins produced zero orphaned shares… Each happens
mid-hash with four workers running — the obvious place to lose an in-flight
share. **601 shares found, 601 responses received**, so none was lost across any
of them." Replaying the log as a running count of found-minus-responded gives, at
each of the twelve `Donation: mining to` lines, **outstanding = 0**. This is
pairing-free — it needs no attribution of response to submission. The same
replay at the **socket** instants, which is the actual risk moment, gives the
same answer: outstanding = 0 at all 13 `Connecting to pool` lines, all 13
`Connected to pool` lines and all 13 `Login successful` lines. No share was
ever in flight across a rotation, so the run never entered the window it claims
to have cleared, and the 601 = 601 identity is a global count that would hold
even if the orphaning path were broken. Present in both `AUDIT.md` and the
`CLAUDE.md` row ("12 re-logins with 601 found = 601 responses, so nothing was
orphaned"). The honest statement is: twelve rotations occurred, no share was
outstanding at any of them, so the orphaning path is untested — it belongs under
*Not established*.

**R2-F3 (major) — the replacement reasoning leans on a pillar whose documented
limit the entry does not state.** The wrong-hash conclusion now rests on "600 of
601 accepted" plus "zero verifier withholds". `miner.rs:740-747` says of the
second, in its own words: *"the two paths are not independent. Both run
`emit_body`, so a defect in the shared instruction emitter produces the same
wrong hash on both sides and passes."* The entry never says this — `grep` over
the whole LIVE-01 entry for `emit_body`, `independen`, `both paths`, `shared`
returns only the *pool*-acceptance sentence. Worse, the entry's own opening
argues that the problem with every existing check is that it "compares the JIT
against *itself*", and then makes such a check one of two pillars. Pool
acceptance is the only external evidence here; the withholds are an internal
consistency check over the native-loop scaffolding. One clause fixes it.

**R2-F4 (minor) — new numeric error introduced by the correction: "superseded
four times" is six.** Between the 23:22:27 find and the 23:24:03 responses the
log shows six job pushes (`Hy6Fp3CFgv0BI4m2`, `UFJ7QKSROdad8HXv`,
`a0DWItt0PaVdbcLE`, `d1A05BcPa1TJh57L`, `nRnKGav31g3d17iB`, `WenvUpDyx1EpocNJ`).
Four is the count of jobs strictly *between* `OdNs0sPTHTyrpfXY` and
`nRnKGav31g3d17iB`, which is not what the sentence says. The error runs against
the entry's own argument, so it is harmless in direction — but it is a wrong
number in the authoritative record, in the paragraph written to replace a wrong
number.

**R2-F5 (minor) — the latency figures presuppose the pairing the same paragraph
declares unsupported.** "Any statement of the form '*this* share got *that*
response' is therefore unsupported" is immediately followed by "median
submit-to-response 8 s, p90 18 s". Those two are order statistics over
found→response pairs zipped in log order, i.e. FIFO — the very assignment the
entry argues is *probably wrong* at 23:24:03 ("the *opposite* of the assignment
used"). The maximum, 96 s, survives regardless: some submission was outstanding
from 23:22:27 to 23:24:03 under any pairing. The median and p90 need a stated
assumption ("responses arrive in submission order on a single connection") or a
hedge.

**R2-F6 (minor) — issue #17 is not cited.** The entry says only "Filed as a
follow-up rather than fixed here"; the `CLAUDE.md` row does not mention it
either. #17 exists and is exactly this. `AUDIT.md` is the record a future reader
searches; an uncited follow-up is unfindable.

**R2-F7 (minor) — the Verification paragraph omits the derivations a future
reader most needs.** It covers share counts, hashrate quantiles, difficulty range
and donation cadence. It does not cover:
- **How "0 withheld" is known** — and it is knowable *directly*, better than the
  entry claims: `ShareVerdict::Withhold` logs at `log::error!`
  (`miner.rs:764-776`) and the run has **0 `ERROR` lines**. That is an observed
  absence of the withhold line, not an inference from 601 = 600 + 1. It is the
  single most load-bearing figure in the entry and its derivation is nowhere.
- **The definitions behind the three non-obvious numbers**: 307 counts *adjacent
  differing* difficulties over the 1,865-push series; 578 uses a **±1 s** window;
  p90 = 18 s under both nearest-rank and linear interpolation (I got 17 s on a
  first pass with an off-by-one — which is the argument for recording the
  convention).
- **A base rate for 578/601.** Without one the claim could be trivially true.
  With one it is decisive: the ±1 s windows around 1,838 distinct push seconds
  cover 5,381 of 28,807 seconds — **18.7% of the timeline holding 96.2% of the
  responses**. That is what actually forecloses the starved-receiver reading.

**R2-F8 (minor) — second new error in the correction: the run did not begin
mid-cycle.** The 4.17% figure is right; the explanation given for it is not.
"the run began mid-cycle and ended mid-cycle, so it does not contain a whole
number of periods" — but the donation cycle is anchored to process start, and the
donation block sits at roughly minutes 95–100 of each 100-minute window. Mining
starts 21:13:55; the first `Author` stint is 94m59s later at 22:48:54;
Author→Author spacing is 100m01s / 100m00s / 100m00s; and 21:13:55 + 400 min =
**03:53:55**, exactly the fourth `User` resume. So the run began *on* a cycle
boundary and contained **four complete cycles**, in which exactly 5% was donated.
The entire shortfall is the undonated 79m58s tail of cycle 5. The second clause
("eight hours is simply not a multiple of 100 minutes") is the real and
sufficient reason; the first clause is a wrong causal claim, written into the
authoritative record inside the paragraph added to correct a number — same class
as R2-F4.

**R2-N1 (nit) — "Vardiff changes: 307" includes 12 login resets.** Twelve of the
307 adjacent changes are the drop back to 50,000 at a re-login, plus the climbs
that follow; they are not vardiff responding to the miner. The row label
overstates slightly; the prose elsewhere is aware of the resets.

**R2-N2 (nit) — round 1's N1 is unactioned and unmentioned.** n=2817 is still
quoted without noting that overlapping 10-minute averages sampled every 10 s are
not independent samples. The entry claims only that the *nine minors* were
actioned, which is true, so this is not a misstatement — but the caveat is still
missing.

### Round 1 completeness check

The entry asserts "nine minors — all actionable, all actioned above." Checked
against round 1's actual F1–F9 rather than the entry's self-description: F1
(1,865→307) ✓, F2 (sawtooth, min/max/end) ✓, F3 (timeline withdrawn) ✓, F4
(distinguisher withdrawn, replaced with the hedged "on its face") ✓, F5 (8→12) ✓,
F6 (600→601) ✓, F7 (2226.8→2226.9) ✓, F8 (both over-claims; the cherry-picked
132,515→335,439 pair went out with the withdrawn paragraph) ✓, F9 (the new
attributability paragraph) ✓. **The claim holds.** N1 and N2 were nits and are
not claimed.

### What I could not verify

- **That the log is authentic.** An 8-hour live run is not reproducible. I rely
  on 2877/2877 internal consistency, which is tamper-*evidence*, not proof, and
  on 0 non-monotonic timestamps.
- **That the binary was `ef40bea`.** The log header asserts it; nothing in the
  log proves it.
- **Which submission the rejection belongs to.** Undeterminable, as the entry
  now correctly says.
- **Break-testing: N/A.** Docs-only diff, no test added, no code under test.
  Log re-derivation is the substitute and was done in full.

### Verdict

**NOT MERGEABLE — three majors, all three required.**

- **R2-F1** — the PR body still states as fact the timeline the entry withdraws,
  the distinguisher the entry says was never observed, and all five corrected
  figures. `AUDIT.md` and `CLAUDE.md` would merge saying one thing while the PR
  carrying them says another, and issue #17 points a reader straight at it. This
  one is additionally visible to the public.
- **R2-F2** — a false claim in `AUDIT.md` *and* the `CLAUDE.md` row. The record
  this repo trusts later rather than re-derives.
- **R2-F3** — the replacement wrong-hash reasoning omits the limit `miner.rs`'s
  own comment already states.

No blockers. **Three majors, five minors, two nits.** R2-F1 through R2-F8 are all
**ACTIONABLE**; R2-N1 and R2-N2 are optional additions, not misstatements.
