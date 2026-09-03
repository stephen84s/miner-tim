use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::hex::hex_encode;
use crate::pool_connection::{target_to_difficulty, PoolConnection};
use crate::randomx::dataset::RandomXDataset;
use crate::randomx::vm::RandomXVm;

/// Shared dataset cache — generated once per seed_hash, shared across all workers.
struct DatasetCache {
    seed_hash: Vec<u8>,
    dataset: Arc<RandomXDataset>,
}

type SharedDatasetCache = Arc<Mutex<Option<DatasetCache>>>;

/// Tracks the best (lowest) hash top-4-bytes seen across all workers.
struct MiningStats {
    best_hash_val: AtomicU32,
    current_difficulty: AtomicU64,
    shares_found: AtomicU64,
    /// Epoch millis of last share found (0 = never)
    last_share_time_ms: AtomicU64,
    start_time_ms: AtomicU64,
    /// Shares the native loop produced that the reference path disagreed with.
    /// Must always be 0. Anything else means the JIT is miscomputing and the
    /// share was withheld rather than submitted.
    verify_failures: AtomicU64,
}

// ============================================================================
// Rolling hashrate tracker
// ============================================================================

/// A timestamped hash count sample.
struct HashSample {
    time: Instant,
    hashes: u64,
}

/// Tracks hashrate over rolling 1m, 5m, and 10m windows.
/// Call `record()` periodically (e.g. every 5s) with the current cumulative hash count.
pub struct HashrateTracker {
    samples: VecDeque<HashSample>,
}

impl HashrateTracker {
    fn new() -> Self {
        Self {
            samples: VecDeque::new(),
        }
    }

    fn record(&mut self, hashes: u64) {
        self.samples.push_back(HashSample {
            time: Instant::now(),
            hashes,
        });
        // Keep at most 10 minutes + a little margin of samples
        let cutoff = Instant::now() - std::time::Duration::from_secs(660);
        while self.samples.front().is_some_and(|s| s.time < cutoff) {
            self.samples.pop_front();
        }
    }

    /// Compute hashrate over the given window (seconds).
    /// Returns None if there aren't enough samples.
    fn rate_over(&self, window_secs: u64) -> Option<f64> {
        if self.samples.len() < 2 {
            return None;
        }
        let now = Instant::now();
        let cutoff = now - std::time::Duration::from_secs(window_secs);

        // Find the oldest sample within the window
        let oldest = self.samples.iter().find(|s| s.time >= cutoff)?;
        let newest = self.samples.back()?;

        let elapsed = newest.time.duration_since(oldest.time).as_secs_f64();
        if elapsed < 1.0 {
            return None;
        }
        let delta_hashes = newest.hashes.saturating_sub(oldest.hashes);
        Some(delta_hashes as f64 / elapsed)
    }
}

/// Snapshot of rolling hashrates returned to the caller.
pub struct HashrateSnapshot {
    pub rate_1m: Option<f64>,
    pub rate_5m: Option<f64>,
    pub rate_10m: Option<f64>,
}

/// Tracks shares found and timing for the CLI display.
pub struct ShareStats {
    pub total_found: u64,
    pub last_found_elapsed_secs: Option<f64>,
}


pub struct Miner {
    /// Whether workers use the native-loop JIT. Runtime kill switch: a JIT
    /// defect here does not crash, it silently produces wrong hashes, and the
    /// only symptom is shares being rejected by the pool. Being able to fall
    /// back without a rebuild is the point. See `RandomXVm::set_native_loop`.
    native_loop: bool,
    /// Re-check every candidate share on the reference path before submitting.
    verify_shares: bool,
    pool_connection: Option<Arc<PoolConnection>>,
    workers: Vec<JoinHandle<()>>,
    thread_count: u32,
    hashrate_bits: Arc<AtomicU64>,
    mining_active: Arc<AtomicBool>,
    start_time: Option<Instant>,
    total_hashes: Arc<AtomicU64>,
    stats: Option<Arc<MiningStats>>,
    hashrate_tracker: HashrateTracker,
}

impl Miner {
    pub fn new(mining_active: Arc<AtomicBool>) -> Self {
        Self {
            native_loop: true,
            verify_shares: true,
            pool_connection: None,
            workers: Vec::new(),
            thread_count: 2,
            hashrate_bits: Arc::new(AtomicU64::new(0)),
            mining_active,
            start_time: None,
            total_hashes: Arc::new(AtomicU64::new(0)),
            stats: None,
            hashrate_tracker: HashrateTracker::new(),
        }
    }

    /// Enable or disable the native-loop JIT for all workers. Must be called
    /// before [`Miner::start`]; workers capture the value when they spawn.
    ///
    /// Only has an effect on aarch64 with rx/0 in full mode — every other
    /// configuration runs the per-iteration body JIT or the interpreter
    /// regardless.
    pub fn set_native_loop(&mut self, enabled: bool) {
        self.native_loop = enabled;
    }

