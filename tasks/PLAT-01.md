# PLAT-01 — JIT ported to Linux aarch64 (issue #2, phase 1a).

**Status:** Completed

`JitMemory` split into cfg'd platform arms: Darwin keeps `MAP_JIT` + `pthread_jit_write_protect_np` + `sys_icache_invalidate` byte-for-byte; Linux uses `mmap(RW)` -> `mprotect(RX)` + `__clear_cache`, with checked `mprotect` and constants read from the platform headers. `compiler.rs` unchanged, as the API shape was preserved. The "only `memory.rs` is platform-specific" assumption **held** and was confirmed. Verified natively on arm64 Linux (colima, no emulation): full suite **131 lib + 10 bin, 2 ignored, 0 failed** — parity with macOS — including the native-loop differential tests against the interpreter and `full_mode_v1_vm_reports_the_native_loop_effective`, the one test that hard-requires a live JIT allocation. Phase 1b (the arm64 CI job) **not** done: the pipeline has no arm64 runner (`no_matching_runner`).

---

*Full record: the `PLAT-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
