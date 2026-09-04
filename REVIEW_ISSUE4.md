# Independent review — `fix/jit-alloc-failure-visible` (issues #4, #3)

Reviewer: independent (did not write this code). Base `main`, head `1ae7bd7`
(2 commits). Diff reviewed: `git diff main..HEAD` — 446 insertions / 30
deletions across `AUDIT.md`, `CLAUDE.md`, `src/bin/minertim.rs`,
`src/miner.rs`, `src/randomx/vm.rs`.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Hot path semantically unchanged (`execute_vm_inner`) | **done — clean** |
| 2 | Arming holds across seed rotation | **done — clean** |
| 3 | `native_loop_effective()` in every mode / cfg arm | **done — clean, one nit** |
| 4 | Are the six new tests testing what is claimed | **done — F2, F3** |
| 5 | x86_64 behaviour change, honesty in `AUDIT.md` | **done — correct and honestly recorded** |
| 6 | Self-reported limits | **done — F2b, F4** |
| R | Reproduce: clippy (both targets), `make check`, `cargo test --release` | **done — all green, claims reproduce** |

## Verdict

**Mergeable.** No wrong-hash risk. No memory-safety risk. No blocker.** Four findings, all
minor: two orphaned doc comments (F1), a coverage gap whose stated justification
in `AUDIT.md` is factually wrong (F2/F2b), a test assertion that cannot fail
(F3), and a misleading `error!` string when the *verifier's* VM is the one whose
allocation failed (F4).

---

## Findings

### F1 — MINOR (documentation integrity, 2 sites, both new on this branch)

Both new items were spliced in immediately after an existing doc comment with no
separator, so the existing doc comment now documents the **new** function and the
function it was written for is left undocumented.

**F1a — `src/bin/minertim.rs:372-388`.** The block

```
/// The native-loop JIT switch. Malformed input falls back to **off**: slower,
/// but it cannot mine wrong hashes.
/// The startup configuration line.
/// ...
fn startup_state_line(...) -> String
```

used to document `parse_native_loop` (now at `:400`, doc-less). The lost
sentence is the *fail-safe direction* of the native-loop switch — arguably the
single most safety-relevant comment in that file. Its sibling
`parse_verify_shares` (`:404`) still carries the equivalent comment, so the
asymmetry now reads as intentional.

**F1b — `src/miner.rs:437-463`.** The R9-F1 / R11-F2 rationale — "Deliberately
**not** `enabled && dataset.is_some()` … a defence that cannot be reached is not
a defence" — was written for `is_enabled` and is now the opening three
paragraphs of `set_enabled`'s doc. `is_enabled` (`:465`) has no doc at all, and
`is_armed`'s doc at `:469-471` points the reader at "`is_enabled`" for a
rationale that is no longer there. That rationale is exactly the trap a future
editor of the new `set_enabled(...)` call site would need.

No behaviour impact. Fix is two blank lines and moving two blocks.

(Pre-existing, not this branch: `src/miner.rs:283` "Get share timing stats for
display." is the same defect on `get_verify_failures`, already on `main`. Worth
noting only because it makes three instances of one pattern.)

### F2 — MINOR (test coverage): nothing asserts `native_loop_effective() == true`

Every assertion on `native_loop_effective` in the tree is negative:
`src/randomx/vm.rs:2204/2209/2213` (light mode, three times) and
`src/miner.rs:624` is production. `grep -rn native_loop_effective src/ benches/`
returns no positive case.

Consequence: if `native_loop_effective()` ever returned a constant `false` on
aarch64 — a `cfg` slip of exactly the class that produced issue #3, or an
over-broad `#[cfg(not(target_arch = "aarch64"))]` — the full suite still passes,
and the shipping platform silently runs with **share verification disarmed on
every worker**. That is the same failure mode issue #4 was filed about (the
safety net reading green while measuring nothing), relocated from "vacuous" to
"off". Partly mitigated: the new per-worker `warn!` would fire, so it is loud
rather than silent. That mitigation is real and is why this is minor, not major.