    /// Re-check every candidate share on the reference (body-JIT) path before
    /// submitting it, and withhold any share the two paths disagree on.
    ///
    /// Costs one extra hash per share found — roughly 0.005% of mining time,
    /// because shares are rare. Has no effect when the native loop is off,
    /// since then the mining path already *is* the reference path.
    pub fn set_verify_shares(&mut self, enabled: bool) {
        self.verify_shares = enabled;
    }

    pub fn initialize(
        &mut self,
        pool: &str,
        wallet: &str,
        threads: u32,
        donate_level: u8,
    ) -> Result<(), String> {
        let max_threads = thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4);
        self.thread_count = threads.clamp(1, max_threads);

        // Using every core for mining starves the pool receiver thread: it can't
        // update the current job promptly, so shares are submitted against
        // superseded jobs and rejected as "Invalid job id". Measured on M2 Max,
        // all 12 cores gives ~15% rejects while 11 gives ~0% at the same hashrate.
        // Leave one core free.
        if self.thread_count >= max_threads && max_threads > 1 {
            log::warn!(
                "Using all {} cores leaves none for the pool receiver, which causes \
                 stale-share (\"Invalid job id\") rejects under load. Consider \
                 THREADS={} — near-zero rejects at essentially the same hashrate.",
                max_threads,
                max_threads - 1,
            );
        }

        let connection = Arc::new(PoolConnection::new(donate_level));
        connection.connect(pool).map_err(|e| format!("Connection failed: {}", e))?;
        connection.login(wallet).map_err(|e| format!("Login failed: {}", e))?;
        connection.start_receiver();

        connection.reset_share_counters();
        self.pool_connection = Some(connection);
        self.total_hashes.store(0, Ordering::SeqCst);
        self.hashrate_bits.store(0, Ordering::SeqCst);

