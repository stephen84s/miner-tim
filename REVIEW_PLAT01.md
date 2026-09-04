# Independent review — PLAT-01 (`feat/jit-linux-aarch64`, issue #2 phase 1a)

Reviewer: independent agent, no prior context on this branch.
Base: `main` (`1790a9f`). Head: `1d444aa`. Diff: 5 files, 362 insertions.
Code change is confined to `src/randomx/jit/memory.rs` (+`mod.rs` comments);
`AUDIT.md` / `CLAUDE.md` are prose.

## Coverage ledger

| # | Item | Status |
| :- | :- | :- |
| 1 | Darwin path genuinely unchanged? | **done — unchanged** |
| 2 | Linux path correct (constants, alignment, ordering, cache clear)? | **done — correct** |
| 3 | In-place rewrite test — does it actually fail if cache maintenance is removed? | **done — guard is live on Linux** |
| 4 | Privatisation of `enable_write`/`enable_execute` | done |
| 5 | `compile_error!` + module gate | **done — fires cleanly** |
| 6 | Implementer's disclosures | **done — all honest; two narrowed** |
| R | Reproduce claimed test results (macOS + Linux aarch64) | **done — every claim reproduced** |

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

### F6 — Same mutation on macOS is also caught. (informational)

Symmetric experiment on a scratchpad copy of the tree (the working tree was not
touched): replaced `sys_icache_invalidate(p, code_len)` with a no-op inside the
Darwin `enable_execute`.

- `cargo test --release --lib randomx::jit::` → **65 passed, 1 failed**, again
  only `test_jit_memory_rewrite_in_place`, again `left: 42 / right: 55`.
- Repeated **200 times** standalone: **0 passed / 200 failed**.

So the new test is a live guard on *both* platforms, deterministically. Worth
noting that this is new coverage for Darwin too: before this branch, nothing in
the tree would have caught a removed `sys_icache_invalidate`, even though the
two-pass native-loop compile at `compiler.rs:834` depends on it. That is a small
net safety gain for the shipping platform.

### F7 — Linux mechanics: ordering, alignment and range are all right. (informational)

- **Ordering.** `write_code` is `assert` → `mprotect(R|W)` → `copy` → set
  `code_len` → `mprotect(R|X)` → `__clear_cache(p, p+code_len)`. Writes precede
  the `dc cvau`, and the region is never `W|X` — the security property claimed.
  Clearing *after* the `mprotect` is fine: `DC CVAU` / `IC IVAU` need only read
  permission, which `PROT_READ|PROT_EXEC` grants, and the tests execute the code
  successfully 66/66.
- **Alignment.** `mprotect` requires a page-aligned `addr`; `mmap` guarantees it.
  `len` is rounded up to a page by the kernel, so the 4096-byte test regions and
  the 65536-byte `JIT_CODE_SIZE` are both fine. `getconf PAGESIZE` = 4096 in the
  container; a 64K-page kernel is untested but not defective for the same reason.
- **Range.** `__clear_cache` covers `[p, p+code_len)`, i.e. exactly the bytes just
  written. Bytes beyond `code_len` left over from a previous, longer program are
  never reached (every emitted program ends in `RET`), and memory and I-cache
  agree about them anyway.
- **Error handling.** `protect` uses `assert!` (not `debug_assert!`), so the
  `mprotect` check survives `--release`. Format arguments — including
  `std::io::Error::last_os_error()` — are only evaluated on failure, so there is
  no per-call cost. Verified against the repo's history of vacuous assertions:
  this one is real, and the mutation experiments above trip it nowhere, meaning
  `mprotect` never fails in the tested paths.
- **`as_fn` before `write_code`.** New platform divergence: on `main` the region
  was mapped `RWX`, so calling `as_fn()` on a virgin region executed garbage; on
  Linux the virgin region is `R|W`, so the same call is a SIGSEGV. Unreachable in
  practice — `JitCompiler::{get_fn,get_loop_fn}` both
  `assert_eq!(self.kind, Some(...))` and `kind` is `None` until `write_code`
  (`compiler.rs:141-147, 165-171`). Informational only.

