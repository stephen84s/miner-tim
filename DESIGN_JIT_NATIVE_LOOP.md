# Design: move the RandomX iteration loop into the JIT

**Status:** proposed — implementation staged behind this document
**Branch:** `feat/jit-native-loop`
**Motivation:** AUDIT.md 2026-08-29 (profiling findings)

---

## 1. The problem, measured

Our aarch64 JIT compiles only the 256-instruction **program body**. The
2048-iteration loop around it stays in Rust, so every iteration crosses a
function boundary and the prologue/epilogue reload and re-store the entire
RandomX register file.

Measured on this repo (M2 Max):

| | words |
|---|---|
| total emitted per program | 1024 |
| **prologue + epilogue** | **83 (8.1%)** |
| program body | 941 |

The JIT function is invoked `RANDOMX_PROGRAM_ITERATIONS * RANDOMX_PROGRAM_COUNT`
= 2048 × 8 = **16,384 times per hash**, so that 83-word preamble runs
**~1.36 million times per hash** doing nothing but spilling and refilling
registers that the previous iteration just computed.

xmrig's ARM64 JIT does not pay this: it generates the whole loop natively and
keeps the register file resident across iterations. This is the single largest
structural gap between us and xmrig, and — unlike the three changes measured on
2026-08-29 — it is large enough (~8% of executed JIT instructions) to clear this
machine's measurement noise floor (see §7).

## 2. Scope

**In scope (this MR):** a native-loop JIT path for **RandomX v1 (rx/0) in full
mode** — i.e. exactly the configuration the miner runs in.

**Explicitly out of scope, keeping the existing path:**
- **Light mode.** Its dataset read is `init_dataset_item()`, which executes
  SuperscalarHash programs; JITing that is a separate, much larger job. Light
  mode is used by tests and by the Android target, never by the CLI miner.
- **RandomX v2 (rx/2).** The v2 F/E mix (4 AES rounds) and the `mp`-aliasing
  prefetch would need emitting too. v2 is gated and unreleased; it keeps the
  current (correct, vector-verified) path until a follow-up MR.
- **NEON-vectorised FP registers.** Orthogonal, sized separately in
  `NEON_FP_PORT_NOTES.md`. A native loop makes it *more* attractive later, but
  bundling them would make this MR unreviewable.

So after this MR there are two execution paths. That is a deliberate, temporary
cost, and §8 states how it collapses.

## 3. Target structure

Today (per iteration, Rust drives):

```
Rust: compute sp_addr0/1 -> load r,f,e from scratchpad
JIT:    prologue (load 32 regs) -> body -> epilogue (store 24 regs)   <-- 83 words
Rust: dataset read -> xor r -> swap -> prefetch -> store r -> f^=e -> store f
```

Proposed (per program, JIT drives):

```
JIT: prologue (load register file ONCE)
     loop 2048x:
       sp_addr0/1 from r[readReg0]^r[readReg1]
       load r from sp[sp_addr0]; load f,e from sp[sp_addr1] (cvt + e-mask)
       <program body, unchanged>
       dataset read at dataset_offset + (ma & CACHE_LINE_ALIGN_MASK); xor into r
       mx ^= r[readReg2]^r[readReg3]; swap(mx, ma); prefetch
       store r to sp[sp_addr1]; f ^= e; store f to sp[sp_addr0]
       sp_addr0 = sp_addr1 = 0; decrement counter; branch
     epilogue (store register file ONCE)
```

The body emission is **unchanged** — this MR adds scaffolding around it, it does
not touch the 28 instruction emitters. That is the main reason it is reviewable.

## 4. Register allocation

Existing allocation is preserved; new loop state uses currently-free callee-saved
registers. (`x18` is reserved on macOS and remains unused.)

| Reg | Holds | Status |
|---|---|---|
| `x8`–`x15` | `r[0..7]` | existing |
| `x16` | scratchpad base | existing |
| `x19`,`x20` | `e_mask[0]`, `e_mask[1]` | existing |
| `x21` | `nreg` pointer (for the single final store) | existing |
| `x22` | dataset base pointer | **new** |
| `x23` | `dataset_offset` | **new** |
| `x24` | `ma` | **new** |
| `x25` | `mx` | **new** |
| `x26` | `sp_addr0` | **new** |
| `x27` | `sp_addr1` | **new** |
| `x28` | iteration counter | **new** |
| `x0`–`x7`, `x17` | temporaries | existing |
| `d0`–`d7` / `d8`–`d15` / `d16`–`d23` | `f` / `e` / `a` | existing |
| `d24` | FSCAL mask | existing |
| `d25`–`d31` | temporaries (FP conversion) | existing/free |

