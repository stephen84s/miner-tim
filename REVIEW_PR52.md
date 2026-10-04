# REVIEW_PR52 — Fix #41: generation tagging (NET-08)

Reviewer: pr-reviewer, Opus tier, round 1. Branch fix/issue-41-stale-generation,
HEAD a61d4ad, base 121ef4c == origin/main tip (verified after git fetch).
Scope: no jit/, benches/, workflows, Makefile, scripts/, .cargo touched -> all mine.

## Coverage ledger
1. Correctness — in progress
2. Silent failure — pending
3. Safety switches — pending (none touched expected)
4. Tests / break-tests — pending
5. Resource use — pending
6. Docs / audit — in progress
7. Concurrency — in progress

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
