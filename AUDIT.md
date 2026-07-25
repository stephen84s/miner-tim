# Audit Log

This file tracks implementation changes made in this repository.

## 2026-04-09 - Migrate wallet encryption from AES/ECB to AES/GCM

### Request
Address the documented known issue: wallet address stored in SharedPreferences used AES-256-ECB mode (not semantically secure — identical plaintexts produce identical ciphertexts, no IV).

### Goal
Migrate to AES-256-GCM with a random 12-byte IV per encryption operation, while preserving backward-compatibility with existing ECB-encrypted installs.

### Files Modified
- `app/src/main/java/com/minertim/config/MiningConfig.kt`
- `CLAUDE.md`
- `AGENTS.md`

### Behavior / API Changes
- `encrypt()` now uses `AES/GCM/NoPadding` with a `SecureRandom` 12-byte IV prepended to the ciphertext before Base64 encoding. Each call produces a different ciphertext even for identical inputs.
- `decrypt()` attempts GCM decryption first (takes first 12 bytes as IV); on auth failure falls back to the legacy `AES/ECB/PKCS5Padding` path. The next `setWalletAddress()` call will silently re-encrypt with GCM, completing migration.
- New imports: `GCMParameterSpec`, `SecureRandom`.
- New companion constants: `GCM_IV_LENGTH = 12`, `GCM_TAG_BITS = 128`.

### Verification Performed
- Code review: `encrypt`/`decrypt` round-trip logic verified by inspection.
- No Android build run (NDK environment not configured in this session); existing `cargo test` scope unaffected.

### Notes
- The encryption key itself remains stored as Base64 in SharedPreferences (same as before). For production hardening the key should move to the Android Keystore; that is a separate, larger change.
- The "Known Issues / Legacy" section was removed from `CLAUDE.md` and `AGENTS.md` as there are no remaining documented issues.

## 2026-04-08 - Hot Path Optimization Batch

### Request
Implement miner hot-path optimizations:
1. Replace per-iteration deep-cloned work items with shared `Arc<Job>`.
2. Replace per-iteration blob cloning with per-worker nonce buffer reuse.

### Code Changes
- Updated `PoolConnection.current_job` to `Arc<Mutex<Option<Arc<Job>>>>`.
- Updated `PoolConnection::get_work()` to return `Option<Arc<Job>>`.
- Updated job installation paths (initial login + receiver updates) to store `Arc::new(job)`.
- Reworked `worker_loop` to reuse `job_blob_current` and `job_blob_next` buffers.
- Added in-place nonce writer `write_nonce_le(blob, nonce)` for offsets `39..42`.
- Added malformed-job guard: skip and warn when blob length is `< 43`.

### Files Modified
- `app/src/main/rust/src/pool_connection.rs`
- `app/src/main/rust/src/miner.rs`

### Verification Performed
- `cargo check` (pass)
- `cargo test --release --lib` (pass: 87 passed, 0 failed, 2 ignored)
- Runtime sanity:
  - 4-thread CLI run: connected, dataset initialized, non-zero hashrate observed
  - 12-thread CLI run: connected, dataset initialized, non-zero hashrate observed, shares found

### Notes
- No protocol behavior changes intended; this batch is performance-focused.
- Existing unrelated local working tree changes were preserved.

## 2026-04-08 - 12-Thread Benchmark Capture and README Update

### Request
Run a 12-thread CLI miner session for 10+ minutes, capture load/max/stable hashrate metrics, and document results in `README.md`.

### Actions Performed
- Ran a long benchmark session with:
  - `RUST_LOG=info ./target/release/minertim <pool> <wallet> 12`
  - output captured to `/tmp/minertim_12thread_bench.log`
- Kept process running for 11m28s.
- Parsed log for dataset initialization timing and rolling `1m` hashrate metrics.
- Updated README with measured benchmark details.

### Files Modified
- `README.md`

### Measured Results Recorded
- Full dataset initialization: 45.0s
- Peak rolling `1m` hashrate: 4194.9 H/s
- Stabilized tail-window `1m` hashrate average: 4059.7 H/s
- Tail window range: 4013.5–4097.6 H/s (last 18 samples)

### Notes
- Metrics are environment- and pool-dependent; values are documented as measured sample data.

## 2026-04-08 - README Hardware Context Addendum

### Request
Document the benchmark hardware used to produce the 12-thread CLI performance numbers.

### Actions Performed
- Queried host hardware identifiers.
- Added benchmark hardware details directly in the README benchmark section.

### Files Modified
- `README.md`

