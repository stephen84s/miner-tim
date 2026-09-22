# PLAT-02 — JIT gate made explicit (GitHub #2 interim mitigations).

**Status:** Completed

`scripts/verify-jit.sh` + `make verify-jit` (macOS host) and `make verify-jit-linux` (native linux/arm64 via colima; read-only repo mount, container-local `CARGO_TARGET_DIR`). 92 tests — JIT unit + native-loop differential + known-answer vectors — in **both** debug and release, so the native loop's `debug_assert!` guards finally execute (GitLab #6 — now GitHub #4). Hard gates: non-zero exit on any failure *and* on an unexpected test count, so a renamed module cannot empty a filter and leave the gate green (verified by a deliberate drift run). Platform-coverage wording landed in README + this file (CI validates the interpreter on x86_64 Linux only; GitLab #9 — now GitHub #6 — is the GitHub-Actions plan); F11's Linux `mprotect`-per-compile cost recorded in `jit/memory.rs`; the gate documented as mandatory before any MR touching `src/randomx/jit/`, with its result pasted into the MR description.

---

*Full record: the `PLAT-02` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
