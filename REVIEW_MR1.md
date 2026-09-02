# Review: MR !1 — JIT native iteration loop
Reviewer: independent agent | Started: 2026-09-01T13:42:20Z | Last updated: 2026-09-02T (round 6 in progress)

## Status
ROUNDS 5-6 COMPLETE — ROUND 7 (delta bbecd15..HEAD) IN PROGRESS

## Coverage ledger
| Area | File(s) | Status | Notes |
|---|---|---|---|
| Design doc | DESIGN_JIT_NATIVE_LOOP.md | DONE | Read in full; C1-C9 noted |
| AUDIT entries | AUDIT.md (2026-09-01) | DONE | Two entries read (stage A-C, stage D) |
| Rust reference loop | src/randomx/vm.rs | DONE | execute_vm_inner:1261-1399 read line-by-line; used as ground truth |
| Emitter encodings | src/randomx/jit/aarch64.rs | DONE | All 6 new encoders assembled+compared; bitmask imms verified |
| Native loop emission | src/randomx/jit/compiler.rs | DONE | Semantic walk + full end-to-end disassembly cross-check (VC13) |
| Memory safety (C1, scratchpad masks) | compiler.rs / dataset.rs / vm.rs | DONE | See VC5/VC6 |
| ABI prologue/epilogue | compiler.rs / memory.rs | DONE | See VC7 |
| Tests | src/randomx/tests.rs | DONE | Read + executed in release; adequacy analysis below |
| Benchmark | benches/nativeloop_ab.rs | DONE | Read + executed; F1 |

## Findings

### F1 — The A/B benchmark now measures the native loop against ITSELF; the +9.01% claim is not reproducible from the committed tree  [MAJOR]
**Where:** `benches/nativeloop_ab.rs:93-96`, `src/randomx/vm.rs:1669` and `:1696`
**Claim:** The "baseline" arm of the paired benchmark is not the body-JIT path.
Both arms run the native loop, so a re-run of the harness as committed can only
report noise around 0%, and its per-round hash-equality assertion degenerates to
comparing a path against itself.

**Evidence:**
```rust
// benches/nativeloop_ab.rs:93
let mut base_vm = RandomXVm::new_full(KEY, dataset.clone());
let mut nat_vm  = RandomXVm::new_full(KEY, dataset.clone());
nat_vm.set_native_loop(true);        // <-- base_vm is never set to false
```
and, in the *same commit* (260bc89, "stage D"), `RandomXVm::new_full` gained
`use_native_loop: true` (vm.rs:1696; `new` likewise at :1669). Before that commit
the field defaulted to `false` and `base_vm` really was the body JIT — which is
almost certainly the state in which the +9.01% figure was actually measured. The
default flip and the harness landed together, so the harness was invalidated by
the very change it was written to justify. `src/randomx/tests.rs` was updated in
that commit to add an explicit `set_native_loop(false)` to
`test_vm_calculate_hash_jit`; the bench was not given the same treatment.

**Failure scenario:** No wrong hashes and no memory unsafety. The damage is to
the evidence:
1. The headline performance claim in AUDIT.md, CLAUDE.md and the design doc
   cannot be reproduced or re-checked by anyone (including CI or a future
   reviewer) by running the harness that is supposed to support it.
2. The "~147,000 hashes verified identical between the two paths" correctness
   argument — described in AUDIT.md as mattering "more than the timing", and as
   the *only* evidence covering program space beyond the single known-answer
   stream — is, for any run of the committed code, a tautology. Whatever run
   produced the original number is not repeatable.
3. A future regression that slows or breaks the body-JIT path would be invisible
   to this harness.

Fix is one line (`base_vm.set_native_loop(false);`), plus re-running the
measurement to confirm the number still holds.

**Also — and this is why F1 is MAJOR rather than a benchmarking nit:** the
harness is the *only* thing in this MR that exercises the native loop over a
broad program space. Recorded coverage after F1:
- differential tests: 4 seeds x {1,2,3} iterations + 1 seed x 2048 = 5 distinct
  programs, covering 4 of the 16 `readReg0..3` combinations;
- known-answer tests: 8 real chains x 2048 iterations each, twice
  (`calculate_hash` and `calculate_hash_pipelined`), against the canonical
  vector — genuinely strong, but one fixed program stream;
- benchmark: claimed ~147,000 hashes over thousands of programs — **void as
  committed**, because both arms are the same code path.
So the surviving broad-space evidence is ~13 programs, not thousands.

**Confirmed empirically.** Ran `cargo bench --bench nativeloop_ab -- 1 3 64` on
this branch:
```
=== 1 thread ===
  body JIT     : mean    606.6 H/s   median    606.2
  native loop  : mean    606.5 H/s   median    608.0
  paired diff  : -0.02%  (95% CI -0.92% .. +0.89%, n=6)
  verdict      : NO MEASURABLE DIFFERENCE (CI includes 0)
```
Two arms of identical code, as predicted.

**Commit archaeology confirms the mechanism** (raises the earlier MEDIUM to
HIGH): `git show 260bc89^:src/randomx/vm.rs` has `use_native_loop: false` at
both constructors; `git show 260bc89:src/randomx/vm.rs` has `true`; and
`benches/nativeloop_ab.rs` is *added* by 260bc89 already without a
`set_native_loop(false)` on `base_vm`. So the harness has never been valid in
any committed tree state — it was presumably valid in the author's working tree
before the vm.rs edit was applied.

**Important framing:** my near-zero result does NOT show the +9.01% was wrong.
It shows the claim is *unverifiable from this tree*. My own absolute numbers
(606 H/s both arms, vs the recorded 337.5 / 358.3) differ from the recorded run
by ~70%, which is itself a reminder that the single-thread phase is dominated by
machine state; I used 3 pairs x 64 hashes rather than the default 12 x 256.

**Confidence:** HIGH.

### F2 — `make test` runs debug, but the AUDIT's verification was release, so the debug_assert safety nets never ran in the profile that was actually verified  [MINOR]
**Where:** `Makefile:42-43` (`test:` -> `cargo test`), AUDIT.md 2026-09-01
("Full suite: 105 passed ... (release)", "106 passed ... (release)")
**Claim:** Three of the guards this MR added are `debug_assert!` and are
therefore inert in a release test run: the imm7 range checks on
`stp_fp_imm`/`ldp_fp_imm` (aarch64.rs), the `subs_imm` imm12 check, and the
CBRANCH forward-target check (compiler.rs:637-640). The AUDIT reports the suite
run in release. So the runs that were used as evidence did not exercise those
asserts, and `make test` (which would) is a different profile from the one
verified.
**Failure scenario:** None today — I proved the CBRANCH invariant holds by
construction (see VC12) and the imm7 offsets used are 0/16/32/48, far inside
range. This is a process gap, not a live bug: the guards read as safety nets in
the AUDIT narrative but do not run there.
**Confidence:** HIGH on the facts; the "no live bug" part is HIGH for CBRANCH
and imm7 specifically.

### F3 — The CBZ zero-iteration patch has no range assertion, unlike the loop back-branch  [MINOR]
**Where:** `src/randomx/jit/compiler.rs:816-822`
**Claim:** The back-branch gets
`debug_assert!((-(1<<18)..(1<<18)).contains(&rel), "loop back-branch out of B.cond imm19 range")`,
but the forward CBZ patch two lines later does
`e.code[zero_guard] = 0xB4000000 | ((skip & 0x7FFFF) << 5) | reg::X28;`
with no check that `skip < 2^18`. A silent `& 0x7FFFF` truncation would produce
a branch to a wrong (possibly negative) offset.
**Failure scenario:** Unreachable today — the whole native-loop blob measures
254 words with an empty body and roughly 1.2k words with a full 256-instruction
program (I emitted and disassembled it), against a 2^18-word limit. It would
take a ~200x growth in emitted body size to reach. Recording it because the
neighbouring branch is asserted and this one is not, so the asymmetry looks like
an oversight rather than a decision.
**Confidence:** HIGH that the assert is absent; HIGH that it is currently
unreachable.

