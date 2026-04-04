use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::pool_connection::PoolConnection;
use crate::randomx::vm::RandomXVm;

#[allow(dead_code)]
pub struct Miner {
    pool_connection: Option<Arc<PoolConnection>>,
    workers: Vec<JoinHandle<()>>,
    thread_count: i32,
    hashrate_bits: Arc<AtomicU64>,
    mining_active: Arc<AtomicBool>,
    start_time: Option<Instant>,
    total_hashes: Arc<AtomicU64>,
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
        }
    }

    pub fn initialize(
        &mut self,
        pool: &str,
        wallet: &str,
        threads: i32,
    ) -> Result<(), String> {
        let max_threads = thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4);
        self.thread_count = threads.clamp(1, max_threads);

        let connection = PoolConnection::new();
        connection.connect(pool).map_err(|e| format!("Connection failed: {}", e))?;
        connection.login(wallet).map_err(|e| format!("Login failed: {}", e))?;
        connection.start_receiver();

        connection.reset_share_counters();
        self.pool_connection = Some(Arc::new(connection));
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

        let thread_count = self.thread_count as u32;

        for thread_id in 0..thread_count {
            let mining_active = self.mining_active.clone();
            let pool_conn = pool.clone();
            let total_hashes = self.total_hashes.clone();
            let hashrate_bits = self.hashrate_bits.clone();

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
                    );
                })
                .map_err(|e| format!("Failed to spawn worker {}: {}", thread_id, e))?;

            self.workers.push(handle);
        }

        log::info!("Started {} mining worker threads", thread_count);
        Ok(())
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

    pub fn set_thread_count(&mut self, count: i32) {
        let max_threads = thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4);
        self.thread_count = count.clamp(1, max_threads);
        log::info!("Thread count set to {}", self.thread_count);
    }
}

fn worker_loop(
    thread_id: u32,
    thread_count: u32,
    mining_active: Arc<AtomicBool>,
    pool: Arc<PoolConnection>,
    total_hashes: Arc<AtomicU64>,
    hashrate_bits: Arc<AtomicU64>,
) {
    log::info!("Worker {} started", thread_id);

    let mut vm: Option<RandomXVm> = None;
    let mut current_key: Vec<u8> = Vec::new();
    let mut nonce: u64 = thread_id as u64;
    let mut local_hashes: u64 = 0;
    let start_time = Instant::now();
    let mut last_hashrate_update = Instant::now();

    while mining_active.load(Ordering::Relaxed) {
        let job = match pool.get_work() {
            Some(job) => job,
            None => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        // Reinitialize VM if the seed hash changed
        if job.seed_hash != current_key || vm.is_none() {
            log::info!("Worker {} initializing RandomX VM with seed_hash", thread_id);
            if let Some(ref mut existing_vm) = vm {
                existing_vm.reinit(&job.seed_hash);
            } else {
                vm = Some(RandomXVm::new(&job.seed_hash));
            }
            current_key = job.seed_hash.clone();
        }

        let rx_vm = vm.as_ref().unwrap();

        // Prepare input blob with nonce
        let mut input = job.blob.clone();
        if input.len() >= 47 {
            let nonce_bytes = (nonce as u32).to_le_bytes();
            input[39] = nonce_bytes[0];
            input[40] = nonce_bytes[1];
            input[41] = nonce_bytes[2];
            input[42] = nonce_bytes[3];
            // Extended nonce bytes for additional entropy
            let nonce_high = ((nonce >> 32) as u32).to_le_bytes();
            input[43] = nonce_high[0];
            input[44] = nonce_high[1];
            input[45] = nonce_high[2];
            input[46] = nonce_high[3];
        }

        // Compute hash
        let hash = rx_vm.calculate_hash(&input);
        local_hashes += 1;
        nonce += thread_count as u64;

        // Compare hash to target (little-endian comparison)
        if meets_target(&hash, &job.target) {
            let nonce_hex = hex_encode(&input[39..43]);
            let result_hex = hex_encode(&hash);

            log::info!(
                "Worker {} found share! job_id={}, nonce={}",
                thread_id,
                job.job_id,
                nonce_hex
            );

            if let Err(e) = pool.submit_share(&job.job_id, &nonce_hex, &result_hex) {
                log::error!("Failed to submit share: {}", e);
            }
        }

        // Update hashrate from thread 0 every 5 seconds
        if thread_id == 0 && last_hashrate_update.elapsed().as_secs() >= 5 {
            let global_hashes = total_hashes.load(Ordering::Relaxed) + local_hashes;
            total_hashes.store(global_hashes, Ordering::Relaxed);
            local_hashes = 0;

            let elapsed = start_time.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                let rate = global_hashes as f64 / elapsed;
                hashrate_bits.store(rate.to_bits(), Ordering::Relaxed);
            }
            last_hashrate_update = Instant::now();
        }
    }

    // Flush remaining local hashes
    total_hashes.fetch_add(local_hashes, Ordering::Relaxed);

    log::info!("Worker {} stopped", thread_id);
}

/// Compare a 32-byte hash against a target in little-endian order.
/// The hash meets the target if it is less than or equal to the expanded target.
fn meets_target(hash: &[u8; 32], target: &[u8]) -> bool {
    if target.len() < 4 {
        return false;
    }

    // Target is 4 bytes (little-endian u32). Expand to a 256-bit threshold.
    // The target represents the upper 32 bits of a 256-bit difficulty threshold.
    let target_val = u32::from_le_bytes([target[0], target[1], target[2], target[3]]);

    if target_val == 0 {
        return false;
    }

    // Compare the last 4 bytes of the hash (big-endian most significant)
    let hash_val = u32::from_le_bytes([hash[28], hash[29], hash[30], hash[31]]);
    hash_val <= target_val
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}
