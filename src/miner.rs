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

    pub fn initialize(
        &mut self,
        pool: &str,
        wallet: &str,
        threads: u32,
    ) -> Result<(), String> {
        let max_threads = thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4);
        self.thread_count = threads.clamp(1, max_threads);

        let connection = Arc::new(PoolConnection::new());
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
        });

        for thread_id in 0..thread_count {
            let mining_active = self.mining_active.clone();
            let pool_conn = pool.clone();
            let total_hashes = self.total_hashes.clone();
            let hashrate_bits = self.hashrate_bits.clone();
            let ds_cache = dataset_cache.clone();

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
                existing_vm.reinit(&job.seed_hash, Some(dataset));
            } else {
                vm = Some(RandomXVm::new_full(&job.seed_hash, dataset));
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

            if let Err(e) = pool.submit_share(&job.job_id, &nonce_hex, &result_hex) {
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
    if let Some(ref cached) = *guard {
        if cached.seed_hash == seed_hash {
            return cached.dataset.clone();
        }
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