### F4 — Two 2 GiB `LazyLock` datasets can be resident simultaneously in the test binary  [MINOR]
**Where:** `src/randomx/tests.rs:30-40` (`test_key_000_dataset`, key
`test key 000`) and `src/randomx/tests.rs` `native_loop_diff_tests::test_dataset`
(key `native loop test key`).
**Claim:** These are two different keys, so they are two different 2 GiB
allocations, both `static LazyLock` and therefore never freed for the life of
the test process. With the default parallel test harness both can be live at
once, plus two 256 MiB Argon2d caches — ~4.5 GiB peak. Separately,
`native_loop_zero_iterations_terminates` calls `test_dataset()` purely to obtain
a pointer that the emitted code never dereferences (0 iterations), forcing a
full 2 GiB build for nothing.
**Failure scenario:** OOM / heavy swapping on a 8-16 GiB machine running
`make test`. Not a correctness issue.
**Confidence:** HIGH.

### F5 — `mean_ci95` hardcodes t = 2.09 but the sample size is a CLI argument  [MINOR]
**Where:** `benches/nativeloop_ab.rs:70-77`
**Claim:** The comment says "2.09 covers df >= 19". With the defaults
(`pairs = 12`) n = 24 and df = 23, where t(0.975) = 2.069 — so 2.09 is correctly
conservative. But `pairs` is `pos.get(1)`, so a user passing `3` gets n = 6,
df = 5, t(0.975) = 2.571, and the reported CI is ~19% too narrow. There is no
guard or warning.
**Failure scenario:** An under-wide CI leading to an over-confident verdict from
a short run. (My own run above hit exactly this: n=6.)
**Confidence:** HIGH.