### F8 — `compile_error!` fires cleanly; the module gate is sound. (informational)

`rustc --edition 2024 --crate-type lib --target aarch64-linux-android
src/randomx/jit/memory.rs` produces **exactly one error** — the `compile_error!`
text, pointing at `memory.rs:24`, with the fix and the Android note spelled out.
No cascading "cannot find function `alloc` in module `platform`" noise: rustc
aborts after expansion, before name resolution. The "unsupported OS fails
clearly" claim is fully true, better than I expected.

Non-aarch64 is unaffected: `randomx/mod.rs` still gates `pub mod jit` on
`target_arch = "aarch64"`, so `memory.rs` is never even parsed elsewhere.
`cargo check --target x86_64-apple-darwin --all-targets` is **clean** on this
branch. GitLab CI runs on x86_64 Linux and therefore does not compile the JIT at
all, so `.gitlab-ci.yml` needed no change and none was made — correct.

The decision to leave the gate at `target_arch = "aarch64"` rather than narrowing
to `all(aarch64, any(macos, linux))` is defensible: the ~40 `cfg` sites in
`vm.rs` are on the shipping path, and the failure mode of not narrowing is a
build error with a message that names the file and the fix, which is the right
failure mode. Newly hard-failing targets that previously compiled: `aarch64-apple-ios`
and friends (`target_os = "ios"`, not `"macos"`). Nobody ships those here.

### F9 — Mechanical confirmation of F1. (informational)

Not trusting the eyeball diff, I normalised both sides (comments stripped,
whitespace collapsed) and compared the Darwin-relevant code from
`main:src/randomx/jit/memory.rs` against the `#[cfg(target_os = "macos")] mod
platform` arm on HEAD. The two normalised strings differ **only** in wrapping:

- added `use std::ptr;` (the arm is now its own module),
- `let ptr =` → `pub(super) fn alloc(size) -> Result<*mut u8, _> { let p = ... Ok(p) }`,
- `self.ptr` / `self.code_len` → the `p` / `code_len` parameters,
- a new `dealloc` wrapper around the same `munmap(p, size)`.

Every token that reaches the kernel or libSystem is identical: the two
`unsafe extern "C"` blocks, all seven constants and their values, the six `mmap`
arguments in order, `p == MAP_FAILED || p.is_null()`, `"mmap MAP_JIT failed"`,
`pthread_jit_write_protect_np(0)` / `(1)`, and
`sys_icache_invalidate(<region ptr>, <code_len>)` in that order.

Test-count corroboration: `memory.rs` goes from 2 `#[test]` to 3, and the macOS
release suite goes 130 → **131**. The delta is exactly the one new test — no
silent test removal, no drift.

### F10 — The implementer's disclosures: all honest; two of them I was able to narrow. (informational)

| Disclosure | Assessment |
| :- | :- |
| Ran `cargo test --release`, not `make test` (debug) | **Honest and accurate.** I reproduced the release run (131 + 10) and additionally ran the **debug** JIT subset on macOS — `cargo test --lib randomx::jit::` → **66 passed**. So the macOS debug gap is now narrowed to the same shape as the Linux one. The full debug suite remains unrun on both platforms; that is issue #6, pre-existing, not caused by this branch. |
| Clippy evidence is macOS-only (image ships no clippy) | **Was true; now closed.** `rustup component add clippy` in the container is a ~20 s download. I ran `cargo clippy --all-targets -- -D warnings` on Linux aarch64: **exit 0, clean**. The disclosed lint gap does not exist in fact. Recommend the implementer record this. |
| musl untested; `__clear_cache` availability unchecked | **True, and I could not build it** — `ring`'s build script needs `aarch64-linux-musl-gcc`, the same class of blocker that stopped the Android check. But I settled the important half: **neither** `libcompiler_builtins-*.rlib` for `aarch64-unknown-linux-musl` **nor** the musl `self-contained/libc.a` defines `__clear_cache`. So a musl build's failure mode is a **loud undefined-symbol link error**, not a silently-linked no-op stub. That is the safe failure mode, and it is the specific risk that mattered. |
| Container had 8 GB; whole suite peaks near 4.5 GiB | **True and relevant.** My container was likewise `-m 8g` / 4 vCPU and completed. Not a defect; issue #7. |
| `#[ignore]`d tests not run on either platform | **True; it is parity, not a Linux gap.** The macOS release run I did reports `2 ignored` on both sides. |

