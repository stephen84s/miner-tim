# NET-06 — Instrument found-to-submit latency; confirm stream-lock starvation (#40)

**Status:** Completed

Added timing instrumentation (`lock_wait_ms`, `write_ms`, `verify_ms`, `submit_call_ms`, `found_to_submit_ms`) and two deterministic `#[ignore]`d reproducer tests, no behavior change. A 1-hour live run confirmed stream-lock starvation as the cause of #40's latency (`lock_wait_ms` accounts for ~100% of `found_to_submit_ms`; `verify_ms` is 2-5ms, negligible) — and found **both of the run's two rejected shares directly explained by it, though not by wait duration alone**: a third share waited longer (60.4s) than the second rejection (43s) and was accepted. The actual mechanism is a block-height change during the wait — both rejected shares' jobs went stale when a new block arrived mid-wait, while the accepted long-wait share's job stayed at a constant height throughout. This elevates the issue from a latency curiosity to a confirmed cause of real share rejections. Filed **#44** as the actual fix, at Opus tier (concurrency/shared-state work touching issue #17's locking invariants) — not attempted in this instrumentation-only change.

---

*Full record: the `NET-06` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
