# Review: MR !1 — JIT native iteration loop
Reviewer: independent agent | Started: 2026-09-01T13:42:20Z | Last updated: 2026-09-01T14:05:00Z

## Status
IN PROGRESS

## Coverage ledger
| Area | File(s) | Status | Notes |
|---|---|---|---|
| Design doc | DESIGN_JIT_NATIVE_LOOP.md | DONE | Read in full; C1-C9 noted |
| AUDIT entries | AUDIT.md (2026-09-01) | DONE | Two entries read (stage A-C, stage D) |
| Rust reference loop | src/randomx/vm.rs | DONE | execute_vm_inner:1261-1399 read line-by-line; used as ground truth |
| Emitter encodings | src/randomx/jit/aarch64.rs | DONE | All 6 new encoders assembled+compared; bitmask imms verified |
| Native loop emission | src/randomx/jit/compiler.rs | IN PROGRESS | Semantic walk done; disassembly cross-check pending |
| Memory safety (C1, scratchpad masks) | compiler.rs / dataset.rs / vm.rs | DONE | See VC5/VC6 |
| ABI prologue/epilogue | compiler.rs / memory.rs | DONE | See VC7 |
| Tests | src/randomx/tests.rs | NOT STARTED | |
| Benchmark | benches/nativeloop_ab.rs | NOT STARTED | |

## Findings
(none yet)

## Verified-correct

**VC1 — All six new encoders in `aarch64.rs` are bit-exact.**
Assembled the reference forms with `as -arch arm64` and compared to the emitter's
constants:
```
subs x28,x28,#1      f100079c   subs x0,x1,#4095   f13ffc20
eor  w25,w25,w0      4a000339
prfm pldl1keep,[x22,x24]  f8b86ac0
prfm pldl1keep,[x16,#64]  f9802200   prfm pldl1keep,[x16]  f9800200
stp d0,d1,[x0]       6d000400   stp d2,d3,[x0,#16]  6d010c02
ldp d0,d1,[x0,#32]   6d420400
stp d0,d1,[x0,#-512] 6d200400   stp d0,d1,[x0,#504] 6d1f8400
```
Every one matches the unit-test expectations in `aarch64.rs:874-925` and matches
what the emitter actually computes. The imm7 sign handling on `stp_fp_imm` /
`ldp_fp_imm` is correct at both range ends.

**VC2 — The bitmask-immediate encodings for the two new masks are correct**, not
merely "encodable". I re-implemented `encode_bitmask_imm` + `and_imm` standalone
and compared against `as`:
```
and x26,x26,#0x1fffc0     -> 927a3b5a  (emitter) == 927a3b5a  (as)
and x27,x27,#0x1fffc0     -> 927a3b7b  == 927a3b7b
and x0, x24,#0x7fffffc0   -> 927a6300  == 927a6300
and x1, x0, #0x1fffc0     -> 927a3801  == 927a3801
and x0, x0, #0x00ffffffffffffff (DYNAMIC_MANTISSA_MASK) -> 9240dc00 == 9240dc00
```
The existing test only asserts `.is_some()`; this closes that gap.

**VC3 — `CBZ x28` zero-iteration guard encodes and patches correctly.**
`cbz x28, .+16` assembles to `b400009c`; the emitter's
`0xB4000000 | ((skip & 0x7FFFF) << 5) | 28` matches. `skip` is measured from the
guard word to the first epilogue word, which is the correct branch target
(compiler.rs:807/822).

**VC4 — Emitted iteration semantics match `execute_vm_inner` line-for-line.**
Walked every step against vm.rs:1263-1395:
- sp_mix is a 64-bit EOR of r[readReg0]^r[readReg1]; `sp_addr0 ^= low32`,
  `sp_addr1 ^= high32` — W-forms, so bits 63:32 are zeroed exactly as the Rust
  `u32` state is (C5 satisfied).
- r loads: `sp_addr0 + 8i`, XOR-accumulate. f loads: `sp_addr1 + 8i` (stride 8,
  ASSIGN). e loads: `sp_addr1 + 32 + 8i`, then `& DYNAMIC_MANTISSA_MASK |
  e_mask[lane]` with lane = lo->x19/e_mask[0], hi->x20/e_mask[1]. Matches
  `mask_register_exponent_mantissa` (vm.rs:460-466), which also uses e_mask[0]
  for lo and e_mask[1] for hi for *every* i.
- `read_ptr` is captured from the pre-update `ma` (x24) before the mx EOR —
  compiler.rs:943-944 precedes :948-949. **D1 ordering is genuinely present and
  correct**: `eor_reg_w(x25,x25,r2^r3)` at :949 is emitted BEFORE the eight
  dataset `ldr`+`eor` at :952-955.