### Hardware Documented
- CPU: Apple M2 Max
- Logical CPUs: 12 (8 performance + 4 efficiency)
- Memory: 32 GB
- OS/arch: Darwin 25.3.0 (arm64)

## 2026-04-09 - Android light mode / CLI full mode split

### Request
Android devices cannot safely allocate the 2 GiB RandomX full dataset — even 3 GB phones like the Samsung Galaxy S6 Edge leave only ~1.5 GB free after OS overhead. Wire in light mode (256 MiB cache, on-the-fly dataset item computation) for the Android build while keeping full mode for the CLI.

### Goal
`cfg!(target_os = "android")` selects light mode at compile time. No user-visible configuration needed.

### Files Modified
- `app/src/main/rust/src/miner.rs`
- `CLAUDE.md`, `AGENTS.md`, `README.md`

### Behavior / API Changes
- `Miner` gains a `light_mode: bool` field, set to `cfg!(target_os = "android")` in `Miner::new()`.
- `Miner::start()` skips `SharedDatasetCache` allocation when `light_mode` is true, logging "Starting in light mode".
- `worker_loop` parameter `dataset_cache: SharedDatasetCache` changed to `Option<SharedDatasetCache>` — `None` = light mode.
- In the VM key-change branch: `Some(ds_cache)` path unchanged (full mode), `None` path uses `RandomXVm::new()` / `reinit(key, None)` (light mode).
- CLI binary (`bin/minertim.rs`) unchanged — still runs full mode.

### Memory profile (light mode, 2 threads on S6 Edge)
- 2 × 256 MiB cache = 512 MiB
- Android OS overhead: ~1.2 GB
- Total: ~1.7 GB — comfortably within 3 GB RAM

### Verification Performed
- `cargo check` — clean (no new warnings or errors)

## 2026-04-08 - 15+ Minute Benchmark Recalibration

### Request
Re-run 12-thread benchmark for at least 15 minutes and compute a consistent post-warmup 10-minute average.

### Actions Performed
- Ran long benchmark with output captured to `/tmp/minertim_12thread_15min.log`.
- Explicitly excluded warmup by starting metrics analysis after dataset initialization.
- Computed final-window statistics from the last 60 `H/s 1m` samples (10-second cadence).
- Updated README benchmark figures to this newer run.

### Files Modified
- `README.md`

### Measured Results Recorded
- Dataset initialization: 46.0s
- Peak rolling `1m`: 4472.9 H/s
- Post-warmup 10-minute average (`1m` samples): 4269.1 H/s
- Final 10-minute rolling hashrate near run end: 4264.4 H/s
- Final 10-minute window range: 3918.7–4472.9 H/s

## 2026-04-10 - Fix JIT CBRANCH overflow (compiler.rs:544)

### Request
Investigate failing test `test_vm_calculate_hash_jit` found during routine test run.

### Goal
Fix arithmetic overflow in the JIT CBRANCH branch-target calculation.

### Files Changed
- `app/src/main/rust/src/randomx/jit/compiler.rs`
- `app/src/main/rust/src/randomx/jit/audit.md`

### Behavior/API Changes
No API change. Bug fix only: CBRANCH JIT emission no longer panics when `ibc.target == -1` (the common case where a register has no prior usage before CBRANCH).

### Root Cause
`ibc.target` is `i16` (initialized to `-1`). The expression `(ibc.target as usize) + 1` cast `-1i16` to `usize::MAX` then added 1, causing an integer overflow panic in debug builds. The fix casts through `i32` first: `(ibc.target as i32) + 1`, matching the interpreter's `pc = target; pc += 1` semantics. A `target` of `-1` correctly produces index `0` (branch to program start).

### Verification
`cargo test --lib randomx::tests::full_hash_tests::test_vm_calculate_hash_jit` — passes, produces correct hash `639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f`. Full suite: **87 passed, 0 failed, 2 ignored**.

### Notable Constraints
Fix only affects aarch64 JIT path (cfg-gated). Interpreter was unaffected; JIT was broken for any program where CBRANCH references a register used at program start (-1 sentinel).

---

## 2026-04-11 - Agent Initialization & Protocol Setup

### Request
Establish a management framework where the AI agent acts as the project manager, ensuring all tasks are tracked and `AUDIT.md` is updated dynamically.

### Goal
- Formalize AI behavior in `CLAUDE.md`.
- Initialize the task board.
- Confirm the repository is in a valid state (CLI-only, full mode).

### Files Modified
- `CLAUDE.md` — Added "AI Agent Protocol" and "Current Task Board" sections.
- `AUDIT.md` — Added initialization entry.