The 16-row truth table on `native_loop_applies` does not cover this: it tests
the predicate, not the field wiring from `RandomXVm` into it.

### F2b — MINOR: `AUDIT.md`'s justification for F2's gap is factually wrong

The new `AUDIT.md` entry says:

> exercising the `has_dataset = true` arm on a real VM costs a 2 GiB dataset
> build, which was judged not worth it

That cost is **already being paid in the default test run**.
`src/randomx/tests.rs:44` `test_key_000_dataset()` is a `LazyLock` full dataset
used by three *non-ignored* tests (e.g.
`share_verifier_builds_lazily_and_resets_on_seed_rotation` at `:632`, which
constructs `RandomXVm::new_full(b"test key 000", test_key_000_dataset())` at
`:659`). The positive assertion would have cost one extra `new_full` on an
already-built `Arc`, not a dataset build. This matters beyond the missing test:
`AUDIT.md` is the project's authoritative append-only record, and a future
reader will take the cost claim at face value and not revisit the gap.

### F3 — MINOR (test quality): one assertion that cannot fail

`src/bin/minertim.rs`, `startup_line_reports_the_request_and_the_target`:

```rust
assert!(aarch64.contains("requested"), "the line must not read as effective state: {aarch64}");
```

`startup_state_line`'s format string ends with the unconditional literal
`(requested; each worker reports its effective state once its VM is built)`, so
this holds for **every** input triple. It is asserting that a constant is
present in itself. It would catch deletion of the suffix, which is not nothing,
but it does not test what its message claims (that the line does not read as
effective state), and the project has twice been bitten by exactly this shape.

It is wrong in a second, provable way. The rendered line is

```
Native-loop JIT: on | share verification: on (requested; each worker reports its effective state once its VM is built)
```

— the parenthetical trails the **last** field, so on the plainest reading it
qualifies *share verification*, not the `Native-loop JIT:` field, which is the
field issue #4 was filed about over-claiming. So the assertion certifies a
disclaimer that (a) is unconditional and (b) is not attached to the thing that
needed disclaiming. Still minor, still no behaviour impact — the startup line's
over-claim is now bounded and the per-worker line is the stated authority — but
the assertion should not be read as evidence that the wording problem is closed.

Related but weaker: `native_loop_guard_tests::every_precondition_is_load_bearing`
computes `expected = flag && version == RxVersion::V1 && ds && jit`, which is a
verbatim restatement of `native_loop_applies`'s body. It pins the predicate
against change, which is its stated purpose, but it cannot detect the predicate
being *wrong* — only different. And
`a_failed_jit_allocation_is_not_the_native_loop` is a strict subset of it
(row `flag=1, V1, ds=1, jit=0`), so it is documentation rather than coverage.
Fine as-is; noted because "six new tests" oversells to "four independent
checks".

### F4 — MINOR (log accuracy): `new_jit()`'s message is wrong for the verifier's VM

`src/randomx/vm.rs:1691-1700`. The `error!` says "share verification for this
worker is switched off because it would otherwise compare the interpreter
against itself." `new_jit()` runs on **every** `RandomXVm` construction,
including `ShareVerifier::reference()`'s reference VM (`src/miner.rs:430`). If
the mining VM allocated fine and only the *verifier's* VM fails, verification
stays correctly armed and still works (interpreter reference vs native-loop
mining) — but the operator is told it was switched off.

Worth stating explicitly rather than leaving implicit: that failure silently
changes what the reference path *is* (interpreter rather than body JIT), and the
arming decision does not model it. It is harmless only because
`native_loop_diff_tests::native_loop_matches_interpreter*`,
`test_native_loop_known_answer*` and `test_vm_calculate_hash_jit` pin all three
paths bit-identical in the default suite. Log-only today; the code does the
right thing, and it does so because of a dependency nothing in this branch
records.

---

## Item-by-item

