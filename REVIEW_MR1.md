# Review: MR !1 — JIT native iteration loop
Reviewer: independent agent | Started: 2026-09-01T13:42:20Z | Last updated: 2026-09-02T (round 6 in progress)


## Standing protocol — read this first if you are resuming cold

This file is the durable state for an ongoing independent review of MR !1. It is
written to continuously *because* review sessions have been ended twice by usage
limits mid-round. If you are picking this up with no conversation context,
everything you need is here.

**Rules that apply to every round:**
1. Write findings to this file **as you go** — after each finding, not at the
   end. Assume you can be killed at any moment.
2. Update the round's coverage ledger **before** starting each item and again
   when it is done, so an interruption leaves an accurate picture.
3. `git add REVIEW_MR1.md && git commit` periodically. **This file only.** Never
   commit anything else, never amend, never push.
4. Each round has a `## Round N brief` section stating scope and questions. If
   you are resuming, that brief is your instructions — the requester's message
   is not available to you.
5. Verify claims by reading and running, not by trusting commit messages or the
   requester's summary. Several rounds found defects in fixes whose commit
   messages described them as correct.
6. Do not fix anything. Review only.
7. Finish by setting the round Status, filling "remaining work", committing, and
   stating plainly whether the MR is mergeable.

**Resume procedure:** find the last `## Round N coverage ledger`, take the first
row not marked DONE, and continue from there.

## Status
COMPLETE — rounds 5-10 finished. Round 10 (`5fe7eb3..6f2b95b`): no blockers, one major (R10-F2, a regression introduced by this commit), one minor. **Mergeable, but land R10-F2's two-line fix first.**

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

## Round 7 verdict

**Blockers: none.** The verification is sound: no off-by-one (proved by reading
*and* empirically), no false-positive vector I could find or construct, the
counter reaches the operator, and the committed HEAD builds clean with clippy
clean on both targets.

**Major: R7-F1** — the verifier's true cost is 256 MiB + ~0.4 s per worker, not
"the 2 MiB scratchpad". At 11 threads that is +2.75 GiB of resident memory that
full mode never reads (it is the Argon2d cache, used only by light mode),
doubling the dead weight the mining VMs already carry.

**Minors:** R7-F2 (fail-safe policy inverted for `--verify-shares`), R7-F3
(doc block re-parented), R7-F4 (`parse_native_loop` duplicates `parse_switch`),
R7-F5 (two doc inaccuracies), R7-F6 (the reference path is not independent).
**Recommendation:** R7-Q1 (test the decision, not the JIT — already in flight in
your working tree).

## Answers to the three round-7 priority questions

**1. Is the verification sound / is there an off-by-one?** No off-by-one.
`calculate_hash_pipelined(&job_blob_next)` returns the hash of the previously
primed scratchpad, and `job_blob_current` is written with exactly that nonce
immediately before each call; `pipeline_ready` is reset on **both** a job_id
change and a seed change, with the blobs re-copied, so the priming iteration
always re-establishes the invariant. Proved by reading (R7-VC1) and then
empirically with a harness replicating `worker_loop`'s exact pattern including a
job change: **24 comparisons, 0 mismatches** (R7-VC2).

**2. Can it produce a false positive?** I could not construct one.
FP/rounding state is safe because both hash entry points save, zero and restore
FPCR around themselves (R7-VC3). The two VMs share no mutable state — only the
read-only `Arc<RandomXDataset>`. The verifier cannot be keyed to a stale seed:
`verify_vm = None`, `verify_dataset = Some(..)` and `current_key = ..` are
assigned together, before any hashing for the new seed (R7-VC4). The
`verify_dataset == None` arm is unreachable and correctly fails *open*.

**3. Does the counter reach the operator?** Yes — the worker increments the same
`Arc<MiningStats>` the `Miner` stores (`self.stats = Some(stats)`), and the
10-second loop logs an error whenever it is non-zero. Three independent signals
on a real mismatch: the per-share error with both hashes, the recurring stats
error, and the share not being submitted (R7-VC5).

**4. Cost.** The recurring component is right to an order of magnitude but ~25%
optimistic: it is `1/D`, i.e. 0.010% at D=10k and 0.0010% at D=100k, slightly
more because `calculate_hash` is unpipelined (R7-VC7). Negligible, as claimed.
The one-off construction cost is the one that is wrong, by ~100x — R7-F1.

**On the fault-injection question:** yes, worth adding, but the more important
untested property is that the verification is wired up *at all* — if `verified`
became unconditionally `true`, no existing test would notice. Extract the
decision and unit-test both branches; injecting a JIT fault is the hard way to
get there (R7-Q1).

**R7-VC10 — the six native-loop tests still pass in release on the committed
HEAD.** Run against the clean `git archive faa4131` export, single-threaded:
```
running 6 tests
test randomx::jit::compiler::tests::native_loop_emitted_instruction_accounting ... ok
test randomx::tests::full_hash_tests::test_native_loop_known_answer ... ok
test randomx::tests::full_hash_tests::test_native_loop_known_answer_pipelined ... ok
test randomx::tests::native_loop_diff_tests::native_loop_matches_interpreter ... ok
test randomx::tests::native_loop_diff_tests::native_loop_matches_interpreter_full_program ... ok
test randomx::tests::native_loop_diff_tests::native_loop_zero_iterations_terminates ... ok
test result: ok. 6 passed; 0 failed; finished in 114.20s
```
The round-6 JIT change (`1 << 19` -> `1 << 18`) is a `debug_assert!` and so is
inert in this profile, but the known-answer and differential gates confirm the
emitted loop is unchanged in behaviour.

**R7-VC9 — clippy is clean on the committed HEAD for both targets.** Run against
the clean `git archive faa4131` export, so unaffected by the working-tree edits:
`cargo clippy --all-targets -- -D warnings` and the same with
`--target x86_64-apple-darwin` both finish with no diagnostics. The six CLI
parser tests pass (R7-VC8).


## Remaining work if this review is interrupted
- **Round 7 is complete.** All four priority questions answered, all six
  round-7 findings written up, all five round-6 fixes verified against the
  files, and the committed HEAD independently built, linted and tested from a
  clean export.
- **Not reviewed, deliberately out of scope:** the uncommitted working-tree
  changes present while I was reviewing (`src/miner.rs` `ShareVerdict` /
  `classify_share`, `src/bin/minertim.rs` env-branch test,
  `src/randomx/jit/compiler.rs`). They implement R7-Q1 option 1 and appear to
  improve on it, but they were changing under me and are not in `faa4131`.
  **They need their own round**, particularly: whether `classify_share` preserves
  the fail-open behaviour on `SubmitVerifierUnavailable`, and whether the
  extracted call site still reaches the counter.
- **Highest-value follow-ups, in order:**
  1. R7-F1 — stop the verifier building a 256 MiB Argon2d cache it never reads.
     (A full-mode `RandomXVm` constructor that skips `argon2d_cache` would fix
     this for the *mining* VMs too, halving an existing ~2.75 GiB of waste.)
  2. R7-F2 — make the fail-safe target per-switch; `--verify-shares` should fail
     to `true`.
  3. R7-F4/R7-F3 — collapse `parse_native_loop` into `parse_switch` and repair
     the doc block, which also removes the divergence that produced R7-F2.
  4. R6-F7 (still open from round 5) — the 8 redundant FMOVs per iteration in
     the f-load path.

---

# Round 8 — final pass
**Scope note:** the range given was `3fcc388..3c281dc`, which contains only
**one** commit. The three unreviewed commits described are `e6724ce`, `3fcc388`
and `3c281dc` — `e6724ce` landed *before* my round-7 doc commit `9ab205d`, which
is why it fell outside. It is the `ShareVerdict` work I explicitly flagged in
round 7 as "needs its own round", so I am reviewing all three.

## Round 8 coverage ledger
| Area | Commit / file | Status | Notes |
|---|---|---|---|
| P3 — R7-F1 root fix: no Argon2d cache in full mode | 3fcc388 / vm.rs | DONE | Highest production risk; attacking the reasoning first |
| P1 — C1 worst-case test | 3c281dc / tests.rs | DONE | |
| P2 — differential helper split | 3c281dc / tests.rs | DONE | |
| P4 — wired-up verification test | e6724ce / miner.rs | DONE | |
| Doc corrections (R7-F5, framing) | 3c281dc / docs | DONE | |
| Deferred-item merge judgement | AUDIT.md | DONE | |

## Round 8 findings

### R8-VC1 — P3: the "no Argon2d cache in full mode" reasoning is SOUND. I tried to break it and could not.
I enumerated every reader of `cache_memory` in the crate rather than trusting
the claim:
- `vm.rs:1321` — `init_dataset_item(cache_memory, ...)`, reachable **only** from
  the `None` arm of `match dataset`. A full-mode VM has `dataset: Some(_)`, so
  this arm is unreachable for it.
- `vm.rs:1733` — `cache_and_programs()`, the public accessor.
- `dataset.rs:64` / `dataset.rs:103` — both take the cache as a *parameter*;
  they never reach into a `RandomXVm`.
- `vm.rs:1533/1576` — the free function `calculate_hash_versioned` builds its
  own local cache and is untouched by this change.
- `vm.rs:1766/1831/1947` — the three `execute_vm` call sites pass
  `&self.cache_memory`, but each is on a VM whose `dataset` decides the arm.

The load-bearing question is whether `dataset` can become `None` on a VM that
has no cache. **It cannot:** `grep -n "self.dataset = "` finds exactly **one**
assignment in the whole file (`vm.rs:1719`), and it is inside `reinit`, which
rebuilds the cache on the `None` branch in the same statement. There is no
`set_dataset`, no `pub` field, and `cache_memory`/`dataset` are private. So the
invariant "cache is empty ⟹ dataset is Some" is established at both
constructors and preserved by the only mutator.

`new_versioned` (light) still builds the cache, so light mode is unaffected.
The change is also correct for rx/2: `new_full_versioned` covers both versions
and neither reads the cache when a dataset is present.

