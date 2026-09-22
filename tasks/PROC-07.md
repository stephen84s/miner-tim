# PROC-07 — No-ledgers-on-main rule enforced (#19).

**Status:** Completed

Review ledgers committed during review (crash recovery, LEDGER-01) and stripped before merge — a prose rule with nothing enforcing it, now broken into two layers. `.gitignore` gets `REVIEW_*.md` to close the accidental path (a `git add -A` can never sweep them in); a CI check in the `lint` job fails if any reach the PR, closing the deliberate path. Force-add is required and intended; `.claude/agents/_shared-context.md` rules 1-3 updated to specify `git add -f` with a cross-reference. Verified by reading: gitignore behavior confirmed (plain `git add` fails with hint, `git add -f` succeeds); CI shell logic confirmed in both states (exits 0 with no ledgers, exits 1 with ledgers, naming each); workflow file still valid (5 steps in `lint` job, step placed before clippy). The design tension resolved: a check red throughout review trains people to ignore it; the maintainer added the gitignore layer first, so the check gates a deliberate path and the accidental path is closed.

---

*Full record: the `PROC-07` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
