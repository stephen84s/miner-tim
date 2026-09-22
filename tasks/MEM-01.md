# MEM-01 — Test-suite peak RSS (GitLab #7).

**Status:** Completed

The binary held two never-freed 2 GiB `LazyLock` datasets; now one. Measured, not inferred: release `--lib` peak **8.16 GB → 6.23 GB** at this host's 12-thread default, **~4.07 GB** at `--test-threads=3` (the macos-14 runner's core count, ~2.9 GB headroom under 7 GB); debug `verify-jit` filter **6.27 GB → 5.43 GB**; wall clock 94s→50s and 316s→193s. The issue's "~4.5 GiB" estimate was wrong — the real 12-thread baseline was 8.16 GB. (Review corrections: the debug pair was first recorded as 6.77→4.50 GB and does not reproduce; and "already over GitLab #9's (GitHub #6's) budget" holds only at 12 threads — at the runner's 3 cores `main` measured 6.00 GB, marginal rather than over.) Differential coverage is unchanged: the diff tests' programs, entropy, ma/mx and `dataset_offset` all derive from the seed, not the key, and both paths read the same dataset. The verifier rotation test genuinely needs a second distinct dataset (R9-F2), so it got a synthetic zeroed one. `make verify-jit` 92/92 debug+release; 131 lib + 10 bin green. Unblocks GitLab #9 (GitHub #6).

---

*Full record: the `MEM-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
