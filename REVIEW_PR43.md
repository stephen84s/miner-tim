# REVIEW_PR43 — Fix #32: wait out an in-flight submission before a donation rotation

Reviewer: pr-reviewer (raised to Opus). Branch `fix/issue-32-rotation-share-loss` @ 6bae76f, base origin/main 06ec147.
Scope: no `src/randomx/jit/`, `benches/`, workflows, Makefile, scripts, `.cargo/` touched — nothing handed off.

## Coverage ledger
| # | Item | Status |
|---|---|---|
| 1 | Correctness of the change | in progress |
| 2 | Silent failure | pending |
| 3 | Safety switches | pending |
| 4 | Tests (break-tests, mutants) | pending |
| 5 | Resource use | pending |
| 6 | Docs / audit accuracy | pending |
| 7 | Concurrency | in progress |

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
