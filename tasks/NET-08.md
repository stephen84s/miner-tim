# NET-08 — Close the stale-session-id window via generation tagging (#41)

**Status:** Active

A share can theoretically be hashed with a stale session-id from a connection that no longer exists — `submit_share` reads `session_id` before taking any lock, while `login()` writes it only after releasing the lock. Investigation revealed no revenue is lost (those shares are rejected by the pool anyway), but accounting is wrong (counted `rejected` instead of `unsent`). Fix: stamp each `Job` with the TCP connection's generation number at install time; refuse it in `write_and_register` if the connection has been replaced since. Design refined by Opus planning passes and implemented at Opus tier (dispatched before PROC-11 existed — this task is what motivated that refinement, not an example of it; see AUDIT.md's corrected note). PR #52 reviewed (Opus, round 1): mergeable logic, no blockers/majors, 7 minor/nit findings all fixed or disclosed (a wrong mutants-timeout attribution, a miscounted break-test, two stale status lines, a genuinely uncovered invariant disclosed rather than hidden, a pre-existing unrelated follow-up filed as #53, and a log-level nit). 195/0/3 lib tests + 20 bin, clippy/audit clean, all six CI checks green. **Live acceptance run (2h, real pool, under `caffeinate`): clean** — 3 donation rotations (each a `relogin_as()` call), zero `Stale job` lines, zero anomalies, ledger balanced 170/170. **Mergeable.**

---

*Full record: the `NET-08` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
