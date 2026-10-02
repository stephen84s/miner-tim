# REVIEW_PR43 — Fix #32: wait out an in-flight submission before a donation rotation

Reviewer: pr-reviewer (raised to Opus). Branch `fix/issue-32-rotation-share-loss` @ 6bae76f, base origin/main 06ec147.
Scope: no `src/randomx/jit/`, `benches/`, workflows, Makefile, scripts, `.cargo/` touched — nothing handed off.

## Coverage ledger
| # | Item | Status |
|---|---|---|
| 1 | Correctness of the change | done — write_and_register body byte-equivalent to the old inline block; gate correct |
| 2 | Silent failure | done — unsent now counted+logged; F8 residual |
| 3 | Safety switches | n/a — no switch touched |
| 4 | Tests (break-tests, mutants) | done — F1, F5; mutants reproduced 17/13/1/3 |
| 5 | Resource use | done — no allocation added |
| 6 | Docs / audit accuracy | done — F2, F3, F4, F7 |
| 7 | Concurrency | done — read order, guard, lock-free sleep all verified; F2, F6 |

## Findings

### F1 (major) — the in-flight term's *wiring* into the gate is not deterministically tested
Break-test: in `rotation_settled`, replaced `let in_flight = self.submits_in_flight.load(SeqCst)` with `let in_flight = 0u32`
(i.e. the gate ignores in-flight submitters entirely — exactly the Gap-B fix removed). Ran
`rtk proxy cargo test --release --lib pool_connection::` six times: **5 of 6 green**, 1 failure in
`a_blocked_submitter_at_rotation_is_sent_and_answered_on_the_old_session`. Restored, `cmp` clean.
- `rotation_may_proceed_truth_table` pins the pure function only; it cannot see whether `rotation_settled` feeds it the real counter.
- `a_submitter_blocked_on_the_stream_lock_counts_as_in_flight` pins the counter only; it never calls the gate.
- The blocked-submitter test's own doc comment admits it is non-deterministic and then says the truth table "is what pins the in-flight term specifically" — it pins the *term in the formula*, not the read. The NET-05 mutation list (#5, "in_flight == 0 → true") shares this blind spot.
Why it mostly passes: main drops the guard right after spawning the receiver, so the submitter usually writes+registers before the receiver's first `rotation_settled`, which then defers on `pending`, not on `in_flight`.
Fix is cheap and deterministic: hold `conn.stream` lock, spawn a submitter, wait for `submits_in_flight == 1`, then call `conn.rotation_settled(Beneficiary::Author, &mut None)` directly and assert `false` (and `true` with a 0ms limit once wait_since is set). Re-run this mutation against it.

### Verified correct (lead's questions 1, 2, 4)
- **Read order.** Submitter: `fetch_add(SeqCst)` -> stream lock -> write -> pending insert (pending mutex) -> unlock -> guard `fetch_sub(SeqCst)`. Gate: `load(SeqCst)` in_flight, then pending under its mutex. If the gate reads in_flight==0 having observed a submitter's decrement, that RMW happens-after its pending insert, so the subsequent pending read sees the entry. Reverse order (pending=0, then submitter registers+exits, then in_flight=0) lets a registered share through. Reasoning is right. Residual (nit, not in the "deferred" list): a submitter that *starts* after the gate reads 0/0 and registers before `relogin_as` takes the stream lock is drained lost; window is the few µs from gate return to `relogin_as`'s stream lock.
- **InFlight guard.** Named binding `_in_flight` (not `let _ =`), constructed immediately after the increment with nothing between that can fail; lives to function end, so every `?`/early return and unwinding drops it. No unmatched increment.
- **Locks.** `write_and_register` holds the stream guard across write and pending insert exactly as before (diffed). `rotation_settled` takes only the pending mutex inside `get_pending_shares` (temporary, released) and sleeps with no lock; the receiver's read guard is block-scoped in the previous iteration.
- **#37 / PR #42.** No assumption about `relogin_as` beyond "drains pending, then connect+login". #42 keeps the drain; #32's fake login replies carry a session id so #42's new check passes. Merged in a scratch worktree (resolved to #32's side): `pool_connection::` 46/46 pass (41 + #42's 5). See F6 for the textual conflict.

