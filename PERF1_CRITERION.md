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

### Correction, made before the first run

The first version of this section said "phase 1 paired diff CI excludes zero,
positive". **That is trivially satisfied and measures nothing** — the harness's
paired diff is native-loop vs body-JIT, which is already about +6% before this
change. Corrected below. No measurement had been taken when this was rewritten;
the pre-registration still stands.

### What the harness actually gives us here — and it is a good design for this

`emit_iteration_pre` is called **only** from `compile_native_loop`. The body JIT
(`JitCompiler::compile`) never touches it; it emits one program body and leaves
the iteration work in Rust. Verified, not assumed: the only two call sites are
inside `compile_native_loop` and inside a unit test.

So within a single harness run the **body-JIT arm is an untouched control** and
the native-loop arm is the treatment, measured paired, A-B-B-A, in one process.
Machine-level drift — thermals, scheduling, other load — moves both arms and
cancels out of the diff. That is a far better control than comparing raw
hashrates between processes, which is the design weakness that produced a
withdrawn claim in BENCH-02.

The quantity of interest is therefore the **change in the paired diff**:

    effect = (native-vs-body diff, this branch) - (native-vs-body diff, main)

### Keep only if ALL of these hold

1. **Phase 1 (1 thread) effect is positive in every paired branch/main
   comparison**, at least **three** of each, run alternately. Phase 1 is the
   sensitive configuration: no inter-thread memory-bandwidth contention to swamp
   a per-iteration change.
2. **The separation exceeds the between-run spread.** Concretely: the minimum
   phase-1 diff observed on the branch must exceed the maximum observed on
   `main`. Overlapping ranges are a null result, not a small win — the observed
   run-to-run spread of the phase-1 diff on unmodified code is about 0.4 pp
   (5.89-6.30 across five runs), which is larger than any effect this change can
   plausibly produce.
3. **Phase 2 (11 threads), per-thread paired diff (n=11, the authoritative
   statistic per BENCH-02): no regression.** The branch's range must not sit
   entirely below `main`'s.

**Revert if any fails**, and record the measurement rather than re-running until
a run cooperates. "Null result, reverted" is a valid and complete outcome for
this issue; it is exactly what happened to `emit_mem_addr`.

### A deterministic check that needs no benchmark

`native_loop_emitted_instruction_accounting` counts emitted instructions per
section. The instruction-count reduction is an exact, reproducible fact and will
be recorded regardless of what the timing shows — a smaller loop that runs no
faster is still a true and useful thing to have measured, and is the outcome the
issue predicts.

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
