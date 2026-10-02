# REVIEW PR #43 — round 3 (Opus, pr-reviewer)

Head reviewed: fa0a2d4. Scope: src/pool_connection.rs, src/miner.rs, src/bin/minertim.rs,
AUDIT.md, CLAUDE.md, tasks/. No JIT, benches or CI files touched -> no hand-off needed.

## Coverage ledger
| # | Item | Status |
|---|---|---|
| 1 | Correctness of change (reconnect clear placement, call sites, relogin boundary) | done (reading) |
| 2 | Silent failure | done — unsent counter logs+counts; no new swallowed error |
| 3 | Safety switches | n/a (no switch changed) |
| 4 | Tests / break-test | done — new test red under 2 mutations; mismatch test red; suite 187/20; clippy clean; mutants 20/15/1/4 |
| 5 | Resource use | done — none |
| 6 | Docs / AUDIT accuracy | done — R3-F3, R3-F4, R3-F5 |
| 7 | Concurrency / lock order | done (reading) |

## Findings

### R3-F1 (minor; CONFIRMED by temporary probe test, reverted + cmp) — a third variant survives at --donate-level 100
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

R3-F1 probe (temporary, reverted, cmp-verified): asserted level-100 schedule never
yields User; rotation_settled(Xmrig) with in_flight=1 defers; backdated stored
entry 3000s (stands in for the 50-min want==active stretch — receiver_loop has no
else-branch at the `if want != active && self.rotation_settled(want)` line ~785);
second rotation_settled(Xmrig) with in_flight=1 returned TRUE. Schedule asserts
passed, final assert failed => bug present on fa0a2d4.
Preconditions (conjunction): --donate-level 100 (accepted: clamp max 100, CLI
passes it through); outage beginning in the Author slice (or first ~5s of Xmrig)
and ending within 5s of the Xmrig->Author boundary; a share in flight/pending at
the first post-reconnect gate call (workers idle through reconnect, so a fresh
share must be found within ms of login); then ~50 healthy minutes. Narrow.
Completeness of remedy (reconnect clear + mismatch + restored want==active clear):
at each later iteration while an entry (B,T) exists: want==active -> clear;
want==B -> resolves within limit of the same episode; want==C (C!=active,B) ->
mismatch clear; reconnect between -> clear. No other writer/caller (only the
receiver thread calls rotation_settled). So entry lifetime is bounded to one
contiguous same-want episode. Alternative: key on slice instance
(elapsed/CYCLE_SECS cycle number + beneficiary).

### R3-F3 (minor, doc) — AUDIT NET-05 body still asserts the superseded round-1 fix as complete
"**Fixed**: ... closing the gap unconditionally ..." and "The now-redundant
external `if want == active` reset ... was removed; the per-`want` check ...
subsumes it" remain in the main narrative. Round 2 disproved both; R3-F1 shows
the want==active reset is not subsumed. The entry now argues with itself
(later "Fixed, properly this time" bullet). Unmerged entry: may be edited in place.
Field doc comment (pool_connection.rs ~337-353) also presents reconnect clearing
as closing the class; false at level 100.

### R3-F4 (minor, doc) — tasks/NET-05.md stale
Says "Nine new tests" (10), "186 lib" (187), "17 mutants" (20/15/1/4), describes
only the round-1 beneficiary-keyed fix, one review round, and keeps "later
confirmed directly in NET-06" which R2-F5 softened in AUDIT.md.

### R3-F5 (nit, doc) — AUDIT internal inconsistencies
"Tests added (9 new + 2 extended)" header vs 10 new (list omits
a_reconnect_discards_...); "twelve tests total — 10 new" elsewhere. R2-F2 bullet
says "see the rewritten F2 bullet above" — it is below. Round-1 F1-F8 bullets sit
after the round-2 paragraph and ledger line. "Round 2 itself noted ... and in
`reconnect()`" — round 2 predates the reconnect() code.

### Verified OK
- reconnect(): clear is unconditional, before the address/wallet early return
  (pinned: moving it below the return turns the new test red).
- 5 reconnect() call sites in receiver_loop (lines 829,852,868,884,906), all
  self.reconnect(). relogin_as does not clear, but needs not: it is reached only
  right after rotation_settled returned true, which sets the field None; on
  relogin failure the next read is NotConnected -> reconnect() clears.
- Lock order: rotation_wait_since -> pending_shares (in rotation_settled); only
  the receiver thread touches rotation_wait_since; reconnect() releases it
  before drain. No deadlock.
- Branch contains origin/main; CI 6/6 green on fa0a2d4.
- Mutants 20/15/1/4 reproduced; 3 misses on line 1277 (warn-vs-info selector,
  log-only); ||->&& at 941 pre-existing (main:847). Note the new test depends on
  that very early-return path (both fields empty).
- Ledger shas 995a1ee and 65b8c4d resolve and are ancestors of HEAD.

## Verdict
Not mergeable as-is: no blocker; R3-F1 is a real (narrow, level-100-only) third
variant, worst case = pre-#32 behaviour for one rotation; R3-F3/F4 leave the
record claiming a completeness it lacks. Fix (or explicitly record R3-F1 as a
known limitation) and correct the docs, then mergeable.