        log::info!(
            "Miner initialized: pool={}, threads={}",
            pool,
            self.thread_count
        );
        Ok(())
    }

    pub fn start(&mut self) -> Result<(), String> {
        let pool = match &self.pool_connection {
            Some(p) => p.clone(),
            None => return Err("Pool connection not initialized".into()),
        };

        self.start_time = Some(Instant::now());
        self.total_hashes.store(0, Ordering::SeqCst);

        let thread_count = self.thread_count;
        let dataset_cache: SharedDatasetCache = Arc::new(Mutex::new(None));
        log::info!("Starting in full mode (2 GiB dataset, shared across workers)");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let stats = Arc::new(MiningStats {
            best_hash_val: AtomicU32::new(u32::MAX),
            current_difficulty: AtomicU64::new(0),
            shares_found: AtomicU64::new(0),
            last_share_time_ms: AtomicU64::new(0),
            start_time_ms: AtomicU64::new(now_ms),
            verify_failures: AtomicU64::new(0),
        });

        for thread_id in 0..thread_count {
            let mining_active = self.mining_active.clone();
            let pool_conn = pool.clone();
            let total_hashes = self.total_hashes.clone();
            let hashrate_bits = self.hashrate_bits.clone();
            let ds_cache = dataset_cache.clone();
            let native_loop = self.native_loop;
            let verify_shares = self.verify_shares;

            let stats = stats.clone();

            let handle = thread::Builder::new()
                .name(format!("miner-worker-{}", thread_id))
                .spawn(move || {
                    worker_loop(
                        thread_id,
                        thread_count,
                        mining_active,
                        pool_conn,
                        total_hashes,
                        hashrate_bits,
                        ds_cache,
                        stats,
                        native_loop,
                        verify_shares,
                    );
                })
                .map_err(|e| format!("Failed to spawn worker {}: {}", thread_id, e))?;

            self.workers.push(handle);
        }

        self.stats = Some(stats);

        log::info!("Started {} mining worker threads", thread_count);
        Ok(())
    }

    /// Get the current pool difficulty (0 if not yet known).
    pub fn get_difficulty(&self) -> u64 {
        self.stats.as_ref()
            .map(|s| s.current_difficulty.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Get the best (lowest) hash value seen so far. Lower = closer to finding a share.
    pub fn get_best_hash_val(&self) -> u32 {
        self.stats.as_ref()
            .map(|s| s.best_hash_val.load(Ordering::Relaxed))
            .unwrap_or(u32::MAX)
    }

    /// Get share timing stats for display.
    /// Number of shares withheld because the reference path disagreed with the
    /// native loop. Any non-zero value means the JIT is producing wrong hashes.
    ///
    /// Zero is NOT by itself evidence that the JIT is correct: it is also what
    /// a worker reports when verification is disarmed — `--verify-shares off`,
    /// or a mining VM that is not on the native loop at all (issue #4). The
    /// per-worker startup line says which of the two a given run is in.
    pub fn get_verify_failures(&self) -> u64 {
        self.stats.as_ref()
            .map(|s| s.verify_failures.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub fn get_share_stats(&self) -> ShareStats {
        let stats = match &self.stats {
            Some(s) => s,
            None => return ShareStats { total_found: 0, last_found_elapsed_secs: None },
        };
        let total_found = stats.shares_found.load(Ordering::Relaxed);
        let last_ms = stats.last_share_time_ms.load(Ordering::Relaxed);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let elapsed = if last_ms > 0 {
            Some((now_ms - last_ms) as f64 / 1000.0)
        } else {
            // Time since mining started
            let start_ms = stats.start_time_ms.load(Ordering::Relaxed);
            if start_ms > 0 {
                Some((now_ms - start_ms) as f64 / 1000.0)
            } else {
                None
            }
        };
        ShareStats { total_found, last_found_elapsed_secs: elapsed }
    }

    /// Record a sample and return rolling hashrate snapshots.
    pub fn snapshot_hashrates(&mut self) -> HashrateSnapshot {
        let hashes = self.total_hashes.load(Ordering::Relaxed);
        self.hashrate_tracker.record(hashes);
        HashrateSnapshot {
            rate_1m: self.hashrate_tracker.rate_over(60),
            rate_5m: self.hashrate_tracker.rate_over(300),
            rate_10m: self.hashrate_tracker.rate_over(600),
        }
    }

    pub fn stop(&mut self) {
        self.mining_active.store(false, Ordering::SeqCst);

        let workers = std::mem::take(&mut self.workers);
        for handle in workers {
            if let Err(e) = handle.join() {
                log::error!("Worker thread panicked: {:?}", e);
            }
        }

        log::info!("All mining workers stopped");
    }

    pub fn get_hashrate(&self) -> f64 {
        f64::from_bits(self.hashrate_bits.load(Ordering::Relaxed))
    }

    pub fn get_accepted_shares(&self) -> u32 {
        self.pool_connection
            .as_ref()
            .map(|p| p.get_accepted_shares())
            .unwrap_or(0)
    }

    pub fn get_rejected_shares(&self) -> u32 {
        self.pool_connection
            .as_ref()
            .map(|p| p.get_rejected_shares())
            .unwrap_or(0)
    }

    pub fn set_thread_count(&mut self, count: u32) {
        let max_threads = thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4);
        self.thread_count = count.clamp(1, max_threads);
        log::info!("Thread count set to {}", self.thread_count);
    }
}

/// Owns the reference-path VM used to re-check candidate shares.
///
/// This exists as a type purely so its state machine is testable. It previously
/// lived as three loose locals inside `worker_loop`, which needs a live pool
/// connection and a 2 GiB dataset, so the two things most likely to go wrong —
/// the lazy build and the reset on seed rotation — could not be exercised at
/// all. A stale verifier surviving a rotation would withhold **every** share
/// from that point on, which is the worst failure this feature can have: it
/// looks exactly like the JIT fault it is supposed to detect.
pub(crate) struct ShareVerifier {
    /// Built on the first share, not on construction: most workers never find
    /// one for a given seed, and building costs a VM plus a 2 MiB scratchpad.
    vm: Option<RandomXVm>,
    /// The dataset the current seed resolved to. `None` before the first job.
    dataset: Option<Arc<RandomXDataset>>,
    key: Vec<u8>,
    enabled: bool,
}

impl ShareVerifier {
    pub(crate) fn new(enabled: bool) -> Self {
        Self { vm: None, dataset: None, key: Vec::new(), enabled }
    }

    /// Point the verifier at a new seed and its dataset.
    ///
    /// **Drops any cached VM.** In full mode the *dataset* is what determines
    /// the hash — the key reaches `RandomXVm` only through `cache_memory` and
    /// `ss_programs`, and the sole read of either during hashing is the
    /// light-mode arm of `execute_vm_inner`'s `match dataset`. So a VM held
    /// across a rotation would verify against the previous seed's data and
    /// disagree with every share the miner found.
    ///
    /// That holds only while the full/light split stays absolute. A future
    /// lazily-filled dataset with a compute-on-miss path would make the key
    /// load-bearing again and silently weaken the rotation test, which
    /// deliberately leans on the dataset being the only staleness vector
    /// (review round 10).
    pub(crate) fn rekey(&mut self, key: &[u8], dataset: Arc<RandomXDataset>) {
        self.vm = None;
        self.dataset = Some(dataset);
        self.key.clear();
        self.key.extend_from_slice(key);
    }

    /// The reference hash for `blob`, or `None` if verification does not apply
    /// — disabled, or no dataset seen yet.
    ///
    /// `calculate_hash` refills the scratchpad from the blob, so this neither
    /// disturbs nor is disturbed by the mining VM's pipeline state.
    pub(crate) fn reference(&mut self, blob: &[u8]) -> Option<[u8; 32]> {
        if !self.enabled {
            return None;
        }
        let ds = self.dataset.clone()?;
        let key = &self.key;
        let vm = self.vm.get_or_insert_with(|| {
            let mut v = RandomXVm::new_full(key, ds);
            v.set_native_loop(false);
            v
        });
        Some(vm.calculate_hash(blob))
    }

    /// Arm or disarm the verifier from the mining VM's *actual* state.
    ///
    /// Called once the worker's VM exists, because that is the first moment all
    /// four native-loop preconditions are knowable. Verification is only
    /// meaningful when the mining path differs from the reference path, so a VM
    /// that fell back to the interpreter — a failed `mmap(MAP_JIT)`, a non-v1
    /// program, light mode, or a non-aarch64 build — disarms it rather than
    /// letting it compare the interpreter against itself and report a clean
    /// counter forever (issues #4 and #3).
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Whether verification is switched on for this worker.
    ///
    /// Deliberately **not** `enabled && dataset.is_some()`. Using the stricter
    /// form as `classify_share`'s first argument made
    /// `SubmitVerifierUnavailable` unreachable — `is_armed` would have implied
    /// `reference()` is `Some` — which silently retired the fail-open branch
    /// instead of enforcing it (review round 9, R9-F1). No share was at risk,
    /// since both verdicts submit, but a defence that cannot be reached is not
    /// a defence.
    ///
    /// To be precise about what this buys: the arm is still not reachable from
    /// `worker_loop` as the code stands, because a share cannot exist before
    /// `rekey` has run. Restoring the distinction makes it reachable *in
    /// principle*, so a future edit that separates those two facts fails open
    /// rather than silently submitting unverified (R11-F2).
    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Whether a mismatch could actually be detected right now: switched on
    /// *and* holding a dataset. Test-only — deliberately not what
    /// `worker_loop` passes to `classify_share`; see `is_enabled`.
    #[cfg(test)]
    pub(crate) fn is_armed(&self) -> bool {
        self.enabled && self.dataset.is_some()
    }

    #[cfg(test)]
    pub(crate) fn has_cached_vm(&self) -> bool {
        self.vm.is_some()
    }

    /// Whether the cached VM is on the reference path. `None` if none is built.
    /// See `RandomXVm::uses_native_loop` for why this is asserted rather than
    /// assumed (R9-F7).
    #[cfg(test)]
    pub(crate) fn vm_is_on_reference_path(&self) -> Option<bool> {
        self.vm.as_ref().map(|v| !v.uses_native_loop())
    }

    #[cfg(test)]
    pub(crate) fn holds_dataset(&self, other: &Arc<RandomXDataset>) -> bool {
        self.dataset.as_ref().is_some_and(|d| Arc::ptr_eq(d, other))
    }
}

/// What to do with a candidate share once the reference path has had its say.
///
/// Split out of `worker_loop` because that function needs a live pool
/// connection and a 2 GiB dataset, which makes its branches untestable. This
/// carries the whole decision and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareVerdict {
    /// Verification did not apply — either switched off, or the native loop is
    /// off and the mining path already *is* the reference path, so checking
    /// would compare it against itself.
    SubmitUnverified,
    /// The reference path agreed. Normal case.
    SubmitVerified,
    /// Verification applied but the verifier could not run. **Fails open**: a
    /// genuine share is worth more than the check, and withholding here would
    /// turn a bookkeeping slip into lost revenue.
    SubmitVerifierUnavailable,
    /// The two paths disagree. The JIT is miscomputing; do not submit.
    Withhold,
}

impl ShareVerdict {
    pub(crate) fn should_submit(self) -> bool {
        !matches!(self, ShareVerdict::Withhold)
    }
}

pub(crate) fn classify_share(
    verification_applies: bool,
    mined: &[u8; 32],
    reference: Option<&[u8; 32]>,
) -> ShareVerdict {
    if !verification_applies {
        return ShareVerdict::SubmitUnverified;
    }
    match reference {
        None => ShareVerdict::SubmitVerifierUnavailable,
        Some(r) if r == mined => ShareVerdict::SubmitVerified,
        Some(_) => ShareVerdict::Withhold,
    }
}

#[allow(clippy::too_many_arguments)]
fn worker_loop(
    thread_id: u32,
    thread_count: u32,
    mining_active: Arc<AtomicBool>,
    pool: Arc<PoolConnection>,
    total_hashes: Arc<AtomicU64>,
    hashrate_bits: Arc<AtomicU64>,
    dataset_cache: SharedDatasetCache,
    stats: Arc<MiningStats>,
    native_loop: bool,
    verify_shares: bool,
) {
    log::info!("Worker {} started", thread_id);

    let mut vm: Option<RandomXVm> = None;
    let mut current_key: Vec<u8> = Vec::new();
    let mut current_job_id = String::new();
    let mut job_blob_current: Vec<u8> = Vec::new();
    let mut job_blob_next: Vec<u8> = Vec::new();
    let mut warned_short_blob_for_job = false;
    let mut nonce: u64 = thread_id as u64;
    let mut local_hashes: u64 = 0;
    let start_time = Instant::now();
    let mut last_hashrate_update = Instant::now();
    let mut pipeline_ready = false;
    // Reference-path verifier. Built lazily on the first share, and re-pointed
    // whenever the seed rotates — see `ShareVerifier`.
    //
    // Starts DISARMED regardless of the switches: whether verification can
    // detect anything depends on the mining VM's real state, which does not
    // exist yet. It is armed from `native_loop_effective()` below, before any
    // share can be found. Composing it here from `verify_shares && native_loop`
    // was how it came to be armed on x86_64 against a mining path that was
    // already the reference path (issue #3), and how a failed JIT allocation
    // left it comparing the interpreter against itself (issue #4).
    let mut verifier = ShareVerifier::new(false);
    // The effective-state line is per worker and printed once, not per rotation.
    let mut reported_effective_state = false;

    while mining_active.load(Ordering::Relaxed) {
        let job = match pool.get_work() {
            Some(job) => job,
            None => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        if job.job_id != current_job_id {
            current_job_id = job.job_id.clone();
            pipeline_ready = false;
            warned_short_blob_for_job = false;

            job_blob_current.clear();
            job_blob_current.extend_from_slice(&job.blob);
            job_blob_next.clear();
            job_blob_next.extend_from_slice(&job.blob);
        }

        // Reinitialize VM if the seed hash changed
        if job.seed_hash != current_key || vm.is_none() {
            let dataset = get_or_generate_dataset(&dataset_cache, &job.seed_hash, thread_id);
            log::info!("Worker {} ready with full dataset", thread_id);
            if let Some(ref mut existing_vm) = vm {
                existing_vm.reinit(&job.seed_hash, Some(dataset.clone()));
            } else {
                let mut new_vm = RandomXVm::new_full(&job.seed_hash, dataset.clone());
                // `reinit` keeps the flag, so this only needs setting on the
                // VM's first construction.
                new_vm.set_native_loop(native_loop);
                vm = Some(new_vm);
            }
            // Re-point the verifier at the new seed. This must happen on every
            // rotation: in full mode the dataset determines the hash, so a
            // verifier left on the old one would reject every share found.
            verifier.rekey(&job.seed_hash, dataset);

            // Arm the verifier from what the VM actually does, and say so.
            //
            // All four guard terms are fixed for this VM's lifetime — version
            // and JIT at construction, the flag once, and `reinit` above is
            // always passed a dataset — so re-deriving on every rotation is
            // free insurance rather than a necessity. It stays correct if a
            // future edit ever calls `reinit(key, None)`.
            let native_effective =
                vm.as_ref().is_some_and(|v| v.native_loop_effective());
            verifier.set_enabled(verify_shares && native_effective);
            if !reported_effective_state {
                reported_effective_state = true;
                let on_off = |b: bool| if b { "on" } else { "off" };
                log::info!(
                    "Worker {}: native-loop JIT {} | share verification {}",
                    thread_id,
                    on_off(native_effective),
                    on_off(verify_shares && native_effective),
                );
                if native_loop && !native_effective {
                    // The loud path. The startup line announced the native loop
                    // and this worker is not running it, which also means its
                    // share verification measures nothing (issue #4, R13-F2).
                    // Deliberately says nothing that depends on the target.
                    // Re-deriving "does this build even have a native loop?"
                    // here would put a `cfg!` term back into the reporting of
                    // an enablement decision, which is precisely issue #3. So
                    // the text lists every cause instead of predicting one.
                    log::warn!(
                        "Worker {}: native-loop JIT was requested but is NOT active — \
                         running the per-iteration body JIT / interpreter. Share \
                         verification is off for it, because the mining path and the \
                         reference path are now the same and a zero failure count would \
                         mean nothing. Possible causes: this build targets an \
                         architecture with no native loop (nothing is lost there — it \
                         has none to run), the program is not rx/0, the VM is in light \
                         mode, or `mmap(MAP_JIT)` failed (logged as a 'JIT allocation \
                         failed' error above). Wherever the native loop does exist, \
                         running without it costs this worker a large fraction of its \
                         hashrate.",
                        thread_id
                    );
                }
            }

            current_key = job.seed_hash.clone();
            pipeline_ready = false;
        }

        let rx_vm = vm.as_mut().unwrap();

        if job_blob_current.len() < 43 || job_blob_next.len() < 43 {
            if !warned_short_blob_for_job {
                log::warn!(
                    "Skipping malformed job {}: blob too short ({} bytes, expected >= 43)",
                    job.job_id,
                    job.blob.len()
                );
                warned_short_blob_for_job = true;
            }
            thread::sleep(std::time::Duration::from_millis(100));
            continue;
        }

        // Overlap final hashing of the current nonce with scratchpad fill for the next one.
        let next_nonce = nonce + thread_count as u64;
        write_nonce_le(&mut job_blob_current, nonce as u32);
        write_nonce_le(&mut job_blob_next, next_nonce as u32);

        let hash = if pipeline_ready {
            rx_vm.calculate_hash_pipelined(&job_blob_next)
        } else {
            rx_vm.prepare_scratchpad(&job_blob_current);
            pipeline_ready = true;
            rx_vm.calculate_hash_pipelined(&job_blob_next)
        };
        local_hashes += 1;
        nonce += thread_count as u64;

        // Track best hash and difficulty for progress reporting
        let hash_val = u32::from_le_bytes([hash[28], hash[29], hash[30], hash[31]]);
        let _ = stats.best_hash_val.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            if hash_val < current { Some(hash_val) } else { None }
        });
        let difficulty = target_to_difficulty(&job.target);
        if difficulty > 0 {
            stats.current_difficulty.store(difficulty, Ordering::Relaxed);
        }

        // Compare hash to target (little-endian comparison)
        if meets_target(&hash, &job.target) {
            let nonce_hex = hex_encode(&job_blob_current[39..43]);
            let result_hex = hex_encode(&hash);

            // Record share timing
            stats.shares_found.fetch_add(1, Ordering::Relaxed);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            stats.last_share_time_ms.store(now_ms, Ordering::Relaxed);

            log::info!(
                "Worker {} SHARE FOUND! job_id={}, nonce={}, hash_val={}, difficulty={}, hash={}",
                thread_id,
                job.job_id,
                nonce_hex,
                hash_val,
                difficulty,
                result_hex
            );

            // Re-check the share on the reference path before submitting.
            //
            // A JIT defect here does not crash and does not corrupt memory — it
            // produces a wrong-but-plausible hash, so the miner keeps reporting a
            // healthy hashrate while the pool bins everything it sends. Shares are
            // rare (one per ~2-20 s for the whole rig), so recomputing exactly the
            // ones we are about to submit costs on the order of 0.005% of mining
            // time and turns that silent failure into a loud one.
            //
            // Skipped when the native loop is off: the mining path is then already
            // the reference path, so this would compare it against itself.
            //
            // LIMIT, so nobody mistakes this for a general correctness net: the
            // two paths are not independent. Both run `emit_body`, so a defect
            // in the shared instruction emitter produces the same wrong hash on
            // both sides and passes. What this catches is defects in the
            // native-loop scaffolding specifically — the prologue, the
            // per-iteration pre/post, the loop control and the register
            // residency — which is where all the new code in MR !1 lives.
            // (MR !1 review round 7, R7-F6.)
            // The decision itself lives in `classify_share` so every branch is
            // reachable from a test; only the expensive recomputation is here.
            let reference = verifier.reference(&job_blob_current);
            // `is_enabled`, not `is_armed`: the distinction keeps the
            // "verification wanted but unavailable" case *logically* reachable
            // so it fails open loudly instead of being folded into "not
            // verified".
            //
            // It is not reachable in this binary today, and the comment says so
            // rather than implying coverage: `vm` is assigned only inside the
            // block that calls `rekey`, so by the time a share exists the
            // verifier always holds a dataset. This is a guard for future
            // edits, pinned by `an_enabled_but_unfed_verifier_fails_open`
            // (review round 11, R11-F2).
            let verdict = classify_share(verifier.is_enabled(), &hash, reference.as_ref());
            match verdict {
                ShareVerdict::Withhold => {
                    stats.verify_failures.fetch_add(1, Ordering::Relaxed);
                    log::error!(
                        "Worker {} WITHHELD a share: the native-loop JIT and the \
                         reference path disagree. native={} reference={} job_id={} \
                         nonce={}. This is a JIT correctness failure — restart with \
                         --native-loop off (or NATIVE_LOOP=off in mining.conf) and \
                         report it.",
                        thread_id,
                        result_hex,
                        reference.as_ref().map(|r| hex_encode(r)).unwrap_or_default(),
                        job.job_id,
                        nonce_hex,
                    );
                }
                ShareVerdict::SubmitVerifierUnavailable => {
                    log::warn!(
                        "Worker {} submitting a share unverified: no dataset recorded \
                         for the verifier. This should not happen.",
                        thread_id
                    );
                }
                ShareVerdict::SubmitVerified | ShareVerdict::SubmitUnverified => {}
            }
            let verified = verdict.should_submit();

            if verified
                && let Err(e) = pool.submit_share(&job.job_id, &nonce_hex, &result_hex)
            {
                log::error!("Failed to submit share: {}", e);
            }
        }

        // All threads flush their local hashes periodically
        if last_hashrate_update.elapsed().as_secs() >= 5 {
            total_hashes.fetch_add(local_hashes, Ordering::Relaxed);
            local_hashes = 0;
            last_hashrate_update = Instant::now();

            // Thread 0 computes and stores the hashrate
            if thread_id == 0 {
                let global_hashes = total_hashes.load(Ordering::Relaxed);
                let elapsed = start_time.elapsed().as_secs_f64();
                if elapsed > 0.0 {
                    let rate = global_hashes as f64 / elapsed;
                    hashrate_bits.store(rate.to_bits(), Ordering::Relaxed);
                }
            }
        }
    }

    // Flush remaining local hashes
    total_hashes.fetch_add(local_hashes, Ordering::Relaxed);

    log::info!("Worker {} stopped", thread_id);
}

