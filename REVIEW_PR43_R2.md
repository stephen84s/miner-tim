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
