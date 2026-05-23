# RandomX v2 Implementation Plan

**Created:** 2026-04-11  
**Status:** Planning — do not start implementation until activation height is confirmed  
**References:**
- tevador/RandomX#265 — commitment function added
- xmrig/xmrig#3769 — RandomX v2 initial support (algo `rx/2`, program size 256→384, AES tweak)
- xmrig/xmrig#3775 — Stratum `commitment` field added to submit
- monero-project/monero#10038 — Monero v17 consensus changes

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
- **V2:** AES encrypt/decrypt cycles on `f`/`e` registers using the e_mask keys:
  ```
  f[0] = aesenc(f[0], e_key[i]);  f[1] = aesdec(f[1], e_key[i]);
  f[2] = aesenc(f[2], e_key[i]);  f[3] = aesdec(f[3], e_key[i]);
  ```
  (Exact key derivation needs verification against reference source)

### 3. Commitment calculation (new)
After computing the RandomX hash, compute:
```
commitment = blake2b_256(input_blob || randomx_hash)
```
- **The commitment, not the hash, is compared to the mining target**
- The full RandomX hash is still computed (no shortcut) but only used as input to Blake2b
- This enables lightweight share verification by pools using only Blake2b

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
- `result` still carries the full RandomX hash
- `commitment` is the new field (only sent for v17+ jobs)
- Difficulty comparison at the pool uses `commitment`, not `result`

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
5. `submit_share` receives the optional commitment.

**Note:** The pipelined hashing (`calculate_hash_pipelined`) needs to continue working — commitment is computed *after* the hash, so pipeline is unaffected.

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

| # | Question | Where to find answer |
|---|---|---|
| 1 | What are the exact V2 test vectors? | xmrig#3769 benchmark data or RandomX test suite |
| 2 | Does login need `"algo": "rx/2"` or does `"rx/0"` still work for negotiation? | SChernykh/p2pool or pool operator docs |
| 3 | Exact AES tweak key schedule — which keys, which round order? | tevador/RandomX vm_interpreted.cpp diff |
| 4 | Is CFROUND tweak (Tweak_V2_CFROUND) a real change to our CFROUND instruction handling? | RandomX source, not yet confirmed |
| 5 | Monero v17 hard fork activation height / date | monero-project/monero#10038 or community announcements |
| 6 | Do any pools already support `"commitment"` field, or is it future-only? | Pool operator announcements |

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
