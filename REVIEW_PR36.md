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

## Round 2

Fresh reviewer, no memory of round 1's session. Reviewed `git diff
25ab890..HEAD` (three commits: `ae9cbf2` reorders `submit_share` and adds two
tests, `8ee12ca` adds `get_pending_shares()` + stats-line wiring, `c8bd1c0`
folds `send_message_with_id` into `send_message`/`write_with_id` and adds a
keepalive wire test). Round 1's ledger (`REVIEW_PR36.md` as of `25ab890`)
reviewed for context only, not trusted for this round's claims.

### Coverage ledger

1. The reorder (insert-then-write) vs. `reconnect()`/`relogin_as()` interleaving — DONE, **new Major finding below**
2. The deliberately-unshipped ordering test — DONE, judged sound
3. Unmatched-reply logging (warn vs debug, flooding risk) — DONE, no flooding path found, but see the KEEPALIVED-heuristic note under mutants
4. Unrecognised status counted as rejected — DONE, no counter-evidence either way (unverified against a live pool, as round 1 already flagged for the pre-existing `status=="OK"` check)
5. `send_message_with_id` fold + keepalive wire test — DONE, sound
6. Break-test via `./scripts/mutants.sh 'submit_share|write_with_id|send_message|handle_pool_message|drain_pending_shares' 'pool_connection::'` — DONE, 2 MISSED (both discussed below, neither is the live defect this round found)
7. Full suite + clippy reproduced myself — DONE

### Finding R2-1 (Major): the reorder trades round 1's race for a differently-shaped one — a reconnect landing between `submit_share`'s insert and its write causes a false "lost" count *and* leaks the stale request onto the new connection

`submit_share` (pool_connection.rs:498-553) now:
1. reads `session_id` into `params` (line 504-513) — **before** anything else
2. allocates `rpc_id` (525)
3. locks `pending_shares`, inserts the entry (527-536)
4. calls `write_with_id(rpc_id, "submit", params)` (538) — a **separate**
   acquisition of `self.stream`'s lock, released the instant the write
   returns
5. on a write error only, removes the entry again (539-542)

Both `reconnect()` (784-827) and `relogin_as()` (842-866) run on the single
receiver thread and do, in this order: null `self.stream` under its own lock
→ **`drain_pending_shares()`** (which drains and counts-as-lost *everything*
currently in `pending_shares`, regardless of whether it has ever been
written to a socket) → reconnect the stream (possibly establishing a **new**
live connection) → (for `relogin_as`/donation rotation, and for the retry
loop in `reconnect`) log in again, which updates `session_id`.

If step 3 above (the insert) completes and then the submitting thread is
scheduled out before step 4 (the write) runs, and a reconnect's
null+drain fires in that gap, `drain_pending_shares` removes the just-inserted
entry — for a request that **has not been written to any socket yet** — and
logs+counts it `lost_shares += 1`. Unlike round 1's race, this window's
*consequence* is not bounded to CPU-instruction scale: if the reconnect
subsequently succeeds (RECONNECT_DELAY + a real TCP connect + login,
typically on the order of seconds), `self.stream` becomes `Some(new_stream)`
again, and step 4 — which has no idea any of this happened — goes on to
**write the original request onto the new connection**, carrying:
- the **pre-reconnect** `session_id` captured back in step 1 (now stale —
  `login()` on the new connection assigns a fresh one), and
- `rpc_id`, which is never re-inserted into `pending_shares` (the code only
  removes on a write *error*, never re-registers on success after an earlier
  removal), so any reply the pool sends for this exact write can only ever
  land in `handle_pool_message`'s unmatched-reply branch — discarded, now at
  `warn` rather than round 1's `debug`, but still uncounted.

Net effect: a share that was in fact written to a live connection (just the
wrong one, under a session id that connection never issued) is permanently
mis-bucketed as `lost` rather than `accepted`/`rejected`, with no way for
the accounting to ever correct itself. The `submitted ==
accepted+rejected+lost+pending` **identity still holds** — every id is
removed from `pending_shares` exactly once, either by the drain or by a
matched reply — so this is not an arithmetic corruption of round 1's kind.
It is a mislabeling defect in exactly the population this PR exists to get
right, and — unlike round 1's race, whose trigger was "an implausibly fast
pool reply" — this one's trigger is **a reconnect**, which is not a rare
event in tonight's run: donation rotation (`relogin_as`, on the schedule in
`donate.rs`) calls `drain_pending_shares()` on every rotation, and #34's own
silence-detection plus ordinary `Ok(0)`/read-error paths call `reconnect()`
on every connection hiccup. Any `submit_share` call whose insert-to-write gap
overlaps one of those events hits this.

**Demonstrated** (not a genuine timing race over a real socket, for the same
reason round 1 didn't force one — the gap is a handful of instructions and
cannot be manufactured through socket timing). Temporarily added a test that:
1. inserts a `PendingShare` directly (the state right after `submit_share`'s
   insert, line 536)
2. calls `drain_pending_shares()` (simulating a reconnect's drain landing in
   the gap) — confirmed `lost_shares == 1`, `pending_shares` empty
3. connects to a *second*, fresh local listener (simulating the stream
   `reconnect()` installed) and calls `write_with_id` with the same
   `rpc_id` and a `params` blob carrying an old, made-up session id
   (simulating the delayed write from the original `submit_share` call)
4. confirmed the write **succeeds**, the bytes (including the stale session
   id) **reach the new listener**, `pending_shares` is still empty, and
   `lost_shares` is still `1`

Reverted before finishing: `diff /tmp/pool_connection.rs.orig_r2
src/pool_connection.rs` empty, `git status --short` clean.

**Suggested fix direction** (not required of me, but cheaper than it looks):
have `submit_share` acquire the **stream** lock first, do the write, and
*while still holding that same lock* insert into `pending_shares`, releasing
only after both. `reconnect()`/`relogin_as()` already null `self.stream`
under that same lock before calling `drain_pending_shares()` separately, so
this ordering makes the two operations mutually exclusive: either the whole
write+insert happens-before the null+drain, or the null (stream now `None`)
happens-before the write even starts, in which case `write_with_id` fails
cleanly with "Not connected" and nothing is written or registered. This adds
no new stalling beyond what already exists — `write_with_id` already holds
the stream lock for the I/O regardless; the only change is that the
(non-blocking, non-I/O) `pending_shares` insert happens before that lock is
released rather than after. No other code path acquires `pending_shares` and
then `stream` in the opposite order (`handle_pool_message` and
`drain_pending_shares` each take only `pending_shares`), so this does not
introduce a lock-ordering deadlock.

**Reachability, honestly stated, same register round 1 used:** I did not
observe this fire against a real pool tonight and would not predict it does
on any single share. But donation rotation is scheduled, not probabilistic —
it *will* call `relogin_as`/`drain_pending_shares` repeatedly over 12 hours —
and the insert-to-write gap in `submit_share` exists on every one of the
thousands of shares that run will submit. I am treating this the same way
round 1 treated its finding: a real, correctly-shaped, cheaply-fixed race
that directly undermines this PR's stated purpose, on a run whose entire
point is trustworthy share-loss evidence.

### Finding R2-2 (confirmed sound): the deliberately-unshipped ordering test

The comment at the end of `tls_tests` (pool_connection.rs, final block)
explains that a socket-level "does accepted==1 after the fix" test was tried
and found to pass identically against the pre-fix (write-then-insert)
ordering, because `receiver_loop`'s 50ms poll interval plus a loopback round
trip is many orders of magnitude larger than the insert/write gap it would
need to catch. I agree with this reasoning and re-derive the same conclusion
independently: `RECV_POLL_INTERVAL` (50ms, referenced at pool_connection.rs
line ~811 and in the receiver loop) cannot observe an ordering gap that is
itself sub-microsecond. Shipping a test that passes on both the fixed and
the broken code would be exactly the failure mode `_shared-context.md`
names. Leaving a documented comment plus the two narrower tests
(`submit_share_registers_the_same_id_it_writes_to_the_wire`,
`submit_share_write_failure_leaves_nothing_pending`) is the right call here.
Worth noting for the record: my own R2-1 demonstration above *does* reach
the reorder's actual defect deterministically, by driving the two halves
(`drain_pending_shares`, `write_with_id`) directly rather than trying to
force a genuine race — the same technique round 1 used for its own Finding
1. That is why it caught something the author's mutation run (scoped to
`submit_share|send_message_with_id`, which doesn't call `drain_pending_shares`
or trigger a reconnect at all) could not have found: the defect is a *cross-
function* interleaving, not a bug reachable by mutating `submit_share` in
isolation.

### Finding R2-3 (minor, logging): non-KEEPALIVED unmatched replies do not flood the log — but the KEEPALIVED classification itself is unverified against a live pool

Traced every call site that writes to the pool: `login()` uses the
synchronous `send_request`, which holds the stream lock across its own
write+read and consumes the reply line directly — it never reaches
`handle_pool_message`, confirmed by reading (and matches the code comment at
pool_connection.rs ~926). Donation relogin (`relogin_as`) calls `login()`,
same path. So neither ordinary logins nor donation rotations can trigger the
new `warn` branch — only a genuinely unmatched id (a keepalive reply, or
R2-1's race) can. Keepalive replies are told apart from everything else
by `result.status == "KEEPALIVED"` (pool_connection.rs:948), a string
literal introduced fresh in this round (`git log -p -S'"KEEPALIVED"'` shows
only commit `ae9cbf2`, this round's fix commit — no prior test or comment in
the repo pins this shape). I could not find any log in this worktree
(`LIVE8H_RUN.log`, `LIVE6H_TLS_RUN.log`, `LIVE6H_TLS_NEGATIVE.log`) with a
`RUST_LOG=debug` "Pool recv" line showing an actual keepalive reply from a
real pool — all were run at the default `info` level, which does not print
raw wire lines. If the pool used tonight replies to `keepalived` with any
shape other than `{"result":{"status":"KEEPALIVED"}}` (e.g. `{"status":"OK"}`,
matching a share accept, or no `status` field at all), every keepalive reply
— once every `KEEPALIVE_INTERVAL` (60s), ~720 times over 12h — would land in
the `warn` branch instead of `debug`. That is a steady drumbeat, not a
flood, and it would not corrupt any counter (keepalive replies are never in
`pending_shares` either way), but it would add noise to exactly the log this
run exists to produce as evidence, and would look like 720 instances of
"a share response was silently dropped" to anyone reading it without this
context. Minor, not blocking — but worth a note in the AUDIT.md entry as an
unverified assumption, same category as round 1's own "distinguishing a
keepalive reply from a submit reply by shape alone... I did not verify."

### Finding R2-4 (minor, evidence): unrecognised-status-as-rejected — no counter-evidence found either way

Same conclusion round 1 reached for the pre-existing `status=="OK"` check:
I have no live-pool evidence of an alternate "accepted" status string. The
standard Monero-pool/XMRig-compatible protocol shape is `error` for
rejection or `{"result":{"status":"OK"}}` for acceptance; I am not aware of
(and did not find in this repo) any pool that signals acceptance with a
different `status` value. Treating "neither an error nor `status=="OK"`" as
rejected is the conservative choice and I cannot construct a plausible real
pool response it would miscount as rejected-when-actually-accepted. Not
independently verified against a live pool tonight.

### `send_message_with_id` fold + keepalive wire test (priority 5)

`send_message` now calls `write_with_id(self.next_request_id(), ...)`
directly — same behaviour as the old `send_message_with_id(...).map(|_id|
())`, just without the now-unnecessary intermediate function. No functional
change for the keepalive path. `send_message_writes_a_request_to_the_wire`
closes exactly the gap the author's own commit message names (`send_message`
mutable to a no-op `Ok(())` survived the suite before this test existed) —
reran the mutants scope below and confirmed it no longer survives.

### Break-testing (priority 6)

`./scripts/mutants.sh 'submit_share|write_with_id|send_message|handle_pool_message|drain_pending_shares' 'pool_connection::'`
→ **13 mutants: 7 caught, 4 unviable, 2 MISSED**:
```
MISSED src/pool_connection.rs:948:27: replace == with != in handle_pool_message
MISSED src/pool_connection.rs:965:20: delete ! in handle_pool_message
```
- Line 948 (`status == Some("KEEPALIVED")` → `!=`): flips which branch logs at
  `debug` vs `warn`. Purely a log-level cosmetic difference — no test asserts
  on it, no counter is touched by either branch, so this mutant is
  legitimately outside what the current test suite can or should be expected
  to catch without a log-capturing test. Not a bug; a documented gap.
- Line 965 (`!error.is_null()` → `error.is_null()`): changes which messages
  take the "Share rejected: ... {err_msg}" path. I traced the consequence: a
  reply shaped `{"error":null,"result":{"status":"OK"}}` — a common JSON-RPC
  2.0 convention for a *successful* response, and plausible from a real pool
  — is handled **correctly** by the current (unmutated) code: `error` is
  `Some(Value::Null)`, `!error.is_null()` is `false`, so the reject branch is
  skipped and `status=="OK"` is reached normally, incrementing
  `accepted_shares`. No test in the suite sends this exact shape, which is
  why the mutant survives — it is a real test-coverage gap for a plausible
  real-world message shape, but I confirmed by reading (not just by the
  mutant surviving) that the shipped code handles it correctly today. Minor:
  worth a test, not a live defect.

Neither MISSED mutant is R2-1 — R2-1 is a cross-function interleaving this
mutation scope cannot express (mutating `submit_share`/`write_with_id`/
`drain_pending_shares` individually doesn't reproduce a race between them).

### Verification reproduced myself

- `rtk proxy cargo test --release` → 168 lib (2 ignored) + 20 bin pass, no
  failures. Matches the +4 new tests over round 1's 164.
- `cargo clippy --all-targets --release -- -D warnings` → clean.
- `git status --short` clean at the end (temporary R2-1 demonstration test
  fully reverted, confirmed byte-identical against `/tmp/pool_connection.rs.orig_r2`).

### Verdict

**NOT MERGEABLE, ACTIONABLE.**

- Finding R2-1 (Major): the reorder closes round 1's race but opens a
  differently-shaped one — a reconnect (donation rotation or #34's own
  silence-detection reconnect, both of which *will* occur repeatedly over 12
  hours) landing between `submit_share`'s insert and its write causes a
  share to be falsely counted `lost` before it was ever sent, and then
  written onto the *new* connection carrying a stale session id, with any
  reply to that write permanently unmatchable. This is the same category of
  defect round 1 blocked on, in the same function, one fix later — it is
  cheap to close (see suggested fix direction above) and I would not ship it
  into tonight's evidence run.
- Findings R2-3 and R2-4 are minor and not blocking, but belong in the
  AUDIT.md entry as documented, unverified-against-a-live-pool assumptions,
  consistent with round 1's own standard for such claims.
- Everything else — the deliberately-unshipped ordering test's reasoning,
  the `send_message_with_id` fold, the keepalive wire test, clippy, the full
  suite, the `warn`-vs-flooding analysis for logins/relogins — checks out as
  claimed.

Not verified / could not check: a genuine forced-timing reproduction of
R2-1 over a live or even loopback socket under scheduler load (same
limitation round 1 stated for its own finding, and for the same reason); the
actual wire shape of a keepalive reply or an accepted-share reply from any
specific real pool, including the one intended for tonight's run.
