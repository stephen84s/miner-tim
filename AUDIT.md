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
