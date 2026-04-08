# RandomX JIT Compiler — Implementation Audit

## Status: COMPLETE (Phase 1-5)

## Results
- **Before JIT**: ~352 H/s total (88 H/s/thread x 4 threads)
- **After JIT**: ~1129 H/s total (282 H/s/thread x 4 threads)  
- **Speedup**: 3.2x per thread
- **All 81 tests pass** including JIT-specific test with known hash vector

## Files Created
- `jit/mod.rs` — Module exports
- `jit/memory.rs` — mmap MAP_JIT, W^X toggle, icache flush
- `jit/aarch64.rs` — ARM64 instruction encoder (~50 functions, 24 tests)
- `jit/compiler.rs` — Bytecode compiler (28 instruction types, 23 tests)
- `jit/audit.md` — This file

## Files Modified
- `randomx/mod.rs` — Added `pub mod jit` (cfg aarch64)
- `randomx/vm.rs` — Added `#[repr(C)]`, `pub(crate)` visibility, JIT integration in execute_vm
- `randomx/tests.rs` — Added `test_vm_calculate_hash_jit` (full mode, known test vector)

## Key Bugs Fixed During Development
1. **Bitmask immediate encoding** (`encode_bitmask_imm`): immr rotation was wrong — needed `(size - rotation) % size` instead of `rotation`
2. **Register-offset load/store**: `ldr_reg`/`str_reg` had wrong option field (000=UXTB instead of 011=LSL), causing SIGILL
3. **CBRANCH target offset**: Interpreter does `pc = target; pc += 1` (executes target+1 next), JIT was branching to target itself causing infinite loops. Fixed to branch to `offsets[target + 1]`

## Architecture
- JIT replaces only `execute_bytecode()` — the outer iteration loop stays in Rust
- Register allocation: r[0-7]->x8-x15, scratchpad->x16, e_mask->x19/x20, nreg->x21
- FP: f[0-3]->d0-d7, e[0-3]->d8-d15, a[0-3]->d16-d23, FSCAL mask->d24
- Callee-saved registers properly saved/restored in prologue/epilogue
- cfg(target_arch = "aarch64") gating — falls back to interpreter on other platforms

## Future Optimization (Phase 6)
- Prefetch scratchpad (`PRFM PLDL1KEEP`)
- Constant folding for small immediates
- NEON vectorization for paired FP ops
- Avoid JIT recompilation when program hasn't changed