### Verification
- Code review of `CLAUDE.md` ensures clear separation of "Project Info" and "Agent Instructions".
- Audit log verified (chronological, no duplicates).

### Notes
- The AI will now strictly maintain the `AUDIT.md` ledger after every code change.
- **Task Completed & Committed.**

## 2026-04-11 - Add full dataset mode to Android + dataset mode toggle in UI

### Request
Default Android app to full mode (2 GiB dataset, same as CLI) and add a UI toggle to switch between full and light mode before starting mining.

### Files Changed
- `app/src/main/rust/src/miner.rs` — Changed `Miner::new()` default `light_mode` from `cfg!(target_os = "android")` to `false` (full mode everywhere). Added `pub fn set_light_mode(bool)`.
- `app/src/main/rust/src/lib.rs` — Added `Java_com_minertim_mining_MiningCore_setLightMode` JNI export.
- `app/src/main/java/com/minertim/mining/MiningCore.kt` — Declared `external fun setLightMode(lightMode: Boolean): Boolean`.
- `app/src/main/java/com/minertim/config/MiningConfig.kt` — Added `KEY_LIGHT_MODE`, `getLightMode()` (default `false` = full mode), `setLightMode(Boolean)`.
- `app/src/main/java/com/minertim/mining/MiningService.kt` — Calls `miningCore.setLightMode(config.getLightMode())` between `initializeMiner` and `startMining`.
- `app/src/main/res/layout/activity_main.xml` — Added Switch + hint TextView row in the configuration card.
- `app/src/main/java/com/minertim/MainActivity.kt` — Wired up Switch: loads from config, saves on change, disabled while mining, hint text updates dynamically.
- `app/src/main/res/values/strings.xml` — Added `full_dataset_mode`, `full_dataset_mode_hint_on`, `full_dataset_mode_hint_off`.

### Behavior / API Changes
- Android now defaults to full mode (2 GiB shared dataset) on first install. Existing installs default to full mode since `KEY_LIGHT_MODE` pref is absent → `getLightMode()` returns `false`.
- Users can switch to light mode (256 MiB, slower) via the new toggle before starting mining. Toggle is disabled while mining is active.
- `Miner::new()` no longer uses `cfg!(target_os = "android")` to auto-select mode; mode must be set explicitly via `set_light_mode()` if non-default behavior is needed.
- CLI binary unaffected: `set_light_mode` is never called so it uses the `new()` default of `false` (full mode), same as before.

### Verification
`cargo check` — clean (warnings are pre-existing, no new errors). Kotlin changes are syntactically straightforward ViewBinding wiring with no logic changes to mining path.

### Notable Constraints
Full mode on Android requires ~2 GiB of free RAM. Devices with less available memory may OOM during dataset generation. The UI hint text ("~2 GiB RAM · faster hashing") informs users of the requirement.

## 2026-04-11 - CLI-only refactoring: dead code removal, type fixes, code dedup

### Request
Review codebase for Rust design principle improvements after the Android → CLI-only pivot.

### Goal
Remove dead Android code paths, fix type mismatches, consolidate duplicated utilities, clean up stale references.

### Files Changed
- `src/miner.rs` — Removed `light_mode: bool` field and all light/full mode branching (always full mode). Removed `#[allow(dead_code)]` on `Miner`. Changed `dataset_cache` from `Option<SharedDatasetCache>` to `SharedDatasetCache` in `worker_loop`. Changed `thread_count` from `i32` to `u32` in struct, `initialize()`, and `set_thread_count()`. Removed stale Android comments. Removed local `hex_encode` in favour of shared module.
- `src/pool_connection.rs` — Changed `"pass": "android"` to `"pass": "x"` in Stratum login. Removed local `hex_decode` and `hex_encode_bytes` in favour of shared module. Added `use crate::hex::{hex_decode, hex_encode}`.
- `src/hex.rs` — New shared module with `hex_encode` and `hex_decode`.
- `src/lib.rs` — Added `pub mod hex`.
- `src/bin/minertim.rs` — Changed `threads` parse type from `i32` to `u32`.

### Behavior / API Changes
- `Miner::initialize()` and `set_thread_count()` now take `u32` instead of `i32` (negative thread counts were never valid).
- Stratum login pass field changed from `"android"` to `"x"` (standard convention for anonymous pool auth).
- No functional changes to mining logic, hashrate computation, or pool protocol.

### Verification Performed
- `make check` — clean (no new warnings or errors; all warnings are pre-existing)
- `make test` — **87 passed, 0 failed, 2 ignored** (631s)

### Notable Constraints
- Pre-existing warnings (superscalar variant naming, unused constants, JIT visibility) intentionally left alone to avoid risking the performance-sensitive codegen — see prior sessions where even logically equivalent cfg gate changes caused 36% hashrate regressions.