The AUDIT.md entry is unusually candid — it explicitly retracts an earlier wrong
claim of its own about clippy covering the benches. I found nothing it hides.

### F11 — Minor (Linux-only, forward-looking): two `mprotect` syscalls per compile.

Not a defect today, but undocumented. `write_code` now issues `mprotect(R|W)` and
`mprotect(R|X)` on every call. Production hashing compiles 8 programs per hash, so
a Linux aarch64 *miner* would pay **16 `mprotect` syscalls per hash**, each a
kernel VMA permission change with the TLB shootdown that implies, against two
userspace-cheap `pthread_jit_write_protect_np` calls on Darwin. Mature JITs avoid
this with a dual mapping (one `RW` alias, one `RX` alias of the same memory).

Severity: **minor / informational, unreachable from any shipping path.** Verified,
not assumed: `Makefile:19` sets `DIST_NAME := minertim-$(VERSION)-macos-arm64` and
`dist:` builds with `-C target-cpu=apple-m1`, so the only release artifact this
project produces is aarch64-apple-darwin. No Linux binary is shipped, so the
syscall cost cannot reach a user today. Issue #2 phase 1a is explicitly about
making the JIT's *tests* runnable on a second OS, not about shipping a Linux
miner. There is no Linux throughput measurement anywhere in the branch and none
is claimed. This should simply be
written down so nobody later reads "the JIT works on Linux" as "the JIT is fast
on Linux". **Does not block.**

### F12 — Trivial (docs): README is now stale in two places.

- `README.md:14` — "Linux/x86_64 also supported (interpreter fallback, no JIT)".
  Still true for x86_64, but silent about Linux/aarch64 now having the JIT.
- `README.md:111` — the tree diagram annotates `memory.rs` as
  "MAP_JIT memory, W^X toggle (macOS)".

AUDIT.md lists "README/CLAUDE.md platform-coverage wording" as deliberately
deferred, so this is disclosed rather than missed. **Does not block.**

### F13 — Every claimed result reproduced, plus one the implementer could not run. (informational)

| Claim | My result |
| :- | :- |
| macOS `cargo clippy --all-targets -- -D warnings` clean | **exit 0, zero warnings** |
| macOS `make check` clean | **exit 0** |
| macOS `cargo test --release` → 131 lib + 10 bin, 2 ignored, 0 failed | **131 passed / 2 ignored / 0 failed (90.1 s); bin 10 passed** |
| Linux aarch64 JIT tests, release, 66 passed | **66 passed, 0 failed** |
| Linux aarch64 JIT tests, **debug**, 66 passed | **66 passed, 0 failed** |
| Linux aarch64 differential tests, 4 passed | **8 passed** (the 4 diff tests + 4 known-answer/effective, 191 s) |
| Linux aarch64 whole suite 131 + 10 | **131 passed / 2 ignored / 0 failed (194.8 s); bin 10 passed** — exact parity |
| Linux `cargo check --benches/--all-targets --release` clean | **clean** |
| *(not claimed — implementer said clippy was unavailable)* | **Linux `cargo clippy --all-targets -- -D warnings`: exit 0, clean** |
| *(not claimed)* | **macOS debug JIT subset: 66 passed** |