### F6 — 11-thread phase: threads are not synchronised, so "round i is concurrent across threads" is only approximately true  [QUESTION]
**Where:** `benches/nativeloop_ab.rs:200-215`
**Claim:** Each thread runs its own A-B-B-A schedule independently; nothing
barriers them. The aggregation comment asserts "round i of thread 0 is
concurrent with round i of every other thread", which holds only while rounds
take equal wall time — i.e. it assumes the thing being measured. Once the arms
differ in speed the threads drift out of phase, so each arm's rounds partly
overlap the other arm's rounds on sibling threads.
**Failure scenario:** The mixing dilutes a real effect rather than manufacturing
one (each arm sees a blend of both arms' memory pressure), so it does not
threaten the *direction* of the +9.01% result — but it does mean the aggregate
CI of +-0.31% is narrower than the design's independence assumption warrants.
AUDIT.md already caveats the aggregate CI; this is the mechanism behind that
caveat.
**Confidence:** MEDIUM — I read the code but cannot quantify the phase drift
without a working harness (see F1).

### F7 — 8 redundant FMOVs per iteration remain in the f-load path  [MINOR / opportunity, not a defect]
**Where:** `src/randomx/jit/compiler.rs:908-916`
**Claim:** Review round 4 removed the d25/d26 round-trip for the **e**
registers by writing the masked value straight to the destination. The **f**
path still does `fmov d0, d25` / `fmov d1, d26` per lane — 8 FMOVs per
iteration, 131,072 per hash, the exact quantity the round-4 note claims to have
eliminated. Also, `add x0, x27, #imm` followed by `add x1, x16, x0` inside
`emit_cvt_packed_int` is two instructions where the base `x16 + sp_addr1` could
be formed once per iteration (as the r-load path already does with x2).
Confirmed in the disassembly I produced.
**Failure scenario:** None — pure performance.
**Confidence:** HIGH.

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

**VC12 — CBRANCH targets can never branch into the loop prologue.**
This is the one thing that would turn a latent body-JIT wart into a native-loop
hang (word 0 is now the `stp` prologue, and re-entering it pushes 160 bytes per
pass). It cannot happen: `compile_program` (vm.rs:565-567) resets
`register_usage` to **-1**, and `ibc.target = register_usage[creg]` is read
*before* the "mark all registers used" loop sets them to `i`, so `target` is
always in `[-1, i-1]`. `emit_cbranch` uses `offsets[target + 1]`, i.e. index
`[0, i]`, and `emit_body` writes `offsets[i] = e.len()` **before** emitting
instruction `i` — so every index it can reach is already populated. The
`target = -1` case resolves to `offsets[0]`, the first body word, which is also
where the interpreter resumes (`pc = -1; pc += 1`). Semantics match, and the
native loop correctly does *not* re-run `emit_iteration_pre` on such a branch.

**VC13 — the emitted blob was disassembled end to end and is correct.**
I re-implemented the emit path in a standalone binary (including `aarch64.rs`
verbatim and the four scaffolding functions extracted from `compiler.rs`),
emitted a full prologue / CBZ / iteration-pre / iteration-post / subs / b.ne /
epilogue with `readReg = {1,3,5,7}`, `dataset_offset = 524287*64`, and
disassembled it with `as -arch arm64` + `otool -tV`. Everything checks:
- `mov x22,x2; mov x28,x3; mov x23,x4; mov x21,x0; mov x16,x1` — x2/x3 captured
  first, before anything that could clobber them (CFROUND scratch).
- `mov x0,#0xffc0; movk x0,#0x1ff,lsl#16; add x22,x22,x0` — dataset_offset
  (0x01FFFFC0 = 33,554,368) folded into the base (C6).
- 8 r loads off x21, 8 a loads at +0xc0..0xf8, **no f/e loads** (correct: both
  are reassigned at the loop head).
- `cbz x28, 0x35c` lands exactly on the first epilogue word; `b.ne 0xbc` lands
  exactly on the loop head. Both patched offsets are right.
- iteration body exactly as in VC4, including `eor w25,w25,w1` (mx) *before* the
  eight dataset `ldr`/`eor`, and `stp d0,d1,[x0]` .. `stp d6,d7,[x0,#0x30]` for
  the stride-16 f stores.
- epilogue stores x24/x25/x26/x27 to [x23,#0/8/16/24], then r, f, e to
  x21 — **and only then** pops d14/d15..d8/d9, so the e-registers written to
  `nreg` are the loop's values, not the caller's restored ones.
- 10 pushes, 10 pops, exact reverse order, `ret`.

**VC11 — no concurrency hazard.** `calculate_hash_pipelined` is fully
sequential: `execute_vm` for all 8 chains, then `hash_and_fill_aes_1rx4`. The
"simultaneously" in the doc comment is instruction-level overlap by the CPU, not
threads, so nothing else writes the scratchpad while the emitted loop holds a
raw pointer to it.

## Test adequacy (priority 4)

**Executed locally** (this is the mandatory gate — CI is x86_64 and cannot run
any of it):
```
cargo test --release native_loop -- --nocapture --test-threads=1
  native_loop_emitted_instruction_accounting        ok
  test_native_loop_known_answer                     ok
  test_native_loop_known_answer_pipelined           ok
  native_loop_matches_interpreter                   ok
  native_loop_matches_interpreter_full_program      ok
  native_loop_zero_iterations_terminates            ok
  6 passed; 0 failed; finished in 89.83s
cargo clippy --all-targets -- -D warnings                          clean
cargo clippy --all-targets --target x86_64-apple-darwin -- -D warnings   clean
```
The accounting test's own output confirms the AUDIT's figures exactly:
prologue 48 + epilogue 35 = 83 words eliminated per iteration; pre 111 + post 55
+ 2 = 168 added.

**What the tests genuinely gate:**
- The differential test compares against the *real* `execute_vm_inner` (via
  `execute_vm_for_test`), not a re-implementation, so it cannot share a bug with
  the code under test at the loop-driver level.
- It compares the full register file (as raw bits, not floats), the entire 2 MiB
  scratchpad, the **full u64** loop state, and the final FPCR. Comparing the
  full u64 is what makes a C5 violation (64-bit EOR on ma/mx) detectable; the
  comment saying so is accurate.
- Both paths are reset to rounding mode 0 first, which is necessary and easy to
  get wrong.
- N=2 and N=3 close the D2 blind spot; N=2048 for one seed exercises the steady
  state and CBRANCH-heavy behaviour.
- The two known-answer tests are the real anchor: 8 chains x 2048 iterations
  against the canonical `639183aa...` vector, on **both** `calculate_hash` and
  `calculate_hash_pipelined`. This is the only test that exercises FPCR carry
  across chains and the serialize->blake2b->next-program plumbing.
- `test_vm_calculate_hash_jit` now explicitly sets `set_native_loop(false)`, so
  it remains a genuine control on the body-JIT path rather than silently
  becoming a duplicate of the native-loop test.

**What would still pass (defect classes not gated):**
1. **Any prefetch error** — wrong register, missing mask, pre- vs post-swap.
   `PRFM` changes no architectural state. Correct by construction and by my
   disassembly reading only. (Documented in design §6a; I confirmed the emitted
   addresses match `vm.rs:1339` and `:1380-1391`.)
2. **A bug in `derive_program_params` itself.** The differential test calls it
   for the JIT side and `execute_vm_inner` calls the same function for the
   reference side, so a bug there is invisible to it by construction. It *is*
   caught by the known-answer tests (both paths would produce a wrong hash), so
   this is covered — but not by the test that appears to cover it.
3. **Program-space-specific defects.** After F1, the differential + known-answer
   tests cover roughly 13 distinct programs. `readReg0..3` coverage is 4 of 16
   combinations from the differential test plus whatever the 8 known-answer
   chains happen to hit. Seed 78 puts `dataset_offset` at 99.67% of its maximum
   but nothing tests the exact maximum, and nothing tests `ma` at exactly
   `0x7FFFFFC0` (the two together are the C1 worst case).
4. **v2 / light-mode misrouting** is asserted, not tested — no test asserts that
   a V2 or light-mode VM with `set_native_loop(true)` stays off the native path.
   The dispatch guard in `execute_vm_inner` (`version == RxVersion::V1` and
   `dataset.is_some()`) is correct by reading, so this is a low-value gap.
5. **Anything shared by both prologues** — `NativeRegisterFile` offsets,
   `FSCAL_MASK`, `DYNAMIC_MANTISSA_MASK`, and every body emitter. Only the
   known-answer vectors gate those. (Correctly stated in design §6a.)
6. **Concurrency.** Nothing tests multiple threads each driving their own
   native-loop JIT. `pthread_jit_write_protect_np` is per-thread and each VM owns
   its own MAP_JIT region, so I see no hazard, but it is untested here.

## Remaining work if this review is interrupted
- None. All areas in the coverage ledger are DONE.
- If someone wants to go further, the two highest-value additions would be
  (a) fixing F1 and re-running the harness to re-establish the +9.01% claim and
  the broad-program-space correctness evidence, and (b) a differential case
  pinned at the C1 worst case (`dataset_offset` == `DATASET_EXTRA_ITEMS*64` and
  `ma` == `0x7FFFFFC0`), which is currently only argued, never executed.

## Verdict
- **Blockers: none.** I disassembled the full emitted loop and walked every
  emitted instruction against `execute_vm_inner`. The D1 `mx`-before-dataset-XOR
  ordering, the f load-stride-8 / store-stride-16 asymmetry, the W-form u32
  updates (C5), the masked post-swap dataset prefetch, the C1 dataset bound, the
  scratchpad masking, the AAPCS64 save/restore pairing, the FPCR non-restoration
  and its outer containment (C3), and the `CompiledKind` ABI guard are all
  present and correct. All six native-loop tests pass in release; clippy is clean
  on both aarch64 and x86_64.
- **Major: F1** — the A/B benchmark compares the native loop against itself, so
  the +9.01% claim and the "~147,000 hashes verified identical" correctness
  evidence cannot be reproduced from this tree.
- **Minors: F2-F5, F7. Question: F6.**


---

# Round 6 — delta review (d49535a..HEAD)
Scope: `1d25c0b` (F1/F3/F5 fixes + corrected numbers) and `bbecd15` (runtime
fallback switch). Nothing from rounds 1-5 is re-reviewed.

## Round 6 coverage ledger
| Area | File(s) | Status | Notes |
|---|---|---|---|
| F1 fix — is it real? | benches/nativeloop_ab.rs | DONE | R6-VC3: proved empirically by instrumented counters |
| F3 fix — CBZ range assert | src/randomx/jit/compiler.rs | DONE | R6-F1: bound is 2x too loose |
| F5 fix — t-table | benches/nativeloop_ab.rs | DONE | R6-F2: buckets anti-conservative |
| Measurement trustworthiness | AUDIT.md, bench | DONE | Independently replicated; R6-F4, R6-VC8/9/10 |
| Fallback switch — reach | src/miner.rs, vm.rs | DONE | R6-VC1: reinit claim verified |
| Fallback switch — precedence | src/bin/minertim.rs | DONE | R6-VC6; R6-F3; R6-Q1 |
| Makefile / mining.conf wiring | Makefile, mining.conf.example | DONE | R6-VC2 |

## Round 6 findings

### R6-F1 — The new CBZ range assert is 2x too permissive; imm19 is signed  [MINOR]
**Where:** `src/randomx/jit/compiler.rs:822-828`
**Claim:** The fix for my Round-5 F3 asserts `skip < (1 << 19)`, and its comment
says this is the "same imm19 range the back-branch below is checked against".
It is not the same range. The back-branch asserts
`(-(1 << 18)..(1 << 18)).contains(&rel)`, which is the correct signed-imm19
range. CBZ's imm19 is likewise a **signed** 19-bit word offset, so a forward
branch is only representable up to `2^18 - 1`. Any `skip` in
`[2^18, 2^19)` passes the new assert, survives `& 0x7FFFF` unchanged, and then
sign-extends to a **negative** offset — the emitted CBZ would branch backwards
into the middle of the loop instead of forwards to the epilogue.
**Evidence:** The back-branch assert three lines above uses `1 << 18`; ARM ARM
CBZ encoding is `imm19` sign-extended and shifted left 2. Confirmed by
assembling: `cbz x28, .+16` -> `b400009c`, i.e. imm19 = 4 in bits [23:5], with
bit 23 as the sign bit.
**Failure scenario:** A native-loop body exceeding 262,144 emitted words would
silently emit a backwards CBZ — an infinite loop or a jump into the middle of an
instruction sequence, on the zero-iteration path only. Unreachable today: the
full blob is ~1.2k words (I measured 254 with an empty body, and
`native_loop_emitted_instruction_accounting` reports pre 111 + post 55 + a
256-instruction body).
**Confidence:** HIGH on the encoding and the off-by-2x; HIGH that it is
unreachable in practice. Worth correcting because the assert was added
*specifically* to pin this bound, and its comment asserts an equivalence that
does not hold.

### R6-F2 — The new t-table buckets pick the *largest* df in each range, so the CI is anti-conservative — including at the default n=24 used for the headline  [MINOR]
**Where:** `benches/nativeloop_ab.rs:74-85`
**Claim:** The exact per-df values for df = 1..19 are all correct (I checked
each against a standard two-sided t(0.975) table). The three bucketed arms are
not: `20..=29 => 2.045`, `30..=59 => 2.001`, `_ => 1.96` each use the value for
the *highest* df in the bucket, so every df below the top of a bucket gets a
t-value that is too small and therefore a CI that is too narrow.
**Evidence:** t(0.975, 20) = 2.086 but the table returns 2.045; t(0.975, 30) =
2.042 but the table returns 2.001; t(0.975, 60) = 2.000 but the table returns
1.96. The default run is `pairs = 12` -> n = 24 -> **df = 23**, where the true
value is 2.069 and the table gives 2.045 — so the reported interval is ~1.2%
narrower than a correct t-interval. Using the *lowest* df in each bucket (2.086
/ 2.042 / 2.000) would make it conservative in the intended direction.
**Failure scenario:** Reported CIs are slightly optimistic. At the actual
effect size this changes nothing — the headline +6.76% CI (+6.20%..+7.32%)
would become roughly (+6.19%..+7.33%) and still excludes 0 by an order of
magnitude — so it does not threaten the claim. Recording it because the whole
point of this fix was to stop the CI being understated, and it still is, by a
smaller amount.
**Confidence:** HIGH.

## Round 6 verified-correct

**R6-VC1 — `reinit` really does preserve the flag; the coordinator's claim
holds.** `RandomXVm::reinit` (vm.rs:1701-1706) assigns exactly three fields —
`cache_memory`, `ss_programs`, `dataset`. It does not touch `use_native_loop`,
and it is not a `*self = ...` reassignment (which is the shape that would
silently reset it). So a worker that calls `set_native_loop` once at first
construction (miner.rs:393-396) keeps the setting across every subsequent seed
rotation. I grepped every `RandomXVm::new*` call site in `src/`: the only other
one outside tests is `miner.rs:520`, which builds a light VM purely to obtain
`cache_and_programs()` for dataset generation and never executes a hash, so it
needs no flag. There is no path by which a worker gets an unflagged executing VM.

**R6-VC2 — Makefile / mining.conf wiring is correct.** `-include mining.conf` is
the Makefile's first line, so a `NATIVE_LOOP=off` in `mining.conf` is already
set when `NATIVE_LOOP ?=` runs and the `?=` correctly does not clobber it;
`$(if $(NATIVE_LOOP),--native-loop $(NATIVE_LOOP),)` then forwards it. The
example file ships `NATIVE_LOOP=` (empty), which yields no flag and therefore
the built-in default — the same convention already used for `THREADS` and
`DONATE_LEVEL`. `make run NATIVE_LOOP=off` also works (command-line variables
outrank `-include`).

**R6-VC3 — F1 is genuinely fixed. Proved empirically, not by reading.**
I did not trust the presence of `base_vm.set_native_loop(false)`. I exported the
tree at HEAD to a scratch directory (`git archive HEAD | tar -x`), instrumented
**only** `execute_vm_inner` with two atomic counters — one incremented at the
`compile_native_loop` dispatch, one at the body-JIT `jit.compile` dispatch — and
added a purely additive print of the counter deltas around the two warm-up
rounds in `ab_phase`. Neither arm's flag handling was touched. Running the
result (`nativeloop_ab 1 1 8`, i.e. 8 hashes = 8 chains x 8 hashes = 64 chain
executions per arm):

```
PROBE BASE arm (set_native_loop(false)): native_loop_chains=0  body_jit_chains=64
PROBE NAT  arm (set_native_loop(true)):  native_loop_chains=64 body_jit_chains=0
```

Exactly 64 each, perfectly disjoint. The two arms genuinely execute different
emitted code at runtime. The Round-5 F1 defect is closed, not papered over.
(The repo itself was never modified; the instrumented copy lives only in the
scratch directory.)

**R6-VC4 — the per-round hash-equality assertion pairs the correct rounds, and
is now load-bearing for the first time.** Execution order within a pair is
A(base) B(nat) C(nat) D(base); `base_rates` receives `[h/ta, h/td]` and
`nat_rates` receives `[h/tb, h/tc]`, so `report` zips (A,B) and (D,C) — each
pair adjacent in time, mirror-ordered, which is what makes A-B-B-A cancel linear
drift. The checksums line up the same way: both arms start from nonce 0, take an
identical warm-up of `hashes.min(32)`, and thereafter advance by `hashes` per
round, so within pair k the base arm's round A and the nat arm's round B cover
the *same* nonce range, as do D and C. `assert_eq!((ca, cc), (cb, cd))` therefore
asserts A==B and D==C — the correct comparisons. In Round 5 this was vacuous
because both arms ran the same code; my probe run above executed it with the
arms genuinely on different paths and it passed (32 hashes), so the assertion is
now doing real work.

**R6-VC5 — the F5 t-table fix demonstrably changes behaviour in the right
direction.** My probe run produced n=2 (df=1), where the new table returns
t=12.706 and the harness correctly reported
`+66.75% (95% CI -1324.25% .. +1457.75%)` -> "NO MEASURABLE DIFFERENCE". Under
the old hardcoded 2.09 the same data would have reported a CI of roughly
+/-218%, still including zero here, but the mechanism is visibly live. See R6-F2
for the residual inaccuracy.

**R6-VC6 — the precedence logic does what the AUDIT claims.** I traced
`parse_native_loop` for every combination:
| input | result | correct? |
|---|---|---|
| nothing | on | yes (default) |
| `MINERTIM_NATIVE_LOOP=0` | off | yes |
| `--native-loop off` / `--native-loop=off` | off | yes |
| env `0` + `--native-loop on` | on | yes — flag beats env |
| env `0` + `--native-loop garbage` | off | yes — a typo does not silently re-enable |
| `--native-loop off --native-loop on` | on | yes — last flag wins |
| `--native-loop=on --native-loop=off` | off | yes |
| `MINERTIM_NATIVE_LOOP=""` | on + warning | acceptable; and not reachable from the shipped `mining.conf.example`, because `NATIVE_LOOP=` there is a *Makefile* variable that `$(if ...)` suppresses rather than an env var |
Shapes A, C, D and E of that table were additionally executed against the real
release binary (see R6-F3 for the transcript) and behaved exactly as traced.
All eight spellings (`on/off/true/false/yes/no/1/0`) are accepted
case-insensitively and are covered by the four new unit tests. The
space-separated form correctly advances `i` twice so the value token is not
re-scanned. `threads` parsing is unaffected: `args.get(3).filter(|s|
!s.starts_with('-'))` rejects `--native-loop` in the threads slot.

### R6-F3 — `--native-loop` with no value is silently ignored: the one input shape that leaves the native loop ON with no diagnostic at all  [MINOR]
**Where:** `src/bin/minertim.rs:222-227`
```rust
} else if args[i] == "--native-loop" {
    if let Some(v) = args.get(i + 1) {
        value = as_bool(v).or(value);
    }
    i += 1;
}
```
**Claim:** When `--native-loop` is the final argument, `args.get(i + 1)` is
`None` and the branch does nothing — no warning, no error, no change. The switch
resolves to the default (**on**). Every other malformed input at least prints
`warning: unrecognised native-loop value ...`; this one is completely silent.
**Failure scenario:** Two realistic shapes.
1. An operator who assumes it is a boolean flag types
   `minertim pool wallet 11 --native-loop` intending "turn it off" (or, more
   plausibly, copies a wrapper line and drops the value).
2. A wrapper script writes `--native-loop $NL` with `$NL` unset and unquoted —
   the shell removes the token entirely and leaves a bare `--native-loop`.
In both cases the miner starts with the native loop **enabled**, the
"Native-loop JIT DISABLED" warn line is absent, and there is no other signal.
This is precisely the "enabled while believing it is off" case, and it happens
during an incident when the operator is trying to stop losing shares.
Detectable by a careful reader (the DISABLED warning is missing), but the code's
own stated policy — warn on anything unrecognised — is not applied here.
**Confirmed empirically** against the real release binary (all five shapes,
pointed at a dead pool so it fails right after the switch is resolved):
```
A  --native-loop off        -> WARN "Native-loop JIT DISABLED ..."        (off, correct)
B  --native-loop            -> NO output of any kind                      (ON, silent)
C  --native-loop of         -> warning: unrecognised native-loop value "of"; ignoring   (ON, warned)
D  MINERTIM_NATIVE_LOOP=0   -> WARN "Native-loop JIT DISABLED ..."        (off, correct)
E  env=0 + --native-loop on -> no warning                                 (ON, flag beat env)
```
Shape **B** produced not one line — no "unrecognised" warning and no "DISABLED"
warning — which is the finding.
**Confidence:** HIGH. Read directly (the `if let Some` has no `else`) and
reproduced on the shipped binary.

### R6-Q1 — Judgement: an unrecognised value should fail *safe*, not fail to the default  [QUESTION — pushback requested]
**Where:** `src/bin/minertim.rs:200-211`, and the AUDIT's stated rationale.
You asked me to push back if I disagree. I partly do — not on "don't be fatal",
which I think is right, but on *which* non-fatal outcome.

The two failure modes are asymmetric in a way the current choice gets backwards:
- **Refusing to boot** on a typo is loud, immediate, and fixed in five seconds.
- **Silently resolving to ON** is quiet, and it continues the exact behaviour the
  operator was trying to stop. The comment in the code says this is "the switch
  someone reaches for while shares are being rejected" — so at the moment the
  parse fails, we already know the operator intended to *change* the setting.
  Resolving to the default is the one outcome we can be confident they did not
  want, and its cost is continued rejected shares, i.e. money, for as long as it
  takes them to notice.

A third option gets both properties: **an unparseable `--native-loop` value
resolves to `false`.** It still boots (your requirement), it costs at most ~6%
hashrate if the operator actually meant "on", and it never leaves a suspected-bad
JIT running when someone was trying to disable it. The one surprising case —
`--native-loop tru` turning it off when the operator meant on — fails in the
harmless direction.

Note the code already implements the *best* version of this for one case: with
`MINERTIM_NATIVE_LOOP=0 --native-loop garbage` the `.or(value)` chain correctly
keeps the env's `off`. It is only the "nothing previously resolved" case that
falls back to `on`.

I would not block a merge on this. It is a judgement call and the current
behaviour is warned about (except in the R6-F3 shape, which is the case that
makes me care).
**Confidence:** N/A — this is a design opinion, not a defect claim.

**R6-VC7 — the `warn`-level choice for the disabled message is right; endorsed.**
`log::warn!` rather than `info!` is correct here: the state is abnormal,
persistent, invisible in normal operation, and costs money. It also survives an
operator running `RUST_LOG=warn`, which `info!` would not. The message itself is
well-formed — it states the cost and how to undo it, which is the part most
such messages omit. One trivial arithmetic nit, not worth changing: "roughly 7%
lower hashrate" — if the native loop is +6.76% faster, disabling it costs
6.76/106.76 = 6.3% of the current rate, not 7%. "Roughly" covers it.

**R6-VC8 — I independently replicated the corrected measurement.** Ran the
*unmodified* repo bench at defaults (`cargo bench --bench nativeloop_ab`, 12
pairs x 256 hashes, threads = 11):

| Phase | | body JIT | native loop | paired diff | 95% CI |
|---|---|---|---|---|---|
| 1 thread | coordinator | 570.0 | 604.9 | +6.12% | +6.02%..+6.22% |
| 1 thread | **mine** | **570.2** | **606.9** | **+6.45%** | +5.83%..+7.08% |
| 11 threads | coordinator | 4756.1 | 5077.1 | +6.76% | +6.20%..+7.32% |
| 11 threads | **mine** | **5020.0** | **5392.3** | **+7.42%** | +7.14%..+7.70% |

Single-threaded the two runs agree to a startling degree — baselines 570.0 vs
570.2 H/s (0.03% apart) and native arms 604.9 vs 606.9 (0.3% apart). 24 of 24
paired differences positive in both phases of my run as well, so across the two
runs that is **96 of 96 positive**. The direction and the decision are solid.

**R6-VC9 — I independently reproduced the correctness evidence.** My run
executed the per-round `assert_eq!((ca, cc), (cb, cd))` for all 24 rounds x 256
hashes in both phases with the arms genuinely on different code paths (proved in
R6-VC3), and it passed throughout: ~12,288 hashes single-threaded plus ~135,168
across 11 threads. The AUDIT's "~147,000 hashes verified identical between two
genuinely different execution paths" is now confirmed by a second, independent
run on a separately built binary. This is the single most valuable thing the
corrected harness produces and it holds up.

**R6-VC10 — the A-B-B-A ordering has no position confound, and I checked the
subtle version of it.** The concatenated round sequence is
`A | BB | AA | BB | AA | ...`, so each arm gets exactly one "switched-in" round
and one "continuation" round per pair; neither arm systematically occupies the
cold slot. The pairing zips (position 1, position 2) and (position 4, position
3), which mismatches switched-in against continuation in *opposite* directions
for the two diffs, so the effect cancels once both are averaged — which `report`
does. The first round of all is preceded by the discarded warm-up round on the
same VM, so even position 1 is not anomalous.

### R6-F4 — The quoted 11-thread interval (+/-0.56%) is narrower than the run-to-run reproducibility of the same measurement, so it is not a valid interval for the published quantity  [MAJOR]
**Where:** AUDIT.md 2026-09-02 table, `DESIGN_JIT_NATIVE_LOOP.md` stage-D table,
`CLAUDE.md` JIT-01 row, `src/randomx/vm.rs:1639-1641`
**Claim:** The 11-thread claim is stated as **+6.76% (95% CI +6.20%..+7.32%)**.
My independent re-run of the identical harness on the identical machine gives
**+7.42% (95% CI +7.14%..+7.70%)**. The two intervals barely touch (overlap only
in +7.14%..+7.32%) and the point estimates differ by 0.66 percentage points —
more than either interval's half-width. So the reported CI is not describing the
uncertainty of the quantity being claimed; it is describing within-run round
scatter of an aggregate that is already smoothed by summing 11 threads.
**Evidence:** Both runs above, same binary source, same machine, same defaults,
~40 minutes apart. Note the 11-thread *absolute* rates also moved between runs
(baseline 4756.1 -> 5020.0, native 5077.1 -> 5392.3, both about +5-6%), i.e. a
level shift affecting both arms — the paired design correctly cancels most but
evidently not all of it.
**Failure scenario:** No technical failure and no wrong hashes — the direction
is settled by 96 of 96 positive paired differences across the two runs. The harm
is that the published interval does not describe the published quantity. Anyone
auditing the number later re-runs the harness, gets +7.4%, lands outside the
stated CI, and cannot tell whether they have found a regression or a
fabrication. **That is the same class of error the Round-5 retraction was
supposed to end**, one iteration later, in the same four user-visible places
(AUDIT table, DESIGN stage-D table, CLAUDE.md JIT-01 row, and the
`use_native_loop` doc comment) — which is why I am calling it MAJOR rather than a
documentation nit, despite the underlying decision being correct.
The defensible claim from two runs is **"roughly +6% to +7.5% at 11 threads"**,
or the single-threaded figure where reproducibility genuinely is excellent
(+6.12% vs +6.45%, baselines agreeing to 0.03%). If a tight interval is wanted,
it has to come from repeated *runs*, not repeated rounds within one run.
**Related:** this is the concrete instance of my Round-5 F6 (no barrier between
threads, so the aggregate's independence assumption is not enforced) and of the
AUDIT's own new lesson that "a tight CI is not evidence of a quiet machine". The
lesson was written about the retracted run; it applies to the replacement too.
**Confidence:** HIGH — two direct measurements.

**R6-VC11 — the 337.5 -> 570.0 baseline gap: the coordinator's "contended
machine" attribution is adequate, and there is a stronger argument for it than
the one recorded.** The AUDIT frames it as "a 69% difference in the *baseline
itself*". That framing understates the case, because under the F1 bug the old run
had **no baseline** — both arms were the native loop. The right comparison is
old-native 358.3 H/s vs new-native 604.9 (mine: 606.9), which is the *same* ~69%
gap. A level shift of equal size in **both** arms is the signature of machine
state; a configuration or code-path confound would generally move one arm and
not the other. Three independent single-thread measurements of native-loop
throughput exist: 358.3 (old), 606.6 (my Round-5 run, taken before the fix
existed and therefore not circular), 604.9 / 606.9 (new). Two quiet-machine
measurements agree to 0.3%; the outlier is the old one.
I looked for a second confound and did not find one: `KEY`, blob shape, dataset
build, warm-up and cooldown are identical across runs, and round length is not a
factor (my Round-5 run used 64-hash rounds and my run today used 256-hash rounds,
both giving ~606 H/s). One concrete mechanism consistent with the old run's level
is memory pressure from the test suite's two resident 2 GiB `LazyLock` datasets
(my Round-5 F4) if a test binary was alive at the time — unverifiable
retrospectively. **Adequate, but note the class of explanation is established,
not the mechanism.** The important structural point is that a uniform level shift
does not by itself invalidate a *paired* difference — what invalidated +9.01% was
the identical-arms bug, not the contention.

**One diagnostic worth recording for whoever runs this next:** my single-thread
per-pair differences were `+8.9 +6.1 +5.5 +9.9 +9.4 +5.2` for the first six
pairs and then settled to `+5.8..+6.7` for the remaining eighteen. The mean
(+6.45%) is pulled up by that early instability; the median is ~+6.2%. A longer
discarded warm-up, or reporting the median paired difference alongside the mean,
would make the single-thread phase more robust. The 11-thread phase showed no
such ramp (one tail outlier of +4.9% in the last pair).


---

# Round 7 — delta review (bbecd15..HEAD)
Scope: `faa4131` — verify-before-submit, plus the round-6 fixes. Nothing from
rounds 1-6 is re-reviewed except to confirm those fixes landed.

## Round 7 coverage ledger
| Area | File(s) | Status | Notes |
|---|---|---|---|
| P1 — verification soundness / off-by-one | src/miner.rs | DONE | R7-VC1 + R7-VC2: proved by reading AND empirically |
| P2 — false positives | src/miner.rs, src/randomx/vm.rs | DONE | R7-VC2/VC3/VC4 |
| P3 — counter reaches the operator | src/miner.rs, src/bin/minertim.rs | DONE | R7-VC5; wiring is sound |
| P4 — cost | src/miner.rs, src/randomx/vm.rs | DONE | R7-F1: cost mis-stated by ~2 orders of magnitude |
| Round-6 fixes landed | compiler.rs, bench, minertim.rs, docs | DONE | R7-VC6: all five verified, not trusted |
| Fault-injection test question | — | DONE | R7-Q1: yes, but not by injecting a JIT fault |

## Round 7 findings

### R7-F1 — The lazily-built verifier costs 256 MiB and ~0.4 s, not "the 2 MiB scratchpad"; at 11 threads it adds ~2.75 GiB of RSS that full mode never reads  [MAJOR]
**Where:** `src/miner.rs:395-397` (the comment), `src/miner.rs:545-549` (the
construction), `src/randomx/vm.rs` `new_full_versioned`
**Claim:** The code says
```rust
// Reference-path VM for share verification, built lazily on the first share
// so a worker that never finds one never pays the 2 MiB scratchpad.
```
but `RandomXVm::new_full` does far more than allocate a scratchpad. Its first
statement is `let cache_memory = argon2d_cache(key);` — a **256 MiB, 3-pass
Argon2d fill**. That buffer is only ever read by `init_dataset_item`, i.e. only
in *light* mode. A full-mode VM never touches it. So every verifier allocates and
computes a quarter-gigabyte of memory that is dead on arrival.
**Evidence:** measured directly (release build, this machine), timing
`RandomXVm::new` — which performs exactly the same `argon2d_cache` + 8
superscalar programs and skips only the 2 MiB scratchpad:
```
RandomXVm::new  #0: 0.432 s   cache_memory = 256 MiB
RandomXVm::new  #1: 0.374 s   cache_memory = 256 MiB
RandomXVm::new  #2: 0.372 s   cache_memory = 256 MiB
```
**Failure scenario:** two distinct costs, neither of them the stated one.
1. **Memory.** +256 MiB per worker, retained until the next seed rotation. At the
   default 11 threads that is **+2.75 GiB**. Note the mining VMs already each
   carry an equally-unused 256 MiB cache, so this *doubles* that dead weight to
   ~5.5 GiB, on top of the 2 GiB dataset. On a 16 GB Mac that is a plausible
   swap/OOM trigger, and it appears gradually (only once a worker finds its first
   share), so it looks like a leak rather than a startup cost.
2. **Latency, inline in the share path.** The ~0.4 s build happens *between*
   finding the share and submitting it, once per worker per seed rotation. On its
   own that is a small stale-share risk (0.4 s against a job lifetime of minutes,
   for ~`threads` shares per epoch), so I would not raise it alone — but it is
   latency deliberately inserted into the one code path the whole feature exists
   to protect.
**Why MAJOR and not MINOR:** the resource cost is real, it is ~100x the
documented figure, and the documented figure is what a reader will use to decide
whether the feature is cheap enough to leave on by default. Note also that the
`set_verify_shares` doc comment's "roughly 0.005% of mining time" describes only
the recurring hash, and is silent about the one-off build.
**Confidence:** HIGH — read the constructor and measured it.

## Round 7 verified-correct

**R7-VC1 — There is no off-by-one. `job_blob_current` really is the blob whose
hash was just returned.** The invariant, traced through `worker_loop`:
- `calculate_hash_pipelined(&job_blob_next)` returns the hash of whatever the
  scratchpad currently holds and *then* refills it from `job_blob_next`.
- On a priming iteration (`pipeline_ready == false`) the code calls
  `prepare_scratchpad(&job_blob_current)` first, so the scratchpad holds nonce N
  and the returned hash is H(job_blob_current @ N). The nonce was written into
  `job_blob_current` immediately above, so they agree.
- On every later iteration the scratchpad was filled from the *previous*
  iteration's `job_blob_next`, which carried nonce N+tc; and this iteration wrote
  N+tc into `job_blob_current` before hashing. They agree again.
- Both reset points are covered: `pipeline_ready = false` is set on a **job_id
  change** (miner.rs:411) *and* on a **seed change** (miner.rs:439), and in both
  cases the blobs are re-copied from `job.blob` before the next hash. So a new
  job cannot leave the scratchpad primed from the old blob.
`nonce_hex` is read from `job_blob_current[39..43]`, i.e. the same buffer, so the
submitted nonce, the submitted hash and the verified blob are all consistent.

**R7-VC2 — Confirmed empirically, including across a job change and with the two
VMs interleaved on one thread.** I built a scratch harness (outside the repo,
using only the public API) that replicates `worker_loop`'s exact pattern — two
blob buffers, `write_nonce_le` into both, prime once, then
`calculate_hash_pipelined(&next)` — with a native-loop mining VM and a
`set_native_loop(false)` verifier, calling `verify.calculate_hash(&cur)` between
consecutive pipelined calls exactly as the share path does, and re-priming
mid-run to simulate a job change:
```
checked=24 mismatches=0
PASS: pipelined hash == calculate_hash(job_blob_current), interleaved, across a job change
```
This is the strongest available evidence for priority 1 and it also settles the
FP-state half of priority 2: had the interleaving perturbed anything, these 24
comparisons are precisely where it would show.

**R7-VC3 — FPCR interleaving cannot cause a false positive.** Both
`calculate_hash` and `calculate_hash_pipelined` are self-contained with respect
to the rounding mode: each opens with `save_rounding_mode()` +
`set_rounding_mode(0)` and closes with `restore_rounding_mode(saved_rm)`. So the
verifier's call restores whatever FPCR it found, and the mining VM's next
pipelined call re-establishes mode 0 for itself regardless. The two VMs share no
mutable state at all — separate scratchpads, separate `pipeline_state`, separate
`JitCompiler`/MAP_JIT regions; the only shared object is the
`Arc<RandomXDataset>`, which is read-only.

**R7-VC4 — The verifier can never be keyed to a stale seed.** In the seed-change
block the three assignments happen together and *before* any hashing for the new
seed: `verify_vm = None;` (miner.rs:436) discards the old verifier,
`verify_dataset = Some(dataset);` records the new dataset, and
`current_key = job.seed_hash.clone();` updates the key. The lazy
`get_or_insert_with` closure then reads `current_key`, which by construction is
the seed the mining VM was just re-initialised to. The `None => true` arm for a
missing `verify_dataset` is genuinely unreachable (the dataset is recorded in the
same block that creates or re-inits the VM, and that block runs before the first
hash because `vm.is_none()` forces it), and choosing to **submit** rather than
withhold on that impossible path is the right call — it fails toward not
discarding real revenue.

### R7-F2 — The new fail-safe policy is correct for `--native-loop` but **inverted** for `--verify-shares`: malformed input silently disables the safety net  [MINOR]
**Where:** `src/bin/minertim.rs` `parse_switch`, called as
`parse_switch(&args, "--verify-shares", "MINERTIM_VERIFY_SHARES", true)`
**Claim:** You generalised my R6-Q1 pushback into `parse_switch` and applied it
to both switches, with the doc asserting "for both switches here `false` is the
conservative direction". That is true for `--native-loop` (off = slower but
provably correct) and false for `--verify-shares` (off = **remove the check**).
For a safety net the conservative resolution of an unparseable value is *on*.
**Evidence:** `as_bool` now returns `Some(false)` for anything unrecognised, and
the no-value arm sets `Some(false)`. Three reachable shapes therefore disable
verification:
- `--verify-shares maybe` (typo)
- `--verify-shares` with no value
- **`MINERTIM_VERIFY_SHARES=` set but empty** — `std::env::var` returns `Ok("")`,
  `as_bool("")` hits the `_` arm. This needs no typo at all: a launchd/systemd
  unit or wrapper doing `MINERTIM_VERIFY_SHARES="$SOMETHING_UNSET"` reaches it.
**Failure scenario:** the operator believes shares are being verified; they are
not. The consequence is bounded — it removes defence-in-depth rather than
producing wrong hashes — and it is *not* silent: `parse_switch` prints a
`warning:` line and `main` additionally emits
`log::warn!("Share verification DISABLED while the native-loop JIT is on...")`.
Those two signals are why I am calling this MINOR rather than MAJOR. But it is
structurally the same "believes it is on while it is off" shape as R6-F3, now
reintroduced on the new switch by the fix for R6-F3.
**Suggested direction (not a fix):** make the fail-safe target a parameter —
`--native-loop` fails to `false`, `--verify-shares` fails to `true`. The rule is
not "fail to false", it is "fail to whichever value cannot lose money".
**Confidence:** HIGH on the behaviour (read, and it follows directly from
`as_bool` never returning `None`).

### R7-F3 — `parse_native_loop`'s doc comment has been re-parented onto `parse_switch`, leaving one function undocumented and the other documented as something it is not  [MINOR]
**Where:** `src/bin/minertim.rs:223-247`
**Claim:** The edit merged two doc blocks. The comment now attached to
`parse_switch` opens with *"Resolve the native-loop switch: `--native-loop <v>` /
`--native-loop=<v>` beats `MINERTIM_NATIVE_LOOP`..."* and runs for fifteen lines
about the native-loop switch specifically before the intended
`parse_switch` text begins mid-block at *"Resolve an `--flag on|off` switch..."*.
`parse_native_loop` itself now has **no doc comment at all**. The block also
contains a rustdoc intra-doc link `[`parse_native_loop`]` inside what is now
`parse_switch`'s own documentation, i.e. it points at a sibling from a doc that
claims to describe that sibling.
**Evidence:** read from the file at `src/bin/minertim.rs:223-247`, not from the
diff — the two `///` runs are contiguous with no blank line or item between them.
**Failure scenario:** documentation only; `cargo doc` would render the
native-loop policy as `parse_switch`'s contract. No runtime effect.
**Confidence:** HIGH.

### R7-F4 — `parse_native_loop` is now behaviourally identical to `parse_switch` and should not exist twice  [MINOR]
**Where:** `src/bin/minertim.rs` — `parse_switch` and `parse_native_loop`
**Claim:** After this commit the two functions implement the same algorithm:
same eight accepted spellings, same `=`-form and space-form handling, same
last-flag-wins, same `Some(false)` on garbage, same `Some(false)` on a missing
value, same env fallback, same `unwrap_or(default)`. `parse_native_loop(args)` is
exactly `parse_switch(args, "--native-loop", "MINERTIM_NATIVE_LOOP", true)`. The
only differences are the wording of two `eprintln!` strings.
**Failure scenario:** no current bug; the risk is divergence. Two copies of a
security-relevant parser drift, and the next fix will land in one of them. Note
this is also what let R7-F2 through: the policy was centralised into
`parse_switch` without noticing that "fail to false" is switch-specific.
**Confidence:** HIGH — compared the two bodies line by line.

### R7-Q1 — Yes, a test is warranted for the mismatch branch, but not by injecting a JIT fault  [QUESTION / recommendation]
You flagged that the mismatch branch has never executed. I think the more
important gap is one step earlier: **nothing tests that the verification is
wired up at all.** If `verified` were refactored to be unconditionally `true`,
every existing test would still pass, the feature would be a silent no-op, and
the only symptom would be the absence of a symptom.
Injecting a JIT fault is the hard way to close that. Two cheaper options:
1. **Extract the decision.** Pull the compare-and-log out of the 200-line
   `worker_loop` into something like
   `fn verify_share(reference: &[u8; 32], mined: &[u8; 32], stats: &MiningStats, ...) -> bool`
   and unit-test both branches directly. That covers the inverted-comparison and
   counter-not-incremented failure modes, needs no dataset, and runs in
   microseconds.
2. **Inject at the input, not the JIT.** A test can feed the verifier a blob one
   nonce off and assert the share is withheld and `verify_failures` incremented.
   This exercises the real branch end to end without needing a broken JIT.
Either is worth having before merge; option 1 is the one I would do, because it
also creates the seam that makes option 2 trivial.
**Note on what this check can and cannot catch** — worth stating in AUDIT.md,
because the current framing ("a JIT defect") is broader than the mechanism:
the reference VM runs `set_native_loop(false)`, which is still **JIT-emitted
ARM64 sharing the same `emit_body`**. So the check detects divergence between
the native-loop scaffolding and the body-JIT path — exactly the class the
differential tests cover — and is **blind to any defect in a shared body
emitter**, which would produce identical wrong hashes on both sides. A fully
independent reference would be the interpreter. That is not an argument against
the feature; it is an argument for describing it as "native-loop scaffolding
verification" rather than "JIT verification".

**R7-VC5 — the verify-failure counter really does reach the operator.** Traced
end to end: `MiningStats.verify_failures` is incremented with
`fetch_add(1, Relaxed)` in the mismatch arm; the `Arc<MiningStats>` handed to
each worker is the same one stored as `self.stats = Some(stats)`
(`miner.rs:263`), so `Miner::get_verify_failures()` reads the workers' counter
rather than a detached copy; and the 10-second stats loop in `main` reads it
every tick and emits `log::error!` whenever it is non-zero. Three independent
signals would fire on a real mismatch: the per-share `log::error!` in the worker
(with both hashes, job_id and nonce — genuinely useful for a bug report), the
recurring stats-loop error every 10 s, and the share simply never being
submitted. Two small inaccuracies, neither worth changing:
- The comment says the counter is *"appended to the normal stats line"*; it is
  actually emitted as a separate `log::error!` line immediately before it.
- The `log::error!` repeats every 10 s forever once non-zero. For a persistent
  correctness fault I think that is right, and I would not change it.

**R7-VC6 — all five round-6 fixes landed, checked against the files rather than
the summary.**
- **R6-F1**: `compiler.rs:828` now reads `skip < (1 << 18)` with the message
  "CBZ zero-iteration guard offset out of **signed** imm19 range", and the
  comment above it explains that imm19 is sign-extended and shifted left 2.
  Correct, and it now genuinely matches the back-branch's `(-(1<<18)..(1<<18))`.
- **R6-F2**: `nativeloop_ab.rs:87` now reads
  `20..=29 => 2.086, 30..=59 => 2.042, _ => 2.000` — the *lowest* df in each
  bucket, i.e. conservative in the right direction. At the default n=24 (df=23)
  it now returns 2.086 against a true 2.069, so the interval is slightly wide
  rather than slightly narrow. Correct.
- **R6-F3**: verified on the shipped binary —
  `--native-loop` (bare) now prints
  `warning: --native-loop given with no value - assuming OFF (the safe
  direction); use on|off` followed by the DISABLED warn. The silent shape is
  gone.
- **R6-Q1**: `--native-loop maybe` now resolves to off, with a warning, and
  never aborts. Two new unit tests pin both shapes.
- **R6-F4**: the claim is a **range** in all four places I named —
  `DESIGN_JIT_NATIVE_LOOP.md:325` (a table showing both runs and the range),
  `CLAUDE.md:33`, `src/randomx/vm.rs:1639-1641`, and the AUDIT entry — each also
  stating that the per-run CIs do not describe reproducibility. AUDIT.md:1774
  carries the standing rule. The +9.01% retraction is preserved rather than
  erased. This is a better outcome than I asked for.

**R7-VC7 — cost arithmetic (P4), recurring component.** The claimed
"0.0008%-0.008% of mining time at difficulty 10k-100k" is the right order of
magnitude but ~25% optimistic. One verification hash per share, one share per
`D` hashes, gives overhead `= 1/D` at minimum: **0.010% at D=10,000 and 0.0010%
at D=100,000**. `calculate_hash` is unpipelined, so it costs somewhat more than
one mining hash (it re-fills the scratchpad rather than overlapping the fill),
pushing it to roughly 0.012% / 0.0012%. The `set_verify_shares` doc comment's
"roughly 0.005% of mining time" sits inside that band and is fine as an order of
magnitude. **None of this matters in absolute terms** — the recurring cost is
genuinely negligible, which is the point the claim is making. The cost that is
*not* negligible and is missing from every one of these statements is the one-off
verifier construction (R7-F1).

**R7-VC8 — the committed HEAD (`faa4131`) compiles and its CLI tests pass.**
I hit a compile error while running the suite (`parse_switch` not found in the
test module) and traced it to the *working tree*, not to the commit: `git status`
shows uncommitted modifications to `src/bin/minertim.rs`, `src/miner.rs` and
`src/randomx/jit/compiler.rs`, and I caught them mid-edit. Verified by exporting
the commit cleanly (`git archive faa4131 | tar -x`) into a scratch directory and
building there:
```
running 6 tests
test tests::native_loop_with_no_value_fails_safe_to_off ... ok
test tests::native_loop_defaults_on ... ok
test tests::native_loop_last_flag_wins ... ok
test tests::native_loop_unrecognised_value_fails_safe_to_off ... ok
test tests::native_loop_accepts_the_documented_spellings ... ok
test tests::verify_shares_defaults_on_and_shares_the_fail_safe_policy ... ok
test result: ok. 6 passed; 0 failed
```
**Not a finding against the reviewed commit.** Flagging it only so nobody later
reads a broken build into `faa4131`. All timing and behavioural results reported
in Round 7 were taken either from that clean export or from a release binary
built before the edits began.

**Note — R7-Q1 is already in flight.** The uncommitted work in the tree
introduces `ShareVerdict` (`SubmitUnverified` / `SubmitVerified` /
`SubmitVerifierUnavailable` / `Withhold`) and a `classify_share` function
extracted from `worker_loop`, plus an env-branch test for `parse_switch`. That is
option 1 from R7-Q1, and from the fragment I saw it also makes the
"verifier unavailable" case an explicit *fail-open* verdict rather than an
implicit `None => true`, which is the right call and better than what I
suggested. I have **not** reviewed it — it is outside the `bbecd15..HEAD` scope I
was given and it was changing under me. It should get its own round.

### R7-F5 — Two small documentation inaccuracies carried into user-visible text  [MINOR]
**Where:** AUDIT.md verify-before-submit entry; `DESIGN_JIT_NATIVE_LOOP.md:323`
1. AUDIT says the counter is *"surfaced in the periodic stats line"*. It is
   emitted as its own `log::error!` immediately before the stats line, not
   appended to it. Functionally better than described; just not what it says.
2. The design table's 1-thread row records run 2 as `—`. My independent run did
   produce a 1-thread figure: **+6.45%**, against run 1's +6.12%. That row could
   read `+6.12% | +6.45% | ~+6.1% to +6.5%` and would then be the *strongest*
   replication in the table (the two baselines agreed to 0.03%). Leaving it blank
   understates the evidence.
**Confidence:** HIGH.

### R7-F6 — The "deliberate limits" list omits the one that matters most: the reference path is not independent  [MINOR]
**Where:** AUDIT.md, verify-before-submit entry, "Deliberate limits" list
**Claim:** The list covers three limits but not the sharpest one. The reference
VM runs `set_native_loop(false)`, which is the **body JIT** — still emitted
ARM64, still going through the same `emit_body` and the same 28 instruction
emitters. The check therefore detects divergence between the native-loop
*scaffolding* and the body-JIT path, and is structurally blind to a defect in
any shared body emitter, which would yield identical wrong hashes on both sides
and sail through. The only fully independent reference is the interpreter.
**Failure scenario:** none directly; the risk is that the feature is trusted
more broadly than it can bear — e.g. someone concluding from a clean
`verify_failures` counter that "the JIT is verified", when what is verified is
one half of it.
**Confidence:** HIGH — this follows from `execute_vm_inner`'s dispatch: with
`use_native_loop == false` it falls through to `jit.compile(bytecode, version)`
and the same `emit_body`.
**Related trivia (not worth a finding):** the gate is
`if verify_shares && native_loop`, where `native_loop` is the *requested* flag,
not whether the VM actually took the native path. On a non-aarch64 build the
flag defaults to `true` but `RandomXVm` ignores it, so both VMs run the
interpreter and every share is verified against an identical computation — one
wasted hash per share, cost `1/D`. Harmless, and this is a macOS/Apple-Silicon
project, but it means the stated limit "skipped when the native loop is off"
does not cover "requested but ineffective".
