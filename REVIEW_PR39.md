# REVIEW_PR39.md — docs-only LIVE-03 follow-up entry

Scope: AUDIT.md, tasks/LIVE-03.md, tasks/README.md, CLAUDE.md "Current task" pointer.
No src/randomx/jit/, no benches/, no .github/workflows, Makefile, scripts/, .cargo/config.toml touched.
Confirmed via `git diff main...pr39 --name-only`: AUDIT.md, CLAUDE.md, tasks/LIVE-03.md, tasks/README.md only.
No handoff needed — squarely pr-reviewer scope.

## Coverage ledger

1. Correctness of the change vs claim — DONE, see findings.
2. Silent failure — N/A (docs only, no code).
3. Safety-switch fail-safe direction — N/A (no switches touched).
4. Tests — N/A (no tests touched).
5. Resource use — N/A.
6. Documentation/audit accuracy — DONE, this is the whole review. See findings.
7. Concurrency — N/A.

## Findings

### Minor: "longest job gap 22 seconds" does not match the log

Entry text: "No silence-detection event fired — the pool remained responsive
throughout (longest job gap 22 seconds, same as LIVE-02)."

Independently measured the gap between consecutive `New job:` log lines in
`LIVE7H_RUN.log` two ways (Python datetime diff, and an independent awk-based
seconds-of-day diff). Both agree: **max gap is 24 seconds**, not 22:

```
24.0 between 2026-10-01T21:16:38Z (New job: Fw4aBDCd0XDyxoQ0) and
            2026-10-01T21:17:02Z (New job: 3HHT3yiy59pHSDjZ)
```

The "22 seconds" figure is suspicious on its face: it is the *exact* figure
LIVE-02 reported for a completely different 12-hour run against a different
log. The phrasing "same as LIVE-02" reads as if it were copied from LIVE-02's
entry rather than independently recomputed for this run's log. The real
number (24s) is close enough that the substance of the claim doesn't change —
still nowhere near the 180s silence threshold, still no detection event — but
the entry asserts a specific measured value that does not reproduce. Given
this repo's own stated standard ("every number traces to a measurement"), this
should be corrected before merge: either recompute and state 24s, or soften to
avoid a false precise match with LIVE-02. The entry is still on an unmerged
branch, so CLAUDE.md permits fixing it in place rather than appending a
correction.

### Investigated and resolved: found→submitted latency (not a regression, not a finding against this PR)

While pairing `SHARE FOUND` to `Share submitted` by nonce to check the
465/466/467 arithmetic, noticed the gap between the two is often several
seconds (mean 7.8s, median 7s, max 34s across 466 paired shares) and that 411
of 466 (88%) submitted-timestamps land in the same second as an inbound
`New job`/`Initial job`/`Login successful` line — far above the ~7% you'd
expect by chance given 1733 job lines over 7h. This looked like it could be a
regression from #35/#36 (lock contention between the receiver loop and
`submit_share` on `self.stream`), which would matter a great deal: it would
mean the entry's "regression-free operation" claim is wrong about the exact
PRs this run exists to confirm.

