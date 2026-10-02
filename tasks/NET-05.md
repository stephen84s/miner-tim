# NET-05 — Wait out an in-flight share submission before a donation rotation (#32)

**Status:** Completed

A share found right as a donation rotation starts could be lost two ways: already written and registered in `pending_shares` when the rotation drains it (counted lost by #17 — "Gap A"), or blocked on the stream lock when `relogin_as` wins the race and nulls it first, so the submitter's own write fails and nothing is counted at all ("Gap B"). Live logs show 3 lost shares out of 873 found (870 accepted) during 21 rotations; lock-starvation patterns indicate blocked submitters rarely get turns except when the receiver pauses — later confirmed directly in NET-06. Fixed by adding an `unsent_shares` counter for Gap B, and a bounded rotation-deferral gate that checks for both in-flight submissions (including those blocked on the stream lock, via RAII counter) and pending replies before proceeding with `relogin_as`. The gate waits (10ms yields, no lock held) up to 5 seconds, then proceeds anyway and logs a warning. **11 new tests plus two extended**, all driving real `receiver_loop` or pinning decision-function boundaries.

**Three review rounds, each finding the previous round's fix for a `rotation_wait_since` staleness gap was itself incomplete — all now closed.**

- **Round 1 (Opus): not mergeable** — a major (the in-flight gate's wiring had no deterministic test; a mutation removing it passed 5/6 runs, fixed with a direct unit test) and the first version of the staleness gap, originally analyzed in this entry's own first draft as "unreachable" (bounded by the 5s settle limit) — actually reachable via `reconnect()`'s unbounded retry loop surviving a real pool outage. Fixed by keying the stored timestamp to the specific beneficiary deferred.
- **Round 2 (Opus): not mergeable** — round 1's beneficiary-keyed fix missed the case where the schedule returns to the *same* beneficiary after cycling through others during a long outage. Fixed by clearing the field unconditionally on every `reconnect()` call.
- **Round 3 (Opus): not mergeable, then mergeable after one more fix** — round 2's fix still missed a 2-value-ring case: at `--donate-level 100`, `User`'s slice is zero-width, so the schedule only alternates Author/Xmrig, and a deferral can survive a round trip back to the same value with no reconnect and no beneficiary-mismatch ever observed. Fixed by clearing on *any* transition in `want`, checked unconditionally every loop iteration, not just when it differs from the stored entry.

**Verification (final):** 188 lib + 20 bin tests pass; clippy clean; make check passes. Mutation testing: 22 mutants (expanded scope across all three rounds' fixes), 17 caught, 1 unviable, 4 missed — 3 log-text equivalents, 1 a pre-existing unrelated gap on `main`. Lead independently re-ran all verification steps at each round and confirmed.

**Known, accepted limitation (not a defect)**: the final fix's loop-wiring (the call site inside `receiver_loop`, as opposed to the clearing method's own logic) is untested — `CYCLE_SECS` isn't injectable, so no test can drive a real donation cycle boundary. See `AUDIT.md`'s "Not established" section for the full reasoning.

---

*Full record: the `NET-05` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
