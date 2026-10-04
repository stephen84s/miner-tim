# NET-10 — Compare `current_job` by `Arc` identity, not `job_id` string (#53)

**Status:** Active

`worker_loop` only reset its cached blob (`job_blob_current`) when a fetched job's `job_id` string differed from the previous one — filed as #53 during NET-08/PR #52's review, predating and unrelated to #41's generation tagging. If a pool ever reuses a `job_id`, across or within a connection, the worker keeps hashing a stale blob while the freshly-fetched `Job` already carries a different identity. Fix: a new `is_new_job` helper compares by `Arc::ptr_eq` instead, since `PoolConnection` only ever installs a job as a fresh `Arc::new(parse_job(..))` and never mutates or re-wraps one in place — a strict superset of both the old `job_id` check and #41's `generation` check, also catching a same-connection resend with a reused id. Five new tests (`job_change_tests`) cover both directions; three hand break-tests (old `job_id`-only semantics, the generation-based alternative considered and rejected, and an unconditional `true`) each reproduce a predicted, specific failure and revert cleanly. `./scripts/mutants.sh 'is_new_job' 'miner::'` — the first `miner::`-scoped run recorded in this repo's history — found 3 mutants, all 3 caught. Full suite 200/0/3 lib (+5 over the 195/0/3 baseline) + 20 bin, clippy (debug and release) and `cargo audit` clean. No PR opened yet, no review, no live-pool evidence that a pool actually reuses `job_id` values — the fix is correct by construction against `PoolConnection`'s real invariant, not validated against an observed trigger.

---

*Full record: the `NET-10` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