Traced it down before concluding anything:
- `submit_share` and `receiver_loop` share one `Mutex` on the stream, but
  `receiver_loop` holds it only for the duration of one `read()` call bounded
  by `RECV_POLL_INTERVAL` = 50ms (`pool_connection.rs:702-713`, comment: "Hold
  the stream lock only for the duration of one read so submits/keepalives
  from other threads can interleave"). That bounds lock-wait at ~50ms, not
  seconds — ruled out as the mechanism.
- `write_request` (what `submit_share` calls) is a direct, synchronous
  `write_all` + `flush`, not a wait for the pool's reply — ruled out.
- Checked whether the *delay itself* predates #35/#36 using an older log
  still present in the tree, `LIVE8H_RUN.log` (commit `ef40bea`, well before
  #35/#36 — no `Share submitted` line exists yet in that format, only
  `SHARE FOUND` and an unlabelled `Share accepted by pool`, which is itself
  the exact ambiguity #17/#36 exists to fix). A crude sequential found→accepted
  pairing (approximate only, since the old format can't disambiguate which
  accept answers which submit) gives median 27s, mean 44s, max 284s — i.e.
  multi-second-to-multi-minute found→settle latency was already present
  **before** either PR. That is strong evidence the pattern is a pre-existing
  characteristic of this miner (most likely `verifier.reference()`'s
  recomputation on the body-JIT path, run synchronously on the worker thread
  between the FOUND log line and the submit call — see `miner.rs:770`
  onward), not something #35 or #36 introduced.

**Conclusion: not a finding against this PR.** The entry makes no claim about
submission latency one way or the other, so there's no false statement to
correct, and the "regression-free" framing holds for what it's actually
claiming (no *new* problems from #35/#36 specifically) — the pre-existing
log is the control that rules out a new regression. This is **worth a
separate issue** independent of PR #39: shares sitting unsubmitted for up to
34s is a real, reproducible, previously-undocumented behaviour, and the fact
that 0 shares were rejected on monerohash.com this time doesn't mean every
pool's stale-share window is that forgiving. Flagging it here for the lead to
decide whether to open a ticket; it should **not** block this docs PR.

## Verified as accurate (direct log checks, not trusting the PR prose)

- Run window: log starts `2026-10-01T16:06:04Z`, ends `2026-10-01T23:05:59Z`
  (6h59m55s, "7 hours" as stated).
- Final stats line: `Shares: 465/0 (lost:1) (found:467)` — matches "465
  accepted, 0 rejected, 1 lost, 467 found" exactly.
- Anomaly grep (`ERROR|panic|withheld|Failed to submit|[Rr]ejected|Pool
  closed|No data from pool|Keepalive failed`) against the full log:
  independently re-run, **zero matches** — confirmed.
- "465+1=466≠467" reasoning holds up under direct inspection, not just
  plausibility:
  - `grep -c "Share submitted"` = 466, `grep -c "Share accepted"` = 465,
    `grep -c "SHARE FOUND"` = 467.
  - The single "lost" share is an explicit, named event:
    `21:03:35Z WARN ... Share lost: rpc_id=638 job_id=u3UExt83JQdPT1Gn
    nonce=ce524202 — no response before the connection was replaced`.
  - The 467th found share (`Worker 1 SHARE FOUND! job_id=DMY1wInkFbDQVg7V,
    nonce=e1d52e03`, found at `23:05:48Z`) has no corresponding "Share
    submitted" line anywhere in the log — it was found 11 seconds before the
    alarm fired and the process exited mid-submission-cycle. That is a real,
    verified "snapshot mid-cycle" artifact, not hand-waving: 465 accepted + 1
    explicitly-lost = 466, and the 467th is simply in flight when the log
    ends. Submission latency elsewhere in the log is not always sub-second
    (one earlier share took 20s from found to submitted), so an 11s-old
    in-flight share at process-exit is unsurprising.
- No `minertim` process remains running (`ps aux` after the fact — none
  found), matching the entry's claim.
- `run7h.sh`'s perl-alarm self-terminate mechanism (`SECONDS_TO_RUN=25200` =
  exactly 7h, `perl -e 'alarm shift; exec @ARGV'`) matches the entry's
  description, and is a deliberate improvement over an earlier external
  `sleep; kill` helper that the script's own comment says once overran by 4h.
- No overclaiming repeated from LIVE-02's corrected mistake: this entry
  explicitly says "Like LIVE-02, this run shows the fixes do not create
  problems when they are not needed, and nothing more," and separately: "Not
  established... Whether the silence-detection path itself works — the
  condition it exists to catch still did not occur." Correctly does not
  upgrade "no harm" into "did good," and correctly does not claim the
  silence-detection fix "works" despite it never firing.
- Heading format: `### LIVE-03 (2026-10-02): Confirmatory 7-hour live run with
  both fixes deployed` — matches `### LIVE-02 (2026-10-01): ...` immediately
  above it exactly (verified via `git show pr39:AUDIT.md | grep -n
  "^### LIVE-03"`).
- `tasks/README.md` gets exactly one new bullet, appended last, same format as
  siblings (`- **[LIVE-03](LIVE-03.md)** — ... *(Completed)*`).
- `CLAUDE.md`'s "Current task" section is updated from LIVE-02 to LIVE-03,
  pointing at `tasks/LIVE-03.md`.
- `tasks/LIVE-03.md` is a genuine short summary (9 lines, one paragraph), not
  a duplicate of the AUDIT.md entry — condenses to the headline numbers and a
  pointer back to AUDIT.md as authoritative, consistent with DOC-04's stated
  convention.
- Binary provenance claim ("`main` at `461008a`'s predecessor, carrying both
  PR #35 and #36") checked against git history: `461008a^` is exactly
  `c3f035a` (the #36 merge, itself rebased onto the #35 merge), so this is
  precise and correct, not approximate.
- Timeline sanity: run started `16:06:04Z` on 2026-10-01, after both #35's
  merge (`2026-10-02 01:43:12+10` = `2026-10-01 15:43:12Z`) and #36's merge
  (`2026-10-02 02:04:01+10` = `2026-10-01 16:04:01Z`) — consistent with
  building the binary right after #36 landed and starting the run ~2 minutes
  later.
- **Binary provenance directly confirmed, not just timeline-plausible**:
  `stat -f '%Sm' target/release/minertim` on the primary checkout →
  `Oct 2 02:05:42 2026` local, which sits *between* #36's merge (local
  `02:04:01`) and LIVE-02's own docs commit `461008a` (local `02:45:46`) —
  exactly the window the entry's "`461008a`'s predecessor" claim requires.
  This is better evidence than the git-log timeline alone, which only showed
  plausibility.
- No hidden anomalies beyond the known lost-share WARN: `grep -vE
  '^\[[^]]+ INFO '` against the whole 5739-line log returns exactly one line,
  the `21:03:35Z WARN ... Share lost: rpc_id=638` line already accounted for
  in the headline "1 lost" — i.e. the author's anomaly-grep pattern (which
  doesn't actually match the words in that WARN line) isn't masking some
  *other* unlogged-category anomaly; there is nothing else non-INFO in the
  file to miss.
- Branch currency: `git fetch origin main && git merge-base --is-ancestor
  origin/main pr39` succeeds — the PR branch is NOT stale against `main` (the
  PR #31-shaped blocker does not apply here).
- Bonus accuracy check not asked for but free given the data: the one lost
  share (`21:03:35Z`, rpc_id=638) coincides exactly with one of the run's 13
  logins (`Login successful` also logged at `21:03:35Z`), i.e. it is the same
  "lost at a donation rotation" shape LIVE-02 named against issue #32, and all
  13 logins in this run are donation-schedule rotations (three-login bursts
  ~2:30 apart, no WARN/reconnect lines near any of them except this one) —
  consistent with "0 unplanned reconnects," though the entry doesn't make that
  specific claim so there's nothing to correct, only something it could have
  added.
- "Clean self-terminate" arithmetic: start `16:06:04Z` + `SECONDS_TO_RUN=25200`
  (7h, from `run7h.sh`) = `23:06:04Z`. The log's last stats line is `23:05:59Z`
  and no `23:06:09Z` line (the next 10s-interval tick) exists — consistent with
  the alarm firing and the process exiting cleanly in that 5-10s window, not
  with a crash. The entry's mechanism description was previously verified only
  structurally (the script); this closes the gap by checking the actual
  timestamps against the predicted fire time.

## Not independently verified

- Whether the binary actually used for the run has the exact same bytes as a
  fresh build of `c3f035a` (only mtime was checked, not a content hash against
  a rebuild) — the mtime match plus the script's own `strings`-based marker
  guard make it very likely, not certain.

## Verdict

**Not mergeable as-is — mergeable once one fix lands.** Correct the "longest
job gap 22 seconds" figure, which does not reproduce (actual max is 24
seconds) — **in both `AUDIT.md`'s entry and `tasks/LIVE-03.md`**, which
repeats the same "longest gap 22 seconds" line; fixing only one leaves the
task-file summary wrong. Either state 24s or drop the specific number rather
than asserting a precise match with LIVE-02 that the log doesn't support.

Everything else in the entry — headline share counts, the mid-cycle-snapshot
arithmetic, the anomaly-grep completeness, binary provenance (confirmed via
file mtime, not just git timeline), branch currency against `main`, the
no-overclaim discipline carried over from LIVE-02's own correction, heading
format, and task-board wiring — checks out against direct re-derivation from
the primary log, the older pre-#35/#36 log, and git history. The found→submit
latency investigated above is real but traced to a pre-existing cause, not a
regression from #35/#36, and is out of scope for this docs PR beyond flagging
it as a follow-up.

Severity: 0 blockers, 0 majors, 1 minor (the 22s/24s figure, in two files),
0 nits. One side observation recommended as a follow-up issue (found→submit
latency), not counted against this PR since the entry makes no claim about it.

**Before merge (lead-side, not mine to do):**
- Fix the 22s→24s figure in `AUDIT.md` and `tasks/LIVE-03.md`.
- Add this review's paragraph to the `AUDIT.md` entry: tier **Sonnet**
  (`pr-reviewer`), found 1 minor (the job-gap figure) with 0 false positives,
  investigated and cleared one apparent regression candidate (found→submit
  latency, traced to a pre-existing cause via `LIVE8H_RUN.log`), nothing
  this review could not verify beyond a binary content-hash.
- Record this ledger's commit sha (`25a8609` as of this writing, plus a second
  commit for this update — check `git log --oneline -3` on
  `docs/live7h-run-result` for the final sha) in the `AUDIT.md` entry.
- `git rm REVIEW_PR39.md` in the final commit before merge.