### R8-F1 — The new invariant is load-bearing for a *public* accessor but is documented only inside the constructor body  [MINOR]
**Where:** `src/randomx/vm.rs:1731-1734`
**Claim:** `cache_and_programs()` is `pub`, and its doc comment is unchanged:
`/// Get references to cache and programs (for dataset generation).` It now
returns an **empty** cache slice for any full-mode VM. The reason is explained
only in a body comment inside `new_full_versioned`, which a caller reading the
accessor will not see.
**Failure scenario:** a future caller writes the natural-looking
`let vm = RandomXVm::new_full(key, ds); let (c, p) = vm.cache_and_programs();
RandomXDataset::generate(c, p, n)` and gets an empty slice. `load64_native`
(`dataset.rs:33`) then indexes `memory[offset..offset+8]` on a zero-length
slice, so this **panics** rather than producing a wrong dataset — the safe
failure — but it panics inside `RandomXDataset::generate`'s worker threads,
which is a confusing place to land. No current caller does this: all five
in-tree callers (`miner.rs:671`, `tests.rs:34`, `tests.rs:1054`,
`benches/fullmode.rs:39`, `benches/nativeloop_ab.rs:208`) construct a **light**
VM with `RandomXVm::new` first, exactly as the coordinator believed.
**Suggested (not a fix):** say it on the accessor — "the cache is empty for
full-mode VMs; build a light VM if you need it for dataset generation" — or,
better, return `Option<&[u8]>` so the emptiness is in the type. `minertim` is a
lib crate with these items `pub`, so this is an API contract, not just an
internal note.
**Confidence:** HIGH — enumerated every caller; confirmed the doc comment is
unchanged by reading the file.

### R8-VC2 — P1: the C1 worst-case test does reach the worst case, and a pass means what is claimed.
Checked all three things asked:
1. **Right entropy indices.** `ENTROPY_OFFSET = 0` (`vm.rs:67`), so
   `entropy(idx)` is bytes `idx*8 .. idx*8+8`. The test writes `pb[13*8..13*8+8]`
   and `pb[8*8..8*8+8]` — exactly the words `derive_program_params` reads for
   `dataset_offset` and `ma`. Neither collides with anything else: entropy 0-7
   seed the a-registers, 10 is `mx`, 12 the address registers, 14/15 the
   `e_mask`. Instructions start at byte 128 (`INSTRUCTIONS_OFFSET = 16*8`), so
   both writes stay inside the entropy block and leave the program intact.
2. **The forced values are the maxima, not merely large.**
   `ma = (0xFFFF_FFFF_FFFF_FFFF as u32) & 0x7FFF_FFC0 = 0x7FFF_FFC0` — the
   largest value the mask can produce. `dataset_offset =
   (524287 % 524288) * 64 = 524287 * 64` — the largest the modulus can produce.
   The test asserts both before proceeding, so it cannot silently degrade into
   testing a merely-large address.
3. **The extreme is actually executed.** In both paths the *first* iteration
   reads at `dataset_offset + (ma & mask)`: the interpreter takes `read_ptr`
   from `mem_ma` before any XOR (`vm.rs:1305`), and the emitted prologue loads
   `x24 = init_ma` and `x22 = base + dataset_offset`, with
   `and x0, x24, #0x7fffffc0 ; add x0, x22, x0` in the first `emit_iteration_post`.
   So iteration 1 is the pinned worst case in both arms.

**What a pass proves — and it is the right thing.** The reference arm reads the
dataset through `RandomXDataset::get_item`, which is a **bounds-checked**
`self.items[item_number]`. So if the emitted arithmetic computed an
out-of-range item at the top of the range, the reference would panic and the
test would fail loudly; and if it computed a merely *different* in-range
address, the r-registers would diverge and the comparison would fail. A pass
therefore means "the emitted address arithmetic agrees with a bounds-checked
read at the maximum reachable address", which is exactly the property C1 needs
and which was previously only argued. This retires my round-5 note honestly.

**One overstatement in the test's own comment** (not a finding, but worth
knowing): it says a wrong address "means a segfault or a mismatch". A segfault
is unlikely — 64 bytes past a 2 GiB `Vec` inside a larger heap is almost
certainly mapped — so in practice the detector is the mismatch and the
reference-side bounds check, not a fault. That is still sufficient; the comment
just credits the wrong mechanism.

### R8-VC3 — P2: the helper split changed nothing about what the existing tests cover.
`assert_paths_agree(seed, iters, ds)` now calls
`assert_paths_agree_with(&make_program_bytes(seed), seed, iters, ds)`, i.e. it
passes the same `seed` through as `sp_seed`, and `assert_paths_agree_with` calls
`make_scratchpad(sp_seed)` for **both** arms. So for seeds 1/2/7/78 at N=1/2/3
and seed 11 at N=2048 the program bytes and both scratchpads are byte-identical
to before the split. The renames are `seed` -> `sp_seed` inside the extracted
function only, and every one is in an assertion *message*, not in a value.
The scratchpad seeding is still tied to the right value.

I also confirmed the `str.replace` collateral was fully reverted:
`native_loop_zero_iterations_terminates` still calls `make_program_bytes(3)`
(`tests.rs:1117`) and `make_scratchpad(3)` (`tests.rs:1140`) directly and does
not route through either helper. The only new caller of the extracted form is
the C1 test at `tests.rs:1094`.

### R8-VC4 — P4: the withhold test closes most of the R7-Q1 gap, but not the exact hole I named.
What is now pinned, and it is a lot: `classify_share` is a pure function with
all four verdicts covered (`classify_share_covers_every_branch`), the
verdict-to-action mapping is covered separately
(`only_a_mismatch_blocks_submission`, all four variants), a single differing
byte is covered, and `verifier_withholds_a_hash_that_does_not_match_the_reference`
drives it with two **genuine** RandomX hashes for adjacent nonces rather than
synthetic patterns. `pipelined_hash_matches_calculate_hash_for_the_preceding_blob`
independently reproduces the off-by-one property I verified empirically in round
7. The decision logic can no longer regress silently.

**What remains untested is the three lines of glue in `worker_loop`:**
```rust
let verification_applies = verify_shares && native_loop;   // (a)
let reference = if verification_applies { ...calculate_hash(&job_blob_current) } else { None };  // (b)
let verified = verdict.should_submit();
if verified && let Err(e) = pool.submit_share(...)          // (c)
```
My round-7 wording was "if `verified` were refactored to be unconditionally
`true`, no test would notice" — that mutation lives in (c) and **still** would
not be caught. So: the gap is substantially narrowed, not closed. I checked (c)
by reading and it is correct (`should_submit()` gates `submit_share`, and
`Withhold` is the only variant that returns false).
I do **not** think this blocks a merge. The residue is three lines of
straight-line code in a function that needs a live pool and a 2 GiB dataset to
instantiate; the commit message and AUDIT.md both name it explicitly rather than
implying full coverage, and "make `worker_loop` testable against a fake pool" is
correctly filed as its own MR.

### R8-VC5 — R7-F2/F3/F4 are all correctly fixed, and the fix is better than what I suggested.
`fail_safe` is now a per-switch **parameter** rather than a hardcoded direction:
`parse_native_loop` -> `parse_switch(.., default_on = true, fail_safe = false)`,
`parse_verify_shares` -> `parse_switch(.., default_on = true, fail_safe = true)`.
The warning text interpolates the direction (`assuming ON/OFF (the safe
direction)`), so the message cannot drift from the behaviour. The doc block is
re-parented onto `parse_switch` and states the asymmetry explicitly, and
`parse_native_loop` is now a two-line wrapper — the duplication is gone. The new
test asserts both directions side by side, including
`assert!(parse_verify_shares(&args(&["--verify-shares", "nonsense"])))`, and the
empty-env case I raised (`set_var(VAR, "")` -> "empty env must not disarm") is
pinned directly.

### R8-F2 — The new env-var test's SAFETY comment justifies the wrong thing; `set_var` racing `getenv` across parallel tests is the actual hazard  [MINOR]
**Where:** `src/bin/minertim.rs`,
`switch_reads_the_environment_and_the_flag_overrides_it`
**Claim:** The comment reads
`// SAFETY: a name unique to this test; no other thread reads it.`
Name uniqueness is not what makes `std::env::set_var` unsafe. The hazard is that
`setenv` mutates a process-global `environ` array and may reallocate it, while
any concurrent `getenv` in another thread reads it — a use-after-free regardless
of which *names* are involved. That is precisely why Rust 2024 marked it
`unsafe`.
**Evidence:** the same test binary contains tests that read the environment
concurrently: `parse_native_loop` and `parse_verify_shares` both call
`std::env::var` (via `parse_switch`), and `native_loop_defaults_on`,
`native_loop_accepts_the_documented_spellings`,
`verify_shares_fails_safe_on_because_it_is_a_safety_net` and others call them.
`cargo test` runs these in parallel by default, so a `set_var` in one thread can
overlap a `var()` in another.
**Failure scenario:** flaky or crashing tests, not a production bug — the miner
only reads the environment once, single-threaded, at startup. In practice
libc implementations rarely shrink `environ`, so this is unlikely to bite; but
the SAFETY comment asserts an invariant that does not hold, which is worse than
no comment because it stops the next reader from re-checking.
**Also, one line lower down:** `verify_shares_fails_safe_on_because_it_is_a_safety_net`
and the `parse_native_loop` tests read the **real** `MINERTIM_VERIFY_SHARES` /
`MINERTIM_NATIVE_LOOP` variables, so a developer who has either exported will
see spurious failures. Pre-existing since round 6, now on more tests.
**Confidence:** HIGH on the mechanism; the practical risk is LOW.

### R8-VC6 — R7-F5 and the framing corrections are accurate.
Both halves of R7-F5 are fixed and the fix is correct in substance, not just in
wording:
- The AUDIT now says the counter is "its own `log::error!` immediately before
  the stats line rather than appended to it", which matches
  `src/bin/minertim.rs` exactly.
- The stage-D table's 1-thread row reads `+6.12% | +6.45% | +6.1% to +6.5%`,
  with a note that this is the *stronger* of the two replications. That is
  correct — the two independent baselines were 570.0 and 570.2 H/s.