### F2 (minor, doc + untested reachable line) — the `want == active` reset IS reachable; the AUDIT's unreachability claim is false
The argument bounds the deferral by `ROTATION_SETTLE_LIMIT` (5 s), but that bounds the gate's *budget*, not wall-clock time between `rotation_settled` calls. While `rotation_wait_since` is `Some`, the same loop iteration can enter `reconnect()` (EOF, read error, keepalive failure, silence), which retries **without bound** every `RECONNECT_DELAY`. A pool outage that begins inside a deferral and lasts longer than the donation window (level 1: author 30 s + XMRig 30 s = 60 s; default level 5: 300 s) brings `want` back to `active` with `wait_since` still set — the reset line fires. Without it, the next rotation ~100 min later reads `waited >= limit` and skips its deferral. Also reachable by any future `ROTATION_SETTLE_LIMIT >= 30 s`. Code as written is correct; the claim in AUDIT.md NET-05 and tasks/NET-05.md ("cannot occur under any valid configuration", "reachability analysis confirms") must be corrected, and the line deserves a test. Simpler fix that also closes a related sliver (stale `wait_since` surviving a short reconnect inside the same slice, so the next iteration proceeds immediately past a share submitted on the new session): clear `rotation_wait_since` wherever the loop reconnects, or key it as `Option<(Beneficiary, Instant)>`.
Break-test reproduced: removing the 3-line reset leaves `pool_connection::` 41/41 green. Restored, `cmp` clean.

### F3 (minor) — Gap B mechanism described wrongly; core test mislabelled
AUDIT NET-05 says Gap B is the stream "nulled by the rotation's own `relogin_as()` *after* `submit_share` acquired the lock but before the write completes". That cannot happen — `relogin_as` needs the same lock. Real mechanism: the submitter is *parked* on the lock, `relogin_as` wins it and nulls the stream, the submitter then gets "Not connected". Separately, `a_share_outstanding_at_a_rotation_is_answered_before_the_relogin` is labelled "(Gap B)" in its doc comment but registers the share before the loop starts and describes it being "drained as lost" — that is Gap A.

### F4 (minor) — numbers
- "3 shares lost at rotations out of 870 found" — 870 is LIVE-02's *accepted* count, not found.
- "360 of 417 submits ... after a `New job`": 360 reproduces from `LIVE7H_RUN.log`, 417 does not — the log has **466** `Share submitted` lines (465 accepted + 1 lost). `awk '/Share submitted/{n++; if(prev ~ /New job/) k++} {prev=$0} END{print k"/"n}'` -> `360/466`.
- `ROTATION_SETTLE_LIMIT` comment "641ms max, median 306ms": reproduces (n=465 `latency_ms=`).

### F5 (minor) — `ROTATION_SETTLE_YIELD` is untested
Replaced the `thread::sleep(ROTATION_SETTLE_YIELD)` with a no-op: `pool_connection::` 41/41 green, three runs. The starvation it exists to break is not exercised by any test. Say so in the entry rather than leave it implied.

### F6 (minor, merge hazard) — guaranteed conflict with PR #42
`git merge` of origin/fix/issue-37-stale-relogin-session conflicts in `src/pool_connection.rs` (plus AUDIT.md, CLAUDE.md, tasks/README.md): #42 edits the comment block that this PR moves into `write_and_register`. Taking this side drops #42's "What IS closed (#37)" text silently; it must be carried into the moved comment by hand.

### F7 (nit) — moved comment says "`sid` above is read before any lock"
`sid` now lives in the caller, not above `write_and_register`.

### F8 (nit) — "every way a found share can go is now counted" is slightly strong
A poisoned `pending_shares` lock in `write_and_register` still writes without registering and returns `Ok` (pre-existing), so that share is in no term.

### Mutants
`./scripts/mutants.sh 'submit_share|write_and_register|rotation_may_proceed|rotation_settled' 'pool_connection::'` -> 17 tested, 13 caught, 1 unviable, 3 missed (all `src/pool_connection.rs:1201`, the `in_flight + pending > 0` log-message selector). Reproduces the claim; the three are log-text-only, agreed. Note mutants cannot produce the F1 mutation (a load replaced by 0), which is why the "0 behavioural misses" reading needs F1 alongside it.

### Suite
`cargo test --release`: 178 lib / 20 bin pass, 2 ignored. `cargo test` (debug): 178 / 20 pass. `cargo clippy --all-targets --release -- -D warnings`: clean. Gate break-test (call site forced to proceed): core test and blocked-submitter test both fail — the gate itself is covered.
