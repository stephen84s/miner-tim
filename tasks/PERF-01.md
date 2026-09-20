# PERF-01 — Bench harness + P-core threads.

**Status:** Completed

Added criterion benchmark (regression guard); default threads now = performance-core count (macOS `hw.perflevel0.logicalcpu`) instead of 2. Confirmed via xmrig docs that affinity/huge-pages are unavailable on ARM macOS.

---

*Full record: the `PERF-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
