use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use minertim::donate;
use minertim::miner::Miner;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 || args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("MinerTim - Monero (XMR) CPU miner (pure Rust, rx/0 full mode)");
        eprintln!();
        eprintln!("Usage: {} <pool:port> <wallet> [threads] [--donate-level N] [--native-loop on|off]", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} pool.supportxmr.com:443 4...address 4", args[0]);
        eprintln!("  {} pool.hashvault.pro:443 4...address --donate-level 1", args[0]);
        eprintln!();
        eprintln!("Arguments:");
        eprintln!("  pool:port    Mining pool address with port (TLS auto-detected)");
        eprintln!("  wallet       Monero wallet address");
        eprintln!("  threads      Number of mining threads (default: {} = cores minus one,",
            minertim::miner::recommended_thread_count());
        eprintln!("               leaving a core for the pool receiver; max: {}). Using all",
            thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
        eprintln!("               cores raises hashrate marginally but causes stale-share rejects.");
        eprintln!("  --donate-level N  Percent of mining time donated (default: {}, min: {}).",
            donate::DEFAULT_DONATE_LEVEL, donate::MIN_DONATE_LEVEL);
        eprintln!("                    Split 50/50 between the MinerTim author and XMRig.");
        eprintln!("  --native-loop on|off  Use the native-loop JIT (default: on). Also settable");
        eprintln!("                    via MINERTIM_NATIVE_LOOP=0/1. This is a fallback switch:");
        eprintln!("                    if shares start being rejected, turn it off and restart to");
        eprintln!("                    rule the JIT out without rebuilding. aarch64 rx/0 full mode");
        eprintln!("                    only; ignored everywhere else.");
        eprintln!("  --verify-shares on|off  Re-check every candidate share on the reference");
        eprintln!("                    path before submitting, and withhold any the two paths");
        eprintln!("                    disagree on (default: on). Costs ~0.005% of mining time");
        eprintln!("                    because shares are rare. Also MINERTIM_VERIFY_SHARES.");
        std::process::exit(if args.len() < 3 { 1 } else { 0 });
    }

    let pool = &args[1];
    let wallet = &args[2];
    let threads: u32 = args.get(3)
        .filter(|s| !s.starts_with('-'))
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(minertim::miner::recommended_thread_count);
    let donate_level = parse_donate_level(&args);
    let native_loop = parse_native_loop(&args);
    let verify_shares = parse_switch(&args, "--verify-shares", "MINERTIM_VERIFY_SHARES", true);

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
    miner.set_native_loop(native_loop);
    miner.set_verify_shares(verify_shares);
    if !verify_shares && native_loop {
        log::warn!(
            "Share verification DISABLED while the native-loop JIT is on. A JIT \
             defect would now be submitted to the pool as a wrong share instead \
             of being withheld, and the only symptom would be rejects."
        );
    }
    if !native_loop {
        // Logged at warn, not info: this halves nothing and breaks nothing, but
        // it silently gives up ~7% hashrate, and someone who set it during an
        // incident should not discover it months later in a config file.
        log::warn!(
            "Native-loop JIT DISABLED — running the per-iteration body JIT. \
             Expect roughly 7% lower hashrate. Unset --native-loop / \
             MINERTIM_NATIVE_LOOP to restore it."
        );
    }

    log::info!(
        "Donation: donate-level {}% of mining time, split 50/50 between the MinerTim \
         author and XMRig. MinerTim is an AI-assisted Rust translation of XMRig. \
         Adjust with --donate-level (minimum {}); see README.",
        donate_level,
        donate::MIN_DONATE_LEVEL,
    );

    log::info!("Connecting to {}...", pool);
    if let Err(e) = miner.initialize(pool, wallet, threads, donate_level) {
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
        let verify_failures = miner.get_verify_failures();
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

        let target_val = 0xFFFFFFFF_u64.checked_div(difficulty).unwrap_or(0);
        let progress = if target_val > 0 && best < u32::MAX {
            let pct = (target_val as f64 / best as f64 * 100.0).min(999.0);
            format!("best={} target={} ({:.0}%)", best, target_val, pct)
        } else {
            "waiting".to_string()
        };

        // Only ever non-zero if the JIT is miscomputing. Appended to the normal
        // stats line so it cannot be missed by someone watching the miner rather
        // than the pool dashboard.
        if verify_failures > 0 {
            log::error!(
                "{} share(s) WITHHELD so far because the native-loop JIT disagreed with \
                 the reference path. Restart with --native-loop off and report this.",
                verify_failures
            );
        }

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

/// Parse `--donate-level N` or `--donate-level=N` from the args, defaulting to
/// `donate::DEFAULT_DONATE_LEVEL`. The value is clamped to the permitted range;
/// going below the minimum requires recompiling.
fn parse_donate_level(args: &[String]) -> u8 {
    let mut level = donate::DEFAULT_DONATE_LEVEL;
    let mut i = 0;
    while i < args.len() {
        if let Some(v) = args[i].strip_prefix("--donate-level=") {
            if let Ok(n) = v.parse() {
                level = n;
            }
        } else if args[i] == "--donate-level" {
            if let Some(n) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                level = n;
            }
            i += 1;
        }
        i += 1;
    }
    donate::clamp_level(level)
}

