# NEON FP Port Notes — vectorising the aarch64 JIT FP group

Status: **sizing document only** — no implementation. Written 2026-08-15 against
xmrig `master` (post-PR #3708) and our `src/randomx/jit/{compiler.rs,aarch64.rs}`.

Motivation: our JIT keeps each RandomX FP register in a *pair* of scalar
d-registers and emits every FP operation twice (lo lane, hi lane). xmrig's ARM64
JIT keeps each FP register in one 128-bit vector register and emits single
`.2D` NEON instructions. ~94 of 256 program slots are FP-group (95 counting
CFROUND), so the delta is material *if* the M-series' FP pipes are the
bottleneck. Decision gate: the currently-running xmrig-vs-minertim benchmark.

Sources (all xmrig quotes cite these):

- CPP: <https://raw.githubusercontent.com/xmrig/xmrig/master/src/crypto/randomx/jit_compiler_a64.cpp>
- ASM: <https://raw.githubusercontent.com/xmrig/xmrig/master/src/crypto/randomx/jit_compiler_a64_static.S>
- HPP: <https://raw.githubusercontent.com/xmrig/xmrig/master/src/crypto/randomx/jit_compiler_a64.hpp>
- PR #3708 (instruction selection, EXT-based FSWAP, bfi/rbit CFROUND, `sxtl/scvtf` mem loads; ~7.5% on M2 Air): <https://github.com/xmrig/xmrig/pull/3708>

---

## 1. xmrig's ARM64 register map

From the register-allocation comment at the top of ASM (`jit_compiler_a64_static.S`, lines 64–114):

```
# x0  -> pointer to reg buffer and then literal for IMUL_RCP
# x1  -> pointer to mem buffer and then to dataset
# x2  -> pointer to scratchpad
# x3  -> loop counter
# x4  -> "r0"   x5 -> "r1"   x6 -> "r2"   x7 -> "r3"
# x8  -> fpcr (reversed bits)
# x9  -> mx, ma
# x10 -> spMix1
# x11 -> literal for IMUL_RCP
# x12 -> "r4"  x13 -> "r5"  x14 -> "r6"  x15 -> "r7"
# x16 -> spAddr0
# x17 -> spAddr1
# x18 -> unused (platform register, don't touch it)
# x19 -> temporary
# x20 -> temporary
# x21..x30 -> literal for IMUL_RCP

# v0-v15 -> store 32-bit literals
# v16 -> "f0"   v17 -> "f1"   v18 -> "f2"   v19 -> "f3"
# v20 -> "e0"   v21 -> "e1"   v22 -> "e2"   v23 -> "e3"
# v24 -> "a0"   v25 -> "a1"   v26 -> "a2"   v27 -> "a3"
# v28 -> temporary
# v29 -> E 'and' mask = 0x00ffffffffffffff'00ffffffffffffff
# v30 -> E 'or' mask  = 0x3*00000000******'3*00000000******
# v31 -> scale mask   = 0x80f0000000000000'80f0000000000000
```

Key points versus us:

| Thing | xmrig | ours (`compiler.rs` lines 3–13) |
|---|---|---|
| f0..f3 | v16..v19 (one 128-bit reg each) | d0/d1 .. d6/d7 (two scalar regs each) |
| e0..e3 | v20..v23 | d8/d9 .. d14/d15 |
| a0..a3 | v24..v27 | d16/d17 .. d22/d23 |
| FP temp | v28 | d25, d26 |
| E "and" mask (mantissa) | v29 (constant, both lanes) | none — done via GPR round-trip with `DYNAMIC_MANTISSA_MASK` |
| E "or" mask (e_mask) | v30 (loaded once) | x19 = e_mask[0], x20 = e_mask[1] (GPRs) |
| FSCAL mask | v31 | d24 |
| r0..r7 | x4..x7, x12..x15 (`IntRegMap[8] = {4,5,6,7,12,13,14,15}`, CPP line 105) | x8..x15 |
| fpcr shadow | x8 holds *bit-reversed* FPCR | none |

The in-register vector layout is lane 0 = lo, lane 1 = hi (little-endian f128),
which matches our `(f64, f64)` memory layout exactly.

## 2. Per-instruction emission, xmrig vs us

Register index math in xmrig handlers: `dst = (instr.dst % 4) + 16` selects
f-regs v16–19, `+ 20` selects e-regs v20–23, `src = (instr.src % 4) + 24`
selects a-regs v24–27. Opcode constants (CPP lines 71–76):

```cpp
constexpr uint32_t FADD  = 0x4E60D400;   // FADD  Vd.2D, Vn.2D, Vm.2D
constexpr uint32_t FSUB  = 0x4EE0D400;   // FSUB  Vd.2D, Vn.2D, Vm.2D
constexpr uint32_t FEOR  = 0x6E201C00;   // EOR   Vd.16B, Vn.16B, Vm.16B
constexpr uint32_t FMUL  = 0x6E60DC00;   // FMUL  Vd.2D, Vn.2D, Vm.2D
constexpr uint32_t FDIV  = 0x6E60FC00;   // FDIV  Vd.2D, Vn.2D, Vm.2D
constexpr uint32_t FSQRT = 0x6EE1F800;   // FSQRT Vd.2D, Vn.2D
```

Instruction counts below are *emitted 32-bit words per program instruction*.
Ours are counted from `emit_*` in `/Users/stephen/code/gitlab/miner-tim/src/randomx/jit/compiler.rs`;
"addr" = our `emit_mem_addr` (`mov_imm64` ≈ 2 words for a typical 32-bit imm,
`add_reg`, `and_bitmask` ≈ 4 total); "cvt" = our `emit_cvt_packed_int`
(add + ldrsw + scvtf + ldrsw + scvtf = 5).

### FSWAP_R — xmrig 1, ours 3

xmrig (CPP `h_FSWAP_R`, lines 979–989) — post-#3708 EXT form, one instruction
that swaps the two 64-bit halves; note `dst = instr.dst + 16` spans v16–v23 so
one handler covers both f and e registers (no `fswap_is_e` equivalent needed):

```cpp
const uint32_t dst = instr.dst + 16;
// ext  dst.16b, dst.16b, dst.16b, #0x8
emit32(0x6e004000 | dst | (dst << 5) | (dst << 16), code, k);
```

Ours (`emit_fswap_r`): 3 × `fmov_dd` through d25.

### FADD_R — xmrig 1, ours 2

xmrig (CPP `h_FADD_R`, lines 991–997):

```cpp
const uint32_t src = (instr.src % 4) + 24;
const uint32_t dst = (instr.dst % 4) + 16;
emit32(ARMV8A::FADD | dst | (dst << 5) | (src << 16), code, codePos);
```

Ours (`emit_fadd_r`): 2 × scalar `fadd`.

### FADD_M — xmrig ~6, ours ~11

xmrig (CPP `h_FADD_M`, lines 999–1012): `emitMemLoadFP<28>` (≈5 words, see §3)
then one vector add:

```cpp
constexpr uint32_t tmp_reg_fp = 28;
emitMemLoadFP<tmp_reg_fp>(src, instr, code, k);
emit32(ARMV8A::FADD | dst | (dst << 5) | (tmp_reg_fp << 16), code, k);
```

Ours (`emit_fadd_m`): addr(4) + cvt(5) + 2 × `fadd` = ~11.

### FSUB_R — xmrig 1, ours 2

xmrig (CPP `h_FSUB_R`, lines 1014–1020): single `FSUB Vd.2D` like FADD_R.
Ours: 2 × scalar `fsub`.

### FSUB_M — xmrig ~6, ours ~11

xmrig (CPP `h_FSUB_M`): `emitMemLoadFP` + one `FSUB Vd.2D`. Ours: addr + cvt + 2 × `fsub`.

### FSCAL_R — xmrig 1, ours 2

xmrig (CPP `h_FSCAL_R`, lines 1037–1042) — one 128-bit XOR with the v31 scale mask:

```cpp
const uint32_t dst = (instr.dst % 4) + 16;
emit32(ARMV8A::FEOR | dst | (dst << 5) | (31 << 16), code, codePos);
```

Ours (`emit_fscal_r`): 2 × `eor_v8b` (8-byte SIMD XOR per lane) against d24.

### FMUL_R — xmrig 1, ours 2

xmrig (CPP `h_FMUL_R`, lines 1044–1050): `dst = (instr.dst % 4) + 20` (e-regs),
one `FMUL Vd.2D`. Ours: 2 × scalar `fmul`.

### FDIV_M — xmrig ~8, ours ~19

xmrig (CPP `h_FDIV_M`, lines 1052–1071): mem load (≈5) + vector AND/ORR with
in-register masks + one vector divide:

```cpp
constexpr uint32_t tmp_reg_fp = 28;
emitMemLoadFP<tmp_reg_fp>(src, instr, code, k);
// and tmp_reg_fp, tmp_reg_fp, and_mask_reg
emit32(0x4E201C00 | tmp_reg_fp | (tmp_reg_fp << 5) | (29 << 16), code, k);
// orr tmp_reg_fp, tmp_reg_fp, or_mask_reg
emit32(0x4EA01C00 | tmp_reg_fp | (tmp_reg_fp << 5) | (30 << 16), code, k);
emit32(ARMV8A::FDIV | dst | (dst << 5) | (tmp_reg_fp << 16), code, k);
```

Ours (`emit_fdiv_m`): addr(4) + cvt(5) + per-lane GPR round-trip mask
(`fmov_xd`, `and_bitmask`, `orr_reg`, `fmov_dx` — ×2 lanes = 8) + 2 × `fdiv`
= ~19. This is our single worst FP instruction.

### FSQRT_R — xmrig 1, ours 2

xmrig (CPP `h_FSQRT_R`, lines 1073–1078): one `FSQRT Vd.2D` (0x6EE1F800).
Ours: 2 × scalar `fsqrt`.

### CFROUND — xmrig 4 (6 with v2 tweak), ours 11

xmrig (CPP `h_CFROUND`, lines 1106–1136) — post-#3708 form; x8 permanently
holds the *bit-reversed* FPCR (initialised in the static prologue: `mrs x8, fpcr;
rbit x8, x8`, ASM lines 163–165):

```cpp
// ror tmp_reg, src, imm
emit32(ARMV8A::ROR_IMM | tmp_reg | (src << 5) | ((instr.getImm32() & 63) << 10) | (src << 16), code, k);
// bfi fpcr_tmp_reg, tmp_reg, 40, 2
emit32(0xB3580400 | fpcr_tmp_reg | (tmp_reg << 5), code, k);
// rbit tmp_reg, fpcr_tmp_reg
emit32(0xDAC00000 | tmp_reg | (fpcr_tmp_reg << 5), code, k);
// msr fpcr, tmp_reg
emit32(0xD51B4400 | tmp_reg, code, k);
```

The trick: FPCR.RMode is bits [23:22]; bit-reversed those are bits [41:40].
`bfi x8, tmp, #40, #2` inserts RandomX mode bit0→x8[40], bit1→x8[41]; `rbit`
maps x8[40]→fpcr[23], x8[41]→fpcr[22], so the RandomX→ARM rounding-mode bit
swap (0→0, 1→2, 2→1, 3→3) falls out of the reversal for free, and all other
FPCR bits are preserved.

Ours (`emit_cfround`): ror + and + lsr + lsl + 2×and + orr + lsl + mov_imm64 +
orr + msr = 11 words. Note a semantic difference: we write FZ=1 and zero every
other FPCR bit; xmrig preserves the ambient FPCR. This is orthogonal to
vectorisation but worth revisiting in the same pass (this trick is usable in
our scheme unchanged, scalar or vector — it only touches GPRs).

### Count summary (per instruction)

| Instruction | freq/256 | xmrig words | our words | our words, vectorised* |
|---|---|---|---|---|
| FADD_R | 16 | 1 | 2 | 1 |
| FSUB_R | 16 | 1 | 2 | 1 |
| FMUL_R | 32 | 1 | 2 | 1 |
| FSWAP_R | 4 | 1 | 3 | 1 |
| FADD_M | 5 | ~6 | ~11 | ~8 |
| FSUB_M | 5 | ~6 | ~11 | ~8 |
| FSCAL_R | 6 | 1 | 2 | 1 |
| FDIV_M | 4 | ~8 | ~19 | ~10 |
| FSQRT_R | 6 | 1 | 2 | 1 |
| CFROUND | 1 | 4 | 11 | 11 (or 4 with rbit trick) |

\* "vectorised" = adopting xmrig's vector scheme but keeping our current
address-calc (`emit_mem_addr`) and CFROUND.

## 3. The memory-operand path

xmrig `emitMemLoadFP<tmp_reg_fp>` (CPP lines 612–642). Address calc uses x19,
then a *single* SIMD load of the packed-i32 pair plus a widening convert:

```cpp
constexpr uint32_t tmp_reg = 19;
imm &= instr.getModMem() ? (ScratchpadL1_Size - 1) : (ScratchpadL2_Size - 1);
...
emit32(instr.getModMem() ? andInstrL1 : andInstrL2, code, k);   // and x19, src, #mask
// ldr tmp_reg_fp, [x2, tmp_reg]        (128-bit SIMD load, register offset)
emit32(0x3ce06800 | tmp_reg_fp | (2 << 5) | (tmp_reg << 16), code, k);
// sxtl.2d  tmp_reg_fp, tmp_reg_fp      (sign-extend two i32 lanes -> two i64 lanes)
emit32(0x0f20a400 | tmp_reg_fp | (tmp_reg_fp << 5), code, k);
// scvtf tmp_reg_fp.2d, tmp_reg_fp.2d   (two i64 lanes -> two f64 lanes)
emit32(0x4E61D800 | tmp_reg_fp | (tmp_reg_fp << 5), code, k);
```

So: 1 load + `SXTL Vd.2D, Vn.2S` + `SCVTF Vd.2D, Vn.2D` = 3 words for
load-and-convert, both lanes. Group-E masking (FDIV_M only) is then 2 more
words: `AND Vd.16B` with v29 (mantissa mask, both lanes identical) and
`ORR Vd.16B` with v30 (e_mask, per-lane values e_mask[0]/e_mask[1] packed in
one vector).

Ours (`emit_cvt_packed_int`, compiler.rs lines 614–626): add + `LDRSW` +
scalar `SCVTF Dd, Xn` + `LDRSW` + scalar `SCVTF` = 5 words, two dependent
load+convert chains, results split across d25/d26 — which then forces the
8-word GPR round-trip in FDIV_M because the masks live in x19/x20 instead of
vector registers.

Note also xmrig's per-iteration group F/E loads in the static main loop use the
same pattern with `sxtl`/`sxtl2` off one 128-bit load (ASM lines 216–249),
including applying v29/v30 to all four e-regs at load time — but that part is
our Rust harness's job (`vm.rs` lines 1108–1126), not the JIT's, and stays
untouched.

## 4. Prologue / epilogue

xmrig's harness is static assembly, so its "prologue" differs structurally from
ours, but the register-file I/O is the relevant part (ASM lines 149–161):

```
# Load group A registers
ldp     q24, q25, [x0, 192]
ldp     q26, q27, [x0, 224]
# Load E 'and' mask
movi    v29.2d, #0x00FFFFFFFFFFFFFF
# Load E 'or' mask (stored in reg.f[0])
ldr     q30, [x0, 64]
# Load scale mask
mov     x16, 0x80f0000000000000
dup     v31.2d, x16
```

and the final store-back (ASM lines 437–447):

```
stp     q16, q17, [x0, 64]
stp     q18, q19, [x0, 96]
stp     q20, q21, [x0, 128]
stp     q22, q23, [x0, 160]
```

Layout implications for us — **none**. Their `RegisterFile` layout is identical
to our `NativeRegisterFile` (`vm.rs` lines 199–205): r at 0, f at 64, e at 128,
a at 192, each FP register 16 contiguous bytes, lo then hi. A 128-bit
little-endian `LDR Q` of a `(f64, f64)` yields lane0=lo, lane1=hi — exactly the
in-register layout xmrig's `.2D` ops assume. Two caveats:

- `(f64, f64)` is a Rust tuple; its layout is technically unspecified, but our
  *existing* scalar JIT already depends on lo-at-0/hi-at-8, so vectorisation
  adds no new assumption.
- xmrig stashes the E "or" mask in `reg.f[0]` before entering the asm (hence
  `ldr q30, [x0, 64]`). We don't need that hack: our `ProgramConfiguration`
  (x2) has `e_mask: [u64; 2]` at offset 0 — 16 contiguous bytes — so a single
  `LDR Q30, [X2]` loads both lanes directly.

Our new prologue/epilogue (proposal): replace the 32 scalar `ldr_fp_imm` loads
with 6 `LDP Q` (f, e, a) or keep a-regs as 2 `LDP Q`, add `MOVI`/`mov+dup` for
v29/v31 and `LDR Q30, [X2]`; replace 16 scalar FP store-backs with 4 `STP Q`
(f + e; a-regs are constants, unchanged from today). If we adopt the v16–v31
map, we never touch v8–v15 and can *delete* the d8–d15 save/restore entirely
(8 words each side).

## 5. Port impact assessment

### New encodings needed in `src/randomx/jit/aarch64.rs`

All follow the usual `base | (rm << 16) | (rn << 5) | rd` shape unless noted.
Values cross-checked against xmrig's constants above.

| Emitter fn (new) | Instruction | 32-bit template |
|---|---|---|
| `fadd_2d` | FADD Vd.2D, Vn.2D, Vm.2D | `0x4E60D400` |
| `fsub_2d` | FSUB Vd.2D, Vn.2D, Vm.2D | `0x4EE0D400` |
| `fmul_2d` | FMUL Vd.2D, Vn.2D, Vm.2D | `0x6E60DC00` |
| `fdiv_2d` | FDIV Vd.2D, Vn.2D, Vm.2D | `0x6E60FC00` |
| `fsqrt_2d` | FSQRT Vd.2D, Vn.2D (2-op) | `0x6EE1F800 \| (rn<<5) \| rd` |
| `eor_16b` | EOR Vd.16B, Vn.16B, Vm.16B | `0x6E201C00` (we have the `.8B` variant `0x2E201C00`; this just sets Q=1, bit 30) |
| `and_16b` | AND Vd.16B, Vn.16B, Vm.16B | `0x4E201C00` |
| `orr_16b` | ORR Vd.16B, Vn.16B, Vm.16B | `0x4EA01C00` |
| `ext_16b` | EXT Vd.16B, Vn.16B, Vm.16B, #imm | `0x6E000000 \| (imm4<<11)`; FSWAP uses imm=8 → `0x6E004000` |
| `sxtl_2d_2s` | SXTL Vd.2D, Vn.2S (SSHLL #0) | `0x0F20A400 \| (rn<<5) \| rd` |
| `scvtf_2d` | SCVTF Vd.2D, Vn.2D | `0x4E61D800 \| (rn<<5) \| rd` |
| `ldr_q_reg` | LDR Qt, [Xn, Xm] | `0x3CE06800` (xmrig's; or `ldr_d_reg` = `0xFC606800` — D suffices, SXTL only reads the low 64 bits) |
| `ldr_q_imm` / `str_q_imm` | LDR/STR Qt, [Xn, #imm*16] | `0x3DC00000` / `0x3D800000` (imm12 scaled by 16) |
| `ldp_q` / `stp_q` | LDP/STP Qt1, Qt2, [Xn, #imm*16] | `0xAD400000` / `0xAD000000` (imm7 scaled by 16) — prologue/epilogue nicety, optional |
| `dup_2d_x` | DUP Vd.2D, Xn | `0x4E080C00 \| (rn<<5) \| rd` (build v31, and v29 if MOVI is skipped) |
| `movi_2d_bytemask` | MOVI Vd.2D, #bytemask | optional; `mov_imm64` + `dup_2d_x` works and avoids the byte-mask encoder |

No FRINTx variants are needed: rounding stays FPCR-driven (`.2D` arithmetic
honours FPCR.RMode exactly like scalar), so CFROUND is untouched by
vectorisation. INS/DUP-element and `sxtl2` are not needed either (the inner
program never converts the high pair; that only happens in the per-iteration
loads, which stay in Rust).

### `compiler.rs` changes

- Register map: replace `f_regs/e_regs/a_regs` pair-helpers with single-vector
  mapping. Recommended map = xmrig's (f→v16–19, e→v20–23, a→v24–27, temp v28,
  and-mask v29, or-mask v30, scale-mask v31): it leaves v0–v15 untouched, so
  the FP callee-save block disappears (see §6).
- Rewritten emitters: `emit_fswap_r`, `emit_fadd_r`, `emit_fadd_m`,
  `emit_fsub_r`, `emit_fsub_m`, `emit_fscal_r`, `emit_fmul_r`, `emit_fdiv_m`,
  `emit_fsqrt_r`, `emit_cvt_packed_int` (becomes ldr+sxtl+scvtf into v28),
  `emit_prologue`, `emit_epilogue`. Each gets *simpler* (mostly 1 line).
- Unchanged: all integer emitters, `emit_cbranch`, `emit_istore`,
  `emit_cfround` (optionally upgraded to the rbit trick separately),
  `emit_mem_addr`.
- Interpreter (`execute_bytecode` in vm.rs): **unaffected**.
- `NativeRegisterFile` / `ProgramConfiguration` layout: **unaffected** (§4).
- JIT fn signature/ABI: **unaffected**.

### Net instruction-count delta per 256-instruction program

Using the RandomX frequency table (FADD_R 16, FSUB_R 16, FMUL_R 32, FSWAP_R 4,
FADD_M 5, FSUB_M 5, FSCAL_R 6, FDIV_M 4, FSQRT_R 6, CFROUND 1) and the
per-instruction counts from §2 (keeping our addr-calc and CFROUND):

| | current | vectorised |
|---|---|---|
| FADD_R/FSUB_R/FMUL_R/FSQRT_R/FSCAL_R/FSWAP_R (80 slots) | 164 | 80 |
| FADD_M + FSUB_M (10 slots) | 110 | 80 |
| FDIV_M (4 slots) | 76 | 40 |
| CFROUND (1 slot) | 11 | 11 |
| **FP-group total** | **~361** | **~211** |

≈ **−150 emitted instructions (−42%) in the FP group**, i.e. roughly −20% of
the whole program body (integer group ≈ 300–350 words), plus 40 words saved in
prologue/epilogue. More important than code size: FMUL_R (32 slots, the hot
one) drops from 2 dependent-free-but-issue-competing µops to 1, and FDIV_M
loses its two FP↔GPR round-trips (fmov cross-domain moves have multi-cycle
latency on Apple cores). Counter-argument the benchmark will settle: M-series
cores have 4 FP/SIMD pipes and our lo/hi scalar pairs are independent, so the
dual-issue may already hide most of the throughput cost; the wins that
dual-issue can *not* hide are FDIV_M/FSQRT_R (2 divider ops vs 1) and the
fmov round-trips.

### Effort estimate

- `aarch64.rs`: ~13–15 new emitter methods + encoding unit tests ≈ 120–170 LOC.
- `compiler.rs`: rewrite 10 emit fns + prologue/epilogue + map helpers ≈
  150–200 LOC changed (net LOC likely *decreases*).
- `vm.rs`: 0 LOC.
- Files touched: 2 (+ existing JIT tests in compiler.rs keep passing verbatim —
  they go through `nreg`, which is layout-stable).
- Verification: the 87-vector suite (~10 min, per project policy run it — this
  is squarely on the randomx correctness path).
- Total: **roughly 300–400 LOC across 2 files; a focused 1-day change** with
  test time dominating.

## 6. Risks

1. **Callee-saved vector registers (AAPCS64).** Only the *low 64 bits* of
   v8–v15 (d8–d15) are callee-saved; v16–v31 and all upper halves are
   caller-saved. Implications: (a) if we mapped e-regs onto q8–q15 as 128-bit,
   we'd still only need to save d8–d15 (we already do) since the caller can't
   rely on upper halves anyway — but it's cleaner to adopt xmrig's v16–v31 map,
   touch no callee-saved FP regs at all, and delete the d8–d15 save/restore.
   (b) Conversely, the Rust *caller* of the JIT fn must assume every vector
   register's upper half is clobbered — already true today for any extern "C"
   call, so no new hazard, but do not get clever about keeping vector state
   live across `f(nreg, ...)`.
2. **Pipelining (`calculate_hash_pipelined`).** No structural interaction: the
   JIT function's ABI, the nreg store-back per call, and the Rust-side
   per-iteration F/E loads (vm.rs 1108–1126) are all unchanged. The only FP
   state that crosses JIT-call boundaries is FPCR (rounding mode), same as
   today — the Rust harness resets it per hash, and vector ops obey FPCR
   identically to scalar ops.
3. **What xmrig gets from its static-asm harness that we don't have.** Their
   whole 2048-iteration loop, the group F/E scratchpad loads with `sxtl/sxtl2`,
   the E-mask application at load time, the AES FE mix, and the "e_mask parked
   in reg.f[0] / ldr q30, [x0, 64]" trick all live in
   `jit_compiler_a64_static.S`. None of that is portable into our per-program
   JIT body and none is needed: our prologue loads v29/v30/v31 from
   config/immediates instead (§4), and the per-iteration loads stay in Rust
   (they're a separate, smaller optimisation target). Also, their persistent
   bit-reversed-FPCR-in-x8 convention only works because x8 lives across the
   whole asm loop; if we adopt their CFROUND trick we must `mrs+rbit` in our
   prologue each call (2 words — fine).
4. **Silent-wrong-hash encoding bugs.** The failure mode of a mis-encoded NEON
   instruction (Q-bit, size bits, `sz` in FSUB vs FADD — note FSUB is
   `0x4EE0D400`, only bit 23 differs from FADD) is wrong hashes, not crashes.
   Mitigation: per-encoding unit tests in aarch64.rs (execute a 1-instruction
   buffer and check results, as the existing `test_jit_ldr_str_register_offset`
   does) before running the full vector suite.
5. **CFROUND FZ discrepancy (pre-existing).** We currently set FPCR.FZ=1 and
   zero all other bits; xmrig preserves ambient FPCR and only touches RMode.
   Both pass our vectors today, but if we adopt xmrig's CFROUND we change our
   FZ behaviour — verify against the vectors that exercise CFROUND, and treat
   it as a separate commit from vectorisation.
6. **Rust tuple layout.** `(f64, f64)` layout is technically unspecified;
   already load-bearing for the scalar JIT, so no *new* risk — but if it ever
   bites, the fix is `#[repr(C)]` pair structs, not a JIT change.