/// Get a cached dataset or generate one. The first thread to encounter a new
/// seed_hash generates the dataset (using all CPU cores); other threads block
/// until it's ready.
fn get_or_generate_dataset(
    cache: &SharedDatasetCache,
    seed_hash: &[u8],
    thread_id: u32,
) -> Arc<RandomXDataset> {
    let mut guard = cache.lock().unwrap();

    // Already generated for this seed_hash?
    if let Some(ref cached) = *guard
        && cached.seed_hash == seed_hash
    {
        return cached.dataset.clone();
    }

    // We're the first thread — generate the dataset
    log::info!(
        "Worker {} generating full RandomX dataset (~2 GiB)...",
        thread_id,
    );
    let gen_start = Instant::now();

    // Build cache + programs (needed for dataset generation)
    let vm = RandomXVm::new(seed_hash);
    let (cache_memory, ss_programs) = vm.cache_and_programs();

    let num_cpus = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    let dataset = Arc::new(RandomXDataset::generate(cache_memory, ss_programs, num_cpus));

    log::info!(
        "Dataset generated in {:.1}s",
        gen_start.elapsed().as_secs_f64(),
    );

    *guard = Some(DatasetCache {
        seed_hash: seed_hash.to_vec(),
        dataset: dataset.clone(),
    });

    dataset
}