/// Resolve the native-loop switch: `--native-loop <v>` / `--native-loop=<v>`
/// beats `MINERTIM_NATIVE_LOOP`, which beats the default (on).
///
/// Accepts on/off, true/false, yes/no, 1/0, case-insensitively.
///
/// # Malformed input fails SAFE, not to the default
///
/// A bad value never aborts startup — this is the switch someone reaches for
/// while shares are being rejected, and refusing to boot over a typo would be
/// the wrong failure mode. But it resolves to **off**, not to the default.
///
/// The two outcomes are asymmetric. If the value could not be parsed we already
/// know the operator was trying to *change* the setting, so resolving to `on` is
/// the one answer we can be confident they did not want — and its cost is
/// continued rejected shares, i.e. money, until they notice. Resolving to `off`
/// costs at most ~7% hashrate if they actually meant "on", and never leaves a
/// suspected-bad JIT running while someone is trying to disable it.
/// Resolve an `--flag on|off` switch with an environment-variable fallback.
///
/// Same fail-safe policy as [`parse_native_loop`]: a malformed value never
/// aborts startup, but resolves to `false` rather than to `default_on`. If the
/// value could not be parsed we know the operator meant to change something,
/// and for both switches here `false` is the conservative direction — slower,
/// or noisier, but never "keep doing the thing they were trying to stop".
fn parse_switch(args: &[String], flag: &str, env: &str, default_on: bool) -> bool {
    let as_bool = |v: &str| -> Option<bool> {
        match v.trim().to_ascii_lowercase().as_str() {
            "on" | "true" | "yes" | "1" => Some(true),
            "off" | "false" | "no" | "0" => Some(false),
            _ => {
                eprintln!("warning: unrecognised {flag} value {v:?} - assuming OFF; use on|off");
                Some(false)
            }
        }
    };

    let mut value = std::env::var(env).ok().and_then(|v| as_bool(&v));
    let eq_prefix = format!("{flag}=");

    let mut i = 0;
    while i < args.len() {
        if let Some(v) = args[i].strip_prefix(&eq_prefix) {
            value = as_bool(v);
        } else if args[i] == flag {
            match args.get(i + 1) {
                Some(v) => value = as_bool(v),
                None => {
                    eprintln!("warning: {flag} given with no value - assuming OFF; use on|off");
                    value = Some(false);
                }
            }
            i += 1;
        }
        i += 1;
    }

    value.unwrap_or(default_on)
}

