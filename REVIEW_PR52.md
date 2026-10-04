# REVIEW_PR52 — Fix #41: generation tagging (NET-08)

Reviewer: pr-reviewer, Opus tier, round 1. Branch fix/issue-41-stale-generation,
HEAD a61d4ad, base 121ef4c == origin/main tip (verified after git fetch).
Scope: no jit/, benches/, workflows, Makefile, scripts/, .cargo touched -> all mine.

## Coverage ledger
1. Correctness — done
2. Silent failure — done
3. Safety switches — done (none touched)
4. Tests / break-tests — done
5. Resource use — done
6. Docs / audit — done
7. Concurrency — done

## Findings

### Verified
- Base current: merge-base == origin/main == 121ef4c.
- Full suite: 195 passed / 0 failed / 3 ignored lib, 20 bin (release, --locked). Matches claim.
- 12 named rotation tests: each run --exact, each "1 passed". No assertion line in any
  pre-existing test changed (only parse_job args and submit_share->submit_current).
- Break A (check disabled `if false && ...`): 2 fail — refused_not_sent, connect_to_login_window. Restored, cmp OK.
- Break B (always refuse `if true || ...`): 13 fail (10 pre-existing + 3 new). Restored, cmp OK.
  Only 3 of the 13 are from the "rotation/relogin group"; AUDIT wording overstates (see D3).
- Invariant 1: fetch_add is inside the `stream.lock()` block in connect(); the compare in
  write_and_register is under `stream_guard`. Only writer of `generation` is connect() (grep).
- Invariant 2 (calling convention): connect() callers = Miner::initialize (main thread,
  before start_receiver and before workers exist) and reconnect()/relogin_as(), whose only
  callers are receiver_loop. start_receiver called once per PoolConnection. login() only
  ever chained after connect() on the same thread. Holds.

### Findings

**F1 (minor, audit accuracy) — mutants timeout attributed to the wrong test; conclusion holds.**
NET-08 says the `connect -> Ok(())` timeout is `submit_share_registers_the_same_id_it_writes_to_the_wire`
hanging in `server.join()`. Reproduced the mutant by hand (release, `pool_connection::` filter):
that test FAILS promptly — its `submit_current(..).expect(..)` panics on "Not connected" before
`server.join()` is reached. The tests that actually hang (>60 s, killed) are
`a_failed_relogin_login_leaves_no_stream_behind`, `a_submit_is_not_held_hostage_by_a_quiet_receiver`,
`a_submit_is_not_held_hostage_under_full_cpu_load`, `a_submit_reply_read_as_the_login_reply_fails_the_relogin`
(e.g. the first: relogin_as fails, asserts pass, then `server.join()` on a listener that never accepts).
Same mutant applied to a `git archive origin/main` snapshot: the identical four hang, the named test fails.
So "pre-existing harness property, not a new defect" is CORRECT; the named test and therefore the
stated evidence are wrong. None of the 4 new tests hangs. Fix the entry's attribution.

**F2 (minor, audit accuracy) — "Not touched: ... the 'What this does NOT close' block (still accurate)" is false.**
The diff rewrites that block in write_and_register's leading comment (header removed, replaced
by "The stale session id (review round 3, R3-1; #41) ..."). The rewritten text is accurate;
the AUDIT sentence describing it as untouched is not. NET-08 is not on origin/main -> edit in place.

**F3 (minor, audit accuracy) — break-test B description.** "13 tests fail ... the pre-existing
rotation/relogin group": reproduced 13, but they are 3 new + 10 pre-existing, and only 3 of the
10 are among the 12 named rotation tests (a_rotation_waits_at_most_the_settle_limit,
a_share_outstanding_at_a_rotation_is_answered_before_the_relogin,
a_blocked_submitter_at_rotation_is_sent_and_answered_on_the_old_session). The rest are
submit/stream-lock tests. The coverage conclusion stands; the attribution does not.

**F4 (minor, stale status) — "Commits (3 ... not yet pushed)" and "CI ... not yet run (branch not
yet pushed)"; CLAUDE.md "3 commits".** PR #52 is open at a61d4ad, 4 commits; x86 lint/audit/test
and advisory mutants green; jit-macos and jit-linux-arm still pending at review time.

**F5 (minor, test gap — inherent) — the core invariant is guarded by no test.** Mutation: move
`fetch_add` out of the `stream.lock()` block in connect() (plus a 1 ms sleep to widen the window).
`pool_connection::` -> 58 passed, 0 failed. Restored, cmp OK. Expected: the window is a race no
deterministic test can hold open, even with the FairMutex hand-off. But the AUDIT's "five
break-tests" reads as covering the mechanism; it should say plainly that "bump in the same
critical section" is protected by review and the comment only. Optional hardening (not
required): keep the generation inside the mutex with the stream, e.g. `Mutex<Option<(PoolStream,u64)>>`,
so the install/compare pairing is enforced by the type system rather than by convention.

**F6 (minor, pre-existing, follow-up) — the worker spots a job change by `job_id` only.**
worker_loop (miner.rs ~613-630) re-fetches `job` every iteration and stamps the share with that
Arc's `generation`, but resets `job_blob_current` only when `job_id` changes. If a new connection
reissues a job_id string the old connection used (per-connection counters), a share hashed on the
old blob is stamped with the NEW generation and sent; the #41 check cannot see it. Pre-existing,
not introduced here, and unlikely with pools that use unique ids, but it bounds the fix's claim
"a share hashed against [an old-connection job] is refused". Follow-up: also reset when
`job.generation` changes (or use Arc::ptr_eq). No false-refusal path exists from this: the
stamp is always the live Arc's.

**F7 (nit) — a stale refusal is logged twice:** warn "Share not sent ... Stale job" in
submit_share, then ERROR "Failed to submit share" in miner.rs. It's an expected lifecycle event, so
ERROR is loud. Harmless; the live-run grep for 'Stale job' still works.

### Other checks
- xmrig precedent verified: Client.cpp:182 `if (result.clientId != m_rpcId || ...)` and :437
  `m_job.setClientId(m_rpcId)` — the same pattern, keyed on session id.
- Silent failure: refusal returns Err, counted unsent (ledger term already reset in
  reset_share_counters), logged. No swallowed path. No false-refusal path found: every job
  install reads the generation on the same thread that bumped it; receiver clears `pending` after
  every reconnect/relogin, so no old-stream line is parsed after a bump.
- Safety switches: none touched. Resource: +8 bytes per Job. Doc comments: none orphaned
  (generation field, parse_job, submit_current each sit directly above their item).
- Calling convention (item 2): holds. Re-derived above.
- Sealed prediction present before review: yes. It held (doc issues; no logic defect).

## Verdict
Mergeable on code: no blocker or major finding. The generation logic is correct, both
invariants hold in the current diff, and the calling-convention argument holds against the real
threading model. Before merge: fix the audit inaccuracies F1-F4, record F5 as an untested
invariant, and open a follow-up for F6. Still outstanding per the PR itself: the jit-* CI jobs
(pending) and the 2 h live run. Not verified by me: the live run, and the ARM CI jobs.
