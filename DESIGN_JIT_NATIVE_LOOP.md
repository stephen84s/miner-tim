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
JIT: prologue: load r and a from nreg ONCE   (see note on f/e below)
     init: ma, mx from entropy; sp_addr0 = mx; sp_addr1 = ma; counter = 2048
     loop 2048x:
       sp_mix   = r[readReg0] ^ r[readReg1]              # 64-bit
       sp_addr0 ^= sp_mix & 0xFFFFFFFF ; sp_addr0 &= SCRATCHPAD_L3_MASK64
       sp_addr1 ^= sp_mix >> 32        ; sp_addr1 &= SCRATCHPAD_L3_MASK64
       r[i] ^= sp[sp_addr0 + 8i]                         # XOR-accumulate, i=0..7
       f[i]  = cvt_packed_i32(sp[sp_addr1 + 8i])         # ASSIGN, 8-byte read, i=0..3
       e[i]  = emask(cvt_packed_i32(sp[sp_addr1 + 32 + 8i]))   # ASSIGN + mask, i=0..3
       read_ptr = dataset_offset + (ma & CACHE_LINE_ALIGN_MASK)   # captured from PRE-XOR ma
       <program body, unchanged>
       mx ^= (r[readReg2] ^ r[readReg3]) as u32          # BEFORE the dataset XOR (D1)
       r[i] ^= dataset[read_ptr + 8i]                    # i=0..7
       swap(mx, ma)
       prfm dataset[dataset_offset + (ma & CACHE_LINE_ALIGN_MASK)]   # post-swap ma, MASKED
       sp[sp_addr1 + 8i]  = r[i]                         # i=0..7
       f[i] ^= e[i]
       sp[sp_addr0 + 16i] = f[i]                         # 16-byte write (stride differs from load!)
       prfm sp[next sp_addr0] ; prfm sp[next sp_addr1]   # 2 lines (see 2026-08-29 audit)
       sp_addr0 = 0 ; sp_addr1 = 0
       counter -= 1 ; branch if nonzero
     epilogue: store r, f, e to nreg ONCE  (a is loop-invariant, not stored)
