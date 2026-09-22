# Review: PR #35 — Detect a silent pool instead of mining a stale job forever

Branch: `fix/stale-connection-detect` (`d3ff163`), against `origin/main`.
Scope: `src/pool_connection.rs` only (receive-side liveness detection +
one new test). No `src/randomx/jit/`, no `vm.rs` native-loop path, no
`benches/`, no `.github/workflows/`, `Makefile`, `scripts/` or
`.cargo/config.toml` touched — squarely `pr-reviewer` scope, no handoff
needed.

Round: 1 (cold start).

## Coverage ledger

| # | Item | Status |
|---|---|---|
| 1 | Trace every `last_recv` write; reset direction/completeness | Done — two gaps found (corrected mid-review, see Finding 1) |
| 2 | Break-test with chosen mutations, not just the author's | Done — 4 mutations tried beyond the author's |
| 3 | Threshold (180s) defensibility | Done — sound |
| 4 | Injectable `silence_timeout_ms` field | Done — sound |
| 5 | The new test's EOF-proof property, hang/flake risk | Done — property holds; thread-leak minor found |
| 6 | Keepalive-failure-reconnects change | Done — sound |
| 7 | PR body claims vs diff; `AUDIT.md` state | Done — no entry yet (expected, noted) |

`cargo test --release` (via `rtk proxy`): **160 lib + 20 bin passed, 0 failed,
2 ignored** — matches the PR body's claim exactly.
`cargo clippy --all-targets --release -- -D warnings`: **clean** — matches.
No `scripts/mutants.sh` process was running in this worktree at review time
(`ps aux` showed nothing); a `mutants.out/` directory exists from an earlier
run but did not interfere with `cargo test`/`clippy`.

## Findings

### 1. [MINOR, ACTIONABLE] The "reset after every reconnect" invariant holds at 3 of 5 reconnect sites, not all of them

**Correction from the first pass of this review**, caught before commit by
a second read specifically of the fourth branch the brief named (the
oversized-line/overflow arm), which my first pass had wrongly substituted
with "the silence branch itself" and called covered. There are **five**
places the receiver loop re-establishes the connection or otherwise proves
liveness by I/O: `Ok(0)` EOF, `Err(e)` read error, keepalive-write failure,
the silence branch itself, and — separately — `relogin_as()` on a donation
rotation. `last_recv` is correctly reset after success in three of them
(`Ok(0)`, `Err(e)`, keepalive-failure) and in the silence branch itself.
Two are missed:

**1a. The oversized-line (`Err(overflow)`) arm, nested inside `Ok(n) =>`:**

```rust
Ok(n) => {
    last_recv = Instant::now();          // set for the Ok(n) case generally
    pending.extend_from_slice(&chunk[..n]);
    match take_complete_lines(&mut pending) {
        Ok(lines) => { ... }
        Err(overflow) => {
            log::error!(...);
            pending.clear();
            if !self.reconnect() {        // blocks; RECONNECT_DELAY (5s) before
                return;                    // the first attempt, then loops until
            }                              // success — could be minutes
            // <-- no last_recv reset here, and no `continue`
        }
    }
}
```

`last_recv` is set at the top of `Ok(n)`, *before* `reconnect()` is called.
`reconnect()` is the unbounded retry loop (`pool_connection.rs:663`): it
sleeps `RECONNECT_DELAY` (5s) before every attempt, including the first,
and keeps looping until it succeeds — so during a real outage this can
block for minutes. There is no `continue` after this arm, so control falls
straight through to the silence check on the same iteration with a
`last_recv` timestamp that predates however long `reconnect()` just took.
If that duration exceeds 180s (plausible during any real outage, since
`RECONNECT_DELAY` alone is 5s and pool downtime is not bounded), the
silence check fires immediately afterward, forcing a second, redundant
reconnect right after the first one succeeded.

**1b. `relogin_as()` (donation-rotation reconnect), same shape, smaller blast radius:**

