# PROC-01 — `main` protected after six unreviewed commits reached it.

**Status:** Completed

PR required, all five checks required, strict up-to-date, `enforce_admins: true`, force-push and deletion blocked, 0 approvals (a solo maintainer cannot approve their own PR). Verified by a refused direct push (`GH006`) *and* by reading the live API — the push test alone covers only 2 of the 7 settings. Enforces branch/PR/checks, **not** that a reviewer looked. Three review rounds: round 1 found the entry's "CI is green on them" claim false (`6414ba1`'s JIT gate cancelled with zero jobs); round 2 found the same sentence still claiming the six commits are "all documentation" when two changed CI workflows, and that the PR body had never been updated; round 3 found no blockers and confirmed the mechanism independently (live protection matches the table, the five contexts are string-exact against the real check-run names, and pushing to the PR flipped it `clean`→`blocked`), but found the entry had been corrected by accretion into three self-contradictions — rewritten in place rather than appended to again. Ledger: `REVIEW_PR7.md` (removed from the tree; retrieval sha in LEDGER-01).

---

*Full record: the `PROC-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