```

Ordering hazards that a reader must not "simplify" (each verified against
`vm.rs`, line refs in the review on MR !1):

- **`mx` is updated BEFORE the dataset XOR** (`vm.rs:1222-1226` precedes
  `:1234-1236`). Doing it after would derive `mx` from post-XOR registers and
  silently produce wrong hashes.
- **`read_ptr` is captured from the pre-XOR `ma`** (`vm.rs:1216`), before the
  `mx` update and before the swap.
- **`sp_addr0` takes the LOW 32 bits of `sp_mix`, `sp_addr1` the HIGH 32**
  (`vm.rs:1175`, `:1177`).
- **`sp_addr0/1` are `^=`, not `=`.** They are initialised to `mx`/`ma` and
  zeroed at the end of each iteration, so only the *first* iteration differs.
  They are live values distinct from `mx`/`ma` — four separate registers.
- **f loads read 8 bytes at stride 8; f stores write 16 bytes at stride 16.**
  Asymmetric on purpose (packed i32 pair in, two f64 out).
- **The prefetch targets `ma` after the swap, and must be masked.** This reaches
  the same address as the reference implementation's prefetch-`mx`-then-swap;
  do not "correct" it.
- **Three prefetches per iteration:** one dataset, two scratchpad.

**f/e prologue loads become dead.** f and e are assigned from the scratchpad at
every loop head, including the first, so the 16 loads currently in
`emit_prologue` for f/e are never read in the native-loop path. Only r and a
need loading. The epilogue must still store f and e (the register file is hashed
between chains).

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
| ~~`x23`~~ | ~~`dataset_offset`~~ — dropped, folded into x22 (C6) | n/a |
| `x24` | `ma` | **new** |
| `x25` | `mx` | **new** |
| `x26` | `sp_addr0` | **new** |
| `x27` | `sp_addr1` | **new** |
| `x28` | iteration counter | **new** |
| `x0`–`x3` | body temporaries (x2/x3 also CFROUND scratch — see capture order) | existing |
| `x4`–`x7`, `x17` | free — unreferenced by every emitter | free |
| `d0`–`d7` / `d8`–`d15` / `d16`–`d23` | `f` / `e` / `a` | existing |
| `d24` | FSCAL mask | existing |
| `d25`,`d26` | **body-clobbered** scratch (cvt / fswap) — no loop state here | existing |
| `d27`–`d31` | genuinely free | free |

`readReg0..3` are **compile-time constants** for a given program (derived from
program entropy), so register selection is baked into the emitted code rather
than read through the config pointer — a small bonus win.

**`e_mask` is NOT constant-folded.** `emit_fdiv_m` reads it *as registers*
x19/x20 (`compiler.rs:519`, `:524`), and §3 promises the body emitters are
untouched. What changes is only the *source* of those registers: the prologue's
`ldr_imm(X19, X2, 0)` / `ldr_imm(X20, X2, 8)` (`compiler.rs:137-138`) become
`mov_imm64(X19, e_mask[0])` / `mov_imm64(X20, e_mask[1])`. e_mask stays in
registers for the whole loop.

New JIT function signature:

**Argument capture order is mandatory, not stylistic.** `emit_cfround` uses x2
(`compiler.rs:607`, `:609`) and x3 (`:608`, `:609`) as temporaries, so the first
CFROUND in the body destroys both incoming arguments. The new prologue must
therefore, immediately after the ten `stp_pre` saves and **before anything
else**, emit:

```
mov x22, x2      # dataset base   - x2 is CFROUND scratch, capture first
mov x28, x3      # iteration count - x3 is CFROUND scratch, capture first
mov x21, x0      # nreg
mov x16, x1      # scratchpad
<32 register-file loads off x0>
mov_imm64 x19/x20 <- e_mask         (see D1 note above)
mov_imm64 x0, FSCAL_MASK ; fmov d24, x0    # x0 clobbered last, after its final use
```

After this point x0–x3 are dead and free as temporaries.

```rust
type JitLoopFn = unsafe extern "C" fn(
    nreg: *mut NativeRegisterFile,
    scratchpad: *mut u8,
    dataset: *const u8,
    iterations: u64,
);
```

`config` is no longer passed at *call* time: everything it carried is either
constant-folded into the emitted code or held in a register. That means it must
be supplied at *compile* time, so `JitCompiler::compile` gains the per-program
values it now bakes in:

```rust
pub(crate) fn compile(
    &mut self,
    bytecode: &[BytecodeInstruction],
    version: RxVersion,
    config: &ProgramConfiguration,  // readReg0..3, e_mask
    ma: u32,                        // initial ma  (entropy(8) & CACHE_LINE_ALIGN_MASK)
    mx: u32,                        // initial mx  (entropy(10))
    dataset_offset: u64,
);
```

Without this the compiler has no access to entropy at all (`compiler.rs:82`,
call site `vm.rs:1161`) and §5.1 would be unimplementable.

## 5. New emission required

Everything below is scaffolding; none of it changes existing instruction
emitters.

1. **Loop init** — `ma`/`mx` from entropy, `sp_addr0 = mx`, `sp_addr1 = ma`,
   counter = 2048.
2. **Scratchpad address computation** — `EOR` + `AND` with `SCRATCHPAD_L3_MASK64`.
3. **Register loads** — 8 × `LDR`+`EOR` for `r` (XOR-accumulate); 4 ×
   packed-i32→f64 **assignment** for `f` reading 8 bytes at stride 8; 4 ×
   packed-i32→f64 + e-mask (`AND` `DYNAMIC_MANTISSA_MASK`, `ORR` `e_mask[i&1]`)
   for `e` at `sp_addr1 + 32 + 8i`. The conversion helper already exists as
   `emit_cvt_packed_int` (clobbers x0/x1, results in d25/d26).
4. **Dataset read** — `AND` with `CACHE_LINE_ALIGN_MASK`, add `dataset_offset`
   and base, 8 × `LDR`+`EOR` into `r`.
5. **`mx`/`ma` update and swap** — `EOR` with the two hardcoded read registers,
   then a register swap (no memory).
6. **Prefetch** — `PRFM PLDL1KEEP` ×3 per iteration: one dataset line at
   `dataset_offset + (ma & CACHE_LINE_ALIGN_MASK)` **after** the swap (the mask
   is required — an unmasked address silently wastes the prefetch), plus the two
   scratchpad lines established on 2026-08-29 (two, not four — see that audit
   entry for why the `+64` pair was dead).
7. **Register stores** — 8 × `STR` for `r` to `sp_addr1 + 8i`; `EOR`
   (`eor_v8b`) for `f ^= e`; 4 × **16-byte** stores for `f` to `sp_addr0 + 16i`.
   Note the deliberate asymmetry: f is *loaded* 8 bytes at stride 8 and *stored*
   16 bytes at stride 16.
8. **Loop control** — `SUBS` + `B.NE` back to loop head.

Estimated addition: ~120–150 words of scaffolding emitted **once** per program,
replacing ~83 words executed **2048 times** plus the equivalent Rust loop body.


## 5a. Implementation constraints (from the MR !1 ABI review)

These are not suggestions; violating any of them is a correctness or
memory-safety bug.

**C1 — The dataset AND mask must remain literally `0x7FFF_FFC0`.** The JIT path
has *no bounds check* (the Rust path went through `RandomXDataset::get_item`).
Worst case: `dataset_offset` max `524287*64 = 33,554,368`, `(ma & mask)` max
`2,147,483,584`, so the last byte read is `2,181,038,016` against a
`DATASET_TOTAL_SIZE` of `2,181,038,080` — **64 bytes, one single cache line, of
margin.** (The review stated this as exactly zero margin; it conflated
`DATASET_EXTRA_ITEMS` = 524287 with `DATASET_EXTRA_SIZE/64` = 524288. One line
of margin, not none — but the practical conclusion is identical.) Widening the
mask by one bit, or letting `dataset_offset` exceed its modulus, is a 2 GiB-scale
out-of-bounds read, not a wrong hash. The mask is encodable as a bitmask
immediate (`and x24, x24, #0x7fffffc0` = `0x927A6318`) so it needs no temp.