/// Compare a 32-byte hash against a target in little-endian order.
/// The hash meets the target if it is less than or equal to the expanded target.
/// Pools send either a 4-byte compact target (upper 32 bits of the threshold)
/// or a full 8-byte target (upper 64 bits).
fn meets_target(hash: &[u8; 32], target: &[u8]) -> bool {
    if target.len() >= 8 {
        let target_val = u64::from_le_bytes(target[0..8].try_into().unwrap());
        if target_val == 0 {
            return false;
        }
        let hash_val = u64::from_le_bytes(hash[24..32].try_into().unwrap());
        return hash_val <= target_val;
    }
    if target.len() >= 4 {
        let target_val = u32::from_le_bytes(target[0..4].try_into().unwrap());
        if target_val == 0 {
            return false;
        }
        let hash_val = u32::from_le_bytes(hash[28..32].try_into().unwrap());
        return hash_val <= target_val;
    }
    false
}

#[inline(always)]
fn write_nonce_le(blob: &mut [u8], nonce: u32) {
    let nonce_bytes = nonce.to_le_bytes();
    blob[39] = nonce_bytes[0];
    blob[40] = nonce_bytes[1];
    blob[41] = nonce_bytes[2];
    blob[42] = nonce_bytes[3];
}

/// Number of performance ("P") cores, if the platform can report it.
/// On Apple Silicon macOS this reads `hw.perflevel0.logicalcpu`; other
/// platforms return `None` (callers fall back to total parallelism).
#[cfg(target_os = "macos")]
pub fn performance_core_count() -> Option<u32> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_void};

    unsafe extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> c_int;
    }

    let name = CString::new("hw.perflevel0.logicalcpu").ok()?;
    let mut value: i32 = 0;
    let mut len = std::mem::size_of::<i32>();
    let ret = unsafe {
        sysctlbyname(
            name.as_ptr(),
            &mut value as *mut i32 as *mut c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret == 0 && value > 0 {
        Some(value as u32)
    } else {
        None
    }
}

#[cfg(not(target_os = "macos"))]
pub fn performance_core_count() -> Option<u32> {
    None
}

/// Recommended default thread count: one fewer than the number of logical cores,
/// leaving a core free for the pool receiver thread (which otherwise starves under
/// full-core mining and causes stale-share rejects). Measured on M2 Max, this
/// matches all-core hashrate with ~0% rejects instead of ~15%.
pub fn recommended_thread_count() -> u32 {
    let total = thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4);
    total.saturating_sub(1).max(1)
}

