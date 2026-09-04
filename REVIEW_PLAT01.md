# Independent review — PLAT-01 (`feat/jit-linux-aarch64`, issue #2 phase 1a)

Reviewer: independent agent, no prior context on this branch.
Base: `main` (`10b4546`). Head: `5ac5cb4`. Diff: 5 files, 362 insertions.
Code change is confined to `src/randomx/jit/memory.rs` (+`mod.rs` comments);
`AUDIT.md` / `CLAUDE.md` are prose.

## Coverage ledger

| # | Item | Status |
| :- | :- | :- |
| 1 | Darwin path genuinely unchanged? | **done — unchanged** |
| 2 | Linux path correct (constants, alignment, ordering, cache clear)? | in progress — constants + `__clear_cache` verified |
| 3 | In-place rewrite test — does it actually fail if cache maintenance is removed? | **done — guard is live on Linux** |
| 4 | Privatisation of `enable_write`/`enable_execute` | done |
| 5 | `compile_error!` + module gate | in progress |
| 6 | Implementer's disclosures | not started |
| R | Reproduce claimed test results (macOS + Linux aarch64) | not started |

## Findings

### F1 — Darwin arm is semantically unchanged. CONFIRMED. (informational)

Compared `git show main:src/randomx/jit/memory.rs` statement-by-statement against
the `#[cfg(target_os = "macos")] mod platform` arm on HEAD:

| Aspect | `main` | HEAD (Darwin arm) |
| :- | :- | :- |
| `mmap` prot | `PROT_READ\|PROT_WRITE\|PROT_EXEC` | identical |
| `mmap` flags | `MAP_ANON\|MAP_PRIVATE\|MAP_JIT` (0x1000/0x0002/0x0800) | identical values |
| fd / offset | `-1`, `0` | identical |
| failure test | `p == MAP_FAILED \|\| p.is_null()` | identical |
| error string | `"mmap MAP_JIT failed"` | identical |
| write enable | `pthread_jit_write_protect_np(0)` | identical |
| execute enable | `pthread_jit_write_protect_np(1)` **then** `sys_icache_invalidate(ptr, code_len)` | identical, same order |
| `write_code` body | `assert` → `enable_write` → `copy_nonoverlapping` → `code_len = byte_len` → `enable_execute` | **byte-identical** (only the two callees moved) |
| `Drop` | `munmap(ptr, size)` | identical |

