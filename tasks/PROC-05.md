# PROC-05 — Three CI-hygiene rules recorded in the agent protocol.

**Status:** Completed

User instruction, now bullets in Operational Protocol step 0: (1) Actions run only on branches that have a PR — already the implemented behaviour since CI-03, recorded for its *consequence*, that a push to a branch with no open PR is checked by nothing; (2) rebase on `main` before merging and merge only on green — **stricter than the enforced `strict: true`**, which a merge commit also satisfies, so the rule asks for a rebase to keep the tested tree identical to the landed one; (3) batch the **push**, not the commits — keep separate logical commits, but push once the work is done, since each push to a PR head costs a full ~30 runner-minute / ~15 wall-clock-minute pass and cancels any run in flight (30.33 and 14.82 over all 58 successful runs; three independent derivations at three sample sizes agree). (First drafted as "squash and push once"; corrected after the user clarified they want the logical commits kept.) Recorded with the unit corrected: the repo is public, `billable.total_ms` is 0, so what is saved is queue time and reviewer attention, not money. No workflow or code change.

---

*Full record: the `PROC-05` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