fn parse_native_loop(args: &[String]) -> bool {
    fn as_bool(v: &str) -> Option<bool> {
        match v.trim().to_ascii_lowercase().as_str() {
            "on" | "true" | "yes" | "1" => Some(true),
            "off" | "false" | "no" | "0" => Some(false),
            _ => {
                eprintln!(
                    "warning: unrecognised native-loop value {v:?} - assuming OFF \
                     (the safe direction); use on|off"
                );
                Some(false)
            }
        }
    }

    let mut value = std::env::var("MINERTIM_NATIVE_LOOP")
        .ok()
        .and_then(|v| as_bool(&v));

    let mut i = 0;
    while i < args.len() {
        if let Some(v) = args[i].strip_prefix("--native-loop=") {
            value = as_bool(v);
        } else if args[i] == "--native-loop" {
            match args.get(i + 1) {
                Some(v) => value = as_bool(v),
                // Bare `--native-loop` with nothing after it. This previously
                // did nothing at all - no warning, no change - which was the one
                // input shape that silently left the JIT ON while the operator
                // believed they had turned it off. Two realistic ways to reach
                // it: typing it as though it were a boolean flag, or a wrapper
                // script writing `--native-loop $NL` with $NL unset.
                None => {
                    eprintln!(
                        "warning: --native-loop given with no value - assuming OFF \
                         (the safe direction); use on|off"
                    );
                    value = Some(false);
                }
            }
            i += 1;
        }
        i += 1;
    }

    value.unwrap_or(true)
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

#[cfg(test)]
mod tests {
    use super::parse_native_loop;

    fn args(extra: &[&str]) -> Vec<String> {
        let mut v = vec!["minertim".to_string(), "pool:1".into(), "wallet".into()];
        v.extend(extra.iter().map(|s| s.to_string()));
        v
    }

    #[test]
    fn native_loop_defaults_on() {
        assert!(parse_native_loop(&args(&[])));
    }

    #[test]
    fn native_loop_accepts_the_documented_spellings() {
        for off in ["off", "OFF", "false", "no", "0"] {
            assert!(!parse_native_loop(&args(&["--native-loop", off])), "{off}");
            assert!(
                !parse_native_loop(&args(&[&format!("--native-loop={off}")])),
                "{off} (= form)"
            );
        }
        for on in ["on", "true", "yes", "1"] {
            assert!(parse_native_loop(&args(&["--native-loop", on])), "{on}");
        }
    }

    /// A typo must not refuse to boot, but it must fail SAFE — resolving to
    /// `on` would continue the exact behaviour the operator was trying to stop.
    #[test]
    fn native_loop_unrecognised_value_fails_safe_to_off() {
        assert!(!parse_native_loop(&args(&["--native-loop", "maybe"])));
        assert!(!parse_native_loop(&args(&["--native-loop=maybe"])));
    }

    /// Bare `--native-loop` with no value used to be silently ignored, leaving
    /// the JIT ON with no diagnostic — the one shape that could leave an
    /// operator believing they had turned it off. Reachable by a wrapper script
    /// writing `--native-loop $NL` with $NL unset.
    #[test]
    fn native_loop_with_no_value_fails_safe_to_off() {
        assert!(!parse_native_loop(&args(&["--native-loop"])));
    }

    /// Last flag wins, so a wrapper script can append an override.
    #[test]
    fn native_loop_last_flag_wins() {
        assert!(parse_native_loop(&args(&["--native-loop", "off", "--native-loop", "on"])));
        assert!(!parse_native_loop(&args(&["--native-loop=on", "--native-loop=off"])));
    }

    #[test]
    fn verify_shares_defaults_on_and_shares_the_fail_safe_policy() {
        use super::parse_switch;
        let f = |extra: &[&str]| {
            parse_switch(&args(extra), "--verify-shares", "MINERTIM_VERIFY_SHARES_TEST", true)
        };
        assert!(f(&[]));
        assert!(!f(&["--verify-shares", "off"]));
        assert!(f(&["--verify-shares=yes"]));
        assert!(!f(&["--verify-shares", "nonsense"]));
        assert!(!f(&["--verify-shares"]));
    }
}
