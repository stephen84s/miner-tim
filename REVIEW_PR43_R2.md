# REVIEW_PR43_R2 — round 2: lead's response to round-1 findings + rebase onto post-#42 main

Reviewer: pr-reviewer (Opus), cold. Branch `fix/issue-32-rotation-share-loss` @ 78b4850, base origin/main b4501f0.
Scope: diff touches AUDIT.md, CLAUDE.md, src/bin/minertim.rs, src/miner.rs, src/pool_connection.rs, tasks/. No jit/benches/workflows/Makefile/scripts/.cargo — nothing handed off.

## Coverage
- [x] 1 correctness of new code (F1/F2 fixes) — R2-F2
- [x] 2 silent failure — no new swallow; unsent/lost both logged+counted; ledger identity traced through miner.rs worker path, holds
- [x] 3 safety switches — none touched
- [x] 4 tests / break-tests (F1, F2, mutants) — reproduced; R2-F2 has no test
- [x] 5 resource use — nothing new (one Option<(Beneficiary,Instant)> local)
- [x] 6 docs/audit accuracy — R2-F1, R2-F3, R2-F4, R2-F5, R2-F6
- [x] 7 concurrency — no new lock; gate still reads in_flight before pending; Gap-B window between gate-true and relogin_as nulling stream is the documented deferred window
- [x] up-to-date with main, CI on rebased head — yes

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

- mutants.sh 'submit_share|write_and_register|rotation_may_proceed|rotation_settled' 'pool_connection::': 17 tested / 13 caught / 1 unviable / 3 missed — reproduced exactly. All 3 MISSED on line 1255 (`in_flight + pending as u32 > 0`, chooses warn vs info log text) — equivalent for behaviour. Note: the new staleness check sits inside `matches!(...)`, which cargo-mutants does not mutate, and the removed/needed reset lives in `receiver_loop`, outside the regex — so "unchanged 17" means the new logic has ZERO mutant coverage, not that it is covered.

### R2-F5 (minor): AUDIT/tasks cite NET-06 as confirmation, but NET-06 exists only on unmerged PR #45
d5bf8ee added "since **confirmed directly** — see NET-06, which measured `lock_wait_ms` ... and traced two real share rejections to it". `### NET-06` is in dce3d73 on `investigate/issue-40-submit-latency` (PR #45, open), not on main or this branch. If #43 lands first, main's authoritative record cites an entry that does not exist and a result that has not been through its own review. Either merge #45 first or word it as "under investigation in PR #45 (NET-06, unmerged)". Not verified by me: the NET-06 measurements themselves.

### R2-F6 (nit): statuses say "Completed" while the PR is still in review
CLAUDE.md Current task, tasks/NET-05.md and tasks/README.md all say Completed; the review round this file records is still open. tasks/NET-05.md also gives 186 lib tests while AUDIT says 180 — the summary and the authoritative record disagree (see R2-F4).

## Verdict
**Not mergeable as it stands.** No blocker. R2-F2 is a defect introduced by the fix for round-1 F2: deleting the `want == active` reset re-opens the stale-timestamp path for a same-beneficiary return (A deferred -> outage in reconnect() past the slice -> back to User -> next cycle A proceeds without deferral), and AUDIT.md asserts the opposite ("closing the gap unconditionally", "subsumes it"). Behavioural impact is small and counted (a share lands in lost/unsent, not silently), so on impact alone it is minor — but it is a false claim in the authoritative record about the previous round's fix, it reopens a path the prior head closed, and the fix is one restored line plus one test. Fix before merge: restore the external reset (keep both), add a test, correct the AUDIT sentences; also R2-F1 (record 995a1ee, and re-record after any further rebase), R2-F4 (186/177), R2-F5 (NET-06 dependency). R2-F3/R2-F6 optional.
Not verified: NET-06's data; behaviour under a real outage (reasoned + unit-modelled only); x86_64 CI beyond reading its green status.
