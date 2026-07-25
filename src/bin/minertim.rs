use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use minertim::miner::Miner;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 || args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("MinerTim - Monero (XMR) CPU miner (pure Rust, rx/0 full mode)");
        eprintln!();
        eprintln!("Usage: {} <pool:port> <wallet> [threads]", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} pool.supportxmr.com:443 4...address 4", args[0]);
        eprintln!("  {} pool.hashvault.pro:443 4...address", args[0]);
        eprintln!();
        eprintln!("Arguments:");
        eprintln!("  pool:port    Mining pool address with port (TLS auto-detected)");
        eprintln!("  wallet       Monero wallet address");
        eprintln!("  threads      Number of mining threads (default: {} performance cores, max: {})",
            minertim::miner::recommended_thread_count(),
            thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
        std::process::exit(if args.len() < 3 { 1 } else { 0 });
    }

    let pool = &args[1];
    let wallet = &args[2];
    let threads: u32 = args.get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(minertim::miner::recommended_thread_count);

    let mining_active = Arc::new(AtomicBool::new(false));

    // Set up Ctrl+C handler
    let shutdown = mining_active.clone();
    ctrlc::set_handler(move || {
        if !shutdown.load(Ordering::SeqCst) {
            // Already stopped or not started
            std::process::exit(0);
        }
        eprintln!("\nShutting down...");
        shutdown.store(false, Ordering::SeqCst);
    })
    .expect("Failed to set Ctrl+C handler");

    let mut miner = Miner::new(mining_active.clone());

    log::info!("Connecting to {}...", pool);
    if let Err(e) = miner.initialize(pool, wallet, threads) {
        eprintln!("Failed to initialize: {}", e);
        std::process::exit(1);
    }

    mining_active.store(true, Ordering::SeqCst);
    if let Err(e) = miner.start() {
        eprintln!("Failed to start mining: {}", e);
        std::process::exit(1);
    }

    log::info!("Mining started with {} threads. Press Ctrl+C to stop.", threads);

    // Stats loop
    while mining_active.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(10));
        if !mining_active.load(Ordering::SeqCst) {
            break;
        }

        let snap = miner.snapshot_hashrates();
        let accepted = miner.get_accepted_shares();
        let rejected = miner.get_rejected_shares();
        let difficulty = miner.get_difficulty();
        let best = miner.get_best_hash_val();
        let share_stats = miner.get_share_stats();

        let fmt_rate = |r: Option<f64>| match r {
            Some(v) => format!("{:.1}", v),
            None => "-".to_string(),
        };

        let current = snap.rate_1m.or(snap.rate_5m).unwrap_or(0.0);

        // Average time between shares at current hashrate
        let avg_share_secs = if current > 0.0 && difficulty > 0 {
            Some(difficulty as f64 / current)
        } else {
            None
        };

        // Elapsed since last share (or since start if no shares yet)
        let elapsed_str = match share_stats.last_found_elapsed_secs {
            Some(e) => format_duration(e),
            None => "?".to_string(),
        };

        let avg_str = match avg_share_secs {
            Some(s) => format_duration(s),
            None => "?".to_string(),
        };

        let target_val = if difficulty > 0 { 0xFFFFFFFF_u64 / difficulty } else { 0 };
        let progress = if target_val > 0 && best < u32::MAX {
            let pct = (target_val as f64 / best as f64 * 100.0).min(999.0);
            format!("best={} target={} ({:.0}%)", best, target_val, pct)
        } else {
            "waiting".to_string()
        };

        let share_label = if share_stats.total_found > 0 {
            "since last share"
        } else {
            "no shares yet"
        };

        log::info!(
            "H/s 1m:{} 5m:{} 10m:{} | Shares: {}/{} (found:{}) | Diff: {} | {} elapsed ({}, avg {}) | {}",
            fmt_rate(snap.rate_1m),
            fmt_rate(snap.rate_5m),
            fmt_rate(snap.rate_10m),
            accepted,
            rejected,
            share_stats.total_found,
            difficulty,
            elapsed_str,
            share_label,
            avg_str,
            progress,
        );
    }

    miner.stop();
    log::info!("Miner stopped. Final stats: {} accepted, {} rejected",
        miner.get_accepted_shares(), miner.get_rejected_shares());
}

fn format_duration(secs: f64) -> String {
    let secs = secs.max(0.0) as u64;
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}
