# BENCH-02 — Barriered the multi-thread A/B phase (#5).

**Status:** Completed

Threads ran unsynchronised A-B-B-A schedules while the aggregation assumed round i was concurrent across threads — assuming the thing being measured. A barrier makes it true; because a barrier risks trading phase drift for **tail-idle bias**, the harness now *measures* that (levels and a paired CI, not just a difference). Review returned nine minors and **four were errors of reasoning**: a claimed "+0.13 pp point-estimate rise" was between-run drift and is withdrawn (a fourth run gave +7.05%, below both unbarriered runs); "no divergence assert fired" was never evidence about the barrier, since checksums are invariant to it; the barrier had introduced a **deadlock** on a real divergence (panic between two `wait()`s strands every sibling), now fixed and break-tested at the last thread and last pair; and the spread level was never reported, which is what makes the conclusion safe (5-8%, not 30-40%). What survives: the concurrency claim is now true, and the aggregate CI narrows across all four barriered runs (±0.19-0.41 vs ±0.43/±0.64). Per-thread paired diffs are now labelled authoritative. Bench-only; no `src/` change. Two rounds; round 2 found the corrections had repeated the pattern — a withdrawn over-claim replaced by a new one, a half-corrected baseline range, and a stale hardcoded list in the harness's own output. Ledger: `REVIEW_PR13.md` (removed from the tree; retrieval sha in LEDGER-01).

---

*Full record: the `BENCH-02` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