#[cfg(test)]
mod verify_tests {
    use super::*;

    const A: [u8; 32] = [0xAA; 32];
    const B: [u8; 32] = [0xBB; 32];

    /// Every branch of `classify_share`. The mismatch branch in particular has
    /// no way to occur in production without a JIT defect, so it would
    /// otherwise ship having never executed — which is exactly how the last
    /// several defects in this feature got in.
    #[test]
    fn classify_share_covers_every_branch() {
        // Verification off (or native loop off): submit, do not consult the
        // reference even if one is somehow present.
        assert_eq!(classify_share(false, &A, None), ShareVerdict::SubmitUnverified);
        assert_eq!(classify_share(false, &A, Some(&B)), ShareVerdict::SubmitUnverified);

        // Applies, verifier unavailable: fail OPEN. A genuine share is worth
        // more than the check.
        assert_eq!(classify_share(true, &A, None), ShareVerdict::SubmitVerifierUnavailable);

        // Applies, paths agree: the normal path.
        assert_eq!(classify_share(true, &A, Some(&A)), ShareVerdict::SubmitVerified);

        // Applies, paths disagree: withhold. This is the branch that has never
        // run in the field.
        assert_eq!(classify_share(true, &A, Some(&B)), ShareVerdict::Withhold);
    }

