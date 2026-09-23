# Review: PR #36 — Pair share submissions to pool responses by JSON-RPC id (#17)

Reviewing `git diff origin/fix/stale-connection-detect..HEAD` only (stacked on
#35). Scope: `src/pool_connection.rs`, `src/miner.rs`, `src/bin/minertim.rs`.
No JIT/`vm.rs` native-loop/`benches/`/CI files touched — nothing to hand off to
`jit-reviewer` or `ci-reviewer`; this is `pr-reviewer` territory throughout.

## Coverage ledger

1. Concurrency/ordering (insert-vs-response race) — DONE, finding below
2. Accounting identity (found = accepted+rejected+lost+pending) — DONE
3. Break-test — DONE (author's mutants scope reproduced exactly; broader
   scope on `submit_share|send_message_with_id` run separately — 3 MISSED,
   see below)
4. Drain sites (`reconnect`/`relogin_as` only) — DONE, confirmed by reading
5. Logging levels/volume — DONE
6. AUDIT.md / record — DONE (no entry yet, as expected; noted what's needed)
7. Lock ordering / deadlock — DONE, confirmed by reading `receiver_loop`
   (stream guard scope ends before `handle_pool_message` is called) and the
   two `relogin_as`/`reconnect` call sites (both driven from the single
   receiver thread, never concurrently with each other)

## Established so far

- Build/tests reproduced: `rtk proxy cargo test --release` → 164 lib + 20 bin
  pass, matches PR body exactly (4 new tests, all in `pool_connection`).
- `cargo clippy --all-targets --release -- -D warnings` → clean.
- `./scripts/mutants.sh 'handle_pool_message|drain_pending_shares' 'pool_connection::tls_tests'`
  reproduced exactly: 9 mutants, 5 caught, 4 unviable, 0 missed.
- Drain sites: `connect()` (line ~372) only ever writes `Some(pool_stream)`,
  never clears an existing stream itself. The only two places that null the
  stream (`*s = None`) are `reconnect()` (line ~761) and `relogin_as()` (line
  ~825), and both call `drain_pending_shares()` immediately after nulling the
  stream and before reconnecting. `Miner::initialize` calls `connect()` on a
  freshly constructed `PoolConnection` (empty `pending_shares`), so no drain
  is needed there either. PR's claim #4 confirmed by reading, matches the PR's
  own "covered by reading, not a test" admission.

## Finding 1 (Major): `submit_share` inserts into `pending_shares` *after*
writing the request, leaving a window in which the real response is
processed first

`submit_share` (pool_connection.rs:498-534):
```rust
let rpc_id = self.send_message_with_id("submit", params)?;   // stream lock: write, then release
if let Ok(mut pending) = self.pending_shares.lock() {          // separate lock, separate moment
    pending.insert(rpc_id, PendingShare { ... });
}
```
`send_message_with_id` locks `stream`, writes the request, and releases the
stream lock *before returning* — it does not hold anything that would block
the receiver thread from reading a fast reply. The `pending_shares.insert`
happens afterwards, as an unrelated statement.

If `handle_pool_message` (on the receiver thread) processes the pool's
response for that id in the gap between the write returning and the insert
running, `pending.remove(&rpc_id)` finds nothing. The response is logged at
`debug` and discarded uncounted — the real accept/reject is thrown away and
never reaches `accepted_shares`/`rejected_shares`. `submit_share` then goes on
to insert the entry anyway, now describing a submission that has *already
been fully answered*. Nothing will ever remove it again (its one response is
gone) until the next `reconnect()`/`relogin_as()` drains it — at which point
it is logged and counted as **lost**, even though the pool actually answered
it.

**Demonstrated** (not merely argued): temporarily added a test that (1) feeds
`handle_pool_message` an accept for id 55 with no prior `pending_shares`
entry — confirms it is discarded, `accepted_shares` stays 0 — then (2)
inserts the entry (simulating the delayed `submit_share` insert), then (3)
calls `drain_pending_shares()`. Result: `lost_shares == 1`,
`accepted_shares == 0`. The accept is converted into a false "lost" count.
Reverted before finishing the review; `cmp` against `/tmp/pool_connection.rs.orig`
confirmed identical, no diff left in the tree.

This demonstrates the *consequence* of the ordering, not that the
interleaving is reachable on a real network — see the reachability discussion
below. I did not construct a genuine forced-timing interleaving through a live
socket (that would need a deliberate delay inserted between the write and the
insert, driven through #35's real-socket receiver-loop test harness); the
above proves the code path exists and what it does when hit, which is enough
to require a fix regardless of how rarely it fires.

**Reachability.** The gap is not "a few CPU instructions" in the way that
sounds reassuring: it is bounded by ordinary thread scheduling, not by
instruction count. With 11 mining threads runnable and one submitting thread,
the OS can preempt the submitting thread for an arbitrary slice between the
`write()` returning and the `pending_shares.lock()` a few lines later — and it
would need that preemption to last as long as (or longer than) the pool's
round-trip time for the race to be observed, since the response has to arrive
*and be processed by the receiver thread* inside that same window. Against a
real pool over the internet (tens of ms RTT), that is unlikely on any single
share, but tonight's run submits shares continuously for 12 hours on a machine
under sustained near-100% CPU load (11 of 12 cores mining), which is exactly
the condition that makes scheduler preemption of the twelfth thread most
likely to stretch. I would not call this "astronomically rare" without
qualification; I also would not predict it fires tonight. It is a real,
correctly-shaped race with a trivial fix (register the id before writing the
request, or fold the two operations under one critical section), and it
directly undermines this PR's stated purpose — accurate share accounting for
tonight's evidence run — so I am treating it as blocking regardless of the
odds of hitting it in any one run.

**Suggested fix direction** (not required of me to implement, but the natural
one): compute `rpc_id` and insert the `PendingShare` into `pending_shares`
*before* the write, then remove it again if `write_request` returns `Err`
(the submission never left the machine). Do **not** hold the `pending_shares`
lock across the write itself — the stream write has a 10s timeout, and
stalling the receiver thread's ability to look up unrelated ids for that long
would be worse than the current bug.

## Finding 2 (Minor): a response that is neither an error nor
`result.status == "OK"` is removed from `pending_shares` but counted nowhere

`handle_pool_message`, once an id is matched, unconditionally removes the
entry (`pending.remove(&rpc_id)`) via the `let Some(entry) = ... else { return }`
pattern, then checks `error` and, separately, `result.status == "OK"`. Neither
branch is required to match — a `result` without a `status` field, or with a
non-"OK" status, falls through both `if`s with no counter incremented. This
logic predates the PR (the `status == "OK"` check was already there), but
before this PR there was no `pending_shares` bookkeeping to fall out of — now
the entry is consumed and the PR's own accounting identity ("found =
accepted + rejected + lost + still-pending", stated in the
`reset_share_counters` comment) silently loses a unit with no trace, not even
a log line. Low real-world likelihood (pools generally send exactly `{"status":"OK"}`
or an `error`), but worth a one-line `else { log::debug!(...) }` for
completeness, since the whole point of this PR is that nothing should
disappear unaccounted.

## Finding 3 (Minor): the discard line for a non-pending response is the same
severity used for the PR's own "late reply might have been accepted" case,
but it's at `debug`

The line that fires both for an ordinary keepalive reply *and* for the race
in Finding 1 (a late reply after `drain_pending_shares` already ran, or now
also a too-early reply) is `log::debug!("Pool response for rpc_id={} matches
no pending share submission...")`. At the default `RUST_LOG=info` used for the
12-hour run, this line will not appear at all, so if Finding 1's race (or a
late reply after a real reconnect) fires tonight, there will be no log trace
distinguishing "ordinary keepalive" from "a share response was silently
dropped." Given the PR's own stated goal is precisely to stop losing this
information, and that this line is the one place a dropped share response
would be visible, I'd suggest `warn` when the message looks like it could be a
share response context (e.g. contains a `result.status` or `error`) — though
distinguishing a keepalive reply from a submit reply by shape alone is
possible only if keepalive responses have a reliably different shape, which I
did not verify. At minimum this is worth a note in the PR/AUDIT entry as a
known blind spot, not necessarily a blocking one.

## Logging volume (priority 5)

`Share submitted` at `info` on every submit, `Share accepted`/`Share rejected`
at `info`/`warn` as before (just enriched with rpc_id/job_id/nonce/latency).
Volume is fine for a 12h run — share submission frequency is bounded by pool
vardiff (typically one share per several seconds to a minute per connection,
not per thread), so this adds at most a few thousand lines over 12 hours, no
different in order of magnitude from the existing accept/reject lines it
sits next to. Not an issue. The one line at the wrong level is the discard
line above (Finding 3).

## Accounting identity (priority 2)

Traced `total_found` (share_stats, in `miner.rs`) — it is incremented at the
point a share is *found* (hash ≤ target), independent of whether it is
subsequently verified, submitted, or lost to a write failure. That means
`found` (as shown in the stats line) is not the same population as
`accepted + rejected + lost + pending`: a share that fails the verifier
(withheld, never submitted) or that fails `submit_share` outright (`Err("Not
connected")`, never enters `pending_shares`) increments `found` but is not
capturable by any of the other four counters. This is **pre-existing
behaviour, not introduced by this PR** — `total_found` already existed and
already counted at discovery time — but the `reset_share_counters` comment
this PR adds ("found = all three plus whatever is still outstanding") reads
as a claim about the *displayed* `found` count, which is not quite true; it
would only be true of "submitted" shares, not "found" shares. Minor
documentation-accuracy point, not a counting bug in the new code itself.
Separately: **there is no getter or log line for "still outstanding"
(`pending_shares.len()`)** — the identity is asserted in a comment but nothing
in the stats line or the final-stats line lets you check it from tonight's
log. Worth adding for a run whose entire purpose is auditing these counts,
but not a blocker.

## Break-testing (priority 3)

Reproduced the author's own scope exactly:
`./scripts/mutants.sh 'handle_pool_message|drain_pending_shares' 'pool_connection::tls_tests'`
→ 9 mutants, 5 caught, 4 unviable, 0 missed. Matches the PR body precisely.

That scope **excludes `submit_share` and `send_message_with_id`** — i.e. it
excludes the exact functions where Finding 1 lives. None of the four new
tests calls `submit_share` either; all four construct `PendingShare` by hand
and insert directly into the map, so the insert-after-send ordering in the
real code is untested by design, not merely uncovered by accident.

Ran the broader scope suggested:
`./scripts/mutants.sh 'submit_share|send_message_with_id' 'pool_connection::'`
→ **3 mutants, 0 caught, 3 MISSED**:
```
MISSED src/pool_connection.rs:504:9: replace submit_share -> Result<(), String> with Ok(())
MISSED src/pool_connection.rs:993:9: replace send_message_with_id -> Result<u64, String> with Ok(0)
MISSED src/pool_connection.rs:993:9: replace send_message_with_id -> Result<u64, String> with Ok(1)
```
Every mutant that guts `submit_share`/`send_message_with_id` to a no-op
`Ok(...)` — i.e. the request is never written to the socket at all, and no
`pending_shares` entry is ever created — survives the full `pool_connection::`
test module untouched. No test in the suite calls `submit_share` and checks
its effect on the wire, on `pending_shares`, or on the counters; the four new
tests all construct `PendingShare` and drive `handle_pool_message` /
`drain_pending_shares` directly. This confirms the registration path (the
exact code Finding 1 is about) is untested "by design," not merely
uncovered — the PR's break-testing claim covers `handle_pool_message` and
`drain_pending_shares` only, never the function where the bug lives. I am
treating this MISSED result as corroborating evidence for Finding 1 rather
than a separate finding, per the advisor's framing: one root cause
(insert-after-send, entirely untested), one Major.

## Deadlock / lock ordering (priority 1, second half)

`send_message_with_id` locks only `stream`, writes, and drops the guard
before returning — it never holds `stream` and `pending_shares` at once.
`submit_share` then locks `pending_shares` alone. In `receiver_loop`, the
stream guard is scoped to a block (`let read_result = { let mut guard = ...
}`) that ends *before* `self.handle_pool_message(&line)` is called a few
lines later — confirmed by reading; the stream lock is fully released while
`handle_pool_message` (which locks only `pending_shares`) runs. Line 950's
`if let Ok(guard) = self.stream.lock()` guards an unrelated, narrow write
(keepalive), also never nested with `pending_shares`. `drain_pending_shares`
locks only `pending_shares`. `reconnect`/`relogin_as` lock `stream` (to null
it), release it, and only then call `drain_pending_shares` (a separate,
non-nested acquisition of `pending_shares`). Also confirmed: `relogin_as` is
called from inside `receiver_loop` itself (the donation-rotation check near
the top of that loop), and `reconnect()` is called from several places
within the same loop — both run on the single receiver thread, never
concurrently with each other. No code path acquires `stream` and
`pending_shares` nested inside one another in either order — I did not find
a deadlock. (Verified by reading; did not attempt to force a
livelock/deadlock experimentally, since no nested acquisition exists to
race.)

## Stats-line format (checked per advisor prompt)

`grep -rn 'found:' scripts/ Makefile` — no matches; nothing in this repo's
own tooling parses the stats line, so inserting ` (lost:N)` between the
share fraction and `(found:N)` doesn't break any in-repo consumer. (Can't
speak to any of the user's own external tooling/dashboards.)

## AUDIT.md / the record (priority 6)

No `AUDIT.md` entry exists yet for this PR (confirmed: `grep` for `#17` and
"Pair share submissions" only finds historical references to #17 being filed,
not a new entry). When written, it must state:
- Finding 1 (the insert-after-send race) and whether it was fixed before
  merge or accepted as a known, low-probability gap for tonight's run.
- That `total_found` and the new "found = accepted+rejected+lost+pending"
  comment describe two different things (Finding "Accounting identity" above).
- The mutants scope actually run (`handle_pool_message|drain_pending_shares`)
  and that it does not cover `submit_share`/`send_message_with_id`.
- This review's ledger commit sha (per CLAUDE.md's process rule), since this
  file is deleted from the branch before merge.

## Verdict

**NOT MERGEABLE as-is, ACTIONABLE.**

- Finding 1 (Major): the insert-after-send ordering in `submit_share` is a
  real, demonstrated defect in the exact mechanism this PR exists to build —
  accurate share accounting — and it is cheap to fix (reorder to insert
  before send, remove-on-write-error). Given tonight's run is explicitly
  meant to produce trustworthy evidence about share losses, I would not ship
  this ordering into that run without at least the reorder.
- Findings 2 and 3 (Minor) are not blocking on their own but should be noted
  in the AUDIT.md entry either way.
- Everything else — the drain-site wiring, the four new tests' own
  correctness, the mutants result, clippy, the full test suite, lock
  ordering — checks out as claimed.

Not verified / could not check: a genuine forced-timing reproduction of
Finding 1 over a real (even loopback) socket under load; whether any of the
user's external tooling parses the stats line.