**C2 — The emitted loop must contain no `BL`.** x8–x15 (`r`), d0–d7 (`f`),
d16–d23 (`a`), d24 (FSCAL mask), x16/x17 are all caller-saved and all live
across the loop; a single call to an external routine legally destroys every one
of them. This is what permanently forecloses hoisting light mode's
`init_dataset_item` into the native loop, and is consistent with §8 item 3.

**C3 — The epilogue must NOT save/restore FPCR.** `emit_cfround` writes FPCR and
deliberately never restores it: the RandomX rounding mode carries across program
chains (`vm.rs:1106-1108`). It is contained at the outer boundary instead —
`save_rounding_mode()` (`vm.rs:1344`), `set_rounding_mode(0)` (`:1372`),
`restore_rounding_mode()` (`:1427`). A well-meaning "make the JIT ABI-clean"
change here would silently break consensus.

**C4 — `JitMemory::as_fn` cannot distinguish the two ABIs.** It is a
`transmute_copy` guarded only by a pointer-size assert (`memory.rs:93-99`), and
both `JitFn` (3 args) and `JitLoopFn` (4 args) are pointer-sized. Calling loop
code through the old signature silently dereferences a dataset pointer as a
`*const ProgramConfiguration`. Since stages A–C keep both alive, `JitCompiler`
must record which kind it last compiled and `debug_assert!` it in `get_fn()` and
a new `get_loop_fn()`.

**C5 — Use W-forms for `ma`/`mx`/`sp_addr` updates.** These are `u32` in Rust
(`vm.rs:1222-1226`); 64-bit `EOR` would leave bits 63:32 polluted. Numerically
harmless — the masks clear everything ≥ bit 31 before use — but it diverges from
the Rust state and would undermine Stage B's bit-equality premise once the
differential test compares `ma`/`mx` (which D2 requires it to).

**C6 — `x23` is unnecessary; fold `dataset_offset` into the base pointer.**
`dataset_base + dataset_offset` is loop-invariant, so compute it once into x22
and drop x23 from the allocation. (`vm.rs:1216` re-adds them every iteration for
no reason.) Headroom is also larger than §4 implies: x4–x7 and x17 are entirely
unreferenced by any emitter, so the free-temp budget is 9, not 4.

**C7 — Encoders that do not yet exist** and must be added to `aarch64.rs`
(all verified against `as -arch arm64`):
- `SUBS Xd,Xn,#imm` = `0xF1000000` — the existing `sub_imm` is `0xD1000000`,
  which does **not** set flags, so the loop counter needs a new encoder
  (`subs x28,x28,#1` = `0xF100079C`).