The one thing that could have gone wrong silently — `code_len` being assigned
*after* `enable_execute`, which would shrink the `sys_icache_invalidate` range to
the previous program's length — did not: `write_code` is untouched, line for line
(`memory.rs:203-212` vs `main`'s equivalent). `size` is now also passed to the
Darwin callees but both ignore it (`_size`). No new allocation, no reordering, no
new syscall on the Darwin path.

Residual check: `grep -rn "MAP_JIT failed"` over `src/` and `benches/` finds only
the definition — nothing string-matches the error, so Linux returning a different
string (`"mmap PROT_READ|PROT_WRITE failed"`) breaks no caller.

**Verdict: the safety argument for the shipping platform holds.**

### F2 — Linux constants: verified against the container's own headers. (informational)

`gcc -E -dM` on `<sys/mman.h>` inside `rust:1.97.1` / linux/arm64:

```
MAP_ANONYMOUS 0x20     MAP_PRIVATE 0x02     MAP_DENYWRITE 0x00800
PROT_READ 0x1          PROT_WRITE 0x2       PROT_EXEC 0x4
MAP_FAILED ((void *) -1)
```

All six match the source exactly, and the code's claim that Darwin's `MAP_JIT`
bit (0x0800) is `MAP_DENYWRITE` on Linux is **true**. `getconf PAGESIZE` = 4096.

### F3 — `__clear_cache` resolves to the real libgcc implementation, not a stub. (informational — this was the main risk)

The concern: `__clear_cache` can be satisfied by a no-op, in which case the Linux
path would have *zero* cache maintenance and pass tests by luck. It does not.

```
$ nm -D target/release/deps/minertim-<hash>
                 U __clear_cache@GCC_3.0
$ ldd ... libgcc_s.so.1 => /lib/aarch64-linux-gnu/libgcc_s.so.1
```

Disassembling libgcc's implementation shows the correct AArch64 sequence:
reads `CTR_EL0`, `dc cvau` loop over the range, `dsb ish`, `ic ivau` loop,
`dsb ish`, `isb`, `ret` — with the DIC (bit 28) / IDC (bit 29) short-circuits the
architecture allows. It is a genuine implementation.

Two consequences worth recording:
1. It is an **undefined dynamic symbol resolved from `libgcc_s.so.1` at load
   time**, not statically linked. Rust already pulls `libgcc_s` in on
   `*-linux-gnu` for unwinding, so this adds no new runtime dependency there —
   but it does mean a fully-static build (musl, `crt-static`) has to satisfy the
   symbol some other way. See F-musl below.
2. Because libgcc honours `CTR_EL0.{IDC,DIC}`, on hardware that reports cache
   coherency the call degenerates to `dsb`/`isb`. That is correct, but it is why
   the mutation test in item 3 must be interpreted carefully — see F5.


### F4 — Headline deliverable independently reproduced on Linux aarch64. (confirmed)

Container: `rust:1.97.1`, `--platform linux/arm64` on colima (`uname -m` =
`aarch64`, `host: aarch64-unknown-linux-gnu`), 4 vCPU / 8 GB, **no emulation**.
Tree = `git archive HEAD`, copied in; the host working tree was not used.

- `cargo test --release --lib randomx::jit::` → **66 passed, 0 failed**.
- `cargo test --release --lib -- native_loop_diff_tests
  full_mode_v1_vm_reports_the_native_loop_effective test_native_loop_known_answer
  test_vm_calculate_hash_jit` → **8 passed, 0 failed** (191 s), including all four
  named deliverable tests:
  `native_loop_matches_interpreter`, `native_loop_matches_interpreter_full_program`,
  `native_loop_at_the_c1_worst_case_dataset_address`,
  `full_mode_v1_vm_reports_the_native_loop_effective`.

The implementer's load-bearing claim that
`full_mode_v1_vm_reports_the_native_loop_effective` is the one test that
hard-requires a live JIT allocation **checks out**: `vm.rs:1823` shows
`native_loop_effective()` ORs in `self.jit.is_some()`, so a failed
`mmap`/`mprotect` would make it false and the test fail, whereas the
known-answer vectors would still pass via interpreter fallback.

**The claim "emitted ARM64 agrees bit-for-bit with the interpreter on a second
OS" is true and I reproduced it.**

### F5 — The new test is a live guard, not decoration. (confirmed by mutation)

Deleting only the cache-maintenance call in the container copy —

```rust
pub(super) fn enable_execute(p: *mut u8, size: usize, code_len: usize) {
    protect(p, size, PROT_READ | PROT_EXEC, "PROT_READ|PROT_EXEC");
    let _ = code_len; // MUTATION: cache clear removed
}
```

— produces, on Linux aarch64:

- `randomx::jit::` → **65 passed, 1 failed**. The one failure is
  `test_jit_memory_rewrite_in_place`, with exactly the predicted symptom:
  `assertion left == right failed: rewritten code must execute, not a stale
  I-cache line / left: 42 / right: 55`.
- Repeated **200 times** standalone (`--exact`, `--test-threads=1`):
  **0 passed / 200 failed**. Deterministic, not a flaky microarchitectural
  coin-flip.
- The 63 `jit::compiler` tests **all still pass** under the mutation — so the new
  test is not redundant with them; it is the only unit-level guard.
- The differential/known-answer tests under the same mutation die with
  **SIGSEGV** (executing a stale instruction stream), i.e. they detect it too,
  but as a crash rather than a diagnosis.

This is the opposite of the earlier "assertion that could not fail" problem in
this repo's history. The test earns its place, and it also empirically confirms
that Linux `mprotect` does **not** imply I-cache maintenance — the `__clear_cache`
call is load-bearing, not defensive.
