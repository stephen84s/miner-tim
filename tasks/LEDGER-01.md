# LEDGER-01 — Review ledgers removed from the tree; the rule that put them there fixed.

**Status:** Completed

User asked why `REVIEW_*.md` was being committed. Nobody had decided it should be — it fell out of crash recovery, and 13 files reached **530 KB, larger than the entire Rust source** (504 KB). `_shared-context.md` rules 1-3 tell reviewers to commit as they go because agents get killed mid-review (three did; one hit 560k tokens); that is sound and **unchanged**. What it conflated was durability *during* review with permanent retention on `main`. Now the lead `git rm`s the ledger before merge, once findings are in `AUDIT.md` and the PR description. Nothing lost: `git show <sha>:REVIEW_X.md` still works. Two reference classes preserved — the one **source** citation (`jit/memory.rs:113` → PLAT-01 F11) now names a retrieval command, and 24 `AUDIT.md` citations are left unedited (merged history is corrected by appending) with this entry as the pointer that resolves them. Defect in a design from PR #9, four days old. Not addressed: `AUDIT.md` at 289 KB and the task board at 46% of `CLAUDE.md` — the same accretion one level up, mandated by step 4.

---

*Full record: the `LEDGER-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