    /// Only `Withhold` blocks submission. Asserted separately from the
    /// classification so a future variant cannot be added and silently default
    /// to blocking shares — the failure mode that costs money.
    #[test]
    fn only_a_mismatch_blocks_submission() {
        assert!(ShareVerdict::SubmitUnverified.should_submit());
        assert!(ShareVerdict::SubmitVerified.should_submit());
        assert!(ShareVerdict::SubmitVerifierUnavailable.should_submit());
        assert!(!ShareVerdict::Withhold.should_submit());
    }

    /// A single differing byte, anywhere, must withhold — the realistic shape
    /// of a JIT fault is one wrong limb, not a wholly different hash.
    #[test]
    fn a_single_differing_byte_is_enough_to_withhold() {
        for i in 0..32 {
            let mut near = A;
            near[i] ^= 0x01;
            assert_eq!(
                classify_share(true, &A, Some(&near)),
                ShareVerdict::Withhold,
                "byte {i} differing was not caught"
            );
        }
    }

    /// The fail-open arm, composed the way `worker_loop` composes it.
    ///
    /// Round 9 restored the *independence* of `classify_share`'s two arguments
    /// by passing `is_enabled()` rather than `is_armed()`. Round 10 pointed out
    /// that this makes the arm reachable as a matter of logic but still not in
    /// the current binary, because `vm.is_some()` implies `rekey` has run — so
    /// the defence is real only for future edits. This pins the composition
    /// itself, with no dataset and in microseconds, so the arm cannot quietly
    /// become unsatisfiable again (R10-F1).
    #[test]
    fn an_enabled_but_unfed_verifier_fails_open() {
        let mut v = ShareVerifier::new(true);
        assert!(v.is_enabled(), "enabled verifier reported itself disabled");
        assert!(!v.is_armed(), "armed with no dataset");

        let reference = v.reference(&[0u8; 76]);
        assert_eq!(reference, None, "produced a reference with no dataset");

        assert_eq!(
            classify_share(v.is_enabled(), &A, reference.as_ref()),
            ShareVerdict::SubmitVerifierUnavailable,
            "verification was wanted but unavailable; this must fail OPEN and \
             be reported, not be folded into the unverified case"
        );
        assert!(
            classify_share(v.is_enabled(), &A, reference.as_ref()).should_submit(),
            "a genuine share was withheld because the verifier was unavailable"
        );
    }

