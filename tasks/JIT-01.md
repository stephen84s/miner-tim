# JIT-01 — JIT native iteration loop.

**Status:** Completed

Stages A-D done on `feat/jit-native-loop` (MR !1): the 2048-iteration loop is emitted in ARM64 and is now the **default** for rx/0 + full mode + aarch64. Measured **+6.8%-+7.4%** at 11 threads across two independent runs (96/96 paired rounds positive; per-run CIs are tighter than the between-run spread and do not describe reproducibility), via the new paired A/B harness, which also verified ~147k hashes bit-identical against the body JIT. Thirteen rounds of independent review (round 5 caught the harness measuring the native loop against itself; the earlier +9.01% claim is retracted). Round 13: **mergeable, no blockers, no majors**; its three minors plus three older deferred findings are filed as GitLab issues #3–#8. Also repaired CI, red on `main` since the edition-2024 migration. **Merged as MR !1 (`365d288`).**

---

*Full record: the `JIT-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
