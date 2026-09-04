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

---

## Item 5 — is coverage really unchanged?

**Reproduced, and strengthened beyond a count.**

`make verify-jit` on this host (Darwin arm64, M2 Max):

```
test result: ok. 92 passed; 0 failed; 1 ignored; 0 measured; 40 filtered out; finished in 185.41s
verify-jit: OK — debug profile (debug_assert! live), 92 passed
test result: ok. 92 passed; 0 failed; 1 ignored; 0 measured; 40 filtered out; finished in 46.04s
verify-jit: OK — release profile (shipping profile), 92 passed
verify-jit: GATE PASSED on Darwin arm64 — 92 tests, debug + release
```

Claimed 187 s / 46 s; measured 185.41 s / 46.04 s. `native_loop_at_the_c1_worst_case_dataset_address ... ok`
appears in **both** profiles — which, per item 1, is the direct empirical proof
that the C1 worst case is still reached against the new dataset.

Full suite, release: **131 passed, 0 failed, 2 ignored** (lib) — reproduced at
three different `--test-threads` values below. Bin target listed 10 tests.

A pass count only proves cardinality, so I also diffed the **test name lists**
between `main` and this branch:

```
diff <(sort list_main.txt) <(sort list_head.txt)   # main:  133 tests, 0 benchmarks
                                                   # HEAD:  133 tests, 0 benchmarks
=> IDENTICAL
```

Byte-identical. No test was added, removed, renamed, newly `#[ignore]`d, or
cfg'd out. Leaving `EXPECTED_PASSES` at 92 is correct. **No finding.**

---

## Item 4 — reproducing the RSS numbers

Method: `/usr/bin/time -l` on the **release lib test binary run directly**
(`target/release/deps/minertim-<hash>`), not through `cargo`, matching the
implementer's stated method. `main` was measured from a `git worktree` with its
own `CARGO_TARGET_DIR`. M2 Max, `caffeinate -i`.

| Run | `maximum resident set size` | Claimed | Verdict |
|---|---|---|---|
| `main`, `--test-threads=12`, run 1 | 8,154,300,416 (**8.15 GB**) | 8.16 GB | reproduced |
| `main`, `--test-threads=12`, run 2 | 8,154,464,256 (**8.15 GB**) | 8.16 GB | reproduced |
| `main`, `--test-threads=3` | 5,999,820,800 (**6.00 GB**) | *not measured* | **new** |
| HEAD, `--test-threads=12` | 6,230,245,376 (**6.23 GB**) | 6.23 GB | exact |
| HEAD, `--test-threads=3` | 4,067,033,088 (**4.07 GB**) | 4.06 GB | exact |
| HEAD, `--test-threads=1` | 3,252,944,896 (**3.25 GB**) | 3.25 GB | exact |

Wall clock: `main` 88.4 s / 88.5 s, HEAD 46.3 s — the claimed 94 s → 50 s, same
direction and magnitude. All six runs: 131 passed, 0 failed, 2 ignored.

**Every after-number is reproduced to the quoted precision, including the
4.06 GB figure #9 will be planned against.** The saving is real and large.

### FINDING F1 (minor, accuracy of a headline claim) — the "already red from day one" framing is not supported at the runner's parallelism

`AUDIT.md` says of the 8.16 GB baseline: *"Had #9 landed first it would have
been red from day one."* That is the 12-thread number on a 12-core dev box.
At the `macos-14` runner's actual 3-core parallelism — the configuration #9 would
actually run — I measured `main` at **6.00 GB**, i.e. *under* the 7 GB budget,
though with only ~1 GB of headroom for the OS and runner agent. So the honest
statement is "the baseline was marginal and would likely have flaked", not
"would have been red from day one". The implementer explicitly declined to
measure the pre-fix `--test-threads` rows; had they, the headline would have
been weaker. This does not change the merge decision — 6.00 → 4.07 GB at the
runner's parallelism is still a decisive improvement, and it is the honest
version of the same argument.

### Judgement: is "test-binary RSS" the right quantity for #9?

**It is a floor, not a budget.** Two caveats worth recording on #9 rather than
against this branch:

1. `/usr/bin/time -l` reports max-over-waited-children, not the sum of
   concurrently-resident processes. A CI job's true system peak is not bounded
   by any number in that table; the OS, the runner agent and any concurrent
   cargo process sit on top of 4.07 GB.
2. The `macos-14` job compiles before it tests, and the release profile is
   `lto=true, codegen-units=1`. I measured that separately:
   `cargo test --release --lib --no-run` on `main` peaked at **458,784,768 B
   (0.46 GB)** over 18.9 s. So the build is *not* the binding constraint — the
   test run is. This closes the one caveat that could have invalidated the plan.

With ~2.9 GB of slack under 7 GB, both caveats are comfortably absorbed. The
figure is usable for #9 provided it is quoted as "the test binary's peak at the
runner's parallelism", which the AUDIT already does say.
