# Review: MR !1 — JIT native iteration loop
Reviewer: independent agent | Started: 2026-09-01T13:42:20Z | Last updated: 2026-09-01T13:42:20Z

## Status
IN PROGRESS

## Coverage ledger
| Area | File(s) | Status | Notes |
|---|---|---|---|
| Design doc | DESIGN_JIT_NATIVE_LOOP.md | DONE | Read in full; C1-C9 noted |
| AUDIT entries | AUDIT.md (2026-09-01) | NOT STARTED | |
| Rust reference loop | src/randomx/vm.rs | NOT STARTED | |
| Emitter encodings | src/randomx/jit/aarch64.rs | NOT STARTED | |
| Native loop emission | src/randomx/jit/compiler.rs | NOT STARTED | |
| Memory safety (C1, scratchpad masks) | compiler.rs / dataset.rs | NOT STARTED | |
| ABI prologue/epilogue | compiler.rs / memory.rs | NOT STARTED | |
| Tests | src/randomx/tests.rs | NOT STARTED | |
| Benchmark | benches/nativeloop_ab.rs | NOT STARTED | |

## Findings
(none yet)

## Verified-correct
(none yet)

## Remaining work if this review is interrupted
- Everything below the design doc.
