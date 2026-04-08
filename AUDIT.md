# Audit Log

This file tracks implementation changes made in this repository.

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