- swap(mx,ma) after the dataset XOR; dataset prefetch uses the post-swap x24
  masked with CACHE_LINE_ALIGN_MASK against x22 (which already includes
  dataset_offset) — same address as vm.rs:1339.
- r stores: `sp_addr1 + 8i`. `f ^= e` via `eor_v8b`. f stores: **`stp d,d` at
  `sp_addr0 + 16i`** — 16 bytes at stride 16, matching `store_f128`
  (vm.rs:520-525), which writes lo at +0 and hi at +8, exactly the `STP Dt1,Dt2`
  layout. The intentional load-stride-8 / store-stride-16 asymmetry is right.
- two scratchpad prefetches computed from the post-dataset-XOR r registers, both
  masked — same as vm.rs:1380-1391.
- `sp_addr0 = sp_addr1 = 0` at the tail.

**VC5 — C1 (dataset bound) holds, with 64 bytes of margin.**
`DATASET_EXTRA_SIZE = 33_554_432` (dataset.rs:27) so `DATASET_ITEM_COUNT =
34_078_720` and `DATASET_TOTAL_SIZE = 2_181_038_080`. Worst case emitted read is
`dataset_offset(max 524287*64 = 33_554_368) + (ma & 0x7FFF_FFC0)(max
2_147_483_584) = 2_181_037_952`, plus 64 bytes of item = last byte
`2_181_038_015` < `2_181_038_080`. In bounds. The prompt's note is right: the
margin is one cache line, not zero. Backed by three independent guards:
`const _: () = assert!(...)` in vm.rs:42-49 (all profiles), the release
`assert!(dataset_offset <= DATASET_EXTRA_ITEMS*64)` in
compiler.rs:786-789, and `derive_program_params`'s
`% (DATASET_EXTRA_ITEMS + 1)` (vm.rs:1157) which makes the assert unreachable.
The backing store is `Vec<[u64;8]>` of exactly `DATASET_ITEM_COUNT` items, so
`as_ptr()` really does cover the full range.

**VC6 — Scratchpad masking is safe.** `SCRATCHPAD_L3_MASK64 = 0x1F_FFC0` in
compiler.rs matches `vm.rs:34` `(2_097_152/64 - 1)*64 = 2_097_088`. Highest
touched byte: r/f loads and r stores reach `2_097_088 + 8*7 + 8 = 2_097_152`;
f stores reach `2_097_088 + 16*3 + 16 = 2_097_152`. Exactly the buffer size,
never past it.

**VC7 — ABI prologue/epilogue is balanced and AAPCS64-conformant.**
10 x `stp_pre ..., -16` = 160 bytes, 16-byte aligned throughout; the epilogue
pops the same 10 pairs in exact reverse order (x27/x28, x25/x26, x23/x24,
x21/x22, x19/x20, fp/lr; d14/d15 .. d8/d9). x19-x28 and the low 64 bits of
d8-d15 are all saved. x18 is never referenced anywhere in `compiler.rs`. x23 is
now used (loop-state out-pointer) and is saved by the x23/x24 pair. Grepped
every body emitter (compiler.rs:298-757): they touch only x0-x3, x16, x19, x20 —
so nothing in the body can clobber x21-x28 or x16/x19/x20 that the loop hoists.
No `BL` is emitted anywhere in the loop (C2 holds), so x8-x15/d0-d7/d16-d23/d24
being caller-saved is not a hazard.

**VC8 — C3 (FPCR deliberately not restored) is genuinely contained.**
`emit_loop_epilogue` does not touch FPCR. Containment is `save_rounding_mode()`
/ `set_rounding_mode(0)` at the top and `restore_rounding_mode(saved_rm)` at the
bottom of `calculate_hash`, `calculate_hash_pipelined` and
`calculate_hash_versioned` — i.e. around the whole 8-chain hash, which is the
correct granularity (the mode must carry chain-to-chain, and must not leak to
the caller). The differential test additionally compares FPCR between paths.

**VC9 — `CompiledKind` ABI guard is a real `assert_eq!` (not debug_)** in both
`get_fn` (compiler.rs:142) and `get_loop_fn` (compiler.rs:170), so it holds in
release. `kind` is set at the end of both `compile` and `compile_native_loop`.

**VC10 — JIT buffer bound.** `JitMemory::write_code` has a hard
`assert!(byte_len <= self.size)` and `JIT_CODE_SIZE` is 64 KiB; the native-loop
blob is roughly 1.2k words (~4.8 KiB). No overflow risk.

**VC11 — no concurrency hazard.** `calculate_hash_pipelined` is fully
sequential: `execute_vm` for all 8 chains, then `hash_and_fill_aes_1rx4`. The
"simultaneously" in the doc comment is instruction-level overlap by the CPU, not
threads, so nothing else writes the scratchpad while the emitted loop holds a
raw pointer to it.

## Remaining work if this review is interrupted
- Everything below the design doc.