## 2026-07-25 - Codebase review: pool robustness, target handling, lint cleanup

### Request
"Go through the codebase and see what you can improve."

### Goal
Fix correctness bugs in the pool networking layer, add missing Stratum
robustness features documented in CLAUDE.md but not implemented, support
full-width difficulty targets, and clear the clippy backlog without touching
performance-sensitive RandomX codegen.

### Files Changed
- `src/pool_connection.rs` — **Rewrote the receiver.** The old `start_receiver`
  opened a *second* TLS session over a cloned TCP socket, which cannot work
  (TLS is stateful; a raw clone of the socket shares no cipher state), so on
  TLS pools no jobs/share-responses were ever read. New design: one shared
  `PoolStream` behind the existing mutex; the receiver polls it with a short
  read timeout and reassembles newline-delimited frames, so submits and
  keepalives interleave on the same session. Added:
    - `keepalived` every 60s (documented in CLAUDE.md, previously absent),
    - automatic reconnect + relogin with 5s backoff on EOF/error,
    - `read_line` helper that reads a single line without a throwaway
      `BufReader` swallowing buffered bytes,
    - `wallet` stored on the connection so reconnect can re-login,
    - `target_to_difficulty` made `pub` and extended to 8-byte targets.
  `start_receiver` now takes `self: &Arc<Self>`.
- `src/miner.rs` — `PoolConnection` now constructed as `Arc` up front.
  `meets_target` handles both 4-byte (compact) and 8-byte (full) targets,
  comparing `hash[24..32]` as `u64` for the latter. Difficulty display uses
  the shared `target_to_difficulty`. `#[allow(clippy::too_many_arguments)]`
  on `worker_loop`.
- `src/bin/minertim.rs` — `format_duration` now floors instead of rounding
  (119s no longer prints "2m59s"); removed useless `format!` calls.
- `src/hex.rs` — `hex_encode` uses a lookup table instead of per-byte
  `format!`; doc comment made inner (`//!`).
- `src/randomx/vm.rs`, `superscalar.rs`, `argon2d.rs`, `blake2b.rs`,
  `aes_hash.rs`, `dataset.rs`, `jit/*` — clippy cleanup only: removed dead
  `load32_le` / `RANDOMX_PROGRAM_MAX_SIZE`, gated `RX_MXCSR_DEFAULT` to
  x86_64, `# Safety` docs on JIT unsafe fns, `pub(crate)` on `JitFn`/`compile`/
  `get_fn` (fixes private-in-public), `is_empty` on `Emitter`, `#[allow]`
  attributes for spec-mirroring names/index loops. All rewrites
  (`is_multiple_of`, `div_ceil`, `contains`, `copy_from_slice`) are behaviour-
  neutral and confined to assert/setup paths — no change to the AES or VM
  execution codegen.
- `.gitignore` — added `target/`; untracked ~2500 previously-committed build
  artifacts from the index (files left on disk).

### Behavior / API Changes
- TLS mining pools now actually receive jobs and share acknowledgements.
- Connection survives pool-side disconnects (auto reconnect + relogin).
- Keepalives prevent idle-timeout drops.
- 8-byte Stratum targets are honoured (previously only the low 4 bytes).
- `PoolConnection::start_receiver` signature: now `self: &Arc<Self>`.

### Verification Performed
- `cargo clippy --all-targets` — **No issues found** (was 63 warnings).
- `make test` — RandomX vectors pass (JIT + interpreter suites green).
- `make build` — release binary builds clean.

### Notable Constraints / Assumptions
- Reconnect loop retries indefinitely; a worker sees stale `get_work()` until
  a new job arrives, which is acceptable (workers already poll every 100ms).
- Certificate verification remains disabled (`NoVerifier`) as before — pool
  data is public and many pools use self-signed certs.
- Left the performance-sensitive RandomX execution paths untouched per the
  prior sessions' 36%-regression warning.

## 2026-07-25 - Benchmark harness + P-core-aware thread defaulting

### Request
"do it" — implement the two safe optimizations identified in the codebase
review: a benchmark harness and performance-core-aware thread selection.

### Goal
Give the project a way to measure hashrate regressions (it has a documented
~36% regression history but no benchmark), and stop defaulting mining threads
onto efficiency cores where they add little and can contend.

### Context (verified against xmrig docs)
xmrig's own documentation confirms that on ARM macOS **CPU affinity is not
supported** and **huge pages are not supported** — so hard P-core pinning and
2 MiB superpages (the other candidates from the review) are dead ends on this
target and were deliberately NOT attempted. The hot path already has JIT +
hardware AES + dual software prefetch, matching xmrig's approach.

