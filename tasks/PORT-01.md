# PORT-01 — xmrig upstream survey (v6.24–v6.26).

**Status:** Completed

Most items N/A (RISC-V, Zen4/5, VAES, Windows ARM64). RandomX v2 still blocked — Monero mainnet is on HF 16, no v17 entry in `mainnet_hard_forks`; `PLAN_RANDOMX_V2.md` refreshed. JIT `emit_mem_addr` opt (#3708) implemented, measured at 0.35% fewer emitted instructions, **reverted** — codebase is latency-bound, not instruction-count-bound.

---

*Full record: the `PORT-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
