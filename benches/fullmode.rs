//! Full-mode (2 GiB dataset) multi-threaded hashrate harness.
//!
//! The criterion bench in `hash.rs` runs in *light* mode, where per-hash cost is
//! dominated by on-the-fly dataset-item computation — that swamps changes to the
//! VM main loop and hides anything touching the dataset read or its prefetch.
//! This harness reproduces the real mining path instead: one shared precomputed
//! dataset, N worker threads, pipelined hashing, no network.
//!
//! Run with: `cargo bench --bench fullmode -- <threads> <seconds>`
//! (defaults: threads = cores-1, seconds = 60)

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use minertim::randomx::dataset::RandomXDataset;
use minertim::randomx::vm::RandomXVm;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let positional: Vec<&String> = args[1..].iter().filter(|a| !a.starts_with('-')).collect();
    let threads: usize = positional
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get().saturating_sub(1))
                .unwrap_or(4)
        });
    let secs: u64 = positional.get(1).and_then(|s| s.parse().ok()).unwrap_or(60);
    // Idle pause after the all-core dataset build so its thermal load does not
    // bleed into the measurement (this bench is A/B'd against another binary).
    let cool: u64 = positional.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);

    let key = b"benchmark key 000";
    eprintln!("building dataset (this takes ~45 s)...");
    let t = Instant::now();
    let vm_light = RandomXVm::new(key);
    let (cache, programs) = vm_light.cache_and_programs();
    let dataset = Arc::new(RandomXDataset::generate(
        cache,
        programs,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4),
    ));
    eprintln!("dataset ready in {:.1} s; cooling {} s", t.elapsed().as_secs_f64(), cool);
    std::thread::sleep(Duration::from_secs(cool));

    let stop = Arc::new(AtomicBool::new(false));
    let hashes = Arc::new(AtomicU64::new(0));
    let warm = Arc::new(AtomicBool::new(false));

    let mut handles = Vec::new();
    for tid in 0..threads {
        let dataset = dataset.clone();
        let stop = stop.clone();
        let hashes = hashes.clone();
        let warm = warm.clone();
        handles.push(std::thread::spawn(move || {
            let mut vm = RandomXVm::new_full(key, dataset);
            // 76-byte Monero-shaped blob; vary the nonce field per iteration.
            let mut blob = vec![0u8; 76];
            blob[0] = 16;
            blob[39..43].copy_from_slice(&(tid as u32).to_le_bytes());
            vm.prepare_scratchpad(&blob);
            let mut nonce = tid as u32;
            let mut local: u64 = 0;
            let mut counting = false;
            while !stop.load(Ordering::Relaxed) {
                nonce = nonce.wrapping_add(threads as u32);
                blob[39..43].copy_from_slice(&nonce.to_le_bytes());
                let h = vm.calculate_hash_pipelined(&blob);
                std::hint::black_box(h);
                // Only count hashes produced after the warm-up flag flips, so
                // dataset page-faulting and thermal ramp are excluded.
                if !counting {
                    if warm.load(Ordering::Relaxed) {
                        counting = true;
                    }
                } else {
                    local += 1;
                    if local.is_multiple_of(64) {
                        hashes.fetch_add(64, Ordering::Relaxed);
                    }
                }
            }
            hashes.fetch_add(local % 64, Ordering::Relaxed);
        }));
    }

    // Warm-up, then measure.
    std::thread::sleep(Duration::from_secs(15));
    warm.store(true, Ordering::Relaxed);
    let t0 = Instant::now();
    std::thread::sleep(Duration::from_secs(secs));
    stop.store(true, Ordering::Relaxed);
    let elapsed = t0.elapsed().as_secs_f64();
    for h in handles {
        let _ = h.join();
    }
    let total = hashes.load(Ordering::Relaxed);
    println!(
        "RESULT threads={} secs={:.1} hashes={} hashrate={:.1}",
        threads,
        elapsed,
        total,
        total as f64 / elapsed
    );
}