- `PRFM PLDL1KEEP` register-offset `0xF8A06800` and immediate-offset `0xF9800000`.
- 32-bit W-forms for `EOR` (`0x4A000000`) and `AND`-immediate (`0x12000000`);
  every existing ALU encoder is hardcoded 64-bit (per C5).
- A signed-offset `STP Dt1,Dt2,[Xn,#imm]` plus a temp holding `x16 + sp_addr0`,
  because f is stored 16 bytes at stride 16 from `sp_addr0` while f and e are
  loaded 8 bytes at stride 8 from `sp_addr1`.

**C8 — d25/d26 are body-clobbered scratch, not free.** `emit_cvt_packed_int`
writes both; `emit_fswap_r` writes d25. No loop state may live there across the
body. d27–d31 are genuinely unreferenced. §4's table is corrected accordingly.

**C9 — Hygiene worth doing while here:** add `D25`–`D31` to the `reg` module (d25
/d26 are currently bare literals, so nothing prevents a future collision with
d24); add `0x1FFFC0` and `0x7FFFFFC0` to `test_bitmask_imm_scratchpad_masks`
(both assemble cleanly, so the assertion is free); assert that CBRANCH targets
stay within the body once it is emitted at a non-zero offset inside the loop
scaffolding.

## 6. Staging (each stage keeps the tree green)

| Stage | Content | Gate |
|---|---|---|
| A | `JitLoopFn` type, register map constants, emitter helpers still unused | builds; 87 vectors pass (path unused) |
| B | Emit loop scaffolding; differential-test against the Rust path at **N=2** iterations (see warning below) | new differential test |
| C | Full 2048-iteration loop; wire into `execute_vm_inner` for v1+full only | 87 vectors + v2 vectors + JIT test + **known-answer hash through the native loop** |
| D | Gate v1+full onto the native loop; keep the Rust loop live for every other configuration | full suite; instructions-retired check |

**Stage D is DONE** (2026-09-01, number corrected 2026-09-02). The default is
now `true`. Measured via `benches/nativeloop_ab.rs`:

| Phase | run 1 | run 2 (independent reviewer) | claim |
|---|---|---|---|
| 1 thread | +6.12% | +6.45% | **+6.1% to +6.5%** |
| 11 threads | +6.76% | +7.42% | **+6.8% to +7.4%** |

96 of 96 paired rounds positive across both runs. The 1-thread row is the
*stronger* replication of the two — the independent baselines agreed to within
0.03% — and was previously left blank, which understated the evidence (R7-F5). Each run reports a much
tighter interval than the gap between the runs, so **the per-run CI does not
describe reproducibility** — quote the range, not an interval. See AUDIT.md
2026-09-02.

An earlier run of this harness reported +9.01%. It was invalid — the baseline arm
was silently running the native loop too — and the figure was also taken on a
contended machine. See AUDIT.md 2026-09-02 for both.

Note the "instructions-retired check" in the gate above turned out to be the
wrong instrument, and the row is left unedited as a record of that. Only
*emitted* words can be counted, and the body-JIT path also executes
Rust-compiled loop code that no `Emitter` sees — so the comparison is a superset
against a subset and cannot yield a net figure. What it *can* yield is the
apples-to-apples eliminated-overhead number, which
`native_loop_emitted_instruction_accounting` reports and guards:

| | per iteration | per hash (16,384 iterations) |
|---|---|---|
| body ABI prologue+epilogue **eliminated** | 83 words | 1,359,872 |
| native loop pre+post+2 **added** | 168 words | 2,752,512 |

The eliminated column is exact and is pure register save/restore. The added
column replaces Rust work of uncounted size, so **the difference of the two is
not a net saving** and must not be quoted as one. The static proxy is
inconclusive on direction; the paired benchmark decided it.

**Stage C is DONE** (2026-09-01). The path is wired but the default is `false`:
`RandomXVm::set_native_loop()` opts in, and `Miner` never calls it, so a default
build behaves exactly as before. Stage D flips it.

The stage-C gate was strengthened on both reviewers' recommendation. The
differential test proves native-loop == interpreter, which is worth nothing if
both are wrong in the same way — so C also requires a **known-answer hash**:
a complete full-mode RandomX hash driven through the native loop must equal the
reference vector `639183aa…` for key `test key 000` / input `This is a test`.
Asserted on `calculate_hash` *and* on `calculate_hash_pipelined` — the latter is
the path the miner actually runs, the former is used by nothing in production.
This is the only test that anchors emitted native-loop code to a real RandomX
result, and the only one that exercises FPCR carry-over across all eight chains.

