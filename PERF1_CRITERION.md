# GitHub #1 — pre-registered acceptance criterion

**Written and committed before the first benchmark run.** The point is that the
threshold cannot be chosen after seeing the data. This project has form here:
`AUDIT.md` 2026-08-15 records `emit_mem_addr`, which cut emitted instruction
count by 0.35%, measured *slower*, then null, and was reverted.

## The change

`emit_cvt_packed_int_to` takes explicit destinations, so the f-register load in
`emit_iteration_pre` converts straight into `f_regs(i)` instead of landing in
`d25`/`d26` and paying two `FMOV Dd, Dn` per lane. 8 FMOVs per iteration,
16,384 iterations per hash = 131,072 fewer emitted instructions per hash.

The e path keeps the scratch pair: it masks and ORs the value before storing, so
it genuinely needs it somewhere neutral first.

## What must be true to keep it

Measured with `benches/nativeloop_ab.rs` — the barriered harness from BENCH-02,
whose per-thread paired differences are the authoritative statistic (the
aggregate's n=24 overstates independence).

**Keep only if BOTH hold across two independent runs:**

1. **Phase 1 (1 thread), paired diff over 24 rounds: 95% CI excludes zero, positive.**
   This is the sensitive configuration — no memory-bandwidth contention between
   threads to swamp a per-iteration change.
2. **Phase 2 (11 threads), per-thread paired diff (n=11): CI does not exclude
   zero in the *negative* direction.** A real regression under contention
   disqualifies the change even if phase 1 is positive.

**Revert if either fails**, and record the measurement rather than retrying
until a run cooperates. "Null result, reverted" is a valid and complete outcome
for this issue; it is what happened to `emit_mem_addr`.

## Baseline gate — a run that fails this is discarded, not interpreted

Per the standing rule that a tight CI is not evidence of a quiet machine:

* 1-thread body-JIT mean **>= 560 H/s** (known-good ~570; observed 568.1-572.9)
* 11-thread body-JIT mean **>= 4900 H/s** (observed 4982-5020.7)

A run below either figure was thermally compromised and is thrown away without
being read as a result.

## Correctness, which is not negotiable

A wrong destination register here produces **wrong hashes, not a crash** — the
pool silently rejects the shares. `make verify-jit` (92 tests, debug *and*
release) must pass before any number is taken seriously. The differential tests
against the interpreter are the ones that matter; the known-answer vectors alone
pass even with an inert JIT.
