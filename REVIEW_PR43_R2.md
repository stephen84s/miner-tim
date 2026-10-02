# REVIEW_PR43_R2 — round 2: lead's response to round-1 findings + rebase onto post-#42 main

Reviewer: pr-reviewer (Opus), cold. Branch `fix/issue-32-rotation-share-loss` @ 78b4850, base origin/main b4501f0.
Scope: diff touches AUDIT.md, CLAUDE.md, src/bin/minertim.rs, src/miner.rs, src/pool_connection.rs, tasks/. No jit/benches/workflows/Makefile/scripts/.cargo — nothing handed off.

## Coverage
- [ ] 1 correctness of new code (F1/F2 fixes)
- [ ] 2 silent failure
- [ ] 3 safety switches (n/a expected)
- [ ] 4 tests / break-tests (F1, F2, mutants)
- [ ] 5 resource use
- [ ] 6 docs/audit accuracy (F3, F4, ledger sha, rebase comment block)
- [ ] 7 concurrency
- [ ] up-to-date with main, CI on rebased head

## Findings

### R2-F1 (minor→major-for-the-record): recorded ledger sha 99bb076 is the PRE-rebase commit, reachable from no branch
`git branch -a --contains 99bb076` → empty. The rebase rewrote it to 995a1ee (which is on the branch and origin). CLAUDE.md step 0 makes the recorded sha the only retrieval mechanism after squash; an unreachable sha disappears at the next gc. Should read 995a1ee.

### R2-F2 (minor in impact, but a false "unconditional" claim; recommend fix before merge): removing the `want == active` reset re-opens the same hole for a RETURN to the same beneficiary
d5bf8ee deleted `if want == active { rotation_wait_since = None; }` claiming `rotation_settled`'s mismatch check "subsumes it". It does not: `rotation_settled` is only called when `want != active` (short-circuit `&&`), so while the schedule sits on the active beneficiary nothing ever clears the stale entry. Sequence with default schedule (cycle 6000s: User 0-5700, Author 5700-5850, Xmrig 5850-6000):
t=5700 rotation User->Author deferred (share in flight) -> wait_since=(Author,5700); read error -> reconnect() blocks on an outage past t=6000 (unbounded loop, confirmed lines ~902-950); resumes with want=User==active -> no call, entry survives; t=11700 want=Author == stored -> waited ~6000s >= 5s -> rotation proceeds with a share in flight, no deferral. Identical shape to round-1 F2, same reconnect() mechanism, just the "third beneficiary" is User and then the cycle returns to Author. Before d5bf8ee this path was closed by the external reset; after it, it is open and nothing tests it (round 1 already noted removing that reset failed no test).
AUDIT.md: "closing the gap unconditionally" and "the per-want check inside rotation_settled subsumes it (it resets on any mismatch, not just a reversion to active)" are false.
Fix: keep both resets (restore the external one), add a test for A-deferred -> (no call while want==active) -> A again.

Evidence for R2-F2: scratch test (added, run, removed, cmp-verified) — in_flight=1, `rotation_settled(Author)` defers and stores (Author,t); backdate t by 10s; `rotation_settled(Author)` again -> returns true ("stale same-beneficiary entry let the rotation through"). The loop-level half (nothing clears it while want==active) is the `&&` short-circuit at the call site.

## Verified (lead's claims reproduced)
- F1 break-test: `in_flight = 0u32` -> `rotation_settled_defers_while_a_submission_is_in_flight` FAILED 5/5 (`running 1 test` each). Restored, cmp identical.
- F2 break-test: removed the `matches!(... if *b != want)` reset -> stale-timestamp test FAILED 3/3 at the `!proceeded` assert. Restored, cmp identical.
- F2 diagnosis: `reconnect()` is `loop { sleep(RECONNECT_DELAY); connect+login }` with no cap; called from four places in the same `receiver_loop` body as the gate (EOF, overflow, read error, keepalive failure, silence). Confirmed.
- Instant underflow concern for `now() - 3600s` on a fresh runner: probed `Instant::now() - 1e9 s` on macOS, no panic (std Timespec permits negative). Dismissed.
- Rebase: merge-base == origin/main b4501f0; PR head 78b4850 == origin branch; all 6 checks pass on runs whose headSha is 78b4850 (pull_request). Removed-lines audit vs main: only the moved comment/block, the old ledger comment, NET-04 status lines; nothing from #42 lost.
- Comment block above `write_and_register`: #37's "What IS closed" text verbatim, F7 wording applied, lock-order note intact; no duplication/contradiction. `//` comment, sits directly above the fn it describes.
- F3: Gap-B sequence matches `relogin_as` (nulls stream under lock, releases, drains, then connect() re-locks to install) — submitter can take the lock in between and get "Not connected". The renamed test does register before `receiver_loop` spawns -> Gap A. Correct.
- F4: LIVE7H_RUN.log: 466 `Share submitted`, 360 immediately after a `New job` line — reproduced. LIVE12H_FIX.log: 3 `Share lost`, each 2 lines after a `Donation: mining to` line — "3 lost at rotations" reproduced; 21 rotations reproduced.
- Full suite release: 186 lib passed / 2 ignored, 20 bin. Clippy -D warnings clean.

### R2-F3 (nit): F4's corrected "873 found" is the penultimate stats line; the log's last line says found:874
LIVE12H_FIX.log last stats line: `Shares: 870/0 (lost:3) (found:874)` — one share found 5s before the kill, still pending. 873 = 870+3 is the balanced figure, so defensible, but say "873 found at the last balanced snapshot (874 at kill, 1 pending)" or it will be "corrected" again.

### R2-F4 (minor): verification counts are pre-rebase
AUDIT says "180 lib passed (+9 over the pre-change 171)". On the rebased head: 186 passed (+9 over main's 177, per #42's entry). The delta is right, both absolutes are stale — the paragraph describes a tree that no longer exists ("verification measured the wrong tree" is a prior #29 finding shape).
