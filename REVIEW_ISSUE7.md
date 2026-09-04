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

---

## Item 2 — is `zeroed_for_test()` sound for the ShareVerifier test?

**Mutation-tested, not argued.** A detached `HEAD` worktree with its own
`CARGO_TARGET_DIR`; each mutation applied to `src/miner.rs` (production code),
then `cargo test --release --lib -- <the rotation test> --exact`.

Unmutated baseline: `1 passed` in 44.48 s.

| # | Mutation to production code | Result | Assertion that killed it |
|---|---|---|---|
| M1 | `rekey` no longer does `self.vm = None` (stale VM survives rotation) | **FAILED** | `tests.rs:728` — *"cached VM survived a seed rotation — it would verify against the previous seed's dataset and withhold every share"* |
| M2 | `rekey` ignores its `dataset` argument entirely | **FAILED** | `tests.rs:685` — `assertion failed: v.is_armed()` |
| M3 | `reference()` drops `v.set_native_loop(false)` (verifier on the native-loop path) | **FAILED** | `tests.rs:707` — *"the verifier's VM is not on the reference path — verification would be comparing the native loop against itself"* |
| M4 | `rekey` adopts a dataset only when it has none (i.e. **rotation** silently keeps the old one — the R9-F2 shape) | **FAILED** | `tests.rs:733` — *"the new dataset was not adopted"* |

**Four out of four mutations killed.** M4 is the important one: it is exactly the
failure mode the second dataset exists to detect, it is invisible to M1/M2, and
the zeroed dataset catches it. The test is not vacuous and `zeroed_for_test()`
has not weakened it.

### Can the two hashes coincide?

No, and this is settled empirically rather than by argument: the test passes,
and it passes *through* `assert_ne!(after, got)`. That assertion only passes if
the zeroed dataset produced a different 256-bit hash from the real one, so the
zeroed dataset is demonstrably distinguishable on every run of the suite.
Coincidence would require a 256-bit collision.

### One honest qualification, stated so it is not over-claimed

