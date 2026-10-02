# REVIEW PR #43 — round 3 (Opus, pr-reviewer)

Head reviewed: fa0a2d4. Scope: src/pool_connection.rs, src/miner.rs, src/bin/minertim.rs,
AUDIT.md, CLAUDE.md, tasks/. No JIT, benches or CI files touched -> no hand-off needed.

## Coverage ledger
| # | Item | Status |
|---|---|---|
| 1 | Correctness of change (reconnect clear placement, call sites, relogin boundary) | done (reading) |
| 2 | Silent failure | pending |
| 3 | Safety switches | n/a (no switch changed) |
| 4 | Tests / break-test | pending |
| 5 | Resource use | pending |
| 6 | Docs / AUDIT accuracy | pending |
| 7 | Concurrency / lock order | done (reading) |

## Findings

### R3-F1 (minor; candidate) — a third variant survives at --donate-level 100
`rotation_wait_since` outlives its deferral episode whenever `want` returns to
`active` before the episode resolves: `rotation_settled` is only called when
`want != active`, so nothing clears the entry. With the default 3-slot ring
(User -> Author -> Xmrig) the next non-active `want` is always a different
beneficiary, so the mismatch check clears it. At level 100 `user == 0` and the
ring is a 2-cycle Author <-> Xmrig, so the next non-active `want` IS the
persisted beneficiary. Sequence (all connection-healthy after step 2):
1. active=Author. Outage during the Xmrig slice; reconnect() clears the field
   at its start and retries.
2. reconnect() returns with < ROTATION_SETTLE_LIMIT (5s) left in the Xmrig
   slice; a worker submits on the new job; top of loop: want=Xmrig != Author,
   in_flight/pending > 0 -> deferral stored (Xmrig, T).
3. Slice ends inside the 5s: want=Author == active -> gate not called, entry
   persists. Connection healthy -> no reconnect.
4. 50 min later want=Xmrig != Author -> rotation_settled(Xmrig): stored b ==
   want (no mismatch), waited = 50 min >= 5s -> proceeds immediately past any
   in-flight share. Same symptom as R2-F2.
Consequence bounded: identical to pre-#32 behaviour (no deferral) for one
rotation. Root cause is structural: the episode is not ended when the gate
stops being consulted. Restoring `if want == active { clear }` in addition to
the reconnect clear would close it (with both, an entry survives only while
the gate is called every iteration with a changing-or-resolving want).

### R3-F2 (nit/minor) — lock-order fact undocumented, comment inaccurate
rotation_settled holds rotation_wait_since across get_pending_shares():
new order rotation_wait_since -> pending_shares. Not a live deadlock (only the
receiver thread ever takes rotation_wait_since; reconnect() takes it alone and
releases before drain). The comment "dropped at function return, well before
this sleep" is wrong: it is dropped by explicit drop() before the sleep.
