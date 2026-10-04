# RESEARCH-02 — Recheck: RandomX v2 / FCMP++ still blocked on Monero mainnet

**Status:** Completed

Re-verified, against primary sources rather than news coverage, that nothing has changed since the 2026-08-15 "still blocked" finding: Monero mainnet is still on hard-fork version 16 (`xmrchain.net` live API; `monero-project/monero` master's `mainnet_hard_forks` table, unchanged at 16 rows); the `fcmp++ hf` GitHub milestone (updated today) is open at 70% complete with no due date, and still lists RandomX V2 as unfinished. Several crypto-news sites claim FCMP++ mainnet-activated on May 6, 2026 — not corroborated by any primary source, and directly contradicted by a May 11-22 audit window and the still-open milestone; this is the second time this specific claim has surfaced with a fabricated date (first recorded 2026-08-15, re: Q1 2026). xmrig's latest stable is unchanged at v6.26.0. RX2-01's gated port correctly stays dormant; no action needed.

---

*Full record: the `RESEARCH-02` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