**No single mutation of production code can trip the `assert_ne!` itself**, because
`holds_dataset()` and `reference()` both read `self.dataset`, so they cannot
diverge — any mutation that would make the two hashes equal trips
`has_cached_vm` or `holds_dataset` first. The `assert_ne!` is therefore a
**test-vacuity guard** (it fires if a future edit passes the same dataset twice,
which is precisely R9-F2), not a production-regression detector. That is a
legitimate and valuable role, and the implementer's doc comment describes it
correctly ("a dataset that failed to be distinguishable would fail the test
rather than pass it quietly"). It should not be read as the assertion that
catches a stale verifier — M1/M4 are.

### Residency of the zeroed dataset

`vec![[0u64; 8]; DATASET_ITEM_COUNT]` does hit std's `IsZero` specialisation
(`impl IsZero for [T; N]` applies for `N <= 16`; here `N = 8`), so it is
`alloc_zeroed` → lazily-faulted zero pages, as the doc comment claims. Expected
residency: 8 programs × 2048 iterations = 16,384 dataset reads per hash over
2 GiB at 16 KiB pages ≈ 15.4k distinct pages ≈ 250 MB; both hashes in the test
walk the same address sequence, so ~250 MB total, not ~500 MB. That matches the
AUDIT's "roughly 0.2 GB resident" and is consistent with the measured
12-thread delta in item 4 (2× fully-touched 2 GiB → 1× 2 GiB + ~0.25 GB).

**No finding.**

---

## Item 6 — the decided-against list

### 6b — the x86_64 reachability check for `zeroed_for_test` (accepted)

The implementer could not cross-compile and inspected instead. The inspection is
**sufficient, and stronger than a cross-compile would have been**, because the
predicates are identical:

- `RandomXDataset::zeroed_for_test` — `#[cfg(test)]`, no arch predicate
  (`dataset.rs:151`)
- its only caller, `mod full_hash_tests` — `#[cfg(test)]`, no arch predicate
  (`tests.rs:364`)

Identical cfg predicates mean that wherever the caller compiles, the callee is
used. There is no target on which one exists without the other, so GitLab's
x86_64 `clippy -D warnings` job cannot see it as dead code. (Contrast
`as_ptr_for_test`, which is correctly `#[cfg(all(test, target_arch = "aarch64"))]`
because its caller is in the aarch64-gated `native_loop_diff_tests`.) **Accepted.**

### 6a — declining to share the Argon2d cache behind a `LazyLock`

The **decision** is right: at `--test-threads=3` there is ~2.9 GB of headroom
under 7 GB, so spending a permanent 256 MiB and degrading a known-answer
failure into "LazyLock init panicked" across five tests buys nothing. I would
have declined too.

### FINDING F2 (minor, wrong justification for a right decision)

One of the two reasons given for that decision is factually wrong. `AUDIT.md`
says sharing the cache would "remove transients that **only exist at parallelism
the target runner cannot reach**." My measurements contradict that: the same
post-fix binary peaks at **3.25 GB at 1 thread and 4.07 GB at 3 threads**. That
0.82 GB delta is almost exactly three concurrent 256 MiB Argon2d caches, so the
transients very much *do* exist at the `macos-14` runner's parallelism — they
are ~20% of the peak there. The right justification is "they exist, they are
~0.8 GB, and 2.9 GB of headroom makes them not worth the permanent 256 MiB and
the diagnostic regression", which reaches the same conclusion honestly. Worth
correcting in the audit so a future reader planning a tighter runner is not
misled into thinking the lever is unavailable.

---

## Extra evidence that no test got faster by doing *less*

The release suite's wall clock dropped 88.4 s → 46.3 s, which could in principle
mean tests were narrowed. It does not. Compare **user CPU time** across the same
runs:

| | wall | user CPU |
|---|---|---|
| `main`, 12 threads | 88.4 s | 1025.9 s |
| HEAD, 12 threads | 46.3 s | 362.5 s |

The delta is 663 s of CPU. One `RandomXDataset::generate(cache, programs, 8)` is
~330 s of CPU on this host, so the *entire* saving is accounted for by not
generating a second 2 GiB dataset — 663 ≈ 2 × 330 vs 1 × 330. No test lost work;
one dataset build was removed. Combined with the byte-identical `--list` output
(item 5), this is about as direct as "coverage is unchanged" gets.

---

## FINDING F3 (minor, but it is a checked-in measurement that does not reproduce) — the debug RSS figure in `scripts/verify-jit.sh` is understated by ~0.9 GB

`scripts/verify-jit.sh` now carries, as a checked-in comment:

> Max RSS on an M2 Max for this filtered set in the debug profile: 6.77 GB
> before that change, **4.50 GB after**. Both figures are the *test binary*
> measured directly (`/usr/bin/time -l target/debug/deps/minertim-* <filters>`)

I reproduced that exact procedure — same binary, same six filters, default
`--test-threads` — twice:

| run | `maximum resident set size` | wall |
|---|---|---|
| 1 | 5,425,659,904 (**5.43 GB**) | 195.26 s, 92 passed |
| 2 | 5,425,758,208 (**5.43 GB**) | 198.05 s, 92 passed |

The two samples differ by 98 KB, so this is **not run-to-run variance** — the
figure is simply ~0.93 GB (21%) higher than the one committed. Wall clock
reproduces fine (claimed 193 s, measured 195/198 s), so the run itself matches;
only the memory number does not.

Why this is worth a finding rather than a nit: the audit's own headline
criticism of issue #7 is that an *unmeasured, understated* number ("~4.5 GiB")
would have let #9 be planned against a budget it could not meet. The replacement
comment repeats that failure mode in miniature — it understates, and it
understates the profile (`debug`) that `make verify-jit` is documented as
**mandatory** for. 5.43 GB still fits 7 GB, so nothing is broken today; but the
number should be corrected before anyone plans the debug gate's runner against
it. (Release numbers, by contrast, reproduced to the quoted digit — see item 4.)

I am re-measuring the "6.77 GB before" figure to establish whether the *delta*
claim survives; result appended below.

## FINDING F4 (nit) — debug figures sit under the release banner

The new comment block is placed between the `# 2. Release profile` banner and
`run_group "release profile ..."`, but the numbers it quotes are the **debug**
profile's. It does say "in the debug profile", so it is not false, only
misfiled — the debug `run_group` is ~15 lines above. A reader scanning the
release section still finds no release RSS figure. Move it under section 1, or
quote both profiles.

---

## OBSERVATION O1 (informational, not blocking) — the memory win rests on an unguarded std specialisation, and nothing tests for its regression

`zeroed_for_test()` is only cheap because `vec![[0u64; 8]; DATASET_ITEM_COUNT]`
hits std's `SpecFromElem`/`IsZero` specialisation and becomes `alloc_zeroed`
(lazily-faulted zero pages) rather than 2 GiB of writes. That specialisation is
an optimisation, not a stability guarantee.

The property is real today — measured: at `--test-threads=1` HEAD peaks at
3.25 GB, whereas a fully-written second dataset would put that at ~5.4 GB. So
the pages are genuinely not being touched.

But if it ever stopped holding — a std change, or someone "clarifying"
`zeroed_for_test` into a loop that writes items — **every test would still pass
and `make verify-jit` would still report 92**, while the peak silently returned
to roughly where issue #7 started. There is no RSS assertion anywhere in the
gate; `EXPECTED_PASSES` counts tests, not bytes. Given that #9 will depend on
this number, a cheap guard (e.g. asserting `zeroed_for_test()`'s pages stay
untouched, or simply a comment in `verify-jit.sh` naming the invariant as
load-bearing) would be worth having. Not a defect in this branch — the branch
does what it says — but the property it buys is currently unprotected.

---

## The mistake this change did *not* make

Worth stating explicitly, because it is the version of this change that would
have been a real coverage loss: the **`zeroed_for_test()` dataset is not used by
the differential tests.** It has exactly one caller, the ShareVerifier rotation
test (`tests.rs:726`). `native_loop_diff_tests::test_dataset()` still returns the
**real** `test_key_000_dataset()`.

That matters because the emitted loop's dataset read is `r ^= dataset[...]`. Had
the diff tests been pointed at a zeroed dataset to save the 2 GiB, every dataset
read would have degenerated to `r ^= 0`, the r-registers would have stopped
being steered by dataset content, and CBRANCH coverage would have collapsed —
while all 92 tests still passed. The implementer took the 2 GiB hit exactly
where it buys coverage and the synthetic dataset only where it does not. This is
the correct split.

---

## F3, CORRECTED AND EXTENDED — both debug figures fail to reproduce, and the claimed debug *delta* is inflated ~2.7×

The earlier draft of F3 (above) only had the "after" number. I have now measured
the "before" number too, from the `main` worktree's own debug binary, same six
filters, same method. Full debug picture:

| Configuration | measured `maximum resident set size` | wall | claimed |
|---|---|---|---|
| `main`, debug filtered, 12 threads | 6,265,208,832 (**6.27 GB**) | 308.9 s | 6.77 GB |
| HEAD, debug filtered, 12 threads, run 1 | 5,425,659,904 (**5.43 GB**) | 195.3 s | 4.50 GB |
| HEAD, debug filtered, 12 threads, run 2 | 5,425,758,208 (**5.43 GB**) | 198.1 s | 4.50 GB |
| HEAD, debug filtered, **3 threads** | 4,067,426,304 (**4.07 GB**) | 201.5 s | not quoted |

All four runs: `92 passed; 0 failed; 1 ignored; 40 filtered out`.

Three things follow:

1. **Wall clock reproduces on both sides** (claimed 316 s → 193 s; measured
   308.9 s → 195.3/198.1 s). So the runs themselves are comparable and my method
   matches the implementer's stated one. Only the memory figures diverge.
2. **Neither debug figure reproduces, and they diverge in opposite directions** —
   the "before" is quoted 0.5 GB too high, the "after" 0.93 GB too low. The
   `--test-threads` confound does not explain it: at 3 threads HEAD reads
   **4.07 GB**, not 4.50 GB, so no thread count produces the committed number.
3. Consequently the claimed debug saving of **2.27 GB** (6.77 → 4.50) is really
   **0.84 GB** (6.27 → 5.43) — inflated by about 2.7×. The saving is real and in
   the right direction; its magnitude is not.

I cannot explain the divergence from here (the release figures reproduced to the
quoted digit, twice, on both branches, so it is not a method difference on my
side). **Recommendation: re-measure the two debug numbers and correct them in
`scripts/verify-jit.sh`, `AUDIT.md` and the `CLAUDE.md` task-board row before
merge, or drop them and quote only the release figures, which are solid.**

Severity: **minor** — no code is wrong, the direction of every claim holds, and
5.43 GB still fits 7 GB. It is raised at this level rather than as a nit only
because the audit's own thesis is that unverified memory numbers are what got
issue #7 mis-scoped in the first place, and because `make verify-jit` (debug) is
documented as mandatory before every JIT MR, so its footprint is the one a
contributor on a 8-16 GB machine actually feels.

**Useful side-result for #9:** the debug gate at the `macos-14` runner's
3-core parallelism is **4.07 GB** — essentially identical to the release suite's
4.07 GB at the same parallelism. Both profiles fit the 7 GB budget with ~2.9 GB
of headroom. That is the number worth committing.

(I did not measure `main` debug at 3 threads; the 12-thread before/after pair is
enough to establish the delta claim is wrong, and a second "before" number has
no forward value.)

---

## Closing the issue's own acceptance wording — `make test` (debug, unfiltered)

Issue #7's literal complaint is `make test` on an 8-16 GB machine, which is the
**debug, unfiltered** suite — a configuration nobody measured. I did:

| Configuration | measured | wall |
|---|---|---|
| HEAD, debug, **full suite**, 12 threads (`make test`) | 6,222,856,192 (**6.22 GB**) | 188.2 s, 131 passed / 2 ignored |

Two notes. First, this is essentially identical to the release full suite at the
same parallelism (6.23 GB), so the profile does not change the ceiling. Second,
it is **0.79 GB above the debug *filtered* set (5.43 GB)** — so my earlier
assumption that the filtered set approximates the full one was wrong, and the 41
filtered-out tests do carry real Argon2d transients. Worth knowing before anyone
uses the `verify-jit` figure as a proxy for `make test`.

The issue's acceptance ("measure the suite's peak RSS after the fix and confirm
it fits a 7 GB budget with headroom") is met at every parallelism I measured:
6.22-6.23 GB at 12, 4.07 GB at 3, 3.25 GB at 1.

---

# Verdict

**MERGEABLE.** No code defect found. Nothing here risks a wrong hash, a rejected
share, or lost coverage.

The dominant risk the review brief named — a quiet loss of JIT correctness
coverage hidden inside a memory reduction — **does not materialise**, and it is
closed three independent ways:

- **structurally**: every input that shapes the differential tests
  (`ProgramConfiguration`, `ma`, `mx`, `dataset_offset`, scratchpad) is a pure
  function of the `u8` seed via `make_program_bytes` / `derive_program_params`;
  the key cannot reach any of them;
- **empirically on coverage**: `cargo test --lib -- --list` is **byte-identical**
  between `main` and this branch, and `make verify-jit` reports 92/92 in both
  profiles;
- **empirically on work done**: the entire 663 s user-CPU saving equals exactly
  one `RandomXDataset::generate` — no test got cheaper by doing less.

The C1 worst-case guard still reaches the worst case, provably: the test forces
`entropy(8)`/`entropy(13)` to their extremes and asserts `ma == 0x7FFF_FFC0` and
`dataset_offset == DATASET_EXTRA_ITEMS * 64`; both assertions are
dataset-independent and both pass in debug and release.

The `zeroed_for_test()` substitution is confined to the one test whose subject is
the `ShareVerifier` state machine, and four separate mutations of production
`ShareVerifier` code all kill that test.

## Findings

| id | severity | summary |
|---|---|---|
| **F3** | minor | Both debug RSS figures in `scripts/verify-jit.sh` / `AUDIT.md` / `CLAUDE.md` fail to reproduce (6.77→6.27 measured, 4.50→5.43 measured); the claimed 2.27 GB debug saving is really 0.84 GB. Re-measure or drop before merge. |
| **F1** | minor | "Would have been red from day one" is a 12-thread claim; at the runner's 3-core parallelism `main` measures 6.00 GB — marginal, not over budget. |
| **F2** | minor | Right decision on the Argon2d `LazyLock`, wrong justification: those transients *do* exist at 3 threads (~0.8 GB of the 4.07 GB peak), they are simply not worth removing. |
| **F4** | nit | Debug RSS comment filed under the `# 2. Release profile` banner in `verify-jit.sh`. |
| **O1** | info | The whole memory win depends on std's `IsZero`/`alloc_zeroed` specialisation, which nothing asserts; a regression would leave all 92 tests green while the peak returned to ~issue-#7 levels. |

All five are documentation/measurement accuracy or forward-looking hygiene. None
requires a code change, and none should block the merge — though **F3 should be
corrected in the same push**, since it is a checked-in number future work will
be planned against.

## Reproduction summary

| check | claimed | measured |
|---|---|---|
| `make verify-jit` | GATE PASSED, 92 both profiles, 187 s / 46 s | **GATE PASSED, 92/92, 185.41 s / 46.04 s** |
| `cargo test --release` | 131 lib + 10 bin, 2 ignored, 0 failed | **131 + 10, 2 ignored, 0 failed** |
| `cargo clippy --all-targets -- -D warnings` | exit 0 | **exit 0** |
| `make check` | exit 0 | **exit 0** |
| release lib, 12 threads, `main` | 8.16 GB | **8.15 GB** (×2) |
| release lib, 12 threads, HEAD | 6.23 GB | **6.23 GB** |
| release lib, 3 threads, HEAD | 4.06 GB | **4.07 GB** |
| release lib, 1 thread, HEAD | 3.25 GB | **3.25 GB** |
| debug filtered, 12 threads, `main` | 6.77 GB | **6.27 GB** (F3) |
| debug filtered, 12 threads, HEAD | 4.50 GB | **5.43 GB** (×2) (F3) |
| test-name list, `main` vs HEAD | — | **byte-identical** |

Host: M2 Max (8 P + 4 E = 12 logical), macOS, Rust as per `rust-toolchain`,
all runs under `caffeinate -i`, `/usr/bin/time -l`, binaries invoked directly.

## Coverage ledger (final)

| # | Item | Status |
|---|------|--------|
| 1 | Is the key swap genuinely lateral? | **CLOSED — yes**, structurally and empirically |
| 2 | Is `zeroed_for_test()` sound for the ShareVerifier test? | **CLOSED — yes**, 4/4 mutations killed |
| 3 | Does the dummy pointer keep the zero-iteration test meaningful? | **CLOSED — yes**, verified in the emitter |
| 4 | Reproduce the RSS numbers | **CLOSED** — release exact; debug does not reproduce (F3) |
| 5 | Is coverage really unchanged? | **CLOSED — yes**, 92/92 and identical `--list` |
| 6 | The decided-against list | **CLOSED** — both acceptable; F2 on one rationale |