```rust
match self.relogin_as(&addr) {
    Ok(()) => {
        pending.clear();
        continue;          // <-- no last_recv reset
    }
    Err(e) => { ... }       // falls through to the generic read-error path,
                             // which *does* reset last_recv — this arm is fine
}
```

`relogin_as` (`pool_connection.rs:717-735`) clears the job, drops the
stream, calls `self.connect(&address)?` and `self.login(wallet)?` — both
genuine socket I/O, and `login()` reads a real response off the wire,
proving the pool is alive — yet does not reset `last_recv` on success.
Unlike 1a, `relogin_as` itself is bounded (one connect + one login attempt,
not a retry loop), so the staleness it introduces is small under normal
conditions (a donation-cycle boundary is a fixed wall-clock offset,
independent of pool traffic, so `last_recv` is usually only a few seconds
stale when a rotation fires). It only compounds into a spurious reconnect
in the same narrow way as 1a: if `last_recv` happens to already be close to
the 180s threshold at the moment a rotation triggers.

**Combined assessment:** both misses share the same failure shape — a
reconnect-equivalent event that doesn't refresh the clock it should — and
both are self-healing (the *next* reconnect, triggered by the stale check,
resets `last_recv` correctly, so this is one wasted cycle, not a loop) and
over-trigger rather than under-trigger (the safe direction: the miner never
fails to notice real silence because of this, it occasionally reconnects
one extra time when it didn't strictly need to). 1a is the more plausible
of the two in practice — it fires on every overflow-triggered reconnect
whose `reconnect()` call takes >180s, which real pool outages can easily
exceed — where 1b needs the added coincidence of a donation-boundary
alignment. The cost of firing is not zero: each spurious reconnect idles
every mining worker for at least `RECONNECT_DELAY` and briefly loses the
current job. Still rated minor under this repo's rubric (not a wrong hash,
not a failure of the safety net's detection direction), but it should be
fixed as a pair, not just 1b.

**Fix:** in both arms, set `last_recv = Instant::now();` immediately after
the reconnect-equivalent call succeeds — for 1a, right after the
`if !self.reconnect() { return; }` inside the `Err(overflow)` arm; for 1b,
in the `Ok(())` arm of the donation-switch match alongside
`pending.clear(); continue;`.

Not break-tested to failure for either sub-finding (1a's trigger needs a
`reconnect()` call that takes long enough to exceed the test's 180s-scaled
budget without the harness controlling that duration; 1b's trigger is a
timing coincidence neither is practical to force deterministically without
adding more test-only hooks), but verified by direct reading of the
`Err(overflow)` arm, `relogin_as`, `connect`, and `login`, and by confirming
no existing test exercises either interaction (`grep` for
`relogin`/`beneficiary_at`/`overflow` in test code found nothing relevant).

### 2. [MINOR] Orphaned doc comment — the flood test lost its docs to the new test

Verified by reading `src/pool_connection.rs:1058-1149` directly. The
original doc comment for `the_receiver_loop_really_drops_a_newline_free_stream`
(the SEC-03 flood test) ends at line 1070 ("...so this is plain TCP and no
certificate is involved."). The new doc comment for
`a_silent_pool_is_detected_and_reconnected_to` was spliced in **immediately
below it with no blank line** (lines 1071-1088), so the two `///` blocks
merge into one contiguous doc comment that rustdoc attributes entirely to
`a_silent_pool_is_detected_and_reconnected_to` — which now opens with "A
local listener accepts, then sends a megabyte and a half with no
newline..." (a description of the *other* test). Checked the actual flood
test at line 1148-1149: it has **zero** `///` lines above its `#[test]`
attribute; its documentation is gone.

This is purely a documentation defect — it has no effect on test behaviour,
`cargo test`, or `clippy` (no missing-docs lint is enabled) — but it is
exactly the failure mode `CLAUDE.md` names explicitly: "Doc comments still
belong to the function beneath them... Splicing a new function under an
existing doc comment has orphaned two already." This would be a third
instance in this repo.

**Fix:** insert a blank line between line 1070 and line 1071 so each test
keeps its own doc comment.

### 3. [MINOR, test-quality] The reset-after-not-before-`reconnect()` ordering is not covered — and this is the same shape as the two real misses in Finding 1

Break-tested: moved `last_recv = Instant::now();` in the silence branch to
*before* the `if !self.reconnect() { return; }` call instead of after (a
mutation the task specifically asked me to try). `a_silent_pool_is_detected_and_reconnected_to`
still passes. The shipped code has the order correct in *this* branch
(reset **after** `reconnect()` succeeds), so this specific mutation is not
a live bug — but it is not a hypothetical gap either: it is exactly the
invariant that **is** broken in the two arms Finding 1 identifies (the
`Err(overflow)` arm and `relogin_as`'s success arm), where the reset either
happens before the blocking call or not at all. The test suite has zero
coverage of "reset must happen after, not before, a call that can block for
an unbounded time" — which is precisely why 1a and 1b shipped green. This
is not a separate, lower-priority gap from Finding 1; it's the test-side
reason Finding 1 exists.

### 4. [MINOR] The new test's client-side receiver thread is never joined or stopped

`a_silent_pool_is_detected_and_reconnected_to` spawns the receiver loop
(`thread::spawn(move || worker.receiver_loop())`) and never joins it — only
the *server* thread is joined (`let _ = server.join();`). After the test's
assertions complete, the server thread returns, dropping the `TcpListener`
and all held sockets. The client-side `receiver_loop` thread is still
running and will keep retrying `reconnect()` against the now-dead address
forever (`RECONNECT_DELAY` = 5s between attempts), for the remaining
lifetime of the test binary process. This does not hang or flake the test
itself (the thread isn't joined and doesn't block completion), but it is a
genuine resource/log-noise leak — one extra permanently-retrying thread per
run of this test, logging warnings every 5s for the rest of the process's
life. Worth a `Drop`-based or channel-based way to stop the loop, or at
minimum a comment acknowledging the leak is intentional/accepted.

Concrete (if low-probability) consequence beyond noise: the zombie thread
keeps retrying `TcpStream::connect` against the test's ephemeral
`127.0.0.1:<port>` every `RECONNECT_DELAY` (5s) for the rest of the test
binary's life. `the_receiver_loop_really_drops_a_newline_free_stream` runs
in the same process and also binds `127.0.0.1:0`, then asserts on accept
*ordering* on its own freshly-bound ephemeral port. If the OS ever reused a
port number the zombie is mid-retry against (ephemeral port reuse is rare
but not impossible under load, especially with many tests binding
`127.0.0.1:0` back-to-back in one process), the zombie's connection attempt
could land as an unexpected accept on that other test's listener. Not
observed in the three repeated runs of the new test performed for this
review, and I did not attempt to force the collision — noted as a
theoretical flake vector on the *adjacent* test, not a confirmed one.

### Verified sound (items 3, 4, 6 of the brief; part of item 1, 2, 5)

- **`last_recv` reset correctness holds at 3 of 5 reconnect-equivalent
  sites, not all of them — see Finding 1 for the two misses.** `Ok(0)` EOF,
  `Err(e)` read error, and keepalive-write failure all reset `last_recv`
  **after** a successful `reconnect()`, and none of them can reach the
  reset without `reconnect()` having actually succeeded (`reconnect()`
  only returns without looping when address/wallet are unset, in which
  case the function returns before the reset line — verified by reading
  `reconnect()` at line 663). The silence branch itself is also correct.
  The `Err(overflow)` arm and `relogin_as()`'s success arm are not — this
  correction replaces a wrong "all four paths verified sound" claim from
  my first pass, caught before this ledger was committed.
- **`WouldBlock`/`TimedOut` does not count as activity.** Confirmed by
  reading the match arm (empty body) and by break-testing: adding
  `last_recv = Instant::now()` to that arm recreates the pre-#34 defect,
  and the new test correctly fails (`left: Err(Timeout), right: Ok(1)` — no
  second connection observed within the 20s budget).
- **The injectable `silence_timeout_ms` field.** Single initialisation site
  (`with_tls_fingerprint`, the common constructor for both `new()` and
  `Default`), so every construction path sets it. Grep confirms the only
  `.store()` call anywhere is in the test. `Ordering::Relaxed` is sound: the
  only writer is the test thread, and it writes before `thread::spawn`-ing
  the receiver thread, which establishes happens-before independent of the
  atomic's own ordering. Break-tested the alternative (replace the field
  read with the raw `POOL_SILENCE_TIMEOUT` constant, i.e. simulate the field
  being ignored): the test correctly fails rather than passing by
  coincidence within its time budget, confirming the test genuinely drives
  the shipped 180s-scaled logic through the injectable window rather than a
  shortcut.
- **The test's EOF-immunity.** Read the server closure: it holds every
  accepted socket in `held` and only returns (closing them) after observing
  the *second* accept, so EOF cannot be the reason for that second accept —
  matches the PR body's claim and the pattern SEC-03 got wrong on its first
  attempt. The only path to reconnect in this scenario is the silence
  timeout (keepalive is 60s, unmodified, well outside the test's 20s
  budget; no read error is possible since the socket never closes and never
  sends malformed data).
- **Keepalive-failure-reconnects change.** `last_keepalive` is updated
  *before* the send attempt, so a failed keepalive followed by a successful
  reconnect does not cause the next keepalive attempt to fire early — no
  spin risk. The `continue` after a successful reconnect only skips that
  iteration's (now-moot, since `last_recv` was just reset) silence check;
  nothing else in the loop is skipped.
- **Threshold (180s = 3×60s keepalive).** Reasoned rather than tuned, and
  the PR discloses that honestly under "Not established". `last_recv` is
  set unconditionally at the top of every `Ok(n)` arm regardless of what
  the received bytes are, so **any** inbound data resets the clock — a new
  job, a share-submission response, or a keepalive acknowledgement, if the
  pool sends one (`handle_pool_message`'s generic "has an id, no method"
  branch would handle such a reply, but I could not confirm from this
  codebase or the committed live-run logs whether any pool this miner has
  actually run against acks `keepalived`; `grep` for "keepaliv" in
  `LIVE8H_RUN.log`/`LIVE6H_TLS_RUN.log` found nothing at the logged level).
  So the 180s threshold's robustness rests on the more frequent of two
  things: the ~15s observed job cadence (confirmed), or a 60s keepalive-ack
  cadence (not confirmed either way). Under the job-cadence leg alone, 180s
  is roughly 12× normal cadence — generous margin against an ordinary lull,
  a quiet low-difficulty period, or a donation-rotation relogin, while still
  two orders of magnitude tighter than the 121/88-minute windows #34
  measured. I don't think the number is wrong, but the PR's own
  justification ("real jobs arrive roughly every 15s") is the *weaker* of
  the two legs that actually support it, and whether pools ack keepalives
  is worth confirming on the next live run rather than left implicit.

## Break-testing log (mutate → `cargo test --release --lib pool_connection::tls_tests::a_silent_pool_is_detected_and_reconnected_to` → restore → `cmp`)

All mutations applied to a copy of `src/pool_connection.rs`, restored via
`cp` from a saved original and verified byte-identical (`cmp`) after each.

1. **Refresh `last_recv` on `WouldBlock`/`TimedOut`** (recreates the
   pre-#34 defect) → test **FAILS** (correctly): `left: Err(Timeout), right: Ok(1)`.
2. **Bypass the injectable field**, use `POOL_SILENCE_TIMEOUT` constant
   directly instead of `self.silence_timeout_ms` → test **FAILS**
   (correctly): times out at the full 20s budget with no second connection.
3. **Move `last_recv = Instant::now()` before `reconnect()`** in the
   silence branch (author-suggested mutation) → test **still PASSES**
   (not caught — see Finding 3 above; current shipped order is correct,
   the gap is in test coverage of the *ordering*, not a live bug).
4. **`>=` → `>`** on the silence comparison → test **still PASSES** (not
   caught). Judged a likely-equivalent mutant given wall-clock timing
   granularity — hitting the exact tick is not practically reachable, and
   `>` vs `>=` here doesn't create an exploitable window.
5. Whole silence block removed — this is the author's own break-test,
   described in the PR body and consistent with mutation 1's result above
   (removing the trigger mechanism entirely is a superset of not resetting
   correctly); not independently re-run since it duplicates what mutation 1
   already demonstrates about the test's sensitivity.

## Item 7: PR body vs diff, and `AUDIT.md`

PR body claims match the diff and the reproduced test/clippy results
exactly (160 lib + 20 bin, clippy clean, the single break-test mutation
described). No overstatement found.

**No `AUDIT.md` entry exists yet for this PR** (confirmed: no `#34`/`#35`/
`POOL_SILENCE_TIMEOUT` heading in `AUDIT.md`). This is expected at round-1
review time but must be added before merge, per `CLAUDE.md`'s Operational
Protocol step 3. It should record: the goal (#34: silent pool mined a stale
job for up to 209 minutes with shares silently discarded), the files
changed (`src/pool_connection.rs` only), the behavioural changes
(`POOL_SILENCE_TIMEOUT`/`last_recv` tracking, keepalive failure now
reconnects instead of warning, the test-only injectable
`silence_timeout_ms` field), verification performed (this round's
reproduction: 160 lib + 20 bin pass, clippy clean, the mutations above),
the PR's own "Not established" section (no live run yet against a pool
that actually goes silent; the 180s threshold is reasoned not tuned; shares
lost in the window are still not counted), and — per `CLAUDE.md` step 0 —
this ledger's commit sha once it is committed, so it remains retrievable
after the ledger is removed from the branch before merge.

## Verdict

**MERGEABLE, no blockers, no majors.** Four minors, all closable:

1. The "reset after every reconnect" invariant the PR documents holds at 3
   of 5 reconnect-equivalent sites, not all of them: the `Err(overflow)`
   arm (nested inside `Ok(n)`, resets *before* a call to the unbounded
   `reconnect()` retry loop rather than after or not at all effectively)
   and `relogin_as()`'s success arm (no reset at all) both miss it. Both
   are self-healing (one wasted reconnect cycle, not a loop) and
   over-trigger rather than under-trigger, so neither compromises the
   silence-detection safety net's ability to fire when it should — but each
   spurious reconnect idles every worker for at least `RECONNECT_DELAY` and
   drops the current job, and 1a in particular is plausible on any real
   outage whose `reconnect()` takes longer than 180s. Two-line fix.
   **ACTIONABLE.**
2. Orphaned doc comment: the flood test's documentation was absorbed into
   the new test's doc comment (no blank line separating the two `///`
   blocks), leaving the flood test with no doc comment at all —
   documentation-only, one-line fix (insert a blank line). **ACTIONABLE.**
3. The reset-after-not-before-a-blocking-call ordering invariant isn't
   covered by any test — and this is precisely the test-coverage gap that
   let Finding 1's two misses ship green, not a separate lower-priority
   item.
4. The new test's client-side receiver thread is spawned but never joined
   or stopped, leaking a permanently-retrying background thread for the
   rest of the test process's life, with a low-probability but concrete
   port-reuse flake vector against the adjacent flood test.

Not established / could not verify:
- The practical trigger rate of either half of Finding 1 — 1a is plausible
  on a real outage (only needs `reconnect()` to take >180s, which any
  outage longer than a few minutes will), 1b needs a donation-boundary
  coincidence; neither was forced deterministically in a test.
- Whether any pool this miner has run against acknowledges `keepalived`
  messages, which bears on how much of the 180s threshold's safety margin
  comes from job cadence versus keepalive-ack cadence (see the threshold
  discussion above).
- Behaviour against a pool that goes genuinely silent in the field with
  this fix deployed (the PR body already discloses this as not established
  — no live run yet).
- Whether `AUDIT.md` will be completed accurately before merge — flagged as
  an outstanding requirement, not something this review can close.

No finding here reaches wrong-hash or memory-safety territory; this PR does
not touch the JIT or the hashing path at all.