The "native-loop machinery, not a JIT defect" reframing is accurate in all three
user-visible places I checked: `mining.conf.example` ("both paths share the same
instruction generator... a targeted check, not a general guarantee"), the
`--help` text ("Catches faults in the native-loop machinery; both paths share an
instruction generator, so a fault common to both would pass"), and the runtime
warning ("A native-loop defect would now be submitted..."). Each states the
limitation rather than merely softening the claim, which is what R7-F6 asked
for. `src/miner.rs` carries the same limit as a comment at the call site.

## Deferred items — should any block the merge?
**No. None of the six should block, and I would not hold the MR for any of
them.** Reasoning per item:
- **R5-F2** (debug vs release assert gap) — the three `debug_assert!`s are all
  invariants I proved by construction in round 5, and `e6724ce` added release
  tests for the guards that actually matter (v1-only, the C1 bound in both
  directions, both ABI directions). Aligning `make test` with the verified
  profile is a one-line Makefile change whenever someone wants it.
- **R5-F4** (two 2 GiB test datasets) — test-only. Worth noting it interacts
  with the "CI can never run this, so the local differential tests are the
  mandatory gate" framing: a contributor on a 16 GB machine may not be able to
  run the mandatory gate. That is an argument for fixing it sooner than its
  severity suggests, not for blocking.
- **R5-F6** (no barrier in the 11-thread phase) — dilutes rather than inflates,
  and the claim is now a *range* across two independent runs, which absorbs it.
- **R5-F7** (8 FMOVs) — pure performance, and correctly filed as issue #1
  because it changes emitted ARM64 and deserves its own review round.
- **worker_loop testability** — see R8-VC4; three lines of straight-line glue,
  named explicitly rather than papered over.
- **CI coverage (issue #2)** — this is the only one I would flag for attention
  rather than deferral. Not as a blocker for *this* MR (it is pre-existing: the
  JIT has always been aarch64-gated and the runners have always been x86_64
  Linux, and this MR adds a runtime backstop that did not exist before), but it
  means every correctness claim about emitted ARM64 rests on one machine plus
  the maintainer remembering to run `make test`. An ARM64 runner is the real
  fix and should not be deferred indefinitely.

**Addendum to R8-F1 — the accessor is now asymmetric.** `cache_and_programs()`
returns `(&[u8], &[SuperscalarProgram; 8])`. After this change `.0` is empty for
a full-mode VM but `.1` is still fully populated, because
`new_full_versioned` still runs `generate_superscalar` eight times. That is
correct and the cost is negligible (microseconds, a few KB, versus 256 MiB and
0.4 s), but it makes the tuple's two halves behave differently in a way nothing
in the signature or the doc hints at — one is conditionally empty, the other
never is. It strengthens the case for saying so on the accessor.

### R8-VC7 — R7-F1's fix delivered, measured.
Timed the exact sequence `worker_loop` runs to build the verifier on its first
share (`RandomXVm::new_full(key, ds)` then `set_native_loop(false)`), release
build, on the same machine as the round-7 measurement:
```
light-mode cache: 256 MiB
verifier build #0: 0.8 ms   cache = 0 bytes
verifier build #1: 0.7 ms   cache = 0 bytes
verifier build #2: 0.6 ms   cache = 0 bytes
```
Round 7 measured the same construction at **372-432 ms and 256 MiB**. So the
in-line latency in the share-submission path is down by roughly **500x**, to
sub-millisecond, and the per-worker 256 MiB is gone entirely — 2.75 GiB of
verifier memory plus the 2.75 GiB the mining VMs were already wasting. Light
mode still gets its cache (`256 MiB`, first line), confirming the conditional
did not over-apply.

### R8-VC8 — the claimed state is real.
Ran the full suite and both clippy targets myself on `3c281dc`:
```
test result: ok. 121 passed; 0 failed; 2 ignored   (lib, release, 93.28s)
test result: ok. 7 passed; 0 failed                (bin)
clippy --all-targets -- -D warnings                          clean (aarch64)
clippy --all-targets --target x86_64-apple-darwin -- -D warnings   clean
```
Matches the coordinator's stated 121 + 7 exactly.

## Round 8 verdict

**Blockers: none.**

**Majors: none.** This is the first round in four that has not produced one.

**Minors:** R8-F1 (the full-mode empty-cache invariant is load-bearing for a
`pub` accessor but documented only in the constructor body; the tuple is now
asymmetric), R8-F2 (the env-var test's SAFETY comment justifies name uniqueness,
which is not the hazard; `set_var` racing `getenv` across parallel tests is).
Neither affects the mining path.

**All four priorities check out:**
1. The C1 worst-case test really does reach the worst case — right entropy
   words, true maxima (asserted), executed on iteration 1 in both arms — and a
   pass means what is claimed, because the reference arm's read is
   bounds-checked. My round-5 "only argued, never executed" note is properly
   retired.
2. The helper split changed nothing: seeds 1/2/7/78 and the 2048 case run on
   byte-identical program bytes and scratchpads, and the reverted `str.replace`
   collateral is confirmed clean.
3. The "no Argon2d cache in full mode" reasoning survives attack. I enumerated
   every reader of `cache_memory` and every mutator of `dataset` — there is
   exactly one of the latter, inside `reinit`, which rebuilds the cache on the
   light-mode branch. No full-mode path can reach a cache read.
4. The withhold test closes the decision-logic half of R7-Q1 thoroughly; three
   lines of `worker_loop` glue remain untested, which is acknowledged rather
   than hidden.

**Mergeable: yes.** I would merge this. The wrong-hash and memory-safety
surface — which is what matters here — has now been examined four times: the
emitted ARM64 disassembled instruction by instruction, the C1 bound recomputed
independently and now executed, the ABI verified, the differential and
known-answer gates run locally on every round, and ~147k hashes cross-checked
between two genuinely different execution paths across two independent
benchmark runs. Every finding I have raised across rounds 5-8 is either applied
or explicitly deferred with a reason, and the deferred list contains nothing
that can produce a wrong share.

## Remaining work if this review is interrupted
- **Round 8 is complete.** All four priorities answered, two minors filed, the
  full suite and both clippy targets run independently, and R7-F1's fix measured.
- Nothing outstanding blocks a merge. The two round-8 minors (R8-F1, R8-F2) are
  documentation/test-hygiene and can ride in any follow-up.
- Post-merge, in priority order: **issue #2 (an ARM64 CI runner)** — the only
  deferred item I would not leave open indefinitely, since every ARM64
  correctness claim currently rests on one machine plus a manual `make test`;
  then R5-F4 (the two test datasets, because it can stop a contributor running
  the mandatory local gate); then issue #1 (R5-F7, the 8 FMOVs); then the
  `worker_loop` fake-pool seam.

## Round 8 postscript — verification timing, and work in flight

**My round-8 verification was a clean reading of `3c281dc`, not of a torn tree.**
This matters because I hit exactly that trap in round 7. Checked by timestamp:
my suite+clippy run finished at `05:47:04` (mtime of the capture log), and the
next batch of working-tree edits landed at `05:49:54`-`05:50:07`. The tree was
clean apart from `.claude/settings.local.json` throughout the run. So
`121 lib + 7 bin passed, clippy clean on both targets` stands as a statement
about the reviewed commit.

**Uncommitted work has since appeared and is NOT reviewed** (40 lines across
`src/bin/minertim.rs`, `src/randomx/dataset.rs`, `src/randomx/tests.rs`,
`src/randomx/vm.rs`). From a glance it is R8-F1 being addressed, and the
approach is better than what I suggested: the accessor doc now states the
asymmetry explicitly, *and* `RandomXDataset::generate` gained an
`assert!(!cache_memory.is_empty(), ...)` so a full-mode VM's empty cache fails
loudly at the boundary instead of as an out-of-bounds index inside the spawned
workers. That converts my "confusing place to land" objection into a named
programmer error at the call site.

One cosmetic defect in that in-flight edit, flagged only because it is about to
be committed and is a one-character fix — **not a round-8 finding**:
`src/randomx/dataset.rs:97` has a **14-space run inside the panic message**,
between `got an` and `empty one`:
```
"dataset generation needs a light-mode VM's Argon2d cache; got an              empty one (a full-mode VM does not build one)"
```
It reads like a lost `\` line-continuation when the literal was reflowed onto a
single line. Harmless, but it is the text an operator sees at the moment
something has gone wrong.

Nothing in that batch changes any round-8 conclusion: it touches documentation,
a new precondition assert, and tests — not the emitted loop, the C1 arithmetic,
or the share-verification decision.

---

# Round 9 — `3c281dc..5fe7eb3`
Four commits: `914fe88` (R8-F1/R8-F2 fixes), `ea354ee` (panic-message repair),
`8c80d8d` (my own review file — ignored), `5fe7eb3` (`ShareVerifier`
extraction — the substantive one).

## Round 9 coverage ledger
| Area | Commit | Status | Notes |
|---|---|---|---|
| P1 — is the extraction behaviour-preserving? | 5fe7eb3 | DONE | R9-VC1; one exception, R9-F1 |
| P2 — do the tests pin the stale-verifier hazard? | 5fe7eb3 | DONE | R9-F2: half of it; key measured irrelevant |
| P3 — `is_armed()` as the `classify_share` predicate | 5fe7eb3 | DONE | R9-F1: branch is now unreachable |
| P4 — `generate`'s hard assert on a public API | 914fe88 | DONE | R9-VC2: cannot fire legitimately |
| Panic-message repair + branch sweep | ea354ee | DONE | R9-VC3 + R9-F3 |
| `#[cfg(all(test, aarch64))]` on the test accessors | 5fe7eb3 | DONE | R9-F4 |
| AUDIT "no behaviour change" claim | 5fe7eb3 | DONE | R9-F6: inaccurate |

## Round 9 findings

### R9-VC1 — P1: the extraction is behaviour-preserving on the mining path, with one exception (R9-F1).
I compared the old inline code against the new methods statement by statement:
- **Drop timing.** `verify_vm = None` sat as the first statement of the
  seed-change block; `rekey`'s `self.vm = None` is the first statement of
  `rekey`, called from the same position in the same block. Identical.
- **Build timing.** Both build via `get_or_insert_with` reached only from the
  share branch. Identical — a worker that never finds a share still never
  builds a VM.
- **The key.** The old code built the VM from `&current_key`, a worker local
  assigned *after* the verifier reset; the new code snapshots `job.seed_hash`
  into `self.key` at `rekey` time. Both resolve to the same bytes, because
  `rekey(&job.seed_hash, ..)` and `current_key = job.seed_hash.clone()` are
  adjacent statements fed from the same value. The new form is the more robust
  of the two: it captures the key at rotation rather than reading a mutable
  local at share time.
- **The dataset.** Old: `verify_dataset = Some(dataset)` (moving the Arc after
  two `.clone()`s for the mining VM). New: `verifier.rekey(.., dataset)` in the
  same position, same move. Identical.
- **`enabled`.** Old computed `verify_shares && native_loop` per share; new
  computes it once at construction. Both inputs are immutable locals captured at
  spawn, so there is no difference.
- **Guard relationship.** `rekey` is called only inside
  `if job.seed_hash != current_key || vm.is_none()`, exactly where the three
  assignments used to be. Because `vm.is_none()` is true on the first pass, the
  verifier always holds a dataset before any share can be found.

### R9-F1 — The refactor silently retired the `SubmitVerifierUnavailable` branch; it is now provably unreachable, and the AUDIT's "no behaviour change" is not quite right  [MINOR]
**Where:** `src/miner.rs` — `ShareVerifier::is_armed` / `ShareVerifier::reference`,
and the `ShareVerdict::SubmitVerifierUnavailable` arm in `worker_loop`
**Claim:** Before, the predicate and the value were computed independently:
`verification_applies = verify_shares && native_loop` said nothing about the
dataset, while `reference` was `None` when `verify_dataset` was `None`. That
combination — armed but no reference — produced `SubmitVerifierUnavailable` and
a `log::warn!("...no dataset recorded for the verifier. This should not
happen.")`.

After, both derive from the same state:
```
is_armed()   == enabled && dataset.is_some()
reference()  == None  iff  !enabled || dataset.is_none()
```
so `is_armed() == true` **implies** `reference()` is `Some`. The
`SubmitVerifierUnavailable` verdict can no longer be produced by `worker_loop`
at all, and its `log::warn!` is now dead code.

| enabled | dataset | before | after |
|---|---|---|---|
| false | any | SubmitUnverified | SubmitUnverified |
| true | none | **SubmitVerifierUnavailable** (+warn) | **SubmitUnverified** (silent) |
| true | some | SubmitVerified / Withhold | unchanged |

**Failure scenario:** none in practice — the middle row was already unreachable
(`rekey` runs before the first hash because `vm.is_none()` forces the block), and
both verdicts submit, so no share is lost either way. The costs are: (a) the
AUDIT's "no behaviour change" claim is inaccurate for that row; (b) a
fail-open safety property we explicitly agreed on in round 7 is now vacuous
rather than enforced — it cannot fire, so it cannot warn; (c) a future reader
sees the arm handled and reasonably assumes it is live.
**I would keep the arm** — it is correct defensive coding if `reference()` ever
gains another `None` path — but the AUDIT should say the branch became
unreachable rather than that nothing changed.
**Confidence:** HIGH — this follows directly from the two method bodies.

### R9-F2 — The rotation test's dataset assertion is vacuous, and the dataset is the half that actually matters  [MINOR]
**Where:** `src/randomx/tests.rs`,
`share_verifier_builds_lazily_and_resets_on_seed_rotation`
**Claim:** You asked whether re-keying with the same `Arc` weakens the test.
It does, and specifically it hollows out the assertion that covers the real
hazard.

**First, a fact I measured rather than assumed: in full mode the key has no
effect on the hash at all.** Two full-mode VMs over the *same* dataset with
completely different keys:
```
key ALPHA : f04a3a9feec72386571fd896e068f8abca0361b3b8dce2efbddffa6c7c5c46bc
key BRAVO : f04a3a9feec72386571fd896e068f8abca0361b3b8dce2efbddffa6c7c5c46bc
=> key affects full-mode hash? false
```
That follows from `914fe88`: `new_full` no longer builds a cache, and
`ss_programs` are read only by `init_dataset_item` on the light-mode arm. So the
only state that can make a verifier stale is **the dataset**.

Now the two halves of the rotation:
- **Drop the cached VM** — covered properly. `assert!(!v.has_cached_vm())` after
  `rekey` is a real assertion and catches the primary mechanism (a VM built
  against the old dataset being reused).
- **Adopt the new dataset** — **not covered.** The test rekeys with
  `ds.clone()`, the same `Arc` allocation, so `holds_dataset(&ds)` is
  `Arc::ptr_eq(same, same)` — trivially true. It cannot distinguish "adopted the
  new dataset" from "ignored the argument and kept the old one". If `rekey` were
  mutated to `if self.dataset.is_none() { self.dataset = Some(dataset) }`, every
  assertion in this test would still pass, and the production symptom would be
  the catastrophic one: every share withheld after the first rotation, with an
  error message telling the operator to restart with `--native-loop off`, after
  which the rejects continue.

Conversely, the *other* half of `rekey` — the `self.key` bookkeeping the test
also does not check — is **inert**, per the measurement above. So the test's
emphasis is inverted relative to the risk: the untested half that looks
frightening is harmless, and the untested half that looks like bookkeeping is
the load-bearing one.

**Mitigating:** `rekey`'s body is a single unconditional
`self.dataset = Some(dataset);` with no branch that could skip it, so this is a
gap in proof, not evidence of a defect. I read it and it is correct.

**The fix is free — no third dataset needed.** The test binary already holds two
distinct 2 GiB datasets as `LazyLock` statics in the same process:
`full_hash_tests::test_key_000_dataset()` (`b"test key 000"`) and
`native_loop_diff_tests::test_dataset()` (`b"native loop test key"`). Both are
in `src/randomx/tests.rs` and both are compiled on aarch64. Making the latter
`pub(super)` and rotating between the two would give genuinely different `Arc`s
and let the test assert `!holds_dataset(&ds1) && holds_dataset(&ds2)` at zero
additional memory or time cost. That also turns R5-F4 (two datasets as a
liability) into an asset.
**Confidence:** HIGH on the vacuity and on the key-irrelevance (measured).

### R9-VC2 — P4: `generate`'s hard assert cannot fire on any legitimate path.
- **Every in-tree caller passes a light-mode VM's cache.** `miner.rs:733`
  (`get_or_generate_dataset`, which constructs a fresh `RandomXVm::new(seed_hash)`
  on *every* call, so a seed change is covered), `tests.rs:35`, `tests.rs:1135`,
  `benches/fullmode.rs:40`, `benches/nativeloop_ab.rs:209` — all `RandomXVm::new`.
  The only other caller is `tests.rs:507`, which triggers it deliberately.
- **A light-mode cache is never empty.** `argon2d_cache` (`argon2d.rs:376-419`)
  ends with `let mut result = vec![0u8; total_blocks * ARGON2_BLOCK_SIZE]`, a
  fixed 256 MiB whose length is derived from compile-time constants and is
  independent of the key — including an empty key. There is no input that yields
  a zero-length cache.
- **Placement is right:** the assert precedes
  `vec![[0u64; 8]; DATASET_ITEM_COUNT]`, so the programmer error is reported
  before 2 GiB is allocated rather than after.
- It is a real `assert!`, not `debug_assert!`, so it holds in release. Correct
  for a public-API precondition.

### R9-VC3 — `ea354ee` repairs the panic message correctly, and the branch sweep claim checks out.
The literal is now
```rust
"dataset generation needs a light-mode VM's Argon2d cache; got an empty \
 one (a full-mode VM does not build one)"
```
The `\`-newline strips the newline and the next line's leading whitespace while
preserving the space before the backslash, giving `...got an empty one (a
full-mode VM does not build one)`. Correct.

I re-ran the sweep independently across every tracked `.rs` file, looking for the
signature of the damage — a 3+ space run *mid-sentence* (lowercase or
punctuation, spaces, lowercase) rather than at line start:
```
benches/nativeloop_ab.rs:175  ...H/s   median {:8.1}...
benches/nativeloop_ab.rs:176  ...H/s   median {:8.1}...
```
Two hits, both deliberate column alignment in benchmark output. **No remaining
instances of the mangling pattern**, confirming the sweep.

### R9-F3 — The `should_panic` substring still sits before the region that was mangled, so the repair remains untested  [MINOR]
**Where:** `src/randomx/tests.rs:503`
**Claim:** `#[should_panic(expected = "needs a light-mode VM's Argon2d cache")]`
matches a prefix that ends well before `got an ... empty one`. That is exactly
why the mangled message passed CI, clippy and this test in the first place — and
the expected substring was not extended when the message was repaired. If the
same scripted-edit accident recurred tomorrow, this test would pass again.
**Failure scenario:** cosmetic only — a garbled operator-facing panic message
ships. But it is the identical blind spot that let it through once already, and
closing it costs one word: extending the expected substring to reach past the
join, e.g. `"got an empty one"`.
**Confidence:** HIGH.

### R9-F4 — The two `ShareVerifier` tests are gated to aarch64 for a reason that does not hold, and the gate costs CI the only new coverage it could have run  [MINOR]
**Where:** `src/randomx/tests.rs:614` and `:663`; the comment on the accessors in
`src/miner.rs`
**Claim:** The accessors carry
`// aarch64-gated to match their only callers: the tests need a real full-mode
VM, which means the JIT, which is aarch64-only.` A full-mode VM does **not**
require the JIT — `RandomXVm::new_full` is not architecture-gated, and on
x86_64 it simply runs the interpreter. `ShareVerifier` itself is entirely
architecture-independent: the lazy build, the rotation reset and the disabled
path have nothing to do with emitted code.
The plausible real reason would be avoiding a 2 GiB dataset build on x86_64 CI —
but that saves nothing here, because `full_mode_vm_allocates_no_argon2d_cache`
(`tests.rs:489`) and `dataset_generation_rejects_a_full_mode_vms_empty_cache`
(`tests.rs:502`) are **ungated** and both call `test_key_000_dataset()`, so CI
already forces that LazyLock.
**Failure scenario:** no defect. The cost is coverage: CI cannot run any of the
JIT work (issue #2), and these two tests are among the very few new ones it
*could* validate — the state machine whose failure mode is "withhold every
share". Gating them means the only machine that ever exercises them is the same
single machine that exercises everything else.
**Note:** the `#[cfg(all(test, target_arch = "aarch64"))]` on the accessors
themselves is *correct* given the current callers — it prevents `dead_code`
warnings on x86_64. If the tests were ungated the accessors should be too.
**Confidence:** HIGH on the reasoning being wrong; MEDIUM on whether ungating is
worth it to you, since it does add interpreter-speed full-mode hashing to CI.

### R9-F5 — The rewritten SAFETY comment identifies the right hazard but then justifies it with a false claim; the R8-F2 race is still live  [MINOR]
**Where:** `src/bin/minertim.rs`,
`switch_reads_the_environment_and_the_flag_overrides_it`
**Claim:** The new comment correctly replaces the name-uniqueness reasoning with
the real hazard ("the hazard is concurrency, not the name" — exactly right).
But it then justifies the `unsafe` with:
> *"It is acceptable here because no other test in this binary reads the
> environment: `parse_switch` is the only reader and is exercised nowhere else"*

`parse_switch` **is** the only reader function, but it is emphatically exercised
elsewhere — by six other tests in the same binary, each of which reaches
`std::env::var` through a wrapper:

| test | calls | reaches |
|---|---|---|
| `native_loop_defaults_on` | `parse_native_loop` | `env::var("MINERTIM_NATIVE_LOOP")` |
| `native_loop_accepts_the_documented_spellings` | `parse_native_loop` | same |
| `native_loop_unrecognised_value_fails_safe_to_off` | `parse_native_loop` | same |
| `native_loop_with_no_value_fails_safe_to_off` | `parse_native_loop` | same |
| `native_loop_last_flag_wins` | `parse_native_loop` | same |
| `verify_shares_fails_safe_on_because_it_is_a_safety_net` | `parse_verify_shares` + `parse_native_loop` | `env::var("MINERTIM_VERIFY_SHARES")` |

`cargo test` runs these in parallel by default, so the `set_var` in the seventh
test can overlap a `getenv` in any of the six. The precondition the comment
relies on does not hold, so the race I raised in R8-F2 is unchanged.
**Failure scenario:** unchanged from R8-F2 — flaky or crashing *tests*, never a
production issue (the miner reads the environment once, single-threaded, in
`main`). In practice libc rarely shrinks `environ`, so this is unlikely to bite.
**Why I am raising it again rather than letting it go:** the comment now asserts
a specific, checkable precondition, and a future reader who adds a seventh
env-reading test will consult that comment, find it already violated, and have
no way to tell that it was violated on the day it was written. Either make the
claim true (`--test-threads=1` for this binary, or a mutex around the env
tests), or state the residual risk instead of a precondition that does not hold.
**Confidence:** HIGH — enumerated every test in the module.

### R9-F6 — AUDIT's "No behaviour change" for `5fe7eb3` is inaccurate  [MINOR]
**Where:** AUDIT.md, `5fe7eb3` entry: *"No behaviour change: `rekey` drops the
cached VM..."*
Per R9-F1, one row of the truth table did change: `enabled && dataset.is_none()`
moved from `SubmitVerifierUnavailable` (with a `log::warn!`) to
`SubmitUnverified` (silent), and that verdict is now unreachable from
`worker_loop` by construction. The *action* is identical in every case, and the
changed row was already unreachable, so the claim is nearly true — but "no
behaviour change" is exactly the kind of statement this log has been careful to
get right elsewhere. Suggested wording: *"No change to any reachable action; one
already-unreachable diagnostic branch became structurally unreachable."*
**Confidence:** HIGH.

### R9-F7 — Nothing tests that the verifier is actually on the *reference* path; a dropped `set_native_loop(false)` would be a silent no-op, which is precisely the round-5 F1 failure  [MINOR, but note the history]
**Where:** `src/miner.rs` `ShareVerifier::reference`;
`src/randomx/tests.rs` `share_verifier_builds_lazily_and_resets_on_seed_rotation`
**Claim:** The verifier's whole value rests on one line inside the lazy builder:
```rust
let mut v = RandomXVm::new_full(key, ds);
v.set_native_loop(false);          // <-- the entire point
```
It is present and correct. But **no test can detect its removal.** The rotation
test asserts the verifier's output equals a freshly built VM's hash:
```rust
let mut expected_vm = vm::RandomXVm::new_full(b"test key 000", ds.clone());
expected_vm.set_native_loop(false);
assert_eq!(hex_encode(&got), hex_encode(&expected_vm.calculate_hash(&blob)), ...);
```
That assertion cannot fail if the line is dropped, because the native loop and
the body JIT produce **identical** hashes — which is the premise of the entire
MR. Both sides would simply be the native loop, and the test would still pass.
`disabled_share_verifier_does_no_work` never builds a VM, and
`verifier_withholds_a_hash_that_does_not_match_the_reference` drives
`classify_share` directly rather than `ShareVerifier`. So the property "the
reference arm is the reference path" is asserted nowhere.

**This is structurally the same defect as round-5 F1**, where the A/B benchmark's
baseline arm was silently the native loop because the arm was inferred from a
default rather than asserted. The consequence here is the same shape: share
verification would compare the native loop against itself, report a clean
`verify_failures` counter forever, and provide zero protection — while every
test, clippy and CI stayed green. Given this branch has already been burned by
exactly this pattern once, I would not leave it unasserted.

**Cheap fix (not a fix I am making):** a test-only accessor on `ShareVerifier`
returning the inner VM's `use_native_loop` — or making
`RandomXVm::native_loop_enabled()` test-visible — and asserting it is `false`
after the first `reference()`. Two lines, no dataset cost beyond what the test
already pays.
**Failure scenario:** none today; the line is present. This is a missing guard
against a specific, historically-demonstrated regression, not a live bug.
**Confidence:** HIGH — the identical-hash premise is what makes the existing
assertion blind, and that premise is verified elsewhere in this very file.

### R9-VC4 — the claimed state is real, and my run covered the right source.
```
running 126 tests
test result: ok. 124 passed; 0 failed; 2 ignored   (lib, release, 92.75s)
test result: ok. 7 passed; 0 failed                (bin)
clippy --all-targets -- -D warnings                          clean (aarch64)
clippy --all-targets --target x86_64-apple-darwin -- -D warnings   clean
```
Matches your 124 + 7 exactly. `git diff 5fe7eb3..HEAD -- src/ benches/
Cargo.toml Makefile` is **empty**, and every commit of mine since touches only
`REVIEW_MR1.md`, so this is a clean reading of `5fe7eb3`'s source.

## Round 9 verdict

**Blockers: none. Majors: none.**

**Seven minors**, none of which affects a hash or a submitted share:
R9-F1 (a diagnostic branch became structurally unreachable), R9-F2 (the rotation
test's dataset assertion is vacuous), R9-F3 (the `should_panic` substring still
sits before the region that was mangled), R9-F4 (the `ShareVerifier` tests are
aarch64-gated for a reason that does not hold, costing CI the only new coverage
it could run), R9-F5 (the rewritten SAFETY comment's precondition is false),
R9-F6 (AUDIT's "no behaviour change"), R9-F7 (nothing asserts the verifier is on
the reference path).

**The four priorities:**
1. **Behaviour-preserving?** Yes on every reachable path. I walked drop timing,
   build timing, key derivation, dataset move, `enabled` computation and the
   guard relationship — all identical, and the key snapshot is more robust than
   the old read of a mutable local. The single exception is R9-F1, on a row that
   was already unreachable and whose action is unchanged.
2. **Do the tests pin the failure that matters?** Half of it. The VM-drop is
   properly asserted; the dataset-adoption is not, because the same `Arc` is
   re-keyed. And that is the load-bearing half — I **measured** that in full
   mode the key does not affect the hash at all, so only a stale *dataset* can
   make a verifier wrong. The fix is free: two distinct 2 GiB datasets already
   exist as `LazyLock` statics in that same test binary.
3. **Is `is_armed()` right?** It is a behaviour change and it did retire the
   fail-open branch — but the branch was already unreachable and both verdicts
   submit, so no share is at risk. The new structure is arguably better (armed
   now *implies* a reference exists). Keep the match arm as defence; correct the
   AUDIT wording.
4. **The `generate` assert.** Cannot fire on any legitimate path. Every caller
   builds a light VM first, `argon2d_cache` returns a fixed 256 MiB whose length
   is key-independent, and the assert precedes the 2 GiB allocation.

**The one I would act on first is R9-F7**, not because it is a defect — the line
is present and correct — but because it is the round-5 F1 pattern lying in wait:
an experiment arm that is *assumed* rather than *asserted*, in code whose whole
purpose is to catch silent wrongness. Two lines close it.

**Mergeable: yes, still.** Nothing in these four commits touches the emitted
loop, the C1 arithmetic or the hash path; the refactor is behaviour-preserving
where it counts, the new assert is sound, and the panic-message repair is
correct with the sweep independently confirmed.

## Remaining work if this review is interrupted
- **Round 9 is complete.** All four priorities answered, both lower-priority
  items answered, seven minors filed, full suite and both clippy targets run
  independently against `5fe7eb3`'s source.
- Suggested order for the minors: **R9-F7** (assert the verifier is on the
  reference path — the historically-demonstrated failure), then **R9-F2** (rotate
  between the two datasets that already exist), then R9-F5 (make the SAFETY
  precondition true or state the residual risk), then R9-F3/F6 (one-word fixes),
  then R9-F4 (a judgement call about CI cost).
- Still open by choice from earlier rounds and unchanged: R5-F2, R5-F4, R5-F6,
  issue #1 (R5-F7), issue #2 (ARM64 CI). Of these, issue #2 remains the only one
  I would not leave open indefinitely.

---

# Round 10 — `5fe7eb3..6f2b95b`
One commit applying all seven round-9 minors.

## Round 10 coverage ledger
| Area | Status | Notes |
|---|---|---|
| P1 — is `SubmitVerifierUnavailable` reachable again? | DONE | R10-F1: at `classify_share`, not in `worker_loop` |
| P2 — empty-env behaviour change (R9-F5 refactor) | DONE | R10-VC4 + **R10-F2** |
| P3 — dataset hoist: same datasets, still two? | DONE | R10-VC1/VC3: clean, exactly two |
| P4 — `vm_is_on_reference_path()` on x86_64 | DONE | R10-VC2: not vacuous |
| "Keyed to the dataset, not the seed" framing | DONE | R10-VC5: correct, one condition |
| R9-F3 substring; AUDIT accuracy | DONE | R10-VC6/VC7 |

## Round 10 findings

### R10-F1 — R9-F1's fix is correct, but `SubmitVerifierUnavailable` is *still* unreachable in `worker_loop` — for a different reason than before  [MINOR]
**Where:** `src/miner.rs` — `is_enabled` / `is_armed`, and the `classify_share`
call site
**You asked directly whether you moved the problem rather than fixing it. Neither,
exactly.** The fix does the thing it claims: it restores the *independence* of
`classify_share`'s two arguments. With `is_enabled()` the requirement for
`SubmitVerifierUnavailable` is `enabled && dataset.is_none()`, which is a
satisfiable state — whereas with `is_armed()` the predicate implied the value and
the branch was unsatisfiable *as a matter of logic*. That is a genuine
improvement and it is the right shape.

**But the branch still cannot fire in production**, because of an invariant one
level up that has nothing to do with the predicate:
```rust
if job.seed_hash != current_key || vm.is_none() {
    let dataset = get_or_generate_dataset(..);
    ...
    verifier.rekey(&job.seed_hash, dataset);   // dataset always set here
}
let rx_vm = vm.as_mut().unwrap();              // so vm.is_some() => rekey ran
```
`vm` is assigned *only* inside that block, and `vm.is_none()` forces the block on
the first pass, so `vm.is_some()` implies `rekey` has run and the verifier holds
a dataset. There is no path to the share branch with `dataset == None`. I said
the same thing in round 7 about the original `None` arm; that has not changed.
So the `log::warn!("...no dataset recorded for the verifier. This should not
happen.")` remains dead in the shipping binary.

**Why this is still worth having, and what I would not claim:** the branch is now
defence that *would* engage if `worker_loop` ever changed — a `rekey` moved
below the first hash, a second construction site for `vm`, an early-continue
added between them. That is exactly the class of future edit it should catch.
What the code comment should not imply is that it is reachable *today*: "keeps
the case reachable so it can fail open loudly" reads as a live guarantee, and
the guarantee is conditional on a `worker_loop` invariant the comment does not
mention.
**Cheap way to make the claim true and pinned:** a test that composes the two —
`let v = ShareVerifier::new(true);` (no `rekey`), then
`classify_share(v.is_enabled(), &hash, v.reference(&blob).as_ref())` and assert
`SubmitVerifierUnavailable`. That pins the composition rather than
`classify_share` in isolation, needs no dataset, and runs in microseconds. No
such test exists — `classify_share_covers_every_branch` passes the arguments
directly rather than deriving them from a `ShareVerifier`.
**Confidence:** HIGH.

### R10-VC1 — P3: the dataset hoist is clean. Same datasets, still exactly two.
- `native_loop_test_dataset()` uses the identical key (`b"native loop test key"`)
  and identical construction (`RandomXVm::new` → `cache_and_programs` →
  `generate(cache, programs, 8)`) as the `static DS` it replaced.
- `native_loop_diff_tests::test_dataset()` is now a one-line delegation to
  `super::native_loop_test_dataset()`, and the module's own `static DS` is
  **deleted** rather than left alongside. So the differential tests
  (seeds 1/2/7/78, the 2048 case, the C1 worst case, the zero-iteration test)
  all still run against byte-identical dataset contents.
- `grep -n LazyLock src/randomx/tests.rs` returns exactly **two** statics
  (lines 32 and 45) — `native_loop_test_dataset` and `test_key_000_dataset`.
  No third allocation.
- Resolution is correct: `full_hash_tests` opens with `use super::*` (line 370),
  so the unqualified call in the rotation test binds to the top-level helper.

### R10-VC2 — P4: `vm_is_on_reference_path()` is *not* vacuous on x86_64, and ungating the tests was the right call.
`RandomXVm.use_native_loop` is a plain `bool` field with **no** `#[cfg]` (only
`jit` is aarch64-gated), and `set_native_loop` is likewise ungated — it simply
assigns the field on every architecture. So on x86_64:
`ShareVerifier::reference` → `v.set_native_loop(false)` → field `false` →
`uses_native_loop()` → `false` → `vm_is_on_reference_path()` → `Some(true)`.
The assertion passes, and it passes *for the right reason*.

More importantly it still **fails** on x86_64 if the guarded line is removed: the
constructor default is `true`, so a dropped `set_native_loop(false)` yields
`Some(false)` and the assertion trips. The regression R9-F7 describes is a
source-level one, so checking the field is exactly the right instrument — and
since CI can never run the JIT (issue #2), this is now one of the few
native-loop-related regressions CI *can* catch. Ungating both tests was correct.

### R10-VC3 — R9-F2 is fully closed, including a self-check I did not ask for.
The rotation now goes `test_key_000_dataset()` → `native_loop_test_dataset()`,
two genuinely distinct `Arc`s, and asserts both directions
(`holds_dataset(&other)` and `!holds_dataset(&ds)`) — so the `ptr_eq(x, x)`
vacuity is gone. It then re-hashes after the rotation and compares against a VM
built on the new dataset, which pins the behaviour and not just the field. The
`assert_ne!(after, got, "the two datasets produced the same hash; this test
proves nothing")` is the part I would not have thought to ask for: it guards the
test against its own premise silently failing. Good.

### R10-F2 — An empty value now *erases* an explicit `off`, silently turning the native loop back on  [MAJOR]
**Where:** `src/bin/minertim.rs` `parse_switch_with` — `as_bool`'s new empty
short-circuit combined with `value = as_bool(v)` on both flag paths.

**First, your direct question: no, you did not invert R7-F2.** That finding was
about `--verify-shares`, and its outcome is unchanged and correct — an empty
`MINERTIM_VERIFY_SHARES=` still leaves the safety net **on**. Verified against
the shipped binary (case E below). The reasoning you quoted back at me was
applied correctly to the switch it was about.

**But the change did more than choose a default.** Round 7 replaced
`value = as_bool(v).or(value)` with `value = as_bool(v)`, which was safe *only
because* `as_bool` could never return `None`. This round made `as_bool` return
`None` for an empty value without restoring the `.or(value)`. So an empty value
no longer means "no opinion" — it means "discard whatever was decided before".

Measured on the release binary (grepping for the DISABLED warning):
```
A  MINERTIM_NATIVE_LOOP=                          -> native loop ON,  no output at all
B  --native-loop ""                               -> native loop ON,  silent
C  MINERTIM_NATIVE_LOOP=off  --native-loop ""     -> native loop ON,  silent   <-- explicit env erased
D  --native-loop off --native-loop ""             -> native loop ON,  silent   <-- explicit flag erased
E  MINERTIM_VERIFY_SHARES=                        -> verification ON  (correct, unchanged)
F  MINERTIM_NATIVE_LOOP=off                       -> "Native-loop JIT DISABLED" (control, works)
```
**C and D are the finding.** The operator set the switch explicitly, to `off`,
and got `on` with no diagnostic. No precedence model justifies an absent value
outranking a present one; "flag beats env" should mean a *parsed* flag beats
env, not that an empty token clears the field.

**Failure scenario.** `--native-loop "$NL"` with `$NL` unset is ordinary — and
note the *careful* style is the broken one: quoted, the empty argument reaches
case B/C; unquoted, the token vanishes and hits the bare-flag path, which warns
and correctly resolves to off. Two opposite outcomes from the same intent,
decided by quoting.
The harm is bounded — it cannot produce a wrong share, and it fails toward the
*faster* path, so no revenue is lost directly. What it defeats is the switch's
one stated purpose: ruling the JIT in or out during an incident. An operator
following the error message's own advice ("restart with `--native-loop off`")
through a wrapper that passes an empty value would see the rejects continue and
conclude the JIT is innocent. Share verification stays on and would still catch
a real fault, which is why this is not a blocker.

**The justification in the code comment is half right.** *"`NATIVE_LOOP=` in
mining.conf ... arrives here"* — it does not. I verified in round 6 (R6-VC2)
that `NATIVE_LOOP` is a **Makefile** variable and
`$(if $(NATIVE_LOOP),--native-loop $(NATIVE_LOOP),)` suppresses the flag
entirely when it is empty, so the shipped `mining.conf.example` never reaches
`parse_switch` at all. The *other* half — an unset shell variable arriving as an
empty string — is real, and is exactly the path that now misfires.

**Suggested direction (not a fix I am making):** keep "empty means unset" — it is
the conventional reading and I agree with it — but (a) restore `.or(value)` on
both flag arms so an empty value cannot clear an earlier decision, and (b) warn
on it, e.g. `warning: --native-loop given an empty value - ignoring`. That keeps
the semantics you wanted while removing both the erasure and the silence.
**Confidence:** HIGH — measured on the shipped binary, six cases.

### R10-VC4 — the injection refactor itself is exactly right and fully closes R9-F5.
`parse_switch_with` takes `env_value: Option<&str>`; `parse_switch` is a
one-line wrapper doing `std::env::var(env).ok().as_deref()`. No test calls
`set_var` any more — `grep set_var src/` returns nothing — so the
`setenv`/`getenv` race is *removed* rather than argued about, which is the right
way to answer a soundness objection. The temporary `Option<String>` in the
wrapper lives to the end of the call expression, so the `as_deref()` borrow is
sound. Production behaviour is unchanged: the env is still read once per switch.

### R10-VC5 — "keyed to the dataset, not the seed" is the right conclusion. I looked for a route you had not considered and there is none.
You asked me to check this specifically, so I enumerated every path by which the
key could reach a full-mode hash rather than relying on the round-9 measurement.

The key enters `RandomXVm` in exactly two places, both in the constructors:
`cache_memory = argon2d_cache(key)` (now `Vec::new()` for full mode) and
`ss_programs` from `Blake2Generator::new(key, 0)`. Tracing both:
- **`cache_memory`** — read at `vm.rs:1321` only.
- **`ss_programs`** — `grep -rn ss_programs src/` gives 20 hits; every one is a
  constructor (`:1658`, `:1691`, `:1718`), the accessor (`:1757`), a pass-through
  parameter on the three `execute_vm` call sites (`:1791`, `:1856`, `:1972`),
  dataset generation (`miner.rs:751/757`), or the standalone
  `calculate_hash_versioned` which builds its own (`:1537/:1577`). The **only**
  read during hashing is `vm.rs:1321`.
- `vm.rs:1321` is `None => init_dataset_item(cache_memory, ss_programs, ..)`,
  the `None` arm of the single `match dataset` on the hash path (`vm.rs:1319`).
  A full-mode VM has `dataset: Some(_)`, so that arm is never taken.

No other field of `RandomXVm` derives from the key. So the key's *only* channel
to a full-mode hash is the dataset that was generated from it — which is exactly
your framing. It also holds for rx/2 (the AES F/E mix and `mp` aliasing read the
dataset, not `ss_programs`), and the verifier is V1 regardless
(`new_full` → `new_full_versioned(.., RxVersion::V1)`).

**One condition worth writing down, since the framing now carries weight:** this
is true because the full/light split is absolute — full mode *never* falls back
to `init_dataset_item`. If a future change introduced a partial or lazily-filled
dataset with a compute-on-miss path, the key would become load-bearing again and
the rotation test would silently weaken. That is a constraint on future work, not
a defect now.

**Small doc gap:** `ShareVerifier.key` is still stored and refreshed by `rekey`,
and it is genuinely inert — `new_full` needs *a* key argument but ignores it in
full mode. The field's comment does not say so, so a reader who has absorbed
"keyed to the dataset" will wonder why a key is tracked at all. One line would
settle it.

### R10-VC6 — R9-F3 is closed properly.
`#[should_panic(expected = "Argon2d cache; got an empty one")]` now spans the
`\`-join in the literal — the exact point where the 14 spaces were injected. A
recurrence of the scripted-edit accident would break this match, which the old
prefix could not. The comment above it explains why the substring is chosen that
way, so a future edit is less likely to shorten it back.

### R10-VC7 — the AUDIT describes the `is_armed`/`is_enabled` split accurately, with one omission.
The entry states the problem, the mechanism and the fix correctly, and it
retracts the earlier "no behaviour change" claim explicitly. What it does not
say is that `SubmitVerifierUnavailable` is *still* unreachable from
`worker_loop` for an unrelated reason (R10-F1) — the entry reads as though the
defence is now live. Adding a clause such as "reachable at the `classify_share`
boundary; still unreachable in `worker_loop` today because `rekey` always
precedes the first hash" would make it exact.

### R10-VC8 — the claimed state is real, on the right source.
```
git diff 6f2b95b..HEAD -- src/ benches/ Cargo.toml Makefile   -> empty
running 126 tests
test result: ok. 124 passed; 0 failed; 2 ignored   (lib, release, 92.54s)
test result: ok. 7 passed; 0 failed                (bin)
clippy --all-targets -- -D warnings                          clean (aarch64)
clippy --all-targets --target x86_64-apple-darwin -- -D warnings   clean
```
124 + 7 as claimed. The lib count is unchanged from round 9 because the two
`ShareVerifier` tests were already compiled on aarch64; ungating them adds two
to an x86_64 run, which is the point.

## Round 10 verdict

**Blockers: none.**

**Major: R10-F2** — an empty value now erases an explicit `off`, silently
re-enabling the native loop. Introduced by this commit, demonstrated on the
shipped binary, and fixable in two lines.

**Minor: R10-F1** — `SubmitVerifierUnavailable` is reachable again at the
`classify_share` boundary but still unreachable inside `worker_loop`; the fix is
right, the surrounding claims slightly overstate it.

**Six of the seven round-9 minors are cleanly closed** (R9-F2, F3, F4, F5, F6,
F7); R9-F1's fix is correct but incomplete in the way R10-F1 describes.

**Your four questions:**
1. **Did you move the problem?** No — you restored the *independence* of
   `classify_share`'s two arguments, which is the right fix and which
   `is_armed()` had genuinely destroyed. But the branch still cannot fire today,
   because `vm` is assigned only inside the block that calls `rekey`, so
   `vm.is_some()` implies a dataset exists. The defence is now real for future
   edits, not for the current binary; the comment and AUDIT should say so. A
   composition test (`ShareVerifier::new(true)` with no `rekey`, then
   `classify_share(v.is_enabled(), .., v.reference(..).as_ref())`) would pin it
   for microseconds and no dataset.
2. **Did you invert R7-F2?** **No.** That finding was about `--verify-shares`,
   and an empty `MINERTIM_VERIFY_SHARES=` still leaves the safety net on —
   measured. Your reading was right. The problem is elsewhere: `as_bool` can now
   return `None`, but round 7 had already replaced `.or(value)` with a bare
   assignment on the flag arms, so an empty token clears an earlier decision
   instead of declining to make one. That is R10-F2.
3. **Dataset hoist:** clean. Same key, same construction, the module-local
   `static` deleted rather than duplicated, exactly two `LazyLock`s remain, and
   the differential tests run on byte-identical data.
4. **`vm_is_on_reference_path()` on x86_64:** meaningful, not vacuous.
   `use_native_loop` is an ungated field and `set_native_loop` an ungated setter,
   so the assertion passes for the right reason *and* still fails if the guarded
   line is dropped. Since CI can never run the JIT, this is now one of the few
   native-loop regressions CI can catch — ungating was right.

**Mergeable: yes.** One caveat, and it is about sequencing rather than severity:
R10-F2 is a regression *introduced by this commit*, is operator-facing, and is a
two-line fix. I would land that fix before merging rather than after — not
because it endangers a share (it cannot), but because shipping a switch that
silently ignores an explicit `off` is the kind of thing that is much cheaper to
correct now than to explain later.

## Remaining work if this review is interrupted
- **Round 10 is complete.** All four priorities answered, both lower-priority
  items checked, one major and one minor filed, full suite and both clippy
  targets run against `6f2b95b`'s source.
- Order I would take them: **R10-F2** (restore `.or(value)` on both flag arms and
  warn on an empty value), then **R10-F1** (the composition test, plus a clause
  in the comment/AUDIT noting the `worker_loop` invariant), then the one-line
  doc note that `ShareVerifier.key` is inert in full mode.
- Unchanged from earlier rounds and still open by choice: R5-F2, R5-F4, R5-F6,
  issue #1 (R5-F7), issue #2 (ARM64 CI). Issue #2 remains the only one I would
  not leave open indefinitely.

---

# Round 11 — `6f2b95b..309cfda`
One commit fixing R10-F2 and R10-F1.

## Round 11 brief
**Scope:** `git diff 6f2b95b..309cfda` — one commit, fixing R10-F2 and R10-F1.

R10-F2 was a regression introduced while fixing round 9: `as_bool` returning
`None` for an empty value was the right semantics, but round 7 had replaced
`value = as_bool(v).or(value)` with a bare assignment, safe only while `as_bool`
could never return `None`. Result: an empty token *erased* a previously resolved
setting — `--native-loop off --native-loop ""` gave `on`, silently. The fix
restores `.or(value)` on both flag arms and warns on an empty value.

**Questions to attack:**
1. **Is the `.or(value)` composition right in every order?** Both arms now read
   `value = as_bool(v).or(warn_if_empty(flag, v).and(value))`. Check precedence
   rather than trusting it: can an empty value still erase an earlier one via
   the `=` form; does `warn_if_empty` returning `Some(())` unconditionally do
   what is intended; can the warning fire on a non-empty value or fail to fire
   on an empty one?
2. **Does last-flag-wins still hold?** `--native-loop off --native-loop on` must
   give `on`. There is a test, but an `.or` chain is exactly the change that
   could turn "last wins" into "first non-empty wins" unnoticed.
3. **R10-F1's composition test** (`an_enabled_but_unfed_verifier_fails_open`) —
   does it pin what round 10 meant, and is the AUDIT wording (the arm is a guard
   for future edits, not live in the current binary) accurate rather than
   overclaiming?
4. **The framing condition** from round 10 is now in `ShareVerifier::rekey`'s
   doc: "keyed to the dataset" holds only while the full/light split stays
   absolute. Is it stated where a future author would encounter it?

Lower priority: is the new warning text useful mid-incident, and are the
empty-value semantics documented anywhere an operator would look?
(`mining.conf.example` currently says nothing about blank values.)

**State at brief time:** 125 lib + 8 bin tests pass in release; clippy clean on
aarch64 and x86_64.

## Round 11 coverage ledger
| Area | Status | Notes |
|---|---|---|
| P1 — `.or(value)` composition in every order | DONE | R11-VC1: correct; R11-F1 on the env arm |
| P2 — last-flag-wins still holds? | DONE | R11-VC2: holds, verified 4 orders |
| P3 — R10-F1 composition test + AUDIT wording | DONE | R11-VC3/VC4; R11-F2 on the code comments |
| P4 — framing condition placement | DONE | R11-F3: at the claim, not at the break site |
| Warning text; blank-value docs | DONE | R11-VC5; R11-F4 |

## Round 11 findings

### R11-VC1 — P1: the `.or(value)` composition is correct in every order I could construct, including the `=` form.
I did not reason about the precedence; I ran it. Twelve cases against a freshly
built release binary, reporting the resolved state and how many `warning:` lines
were printed:

```
A  env=''                          loop=ON   warnings=0
B  --native-loop ''                loop=ON   warnings=1
C  env=off + --native-loop ''      loop=OFF  warnings=1   <- R10-F2 case C, fixed
D  --native-loop off then ''       loop=OFF  warnings=1   <- R10-F2 case D, fixed
D2 --native-loop=off then =        loop=OFF  warnings=1   <- the `=` form, fixed
E  VERIFY_SHARES=''                loop=ON   warnings=0
F  env=off (control)               loop=OFF  warnings=0
G  off then on                     loop=ON   warnings=0
H  on then off                     loop=OFF  warnings=0
I  =on then =off                   loop=OFF  warnings=0
J  '' then explicit off            loop=OFF  warnings=1
K  env=on + flag off               loop=OFF  warnings=0
```

Answering your three sub-questions from the evidence:
- **Can an empty value still erase an earlier one through the `=` form?** No —
  D2 is the `=` form and the explicit `off` survives. Both arms carry the same
  `.or(...)`, so neither is privileged.
- **Does `warn_if_empty` returning `Some(())` unconditionally do what you
  intend?** Yes. `Option::or`'s argument is eager, so `warn_if_empty` is
  evaluated on *every* value — but it only prints when `v.trim().is_empty()`,
  and the unconditional `Some(())` is what makes `.and(value)` hand back the
  previously resolved value rather than `None`. The `Option<()>` is purely a
  sequencing device for the side effect. It works, though it is doing two
  unrelated jobs in one expression; a reader will have to think about `.and` to
  see that it is a pass-through. That is a style observation, not a defect.
- **Can the warning fire on a non-empty value, or fail to fire on an empty
  one?** Not on a non-empty one: G, H, I and K all pass explicit values and
  print zero warnings. It fires on every empty *flag* value: B, C, D, D2, J each
  print exactly one. The one place it does not fire is the environment arm —
  see R11-F1.

### R11-VC2 — P2: last-flag-wins survives the `.or` chain.
This was the right thing to be suspicious of, and it holds:
```
G  --native-loop off --native-loop on     -> ON
H  --native-loop on  --native-loop off    -> OFF
I  --native-loop=on  --native-loop=off    -> OFF
K  env=on, --native-loop off              -> OFF   (flag still beats env)
```
The reason it cannot degrade into "first non-empty wins" is structural:
`as_bool(v)` is the **left** operand of the `.or`, so any parseable later value
short-circuits and overwrites. Only an *empty* later value defers to the earlier
one — which is precisely the intended semantics. J (`"" ` then `off`) confirms
the reverse order also behaves: an empty first token does not poison a later
explicit setting.

### R11-F1 — The environment arm still has no empty-value warning, so `MINERTIM_NATIVE_LOOP=` remains silent  [MINOR]
**Where:** `src/bin/minertim.rs` — `let mut value = env_value.and_then(as_bool);`
**Claim:** R10-F2 had two halves: (a) an empty value must not erase an explicit
one, and (b) it should warn rather than being silent. **(a) is fully fixed**,
including the `=` form. **(b) is fixed only for the flag arms.** The environment
arm calls `as_bool` directly with no `warn_if_empty`, so an empty environment
value is discarded without a word — cases **A** and **E** above, zero warnings.
**Failure scenario:** the same shape as my round-10 case A, unchanged: a wrapper
writing `MINERTIM_NATIVE_LOOP="$NL"` with `$NL` unset gets the native loop **on**
with no diagnostic, while the operator believes they disabled it. It is strictly
less serious than R10-F2 was, because the env arm is resolved first and so has
nothing to erase — the value simply falls through to the default. And the
default is the faster, correct-hash path, with share verification still on.
**Why I am still raising it:** the asymmetry is hard to justify from the
operator's side. `--native-loop "$NL"` and `MINERTIM_NATIVE_LOOP="$NL"` with an
unset `$NL` arise from the identical shell idiom, and one warns while the other
does not. One line — `.or(warn_if_empty(flag, v).and(None))` on the env arm, or
simply calling `warn_if_empty` before `and_then` — closes it.
**Confidence:** HIGH — measured, and the code path has no call to
`warn_if_empty`.

### R11-VC3 — P3: `an_enabled_but_unfed_verifier_fails_open` pins exactly what I asked for.
It is the composition test from R10-F1, built the way I described: a
`ShareVerifier::new(true)` with **no** `rekey`, `reference()` asserted `None`,
then `classify_share(v.is_enabled(), &A, reference.as_ref())` asserted to be
`SubmitVerifierUnavailable` *and* to `should_submit()`. It goes one better than
my sketch by also asserting `is_enabled()` **and** `!is_armed()` on the same
value, which pins the distinction between the two predicates — the thing that
actually regressed in round 9. No dataset, microseconds. Good.

**Scope note, not a finding:** it pins the composition, not the *call site*. If
`worker_loop` were changed back to `classify_share(verifier.is_armed(), ..)`,
this test would still pass, because it calls `is_enabled()` itself. That is the
same standing limitation as R7-Q1/R9-F7 — `worker_loop` cannot be instantiated —
and it is on the deferred list. If you ever want it closed cheaply, the same move
that worked for `classify_share` works here: extract
`fn verdict_for(v: &mut ShareVerifier, blob: &[u8], hash: &[u8; 32]) -> ShareVerdict`
containing the two lines, have `worker_loop` call it, and point the test at that
instead.

### R11-VC4 — the AUDIT wording for R10-F1 is now accurate and does not overclaim.
> *"`SubmitVerifierUnavailable` is still unreachable in `worker_loop` for an
> unrelated reason: `vm` is assigned only inside the block that calls `rekey`,
> so `vm.is_some()` implies a dataset exists. The AUDIT and comments read as
> though the arm were live. It is not — it is a guard against future edits,
> which is worth having but should be described honestly."*

That is exactly right, including the mechanism. It states the limitation rather
than softening it, and it does not claim the composition test made the arm live.

### R11-F2 — The honest description landed in AUDIT.md but not in the two code comments that make the overclaim  [MINOR]
**Where:** `src/miner.rs:672-674` (the `worker_loop` call site) and the
`is_enabled` doc comment
**Claim:** R10-F1 asked for the *comment and* the AUDIT to say the arm is not
live today. The AUDIT was corrected (R11-VC4). The two code comments were not:
```rust
// `is_enabled`, not `is_armed`: the distinction is what keeps the
// "verification wanted but unavailable" case reachable so it can
// fail open loudly rather than being folded into "not verified".
```
and, on `is_enabled`, *"a defence that cannot be reached is not a defence"* —
both of which read as a statement about the current binary. Neither mentions the
`worker_loop` invariant (`vm.is_some()` implies `rekey` has run) that keeps the
arm unreachable.
**Failure scenario:** documentation only. But note this is the *inverse* of the
concern you raised in P4: a future author editing `worker_loop` reads the comment
three lines above the call, not an AUDIT entry from September. The place the
correction is most needed is the place it did not land. A clause such as "—
reachable at this boundary; not reachable in this function today, because
`vm.is_some()` implies `rekey` has run" would settle it.
**Confidence:** HIGH — read both comments in the current tree.

### R11-F3 — P4: the framing condition is stated next to the *claim*, but not at the site that would *break* it  [MINOR]
**Where:** `src/miner.rs:400-406` (`ShareVerifier::rekey` doc) — and, by
omission, `src/randomx/vm.rs:1319-1321` and the rotation test.
**Claim:** You asked whether the condition is somewhere a future author would
actually encounter it. Partly. `grep -n "full/light\|compute-on-miss\|lazily-filled\|load-bearing"`
across `src/` returns exactly **three lines, all in `ShareVerifier::rekey`**.
That is the right place for the *claim* ("keyed to the dataset") — the caveat sits
directly under it, which is good. But it is the wrong place for the *warning*,
because it is not where the breaking change would be made.

An author introducing a lazily-filled or partial dataset with a compute-on-miss
path would be editing:
- `vm.rs:1319-1321`, the single `match dataset` on the hash path, whose `None`
  arm *is* the fallback in question. Its comment reads only
  `// Full mode: array lookup. Light mode: compute on-the-fly.` — nothing about
  anything depending on that dichotomy staying absolute.
- `RandomXVm::new_full_versioned`, whose comment does state the invariant
  (*"`cache_memory` is read in exactly one place — `init_dataset_item` on the
  `dataset == None` arm"*) but not that a downstream test's validity rests on it.

Neither points back to `ShareVerifier`. Someone working in `randomx/` has no
reason to open `miner.rs`.
**Also:** the rotation test — the thing the caveat says would "silently weaken" —
carries no note of its own. Its comment explains *that* the key does not affect
the hash, not that this is a conditional property. A reader seeing it pass would
not know its premise could lapse.
**Failure scenario:** documentation only, but of the specific kind this caveat
exists to prevent: a silent weakening that no test reports. One line at
`vm.rs:1321` ("full mode must never take this arm — `ShareVerifier`'s rotation
test assumes the dataset is the only staleness vector") would put it in the path
of the person who would break it.
**Confidence:** HIGH — the grep is exhaustive over `src/`.

### R11-F4 — Empty-value semantics are documented nowhere an operator would look, and `mining.conf.example` is the wrong place to fix it  [MINOR]
**Where:** `--help` text; `mining.conf.example`
**Claim:** `grep -i "blank\|empty" mining.conf.example` returns nothing, and the
`--help` entries for `--native-loop` and `--verify-shares` say nothing about
empty values either. So the behaviour an operator can now trip — an empty value
is ignored, the previous setting or the default stands — exists only in the
source and in a warning they may not see (R11-F1: the environment arm prints
none).

**But note where the fix belongs, because your question named the wrong file.**
A blank in `mining.conf` is *inert*: it is a Makefile variable, and
`$(if $(NATIVE_LOOP),--native-loop $(NATIVE_LOOP),)` suppresses the flag
entirely — the same fact you accepted in round 10. So documenting blank-value
semantics in `mining.conf.example` would describe a case that file cannot
produce, and would arguably mislead. The semantics only matter for the two
routes that *can* carry an empty string: `MINERTIM_NATIVE_LOOP=` /
`MINERTIM_VERIFY_SHARES=` in the environment, and `--native-loop ""` on the
command line. `--help` is where both are already described, and is where a
sentence belongs — something like "an empty value is ignored; the previous
setting or the default applies".
**Failure scenario:** documentation only.
**Confidence:** HIGH.

### R11-VC5 — the warning text is good on *what* and *why*, and silent on *what now*.
```
warning: --native-loop given an empty value - ignoring it; the previous
setting or the default applies. Use on|off.
```
Mid-incident this gets the important things right: it names the flag, says the
value was discarded rather than misparsed, and tells the operator the accepted
spellings. Compared with the excellent `Native-loop JIT DISABLED` message (state
+ cost + how to undo), the gap is that it stops one step short of the question
the operator actually has — *is the native loop on or off right now?* "The
previous setting or the default applies" asks them to work it out.

It cannot name the outcome at that point, correctly: a later flag may still
override, so the resolved value is not yet known inside the argument loop. The
cheap fix is at the other end — the startup path already logs loudly when the
switch resolves **off** and says nothing when it resolves **on**, so the state is
inferable only from the *absence* of a line. One unconditional
`log::info!("Native-loop JIT: {on|off}")` at startup would mean no operator ever
has to infer it, and would compose well with this warning. Not a defect; a
suggestion.
