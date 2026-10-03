# NET-07 — Fix #44: make the `stream` lock fair (`parking_lot::FairMutex`)

**Status:** Active

Swapped `PoolConnection`'s `stream` field from `std::sync::Mutex` to `parking_lot::FairMutex` (via a private `StreamLock` alias) so `receiver_loop`'s rapid relock/read cycle can no longer barge ahead of a parked `submit_share` — the mechanism NET-06 measured at `lock_wait_ms` mean 11,564ms, p90 24,459ms, max 81,367ms, with two real share rejections. All other `PoolConnection` mutex fields are untouched. Every call site updated; the one accepted behaviour change is that a panic under the lock no longer poisons it (nothing under this lock is expected to panic). #41 (stale session-id window, same lock) was deliberately scoped out as a separate follow-up — its stale-id capture happens before any lock is taken, so it has no shared design dependency with this fix. Implemented and verified locally (baseline and after-fix timings on macOS and Linux, full suite 191/0/3 lib + 20 bin, clippy/audit clean, hand break-test proving the new fairness-handoff test is the reliable regression gate while the latency reproducers alone are not, and a disclosed residual gap: no test here can distinguish `FairMutex` from the merely-eventually-fair plain `parking_lot::Mutex`). **Still open:** PR not yet opened, independent review (planned Opus tier) not yet run, and the real acceptance test — a repeat of NET-06's live `lock_wait_ms` instrumentation run — not yet done.

---

*Full record: the `NET-07` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