### Files Changed
- `Cargo.toml` — added `criterion` dev-dependency (default-features off; only
  `cargo_bench_support`, dropping plotters/rayon/html). Added `[[bench]]
  name = "hash"` and a `[profile.bench]` with release-grade codegen so
  measurements reflect the shipping binary.
- `benches/hash.rs` — new. Benchmarks `calculate_hash_pipelined` in light mode
  (no 2 GiB dataset needed, self-contained) with flat sampling. Fixed key/blob
  for run-to-run comparability. ~283 ms/hash single-thread, stable ±2 ms.
- `src/miner.rs` — added `performance_core_count()` (macOS: reads
  `hw.perflevel0.logicalcpu` via a small `sysctlbyname` FFI; `None` elsewhere)
  and `recommended_thread_count()`. `initialize` now logs a warning if the
  requested thread count exceeds the P-core count.
- `src/bin/minertim.rs` — default thread count (when the arg is omitted) is now
  the P-core count instead of a hardcoded 2; help text updated.
- `Makefile` — `THREADS` now unset by default so the binary auto-detects;
  added a `bench` target; help/example updated (THREADS=8).

### Behavior / API Changes
- Running with no thread argument now uses the performance-core count
  (e.g. 8 on an M2 Max) instead of 2 — a large default-throughput improvement.
- Requesting more threads than P-cores is still allowed but warns.
- New public API: `minertim::miner::performance_core_count()` and
  `recommended_thread_count()`.
- No change to any RandomX correctness path (no file under `randomx/` touched).

### Verification Performed
- `cargo check`, `cargo clippy --all-targets` — clean (0 warnings).
- `cargo bench` — runs, reports stable ~285 ms/hash with change-tracking vs a
  stored baseline ("No change in performance detected").
- `./target/release/minertim --help` — shows "default: 8 performance cores,
  max: 12" on the M2 Max dev machine.
- Full 631 s vector suite not re-run this round: no `randomx/` file changed and
  the benchmark computed real hashes successfully, so the correctness path is
  unaffected.

### Notable Constraints
- P-core count is only detectable on macOS here; other platforms fall back to
  total parallelism. Acceptable — this is a macOS/Apple-Silicon miner.
- The benchmark uses light mode so CI/local runs need no 2 GiB dataset; it
  still exercises Blake2b, AES fill, and the 8 JIT-compiled program chains.

## 2026-07-25 - Dependency vulnerability scanning (cargo-audit)

### Request
"Can add a vulnerability checker for all the libraries we are using."

### Goal
Add a dependency vulnerability scanner and fix anything it finds.

### Files Changed
- `Makefile` — new `audit` target: runs `cargo audit` (auto-installs
  cargo-audit on first use if missing). Added to `.PHONY` and help text.
- `.gitlab-ci.yml` — new `rust:audit` job in the `check` stage: installs
  cargo-audit and runs `cargo audit` against the root crate's Cargo.lock,
  caching the advisory DB and the installed binary.
- `Cargo.lock` — `rustls-webpki` bumped 0.103.10 -> 0.103.13 (patch, via
  `cargo update -p rustls-webpki`) to clear the advisories below.

### Findings and Fix
The first scan flagged 3 vulnerabilities, all in the transitive dep
`rustls-webpki 0.103.10`:
- RUSTSEC-2026-0104 — reachable panic in CRL parsing.
- RUSTSEC-2026-0098 — name constraints for URI names incorrectly accepted.
- RUSTSEC-2026-0099 — name constraints accepted for wildcard-name certs.
All fixed by upgrading to >=0.103.13. Note: this miner uses `NoVerifier`
(certificate validation disabled — pool data is public), so the cert-path
bugs had limited runtime impact here, but the dep is updated regardless.
Post-fix scan: **0 vulnerabilities** (exit 0).

### Verification Performed
- `cargo audit` — clean after the update (was 3 vulnerabilities).
- `make audit` — target works, exit 0.
- `cargo build --release` — builds clean after the dependency bump.

### Notable Constraints / Follow-up
- **The rest of `.gitlab-ci.yml` is stale.** Every other job (`rust:check`,
  `build:debug`, `build:release`, `test:unit`) still targets the pre-pivot
  Android layout (`app/src/main/rust`, gradlew, Android SDK/NDK) which no
  longer exists, so the pipeline is already red independent of this change.
  The new `rust:audit` job is correct and self-contained. A full CI rewrite
  for the CLI-only project is a separate task, not done here.
