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
        eprintln!("MinerTim - Monero (XMR) CPU miner (pure Rust, rx/0 light mode)");
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
        eprintln!("  threads      Number of mining threads (default: 2, max: {})",
            thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
        std::process::exit(if args.len() < 3 { 1 } else { 0 });
    }

    let pool = &args[1];
    let wallet = &args[2];
    let threads: i32 = args.get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);

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
        let hashrate = miner.get_hashrate();
        let accepted = miner.get_accepted_shares();
        let rejected = miner.get_rejected_shares();
        log::info!(
            "Hashrate: {:.2} H/s | Accepted: {} | Rejected: {}",
            hashrate,
            accepted,
            rejected
        );
    }

    miner.stop();
    log::info!("Miner stopped. Final stats: {} accepted, {} rejected",
        miner.get_accepted_shares(), miner.get_rejected_shares());
}
