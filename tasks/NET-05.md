# NET-05 — Wait out an in-flight share submission before a donation rotation (#32)

**Status:** Completed

A share found right as a donation rotation starts could be lost two ways: already written and registered in `pending_shares` when the rotation drains it (counted lost by #17 — "Gap A"), or blocked on the stream lock when `relogin_as` wins the race and nulls it first, so the submitter's own write fails and nothing is counted at all ("Gap B"). Live logs show 3 lost shares out of 873 found (870 accepted) during 21 rotations; lock-starvation patterns indicate blocked submitters rarely get turns except when the receiver pauses — later confirmed directly in NET-06. Fixed by adding an `unsent_shares` counter for Gap B, and a bounded rotation-deferral gate that checks for both in-flight submissions (including those blocked on the stream lock, via RAII counter) and pending replies before proceeding with `relogin_as`. The gate waits (10ms yields, no lock held) up to 5 seconds, then proceeds anyway and logs a warning. **Nine new tests plus two extended**, all driving real `receiver_loop` or pinning decision-function boundaries.

**Review (Opus): not mergeable as submitted — 1 major, 5 minors, 2 nits, all fixed except a merge-sequencing note and an accepted test limitation.** The major: the in-flight gate's wiring had no deterministic test, so a mutation removing it passed 5/6 runs — fixed with a direct unit test. The most consequential minor corrected a mistake in this entry's own first draft: a `wait_since`-staleness gap first analyzed as "unreachable" (bounded by the 5s settle limit) turned out reachable via `reconnect()`'s unbounded retry loop surviving a real pool outage — fixed by tying the stored timestamp to the specific beneficiary being deferred, not just time elapsed. **Verification:** 186 lib + 20 bin tests pass; clippy clean; make check passes. Mutation testing unchanged after the fix: 17 mutants, 13 caught, 1 unviable, 3 missed (log-text equivalents only). Lead independently re-ran all verification steps and confirmed.

---

*Full record: the `NET-05` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