Stage B is the critical one: a **differential test** that runs both paths from an
identical starting register file and asserts bit-equality of the entire
`NativeRegisterFile` and the scratchpad after N iterations. That catches errors
the end-to-end vectors would only show as an opaque wrong hash.

> **UPDATE (review round 3):** with the loop-state out-pointer in place, N=1
> *does* now catch the D1 ordering bug, so the warning below is partly stale.
> N=2 is retained anyway as belt-and-braces. The original reasoning:
>
> **N=1 was not a sufficient gate.** `mx` is not consumed until the *following*
> iteration's dataset read, so after a single iteration `nreg` and the scratchpad
> are bit-identical whether or not the D1 ordering bug is present — the exact bug
> this design originally contained. **Stage B gates on N=2**; N=1 is kept only as
> a diagnostic for localising a failure. For the same reason the differential
> harness must also write back `ma`, `mx`, `sp_addr0` and `sp_addr1` (via an
> out-pointer or extended signature) and compare them, not just `nreg` and the
> scratchpad.

## 6a. What the tests CANNOT cover (review round 3)

Stated explicitly so nobody mistakes green tests for full coverage:

- **The three prefetches are unverifiable.** `PRFM` never faults and changes no
  architectural state, so a wrong register, a missing mask, or a pre- vs
  post-swap error is invisible to the differential test *and* to the end-to-end
  vectors — and §7 already rules out wall-clock detection at this effect size.
  They are correct by construction and code review only.
- **CI can never run any of this.** `pub mod jit` is
  `#[cfg(target_arch = "aarch64")]` and the GitLab runners are x86_64 Linux, so
  the emitter does not compile there, let alone execute. The differential tests
  are therefore a **mandatory local gate**, not a CI backstop. They now run in
  the default `cargo test` rather than behind `#[ignore]`, so `make test` on an
  Apple Silicon machine is the real gate.
- **The differential test validates the scaffolding, not the body.** Both paths
  go through the same `emit_body`, so anything shared by both prologues — the
  `NativeRegisterFile` offsets, `FSCAL_MASK`, `DYNAMIC_MANTISSA_MASK` — passes
  the diff test and can only fail the end-to-end vectors. Conversely it *does*
  uniquely validate that no body emitter clobbers the values the native loop
  hoists out of the loop (x16, x19/x20, d16–d23, d24), which the body-only path
  would silently recover from by reloading them each iteration.
- **Correction to a standing assumption:** the 87 vectors largely do **not**
  exercise the JIT body. `vm::calculate_hash` passes `None` for the JIT, so the
  light-mode vectors run the interpreter; only about four hashes in the suite
  execute JIT-emitted code end to end. Per-opcode coverage within those is
  saturated, but program-space variety is thin.
- **`sp_addr0`/`sp_addr1` in the loop-state comparison are vacuous** (both sides
  zero them every iteration). Only `ma`/`mx` carry signal there.
- **`readReg0..3` cannot alias** — they are derived into disjoint ranges
  {0,1},{2,3},{4,5},{6,7}, so there is no aliasing case to test. 4 of 16
  combinations are currently covered.

## 7. Verification

- **Correctness (blocking):** all 87 v1 vectors, both v2 vectors, both commitment
  vectors, `test_vm_calculate_hash_jit` (full mode). Any failure blocks the MR.
- **Differential test (new):** JIT-native-loop vs Rust-loop, bit-identical
  register file + scratchpad after 1, 2 and 2048 iterations.
- **Performance — measure the right thing.** It is *wrong* to state this as
  "≈8% of executed JIT instructions removed": the native loop **absorbs** work
  currently done in Rust (scratchpad loads/stores, the i32→f64 conversions, the
  dataset XOR), so JIT-instruction count per hash goes **up**. The real saving is
  (a) the redundant spill/refill of the register file 16,384 times and (b) the
  now-dead f/e prologue loads. The correct metric is therefore **total
  instructions retired per hash across both Rust and JIT**, not a JIT-only ratio.
  Per AUDIT.md 2026-08-29 this machine cannot resolve <10% by wall-clock
  (within-version spread 11–19%), so instructions-retired is the primary
  evidence, with `benches/fullmode.rs` interleaved A/B as corroboration only.
  **No performance claim will be made on wall-clock alone.**
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
