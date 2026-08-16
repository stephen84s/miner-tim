# RandomX v2 Implementation Plan

**Created:** 2026-04-11  
**Last reviewed:** 2026-08-16  
**Status:** ✅ **Offline-verifiable half IMPLEMENTED 2026-08-15/16** (phases 1-4, 6, 7:
version plumbing, VM v2 semantics, commitment, JIT v2, vectors — all reference
vectors green on interpreter and JIT paths; v1 bit-identical). **Remaining:
dispatch + Stratum (phase 5)** — deliberately deferred until Monero schedules
HF v17 and a pool-side reference exists; see AUDIT.md 2026-08-16 for the exact
fork-day punch list.  
**References:**
- tevador/RandomX#265 — commitment function added
- tevador/RandomX#274 → concluded in #317 (Jan 2026) — the VM changes themselves
- xmrig/xmrig#3769 — RandomX v2 initial support (algo `rx/2`, program size 256→384, AES tweak)
- xmrig/xmrig#3775 — Stratum `commitment` field added to submit
- monero-project/monero#10038 — Monero v17 consensus changes

---

## Status review — 2026-08-15

> **AUTHORITATIVE SEMANTICS: see `RANDOMX_V2_SEMANTICS.md`** (delivered
> 2026-08-15, quotes from merged tevador/xmrig sources, includes runnable test
> vectors). Where this plan and that doc disagree, **the doc wins**. Corrections
> already applied below are marked ⚠ CORRECTED; the two biggest:
> 1. **The plan's original Stratum field mapping was backwards.** xmrig submits
>    the *commitment* as `result` (it is what's compared to the target), and puts
>    the *raw RandomX hash* in the new `commitment` field — see §5.2/§8 of the doc.
> 2. **There is a fifth consensus change the plan missed:** dataset prefetch
>    (`mp` aliases `ma`; prefetch runs 2 iterations ahead of the read) — doc §4.
>    This touches our `execute_vm` loop (`vm.rs:1143-1168`), not just program
>    execution.

**Upstream is ready; Monero is not. The "wait" verdict stands.**

- **xmrig shipped full v2 support in v6.26.0 (2026-03-28)** — PRs #3769, #3772,
  #3774, #3775, #3776, #3782, #3783, plus ARM64 follow-up #3778. So the reference
  implementation to translate from now exists in full.
- **Monero mainnet is still on hard-fork version 16.** Verified two ways on
  2026-08-15: `xmrchain.net/api/networkinfo` reports `current_hf_version: 16` at
  height 3,739,507, and `monero-project/monero` master's `mainnet_hard_forks`
  table still ends at `{ 16, 2689608, 0, 1656629118 }` — **no v17 entry, no
  scheduled height**.
- Beware secondary sources: several web articles claim FCMP++/RandomX v2 activated
  in Q1 2026. The consensus code contradicts them. Trust the fork table.

Two of the open questions below are now answered by tevador/RandomX#274:
- **CFROUND *is* tweaked** (was OQ #4, previously "not yet confirmed"): it becomes
  *conditional*, writing `fprc` with probability 1/16 rather than every time.
- **The F/E mixing change** (OQ #3) is "mix F and E registers with AES instead of
  XOR", which doubles AES work per hash without hurting hashrate by using cycles
  otherwise lost to scratchpad stalls. Exact key schedule still needs reading off
  the reference source before implementing.

Note the upstream design intent: v2's gains come from *hiding scratchpad latency*.
Worth remembering when we port the JIT side — see AUDIT.md (2026-08-15) for why
dependency-chain depth, not instruction count, is the thing to protect in
`emit_mem_addr`-style changes, and for why this repo's benchmark cannot resolve
differences below ~3%.

---

## What Changes in RandomX v2

### 1. Program size: 256 → 384 instructions
The RandomX VM now executes 384-instruction programs instead of 256.
- `RANDOMX_PROGRAM_SIZE_V2 = 384` (V1 was 256)
- `RANDOMX_PROGRAM_ITERATIONS` and `RANDOMX_PROGRAM_COUNT` stay at 2048 and 8
- `PROGRAM_BYTES_SIZE` grows: `16 * 8 + 384 * 8 = 3200 bytes` (was 2176)

### 2. AES tweak (Tweak_V2_AES)
After reading `f` and `e` register pairs from the scratchpad, the final combination step changes:
- **V1:** `f[i] = XOR(f[i], e[i])`
- **V2:** ⚠ CORRECTED — single AES rounds on the `f` registers, keyed by the
  **live `e` registers themselves** (bitcast to 128-bit, no derivation at all):
  round `i` uses `e[i]` as the key; `f[0]`/`f[2]` use aesenc, `f[1]`/`f[3]` use
  aesdec. Exact code + ARM64 factorisation (`AESE zero → AESMC → EOR key`) in
  `RANDOMX_V2_SEMANTICS.md` §3.

### 3. Commitment calculation (new)
After computing the RandomX hash, compute:
```
commitment = blake2b_256(input_blob || randomx_hash)
```
- **The commitment, not the hash, is compared to the mining target**
- The full RandomX hash is still computed (no shortcut) but only used as input to Blake2b
- This enables lightweight share verification by pools using only Blake2b

### 3b. Conditional CFROUND (⚠ added 2026-08-15 — was open question #4)
`isrc = rotr(src, imm & 63)`; the rounding mode is written **only if**
`(isrc & 60) == 0` (bits 2–5 of the *rotated* value all zero → 1/16 chance),
then `set_rounding(isrc % 4)` as in v1. Affects interpreter CFROUND and
`emit_cfround`. Doc §2.

### 3c. Dataset prefetch two iterations ahead (⚠ added 2026-08-15 — missed by
### this plan originally)
`mp` aliases `ma` instead of `mx`; `spMix2` XORs into `ma`; prefetch issues for
the *new* `ma` so it runs 2 iterations ahead of the read. Touches our
`execute_vm` outer loop (`vm.rs:1143-1168`), independent of program execution.
Doc §4.

### 4. Activation: Monero hard fork version 17
- Blob byte 0 (major version) encodes the HF version
- `HF_VERSION_RANDOMX_V2 = 17` → use 384-instruction programs + AES tweak
- `HF_VERSION_POW_COMMITMENT = 17` → use commitment for difficulty comparison
- Activation block height: **TBD** (not announced as of 2026-04-11)

### 5. Stratum protocol changes
- Login algo: `"rx/2"` for v17+ blocks (currently `"rx/0"`)
- Submit params add optional `"commitment"` field:
  ```json
  {
    "id": "<session>",
    "job_id": "<id>",
    "nonce": "<4-byte hex>",
    "result": "<32-byte randomx hash hex>",
    "commitment": "<32-byte blake2b commitment hex>"
  }
  ```
- ⚠ CORRECTED (was backwards): **`result` carries the commitment** — the value
  compared against the target — and **`commitment` carries the raw RandomX
  hash**. Per xmrig v6.26.0 `CpuWorker.cpp`: it overwrites `m_hash` with the
  commitment (which then flows through the existing result/target path) and
  saves the raw hash into `m_commitment` for the new field.
- Caveat: no pool-side reference exists yet (p2pool hasn't implemented rx/2),
  so this is xmrig's interpretation — re-verify against a real pool before ship.

---

## Implementation Phases

### Phase 1 — VM: version-aware program size

**Files:** `randomx/vm.rs`

1. Rename `RANDOMX_PROGRAM_SIZE` → `RANDOMX_PROGRAM_SIZE_V1 = 256`, add `RANDOMX_PROGRAM_SIZE_V2 = 384`. Export a `pub(crate) const` or use an enum/flag.
2. Add `RxVersion` enum: `V1` | `V2`.
3. `RandomXVm` gains a `version: RxVersion` field.
4. `PROGRAM_BYTES_SIZE` becomes runtime-computed or two separate constants.
5. The `bytecode` buffer in `execute_program()` must handle 384 entries — either a `Box<[BytecodeInstruction; 384]>` for V2 or a `Vec`.
6. `compile_program` and `execute_bytecode` iterate up to `program_size` (not a compile-time const).
7. Factory methods:
   - `RandomXVm::new(key)` → V1 (unchanged behaviour)
   - `RandomXVm::new_v2(key)` → V2
   - (or auto-detect from the miner based on major_version)

**Risk:** `RANDOMX_PROGRAM_SIZE` is currently used as a const generic in several places (array sizes, JIT `offsets` array). See JIT phase below.

### Phase 2 — VM: AES tweak

**Files:** `randomx/vm.rs`, `randomx/soft_aes.rs`

After reading e-registers from scratchpad (in `execute_vm`, the `Estore` / `Fswap` path), add a V2 branch:
```rust
if version == RxVersion::V2 {
    // AES-based combination of f[i] and e_key[i]
    // (aesenc/aesdec alternating, using soft_aes primitives)
} else {
    // V1: f[i] ^= e[i]
}
```

The exact register/key schedule needs cross-checking against the C++ reference before implementing. The `soft_aes` module already has the primitives.

### Phase 3 — VM: commitment function

**Files:** `randomx/vm.rs`

Add function (3–5 lines):
```rust
pub fn calculate_commitment(input: &[u8], rx_hash: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(input.len() + 32);
    buf.extend_from_slice(input);
    buf.extend_from_slice(rx_hash);
    blake2b::blake2b_256(&buf)
}
```

Known test vector (from tevador/RandomX#265 tests.cpp):
- Input + hash → `d53ccf348b75291b7be76f0a7ac8208bbced734b912f6fca60539ab6f86be919`

### Phase 4 — Miner loop: version detection + commitment comparison

**Files:** `miner.rs`

1. Extract major version from blob byte 0: `let major_version = job.blob[0];`
2. When initialising/reinitialising VM: if `major_version >= 17`, use V2 VM.
3. In the hash comparison block:
   ```rust
   let (check_hash, commitment_opt) = if major_version >= 17 {
       let commitment = calculate_commitment(&job_blob_current[..76], &hash);
       (commitment, Some(commitment))
   } else {
       (hash, None)
   };
   ```
4. `meets_target` receives `&check_hash` (commitment for v17+, hash otherwise).
5. `submit_share` sends commitment as `result` and the raw hash as `commitment`
   (⚠ corrected mapping — see Phase 5).

**Note (⚠ sharpened):** with pipelined hashing, the hash returned by
`calculate_hash_pipelined(next_blob)` belongs to the *previous* input — so the
commitment must be computed over **`job_blob_current`** (the blob whose nonce
was just hashed), not the blob being fed in. xmrig keeps a `prev_job` buffer
for exactly this; our `job_blob_current`/`job_blob_next` pair in
`miner.rs::worker_loop` already provides it. Commitment itself is computed
after the hash, so the pipeline structure is otherwise unaffected.

### Phase 5 — Pool: Stratum submit + login

**Files:** `pool_connection.rs`

1. Update `submit_share` signature:
   ```rust
   pub fn submit_share(
       &self,
       job_id: &str,
       nonce: &str,
       result: &str,
       commitment: Option<&str>,
   ) -> Result<(), String>
   ```
2. Build params conditionally:
   ```rust
   let mut params = json!({
       "id": sid, "job_id": job_id, "nonce": nonce, "result": result
   });
   if let Some(c) = commitment {
       params["commitment"] = json!(c);
   }
   ```
3. Login algo: send `"rx/0"` for now; pools should auto-detect from job major_version. **Verify** whether pools expect `"rx/2"` in the login `algo` field for v17 jobs before changing this (xmrig#3769 suggests `rx/2` is a separate algo identifier).

### Phase 6 — JIT compiler: v2 program size

**Files:** `randomx/jit/compiler.rs`, `randomx/jit/aarch64.rs`, `randomx/vm.rs`

The JIT currently uses `RANDOMX_PROGRAM_SIZE` (256) as a const to size the `offsets: [usize; 256]` array on the stack and bound-check branch targets.

Options (choose one before implementing):
- **A. Const generic:** `JitCompiler<const N: usize>`, `offsets: [usize; N]`. Clean but requires Rust const generics throughout.
- **B. Runtime N:** Replace `[usize; RANDOMX_PROGRAM_SIZE]` with `Vec<usize>` in `compile()`. Simple, minor heap alloc per program (negligible).
- **C. Max-size array:** `offsets: [usize; 384]`, always allocate for V2 max, use only `[..program_size]` slice. Zero-allocation, straightforward.

**Recommendation: Option C** (max-size array) — minimal diff, no generics complexity.

Additional JIT changes:
- Pass `program_size: usize` into `compile()` and `emit_*` helpers.
- `emit_cbranch` uses `program_size` instead of `RANDOMX_PROGRAM_SIZE` for bounds check.
- AES tweak (Phase 2) if implemented in JIT: need ARM64 AES instructions (`AESE`/`AESD`). The `memory.rs` MAP_JIT path already handles code generation; new AES emit helpers needed in `aarch64.rs`.

### Phase 7 — Tests

**Files:** `randomx/tests.rs`

1. `test_commitment_calculation` — known vector from RandomX#265:
   - Input: TBD (need exact input bytes from test)
   - Expected commitment: `d53ccf348b75291b7be76f0a7ac8208bbced734b912f6fca60539ab6f86be919`
2. `test_full_hash_v2_a` — V2 hash with known test vector (once xmrig#3769 publishes vectors).
3. `test_vm_calculate_hash_jit_v2` — JIT test for 384-instruction programs (aarch64 only).
4. All existing V1 tests must continue to pass (no regression).

---

## Open Questions (resolve before implementing)

| # | Question | Status (2026-08-15) |
|---|---|---|
| 1 | What are the exact V2 test vectors? | **Available** — xmrig#3769 added RX_V2 vectors (10K–10M iterations); lift them from its test suite |
| 2 | Does login need `"algo": "rx/2"` or does `"rx/0"` still work for negotiation? | **Open** — check SChernykh/p2pool or pool operator docs |
| 3 | Exact AES tweak key schedule — which keys, which round order? | **Partly answered** — tevador/RandomX#274: F and E registers mixed with AES instead of XOR. Exact key/round order still to be read off #317's merged source |
| 4 | Is CFROUND tweak (Tweak_V2_CFROUND) a real change to our CFROUND instruction handling? | **Answered: yes** — tevador/RandomX#274 makes CFROUND conditional, writing `fprc` with probability 1/16 instead of always. Our `emit_cfround` / interpreter CFROUND both need a V2 branch |
| 5 | Monero v17 hard fork activation height / date | **Still unannounced** — mainnet is on HF 16; `mainnet_hard_forks` has no v17 entry. **This is the blocker** |
| 6 | Do any pools already support `"commitment"` field, or is it future-only? | **Open** — future-only in practice while mainnet is HF 16 |

---

## Scope Summary

| Phase | Effort | Risk |
|---|---|---|
| 1 — Program size | Medium (many const usages) | Medium — JIT `offsets` array |
| 2 — AES tweak | Small–Medium | Low (soft_aes exists) |
| 3 — Commitment fn | Tiny | None |
| 4 — Miner loop | Small | Low |
| 5 — Stratum submit | Small | Low |
| 6 — JIT v2 | Small (Option C) | Low |
| 7 — Tests | Small | None |

Total: ~2–3 days of focused implementation work, excluding AES tweak key schedule research.