Container: colima / `rust:1.97.1` / `--platform linux/arm64`, `uname -m` =
`aarch64`, `host: aarch64-unknown-linux-gnu`, 4 vCPU / 8 GB, no emulation. Peak
observed RSS during the whole-suite run: **3.9 GiB of 7.7 GiB** — tighter than
comfortable, as disclosed, but it completed.

---

## Verdict: **MERGEABLE.** No blockers, no majors. Two minors (F11, F12), both
## already disclosed or inert.

### Item-by-item

1. **Is the Darwin path genuinely unchanged?** **Yes — confirmed by two
   independent methods.** A statement-level table (F1) and a mechanical
   normalised token comparison (F9) both show the `#[cfg(target_os = "macos")]`
   arm is a pure move. Identical `mmap` prot/flags/fd/offset, identical
   `MAP_FAILED || is_null` test, identical `"mmap MAP_JIT failed"` string,
   identical `pthread_jit_write_protect_np(1)` → `sys_icache_invalidate(ptr,
   code_len)` order, and `write_code` is textually unchanged so `code_len` still
   holds the *current* program's length when the invalidate runs. The macOS
   release suite is 131/10/2-ignored, matching main's 130 plus exactly the one
   new test. **No Darwin regression.**
2. **Is the Linux path correct?** **Yes.** All six `mman` constants verified
   against the container's own `<sys/mman.h>` (F2), including that Darwin's
   `MAP_JIT` bit is `MAP_DENYWRITE` on Linux. `__clear_cache` resolves to the
   real libgcc implementation — `dc cvau` / `dsb ish` / `ic ivau` / `isb`, not a
   stub (F3). Ordering, page alignment and the clear range are all right (F7).
3. **Does the in-place rewrite case work, and is the new test a real guard?**
   **Yes to both.** Deleting only the cache maintenance fails the test
   **200/200 on Linux and 200/200 on macOS**, with the exact predicted symptom
   (`left: 42, right: 55`), while the other 63 JIT tests stay green (F5, F6).
   The test is the only unit-level guard and it is live.
4. **Privatisation of `enable_write`/`enable_execute`.** **Correct.**
   `git grep` on `main` finds no caller outside `memory.rs` (exit 1);
   `write_code` is their only caller and its body is unchanged. `compiler.rs`
   needed no edit, as claimed.
5. **`compile_error!` and the module gate.** **Sound.** Firing it on
   `aarch64-linux-android` yields exactly **one** clean error naming the file
   and the fix — no cascading noise. Non-aarch64 is untouched
   (`cargo check --target x86_64-apple-darwin --all-targets` clean); CI runs on
   x86_64 Linux and never compiles the JIT, so leaving `.gitlab-ci.yml` alone is
   right. No silent-wrong-compile combination found.
6. **The disclosures.** **All honest.** Two I narrowed: Linux clippy is in fact
   **clean** (I installed the component), and macOS debug JIT coverage now
   exists (66 passed). The musl risk is real but its failure mode is a **loud
   link error** — nothing in the musl sysroot defines `__clear_cache` as a stub.
   Memory headroom is as stated.

### Net effect on the shipping platform

Not neutral — slightly **positive**. Before this branch nothing in the tree would
have caught a removed `sys_icache_invalidate` on Darwin, even though the
two-pass native-loop compile at `compiler.rs:834` depends on it.
`test_jit_memory_rewrite_in_place` is the first guard against that, on macOS as
well as Linux.

### Non-blocking suggestions

- Record in AUDIT.md that Linux clippy `--all-targets -D warnings` is clean
  (`rustup component add clippy` is a ~20 s step) — the stated gap does not exist.
- Add the F11 note (two `mprotect` syscalls per compile; no Linux release
  artifact exists) so nobody later reads "the JIT works on Linux" as "the JIT is
  fast on Linux".
- Add the musl evidence to the source comment: the failure mode is a link error,
  not a silent no-op.
- README lines 14 and 111 when the deferred doc pass happens.
