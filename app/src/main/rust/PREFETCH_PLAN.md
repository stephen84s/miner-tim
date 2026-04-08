# Scratchpad & Dataset Prefetch for RandomX

## Context

At 12 threads the miner hits 2300 H/s (~192 H/s/thread) vs 282 H/s/thread at 4 threads — a memory bandwidth wall. RandomX is memory-hard: each of the 2048 iterations per program reads/writes 128 bytes from a 2 MiB scratchpad and reads 64 bytes from the 2 GiB dataset. Prefetching hides memory latency by issuing loads before the data is needed. XMRig uses `PRFM PLDL1KEEP` extensively.

## Two Prefetch Sites

### Site 1: Outer loop — scratchpad prefetch (HIGH impact)

**Where:** `vm.rs` lines 1101-1181, the 2048-iteration loop in `execute_vm_inner`.

Each iteration reads 64 bytes from `scratchpad[sp_addr0..]` (r-register load, line 1110-1112) and 64 bytes from `scratchpad[sp_addr1..]` (f/e-register load, lines 1115-1128). The addresses `sp_addr0`/`sp_addr1` are computed at lines 1103-1107.

**Opportunity:** After the store phase at the END of iteration N (lines 1164-1177), we can compute the NEXT iteration's `sp_addr0`/`sp_addr1` and prefetch those cache lines. The entire bytecode execution (256 instructions) runs between the prefetch and the actual load — thousands of cycles of latency hiding.

**Implementation:** This happens in Rust (outer loop), not in JIT. Add `std::arch::aarch64::_prefetch()` or inline assembly `PRFM PLDL1KEEP, [addr]` calls. Prefetch 2 cache lines (128 bytes total) for each of sp_addr0 and sp_addr1.

### Site 2: Outer loop — dataset prefetch (HIGH impact)

**Where:** `vm.rs` lines 1144-1159, dataset read.

Each iteration reads a 64-byte dataset item at `dataset_offset + (mem_ma & CACHE_LINE_ALIGN_MASK)`. After the swap at line 1162, the NEXT iteration's `mem_ma` is the current `mem_mx`. But `mem_mx` is only updated at line 1149 (after bytecode execution), so we can't prefetch until after that point.

**Opportunity:** After updating `mem_mx` at line 1149, we know the NEXT `mem_ma` (it's the current `mem_mx` after the swap). We can prefetch the next dataset line right after the swap. The remainder of the iteration (register stores, f-register XOR, scratchpad stores) plus the NEXT iteration's scratchpad loads and bytecode execution all run before the dataset is needed again.

**Implementation:** After the `std::mem::swap` at line 1162, compute the next dataset address and prefetch it.

## Changes

### File 1: `app/src/main/rust/src/randomx/vm.rs`

Add prefetch calls to the 2048-iteration loop. The loop body becomes:

```rust
for _ic in 0..RANDOMX_PROGRAM_ITERATIONS {
    // --- existing: compute sp_addr0/sp_addr1, load registers ---
    // --- existing: execute bytecode (JIT or interpreter) ---
    // --- existing: dataset read, mem_mx update, swap ---

    // NEW: prefetch next dataset line (mem_ma is now what was mem_mx)
    #[cfg(target_arch = "aarch64")]
    {
        let next_dataset_ptr = dataset_offset + (mem_ma as u64 & CACHE_LINE_ALIGN_MASK as u64);
        unsafe {
            let addr = match dataset {
                Some(ds) => ds.as_ptr().add(next_dataset_ptr as usize),
                None => std::ptr::null(),  // skip prefetch in light mode
            };
            if !addr.is_null() {
                std::arch::aarch64::_prefetch(addr as *const i8, _PREFETCH_READ, _PREFETCH_LOCALITY3);
            }
        }
    }

    // --- existing: store r-registers, XOR f/e, store f-registers ---

    // NEW: prefetch next iteration's scratchpad addresses
    // (sp_addr0/sp_addr1 are reset to 0 at end of iteration, then updated at start of next)
    // We can pre-compute: next_sp_mix = r[read_reg0] ^ r[read_reg1] (registers are final)
    #[cfg(target_arch = "aarch64")]
    {
        let next_sp_mix = nreg.r(config.read_reg0) ^ nreg.r(config.read_reg1);
        let next_sp_addr0 = (next_sp_mix as u32) & SCRATCHPAD_L3_MASK64;
        let next_sp_addr1 = ((next_sp_mix >> 32) as u32) & SCRATCHPAD_L3_MASK64;
        unsafe {
            let base = scratchpad.as_ptr();
            std::arch::aarch64::_prefetch(base.add(next_sp_addr0 as usize) as *const i8, _PREFETCH_READ, _PREFETCH_LOCALITY3);
            std::arch::aarch64::_prefetch(base.add(next_sp_addr0 as usize + 64) as *const i8, _PREFETCH_READ, _PREFETCH_LOCALITY3);
            std::arch::aarch64::_prefetch(base.add(next_sp_addr1 as usize) as *const i8, _PREFETCH_READ, _PREFETCH_LOCALITY3);
            std::arch::aarch64::_prefetch(base.add(next_sp_addr1 as usize + 64) as *const i8, _PREFETCH_READ, _PREFETCH_LOCALITY3);
        }
    }

    sp_addr0 = 0;
    sp_addr1 = 0;
}
```

**Important correctness note:** The scratchpad prefetch pre-computation works because at line 1179-1180, `sp_addr0` and `sp_addr1` are reset to 0, then at the START of the next iteration (lines 1103-1107) they are XORed with `sp_mix`. Since `0 ^ x == x`, the next addresses are simply `(sp_mix as u32) & MASK` and `((sp_mix >> 32) as u32) & MASK`. The r-registers used for sp_mix have already been XORed with the dataset line (line 1157-1158), so the prefetch placement after dataset XOR and register stores is correct.

### File 2: `app/src/main/rust/src/randomx/jit/compiler.rs` (optional, Phase 2)

Within the JIT bytecode, we could add prefetch for `_m` instructions by looking ahead to find the next memory instruction and speculatively computing its address. This is lower impact (L1 cache is fast, and the address computation + ALU takes only a few cycles before the load). Skip this for now — the outer loop prefetch is where the wins are.

### File 3: `app/src/main/rust/src/randomx/jit/aarch64.rs` (not needed for Phase 1)

No PRFM encoder needed since we're prefetching from Rust, not from JIT code. If we add intra-bytecode prefetch later (Phase 2), we'd add `prfm_reg()` to the encoder then.

## Verification

```bash
cd app/src/main/rust
cargo test --release --lib              # All 87 tests must pass (hashes unchanged)
cargo build --release --bin minertim    # Build CLI
./target/release/minertim <pool> <wallet> 4   # Compare 4-thread H/s (expect ~same or slight improvement)
./target/release/minertim <pool> <wallet> 12  # Compare 12-thread H/s (expect improvement from 2300)
```

Prefetch doesn't change correctness — only performance. If hashes change, something is wrong.