### 1. Hot path semantically unchanged — CLEAN

Old guard (`main`):

```rust
if use_native_loop && version == RxVersion::V1
   && let (Some(ds), Some(jit)) = (dataset, jit.as_mut())
```

New (`vm.rs:1241`):

```rust
if native_loop_applies(use_native_loop, version, dataset.is_some(), jit.is_some())
   && let (Some(ds), Some(jit)) = (dataset, jit.as_mut())
```

with `native_loop_applies = use_native_loop && version == V1 && has_dataset && has_jit`.

- **Same terms, same result.** `dataset: Option<&RandomXDataset>` (`Copy`) and
  `jit: Option<&mut JitCompiler>`; `.is_some()` on both is a shared borrow that
  ends before `jit.as_mut()`, so the surviving `let` half is a pure re-bind of
  facts already established. It cannot fail when the predicate is true and it
  cannot panic (no unwrap, irrefutable-by-construction refutable pattern with a
  fall-through).
- **Short-circuit difference is inert.** The old form skipped
  `dataset.is_some()` / `jit.is_some()` when `use_native_loop` was false; the
  new form evaluates both eagerly as arguments. Both are side-effect-free
  discriminant reads.
- **Cost.** `execute_vm_inner` is called once per *program chain* (8 per hash,
  `vm.rs:1881/1946/2062`), never inside the 2048-iteration loop. Two
  discriminant loads per chain against a 2048-iteration JIT-ed body is
  unmeasurable. `native_loop_applies` is a private `fn` in the same module with
  `lto = true` / `codegen-units = 1`; no `#[inline]` needed and none of the
  call sites is hot.
- **The reporting/guard link is exact**, which was the point of the change:
  the three mining call sites pass `ds_ref = self.dataset.as_deref()`
  (`vm.rs:1872/1934/2032`), `self.jit.as_mut()`, `self.version`,
  `self.use_native_loop` — the four fields `native_loop_effective()` reads.
  Term-for-term identity, not an approximation.

**No wrong-hash risk. No memory-safety risk.**

### 2. Arming across a seed rotation — CLEAN (the premise in the comment is slightly off)

Traced `src/miner.rs:599-653`. On rotation the VM is **not** rebuilt:
`existing_vm.reinit(&job.seed_hash, Some(dataset.clone()))` (`:603`).
`RandomXVm::reinit` (`vm.rs:1769-1780`) touches only `cache_memory`,
`ss_programs`, `dataset` — `version`, `jit` and `use_native_loop` survive. A new
VM is built only on the `vm.is_none()` branch, and `set_native_loop(native_loop)`
is applied there.

So the "new VM has no JIT" scenario the review brief asks about cannot arise
from a rotation at all — a worker gets exactly one JIT allocation attempt, at
first-job time. And regardless, `native_effective` is **re-derived from the
live VM on every pass through the block** (`:623-625`), so a stale arming
decision is structurally impossible. Ordering is also right: the block is
entered unconditionally on the first job (`vm.is_none()`), and `set_enabled`
runs before the loop can reach `meets_target`, so no share can be classified by
the initial disarmed verifier.

One inaccuracy in the new comment at `:618-622`: it claims all four terms are
"fixed for this VM's lifetime". `use_native_loop` is not — `set_native_loop` is
`pub` and could be called later; it just isn't, in `worker_loop`. Harmless,
since the code re-derives anyway.

Minor behavioural nit: `reported_effective_state` latches the *first* rotation,
so the `warn!` is printed at most once per worker. Given the JIT is allocated
once per worker, there is nothing later to report. Correct as written.

### 3. `native_loop_effective()` in every mode — CLEAN

- **aarch64 full mode, v1, JIT present** → `true`. Matches the guard exactly
  (see item 1). *Untested — see F2.*
- **aarch64 light mode** (`RandomXVm::new`) → `dataset: None` → `false`, even
  though `use_native_loop` is initialised `true` and a JIT is allocated. Tested
  (`vm.rs:2197-2214`), and the emitted loop genuinely has no light-mode
  fallback, so this is the right answer, not merely a conservative one.