`readReg0..3` and `e_mask` are **compile-time constants** for a given program
(derived from program entropy), so register selection is baked into the emitted
code rather than loaded through the config pointer — a small bonus win.

New JIT function signature:

```rust
type JitLoopFn = unsafe extern "C" fn(
    nreg: *mut NativeRegisterFile,
    scratchpad: *mut u8,
    dataset: *const u8,
    iterations: u64,
);
```

`config` is no longer passed: everything it carried is either constant-folded
into the code or held in a register.

## 5. New emission required

Everything below is scaffolding; none of it changes existing instruction
emitters.

1. **Loop init** — `ma`/`mx` from entropy, `sp_addr0 = mx`, `sp_addr1 = ma`,
   counter = 2048.
2. **Scratchpad address computation** — `EOR` + `AND` with `SCRATCHPAD_L3_MASK64`.
3. **Register loads** — 8 × `LDR`+`EOR` for `r`; 4 × packed-i32→f64 for `f`;
   4 × packed-i32→f64 + e-mask (`AND`/`ORR`) for `e`. The conversion helper
   already exists as `emit_cvt_packed_int`.
4. **Dataset read** — `AND` with `CACHE_LINE_ALIGN_MASK`, add `dataset_offset`
   and base, 8 × `LDR`+`EOR` into `r`.
5. **`mx`/`ma` update and swap** — `EOR` with the two hardcoded read registers,
   then a register swap (no memory).
6. **Prefetch** — `PRFM PLDL1KEEP`, both dataset and scratchpad, matching the
   two-line behaviour established on 2026-08-29 (not four).
7. **Register stores** — 8 × `STR` for `r`; `EOR` (`eor_v8b`) for `f ^= e`;
   4 × 16-byte stores for `f`.
8. **Loop control** — `SUBS` + `B.NE` back to loop head.

Estimated addition: ~120–150 words of scaffolding emitted **once** per program,
replacing ~83 words executed **2048 times** plus the equivalent Rust loop body.

## 6. Staging (each stage keeps the tree green)

| Stage | Content | Gate |
|---|---|---|
| A | `JitLoopFn` type, register map constants, emitter helpers still unused | builds; 87 vectors pass (path unused) |
| B | Emit loop scaffolding; run it for **1 iteration** and compare register file against the Rust path | new differential test |
| C | Full 2048-iteration loop; wire into `execute_vm_inner` for v1+full only | 87 vectors + v2 vectors + JIT test |
| D | Remove the now-dead per-iteration Rust path for that configuration | full suite; static word-count check |

Stage B is the critical one: a **differential test** that runs both paths from an
identical starting register file and asserts bit-equality of the entire
`NativeRegisterFile` and the scratchpad after N iterations. That catches errors
the end-to-end vectors would only show as an opaque wrong hash.

## 7. Verification

- **Correctness (blocking):** all 87 v1 vectors, both v2 vectors, both commitment
  vectors, `test_vm_calculate_hash_jit` (full mode). Any failure blocks the MR.
- **Differential test (new):** JIT-native-loop vs Rust-loop, bit-identical
  register file + scratchpad after 1, 2 and 2048 iterations.
- **Performance:** expected ≈8% of executed JIT instructions removed. Per
  AUDIT.md 2026-08-29 this machine cannot resolve <10% by wall-clock, so the
  primary evidence is the **static emitted/executed instruction count**
  (deterministic), with `benches/fullmode.rs` interleaved A/B as corroboration
  only. **No performance claim will be made on wall-clock alone.**
- **Safety review:** the emitted code writes the scratchpad through raw pointers
  in a loop; a bad mask is a memory-safety bug, not just a wrong hash. Every mask
  and bound gets an explicit justification comment, as done for the dataset index
  invariant.

## 8. How the two paths collapse

Once this lands and is proven, the follow-ups in order:
1. v2 support in the native loop (emit AES F/E mix + `mp` aliasing) — deletes
   the v2 exception.
2. NEON-vectorised FP registers (`NEON_FP_PORT_NOTES.md`) — now clearly
   worthwhile, since a native loop keeps FP registers resident.
3. Light mode stays on the interpreter path permanently; that is correct, not
   debt — it exists for tests and memory-constrained targets.

## 9. Risks

| Risk | Mitigation |
|---|---|
| Wrong hashes from a subtle emission bug | Differential test at 1/2/2048 iterations before the end-to-end vectors |
| Memory-safety bug via a bad address mask | Explicit invariant comments per mask; masks are constants, not data-dependent |
| Callee-saved register clobber (x19–x28 now all used) | Prologue/epilogue already save x19–x28; extend and assert |
| Two paths diverge over time | §8 collapse plan; v2 follow-up queued immediately |
| Change too large to review | Staged A–D; body emitters untouched; this document |
