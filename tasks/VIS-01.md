# VIS-01 — Silent MAP_JIT fallback made visible (GitLab #4 + #3 — closed pre-migration, never imported).

**Status:** Completed

`JitCompiler::new()` failure is now logged at `error!` instead of `.ok()`-swallowed; `RandomXVm::native_loop_effective()` evaluates all four native-loop preconditions from the VM's own fields and is the single authority for both the per-worker startup report and for arming the share verifier. The startup line in `main` now reports the *request* through a testable `startup_state_line()`. Closes GitLab #3 as a side effect: the verifier's enablement no longer carries its own `cfg!` term, so x86_64 verification goes from armed-but-vacuous to off. Independent review returned **mergeable, four minors**; all four closed on the branch (two orphaned doc comments, the missing positive assertion on `native_loop_effective()`, a vacuous test assertion, and an over-stated `new_jit()` error), plus the false non-aarch64 warning text. Review record: `REVIEW_ISSUE4.md` (removed from the tree; retrieval sha in LEDGER-01). **Merged as MR !2 (`1790a9f`); GitLab #3 and #4 closed.**

---

*Full record: the `VIS-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
