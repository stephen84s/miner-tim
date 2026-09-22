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
| 1 | Trace every `last_recv` write; reset direction/completeness | Done — one gap found |
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

### 1. [MINOR, ACTIONABLE] `relogin_as()` (donation-rotation reconnect) never resets `last_recv`

`relogin_as` (`pool_connection.rs:717-735`) is a real reconnect: it clears
the job, drops the stream, calls `self.connect(&address)?` and
`self.login(wallet)?` — both genuine socket I/O, and `login()` reads a real
response off the wire, proving the pool is alive. Every *other* place the
receiver loop re-establishes the connection (`Ok(0)` EOF, `Err(e)` read
error, keepalive-write failure, the silence branch itself) sets
`last_recv = Instant::now()` immediately after success. The donation-switch
branch does not:

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

Effect: after a successful donation relogin, `last_recv` still reflects
whenever the *previous* connection last received something. In ordinary
operation this is stale by only a few seconds (jobs arrive roughly every
15s), so it's harmless. The gap only bites if a donation-cycle boundary
(a fixed wall-clock offset, independent of pool traffic) happens to land
while `last_recv` is already close to the 180s silence threshold — which
itself requires the connection to already be in a near-silent state at that
exact moment. Worst case observed by inspection: the very next loop
iteration re-evaluates the (stale) silence check, finds it already expired,
and fires a second, redundant reconnect immediately after the one that just
succeeded. That second reconnect does reset `last_recv` correctly, so this
self-heals in one extra cycle — it is not a loop, and it cannot cause the
miner to *fail* to detect a truly dead pool (if anything it over-triggers,
the safe direction, not the dangerous one). Rated minor rather than major on
that basis, but it is a genuine inconsistency in the invariant the PR
documents ("refreshed on any successful read ... reset after every
reconnect") and worth closing for consistency.

**Fix:** add `last_recv = Instant::now();` in the `Ok(())` arm of the
donation-switch match, alongside `pending.clear(); continue;`.

Not break-tested to failure (the trigger condition is a timing coincidence
that isn't practical to force deterministically in a unit test without
adding more test-only hooks), but verified by reading `relogin_as`,
`connect`, and `login` directly, and by confirming no existing test
exercises the donation-rotation + silence-timer interaction (`grep` for
`relogin`/`beneficiary_at` in test code returned nothing).

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

### 3. [MINOR, test-quality] The reset-after-not-before-`reconnect()` ordering is not covered

Break-tested: moved `last_recv = Instant::now();` in the silence branch to
*before* the `if !self.reconnect() { return; }` call instead of after (a
mutation the task specifically asked me to try). `a_silent_pool_is_detected_and_reconnected_to`
still passes. The shipped code has the order correct (reset **after**
`reconnect()` succeeds), and that ordering matters in a real outage:
`reconnect()` can block for a long time (it loops internally, sleeping
`RECONNECT_DELAY` = 5s between attempts, until it succeeds), so resetting
before the call would understate the true silence and could cause an
immediate redundant reconnect right after a long outage recovers. Today's
code is correct, but nothing in the test suite would catch a future
accidental reordering of these two lines. Not a live defect; flagged so the
gap is known rather than assumed covered.

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

### Verified sound (items 3, 4, 6 of the brief; part of item 1, 2, 5)

- **`last_recv` reset correctness on the four "normal" paths**: `Ok(0)`
  EOF, `Err(e)` read error, keepalive-write failure, and the silence branch
  itself all reset `last_recv` **after** a successful `reconnect()`, and
  none of them can reach the reset without `reconnect()` having actually
  succeeded (`reconnect()` only returns without looping when
  address/wallet are unset, in which case the function returns before the
  reset line — verified by reading `reconnect()` at line 663).
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
  the PR discloses that honestly under "Not established". Given the ~15s
  observed job cadence, 180s is roughly 12× normal cadence — generous
  margin against an ordinary lull, a quiet low-difficulty period, or the
  handful of donation-rotation relogins per session, while still being two
  orders of magnitude tighter than the 121/88-minute windows #34 measured.
  I don't think the number is wrong; no counter-evidence found.

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

1. `relogin_as()` doesn't reset `last_recv` on its success path — one-line
   fix, low real-world likelihood, self-healing, over-triggers rather than
   under-triggers. **ACTIONABLE.**
2. Orphaned doc comment: the flood test's documentation was absorbed into
   the new test's doc comment (no blank line separating the two `///`
   blocks) — documentation-only, one-line fix (insert a blank line).
   **ACTIONABLE.**
3. The reset-after-not-before-`reconnect()` ordering invariant isn't
   covered by a test — current code is correct; noted as a coverage gap,
   not a live bug.
4. The new test's client-side receiver thread is spawned but never joined
   or stopped, leaking a permanently-retrying background thread for the
   rest of the test process's life.

Not established / could not verify:
- The `relogin_as` gap's practical trigger rate — the coincidence window is
  real but I did not attempt to force it deterministically in a test; rated
  by code inspection only.
- Behaviour against a pool that goes genuinely silent in the field with
  this fix deployed (the PR body already discloses this as not established
  — no live run yet).
- Whether `AUDIT.md` will be completed accurately before merge — flagged as
  an outstanding requirement, not something this review can close.

No finding here reaches wrong-hash or memory-safety territory; this PR does
not touch the JIT or the hashing path at all.