    /// Arming is a decision taken from the mining VM, not from the switches.
    ///
    /// `worker_loop` now constructs the verifier disarmed and calls
    /// `set_enabled(verify_shares && vm.native_loop_effective())` once the VM
    /// exists. This pins both ends of that: a disarmed verifier must fold into
    /// the unverified verdict rather than the unavailable one (it is not that
    /// the check failed — no check was wanted), and arming must restore the
    /// fail-open arm (issues #4 and #3).
    #[test]
    fn arming_follows_the_vm_not_the_switches() {
        let mut v = ShareVerifier::new(false);
        assert!(!v.is_enabled(), "a verifier built disarmed reported itself armed");
        assert_eq!(
            classify_share(v.is_enabled(), &A, None),
            ShareVerdict::SubmitUnverified,
            "a disarmed verifier must not claim verification was attempted"
        );

        v.set_enabled(true);
        assert!(v.is_enabled());
        assert_eq!(
            classify_share(v.is_enabled(), &A, None),
            ShareVerdict::SubmitVerifierUnavailable,
            "arming must make the fail-open arm reachable again"
        );

        v.set_enabled(false);
        assert!(!v.is_enabled(), "disarming did not take effect");
    }

    /// A fresh `Miner` has no stats yet; the counter must read 0 rather than
    /// panicking on the `None` branch.
    #[test]
    fn verify_failure_counter_is_zero_before_start() {
        let m = Miner::new(std::sync::Arc::new(AtomicBool::new(false)));
        assert_eq!(m.get_verify_failures(), 0);
    }

    /// Both switches default on, and the setters take effect.
    #[test]
    fn switch_defaults_and_setters() {
        let mut m = Miner::new(std::sync::Arc::new(AtomicBool::new(false)));
        assert!(m.native_loop);
        assert!(m.verify_shares);
        m.set_native_loop(false);
        m.set_verify_shares(false);
        assert!(!m.native_loop);
        assert!(!m.verify_shares);
    }
}
