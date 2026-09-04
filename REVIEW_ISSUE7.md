# Independent review — `fix/test-dataset-memory` (`20fa11e`, issue #7)

Reviewer: independent (no prior context on the implementation).
Scope: `git diff main..HEAD` — `src/randomx/dataset.rs`, `src/randomx/tests.rs`,
`scripts/verify-jit.sh`, `CLAUDE.md`, `AUDIT.md`.

## Coverage ledger

| # | Item | Status |
|---|------|--------|
| 1 | Is the key swap genuinely lateral? | **structural analysis done** — verifying empirically |
| 2 | Is `zeroed_for_test()` sound for the ShareVerifier test? | analysis done, mutation test pending |
| 3 | Does the dummy pointer keep the zero-iteration test meaningful? | **done** |
| 4 | Reproduce the RSS numbers | pending |
| 5 | Is coverage really unchanged (92 / 131+10)? | `make verify-jit` running |
| 6 | The decided-against list | partially done |

---

## Item 1 — is the key swap lateral? (structural analysis)

**Verified against source, not the implementer's prose.**

`src/randomx/vm.rs:1137-1160` `derive_program_params(program_bytes)` reads
*only* `program_bytes`:

- `ma  = (entropy(8) as u32) & CACHE_LINE_ALIGN_MASK`
- `mx  = entropy(10) as u32`
- `config.{e_mask, read_reg0..3}` from `entropy(12,14,15)`
- `dataset_offset = (entropy(13) % (DATASET_EXTRA_ITEMS + 1)) * CACHE_LINE_SIZE`

`src/randomx/tests.rs:1064-1085`: `make_program_bytes(seed)` and
`make_scratchpad(seed)` are both driven by `Blake2Generator::new(&[seed; 32], 0)`
/ `&[seed ^ 0xA5; 32]`. Neither touches a key, a cache, or a dataset.

So *every* quantity the implementer names — programs, `ProgramConfiguration`,
`ma`, `mx`, `dataset_offset`, scratchpad — is a pure function of the `u8` seed.
The dataset key cannot influence any of them. **The claim holds.**

### The `dataset_offset` tuning specifically

The review brief raised the sharpest version of the risk: "a `dataset_offset`
tuned against one dataset may not sit at the worst case against another."
That concern does **not** apply here, and the reason is structural:
`dataset_offset` is derived from `entropy(13)` of the *program*, and the bound it
must respect is `DATASET_EXTRA_ITEMS * 64`, a **compile-time constant of the
dataset shape** (`DATASET_ITEM_COUNT = 34,078,720`, `dataset.rs:24-29`), not of
the dataset *contents*. Every `RandomXDataset` — including
`zeroed_for_test()` — has exactly that shape. A worst case expressed in
addresses is therefore key-invariant.

Better still, `native_loop_at_the_c1_worst_case_dataset_address`
(`tests.rs:1224-1259`) does not *rely* on a seed happening to land at the
extreme: it **overwrites** `entropy(13)` with `DATASET_EXTRA_ITEMS` and
`entropy(8)` with `u64::MAX`, and then **asserts** it got there:

```rust
assert_eq!(ma, 0x7FFF_FFC0, "ma is not at its maximum");
assert_eq!(dataset_offset, vm::DATASET_EXTRA_ITEMS * 64, "dataset_offset is not at its maximum");
```

These two assertions are inside the test, are dataset-independent, and are the
test's own proof that it reached the worst case. As long as the test passes, the
worst case is reached. **Confirmed: the C1 worst case is still reached against
the new dataset.** (Empirical confirmation via `make verify-jit` — see item 5.)

The seed-78 comment ("`dataset_offset` at 99.67% of its maximum") is likewise a
property of `make_program_bytes(78)` alone, so it survives the key swap
unchanged.

### What *does* change, honestly stated

Dataset *bytes* differ between `b"native loop test key"` and `b"test key 000"`,
so the concrete execution trace the differential tests walk (r-register values,
which CBRANCHes are taken, which scratchpad addresses are hit after iteration 1)
is a different pseudorandom draw. That is a different sample, not a smaller one,
and both sides of the comparison consume the identical draw — which is the
property under test. I agree with the "lateral, not reductive" characterisation.

**No finding.**

---

## Item 3 — the dummy pointer in `native_loop_zero_iterations_terminates`

**Verified by reading the emitter, not by trusting the claim.**

`src/randomx/jit/compiler.rs:798-811`. Emission order is:

1. `emit_loop_prologue(e, ...)` — lines 841-895
2. `e.emit(0xB4000000 | reg::X28)` — the CBZ zero-iteration guard (line 807)
3. `loop_head` … `emit_iteration_pre` / `emit_body` / `emit_iteration_post`

Reading the prologue in full (lines 848-895), the dataset base register `x22` is
touched exactly twice:

```
e.mov_reg(reg::X22, reg::X2);   // capture arg
e.add_reg(reg::X22, reg::X22, reg::X0);   // += dataset_offset
```

There is **no load and no `PRFM` through `x22` anywhere before the CBZ**. Every
other prologue memory access is through `x21` (the real `NativeRegisterFile`).
`emit_loop_epilogue` (lines 1018-1044) stores through `x23`/`x21` and restores
`x22` off the stack — it never dereferences it either. **The claim is correct:
with `iterations == 0`, the dataset pointer is never dereferenced.** The test is
not weakened.

### Judgement on the changed failure mode — MINOR

If the CBZ guard regresses, the loop now runs ~2^64 iterations reading
`dummy + dataset_offset + (ma & 0x7FFF_FFC0)` — i.e. a random walk over a 2 GiB
window anchored on a 64-byte **stack** array. That will fault essentially
immediately, so the regression is still caught, and caught *faster* than the old
hang. That is a net improvement over "the suite hangs until someone notices".

The cost is diagnostic, not detective: a SIGSEGV kills the whole libtest
process, and libtest's last printed line will be whatever test happened to be
running, not necessarily this one. The doc comment already says exactly this
("the symptom to look for is 'test binary crashed', not 'test hung'"), which is
the right mitigation for a cost that cannot be removed without paying 2 GiB.
I would accept this. Recorded as a minor, not a blocker.

**No blocking finding.**