- **aarch64, failed `mmap(MAP_JIT)`** → `jit: None` → `false`. This is the
  issue-#4 case and it now flows all the way to the verifier.
- **v2** → `version != V1` → `false`. Correct: `compile_native_loop` asserts v1,
  and nothing selects v2 at runtime yet (RX2-01).
- **non-aarch64** → the `#[cfg(not(target_arch = "aarch64"))]` arm returns
  `false` unconditionally (`vm.rs:1824-1828`). Correct: the whole `jit` field
  and the `use_native_loop` parameter are `cfg`-gated away there, and
  `execute_vm`'s non-aarch64 wrapper (`vm.rs:1114-1129`) drops
  `_use_native_loop` on the floor. Putting the target test inside the method
  instead of at the call site is the right shape and is precisely what closes
  #3 structurally: `grep` confirms there is now **no** `cfg!(target_arch)` term
  anywhere in the enablement path (`miner.rs:624-625` is the sole composition).
- **aarch64 non-macOS** (e.g. aarch64-linux): `JitCompiler::new()` uses
  `MAP_JIT` / `pthread_jit_write_protect_np`. If it fails there, the new code
  logs and disarms rather than mis-reporting — strictly better than `main`.

Nit: the `#[cfg(not(...))]` arm takes `&self` and reads nothing. Intentional
(signature parity) and documented.

### 4. Do the six new tests test what is claimed — MOSTLY, with F2/F3

| Test | Verdict |
|---|---|
| `every_precondition_is_load_bearing` (16 rows) | Real, but mirrors the implementation expression (F3, second half) |
| `a_failed_jit_allocation_is_not_the_native_loop` | Strict subset of the above; documentation, not coverage |
| `light_mode_never_reports_the_native_loop` | Real on aarch64. **Vacuous on x86_64** — the `cfg(not)` arm returns `false` unconditionally, so all three assertions hold for free. Module is `#[cfg(test)]` with no arch gate |
| `arming_follows_the_vm_not_the_switches` | Real for `set_enabled`/`classify_share`. Does **not** test `worker_loop`'s composition, which is the site both issues were about — `AUDIT.md` concedes this ("lives inline in `worker_loop` and is not itself extracted"), but the test's own doc claims it "pins both ends of that", which overstates it |
| `startup_line_reports_the_request_and_the_target` | Real except the `contains("requested")` assertion (F3) |
| `startup_line_never_reports_verification_without_the_native_loop` | Real — the 4-case loop plus the `(true, false, true)` case genuinely pin "verification never reported on without the native loop" |

The claimed "16-row truth table, the `has_jit = false` regression, light mode,
verifier arm/disarm, and the startup line on both targets" is accurate as a
list. What is missing is the positive direction (F2) and the `worker_loop`
composition itself.

### 5. The x86_64 behaviour change — CORRECT, and honestly recorded

Verification going from armed-but-vacuous to **off** on x86_64 is right, and it
is the fix issue #3 asked for ("align `miner.rs:549` with the effective
predicate rather than weakening the report"). Confirmed vacuous on that target:
mining runs `execute_vm_inner`'s interpreter/no-JIT path, and
`ShareVerifier::reference()` (`miner.rs:429-433`) builds
`RandomXVm::new_full(...)` + `set_native_loop(false)` — with the `jit` field
`cfg`-ed out entirely, both sides are byte-for-byte the same code. Comparing
them could never withhold a share, and it cost a second full hash per candidate.
Nothing is lost; the double-hash is saved.

`AUDIT.md`'s behaviour-change list states it in bold and in full, including the
saved second hash. That is honest. `CLAUDE.md`'s VIS-01 row states it too.

Other x86_64 effects: the per-worker `warn!` now fires unconditionally there
(item 6), and `native_loop_applies` / `native_loop_guard_tests` are
`cfg`-compiled out, so the truth table is aarch64-only — acceptable, since the
predicate is aarch64-only.

### 6. The self-reported limits — ACCEPTABLE, with one correction

- **"A real `mmap(MAP_JIT)` failure is not tested."** Accepted. The untested
  span is `JitCompiler::new()` → `Err` → `None`, which is `new_jit()`'s four
  visible lines. Faking it would need a seam (an injectable constructor or a
  `#[cfg(test)]` forced-failure flag) that is arguably worse than the gap.
  Nothing is hidden by the omission.
- **"On non-aarch64 every worker emits the 'requested but NOT active'
  warning."** Accepted as a defect-free nuisance, but the *text* is wrong on
  that target: it says "Expect a large hashrate shortfall on this worker" (there
  is no shortfall — that target has no native loop, so nothing is being lost)
  and "See any 'JIT allocation failed' error above for the cause" (there will
  never be one; the field does not exist). It is one `warn!` per worker at
  startup, not per share, and x86_64 is not a shipping target. Log-quality only.
- **Not self-reported, and it should have been:** F2b — the stated reason for
  omitting the positive-direction test is wrong, and that omission (F2) is the
  one coverage gap with a real, if loud, failure mode.

### R. Reproduction of the implementer's verification claims — ALL REPRODUCE

| Claim | Reproduced |
|---|---|
| `cargo clippy --all-targets -- -D warnings` clean (aarch64) | **yes**, exit 0, no diagnostics |
| `cargo clippy --all-targets --target x86_64-apple-darwin -- -D warnings` clean | **yes**, exit 0, 0 warnings — re-run in a *fresh* `CARGO_TARGET_DIR` so the result is not a cached replay, which matters because #3 was a cfg-skew defect. This also type-checks the new test modules on x86_64 |
| `make check` clean | **yes**, exit 0 |
| `caffeinate -i` release suite green, 129 lib + 10 bin, 2 ignored | **yes** — `test result: ok. 129 passed; 0 failed; 2 ignored`, then `10 passed`, then 0 doc-tests. Counts match exactly |

Only divergence: the suite finished in **88 s** here, not the reported 306 s.
Explanation is benign — the two full datasets are `LazyLock`-shared across the
binary and this run reused a warm build; nothing about it is a red flag.

Note in passing: `native_loop_diff_tests::native_loop_matches_interpreter*` and
`test_native_loop_known_answer*` do run full-mode native-loop VMs in the default
suite. They are the natural (and free) home for the missing positive assertion
in F2.

### Checked and clean: the `verify_failures` counter is not displayed as a green zero

Issue #4's framing is "a health indicator that cannot go red is worse than no
indicator", and this branch's own change to `get_verify_failures`'s doc concedes
the value became ambiguous ("Zero is NOT by itself evidence that the JIT is
correct"). So the obvious follow-on risk is that the 10 s stats loop renders an
unqualified `0` and gives the operator false reassurance on x86_64 (verification
now off) or on aarch64 after a failed `mmap(MAP_JIT)` (every worker disarmed).

It does not. `src/bin/minertim.rs:147` reads the counter and `:188-194` emits it
**only when `> 0`**, as an `error!`. Zero is never printed. There is no
green-reading indicator to be made vacuous, so the doc-comment fix is the whole
of what was needed and no fifth finding arises here.

Two supporting checks on the same question:

- Consumers are complete: `grep -rn "get_verify_failures\|verify_failures" src/`
  returns only the field/init (`miner.rs:31/228`), the increment (`:755`), one
  test (`:1065`), and the `minertim.rs` display above. Nothing else reads it.
- The loud paths survive `RUST_LOG=warn`, which is the level at which the
  startup line and the per-worker `info!` line both disappear: `new_jit()`'s
  `error!` and the per-worker "requested but NOT active" `warn!` are both at or
  above `warn`. So in the exact scenario issue #4 describes, an operator running
  at the reduced level still gets told — twice. That is the strongest part of
  this fix.
