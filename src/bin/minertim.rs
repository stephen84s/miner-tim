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
        eprintln!("                    because shares are rare. Catches faults in the native-loop");
        eprintln!("                    machinery; both paths share an instruction generator, so a");
        eprintln!("                    fault common to both would pass. Also MINERTIM_VERIFY_SHARES.");
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
    let verify_shares = parse_verify_shares(&args);

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
            "Share verification DISABLED while the native-loop JIT is on. A \
             native-loop defect would now be submitted to the pool as a wrong \
             share instead of being withheld, and the only symptom would be \
             rejects climbing on the pool side."
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

/// Resolve an `--flag on|off` switch with an environment-variable fallback.
///
/// Precedence: the flag beats `env`, which beats `default_on`. Values are
/// `on/off`, `true/false`, `yes/no`, `1/0`, case-insensitively. The last
/// occurrence of the flag wins, so a wrapper script can append an override.
///
/// # Malformed input resolves to `fail_safe`, never aborts
///
/// A bad value must not stop the miner booting — these are the switches someone
/// reaches for during an incident, and refusing to start over a typo would be
/// the wrong failure mode. But it must not resolve to `default_on` either: if
/// the value failed to parse, we already know the operator was trying to
/// *change* something, so the default is the one answer they probably did not
/// want.
///
/// `fail_safe` is per-switch because the conservative direction differs, and
/// getting this backwards is easy — it was, until review round 7 (R7-F2):
///
/// * `--native-loop` -> `false`. Off is slower but cannot mine wrong hashes.
/// * `--verify-shares` -> `true`. This one is a *safety net*, so off is the
///   dangerous direction; a malformed value must leave the net in place.
///
/// The "no value at all" case (`--flag` as the final argument, or a script
/// writing `--flag $VAR` with `$VAR` unset) is treated the same way, and warns.
/// Previously it was silently ignored, which was the one shape that could leave
/// an operator believing they had changed a setting when they had not.
fn parse_switch(args: &[String], flag: &str, env: &str, default_on: bool, fail_safe: bool) -> bool {
    let word = if fail_safe { "ON" } else { "OFF" };
    let as_bool = |v: &str| -> Option<bool> {
        match v.trim().to_ascii_lowercase().as_str() {
            "on" | "true" | "yes" | "1" => Some(true),
            "off" | "false" | "no" | "0" => Some(false),
            _ => {
                eprintln!(
                    "warning: unrecognised {flag} value {v:?} - assuming {word} \
                     (the safe direction); use on|off"
                );
                Some(fail_safe)
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
                    eprintln!(
                        "warning: {flag} given with no value - assuming {word} \
                         (the safe direction); use on|off"
                    );
                    value = Some(fail_safe);
                }
            }
            i += 1;
        }
        i += 1;
    }

    value.unwrap_or(default_on)
}

/// The native-loop JIT switch. Malformed input falls back to **off**: slower,
/// but it cannot mine wrong hashes.
fn parse_native_loop(args: &[String]) -> bool {
    parse_switch(args, "--native-loop", "MINERTIM_NATIVE_LOOP", true, false)
}

/// The share-verification switch. Malformed input falls back to **on**: this is
/// a safety net, so leaving it in place is the conservative direction.
fn parse_verify_shares(args: &[String]) -> bool {
    parse_switch(args, "--verify-shares", "MINERTIM_VERIFY_SHARES", true, true)
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
    use super::{parse_native_loop, parse_switch, parse_verify_shares};

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

    /// The environment-variable branch, and that an explicit flag beats it.
    /// Uses a dedicated variable name so it cannot collide with a real one or
    /// with another test running in parallel.
    #[test]
    fn switch_reads_the_environment_and_the_flag_overrides_it() {
        const VAR: &str = "MINERTIM_SWITCH_ENV_BRANCH_TEST";
        // SAFETY: a name unique to this test; no other thread reads it.
        unsafe { std::env::set_var(VAR, "off") };
        assert!(!parse_switch(&args(&[]), "--x", VAR, true, false), "env beats default");
        assert!(parse_switch(&args(&["--x", "on"]), "--x", VAR, true, false), "flag beats env");

        // An unparseable env value takes the switch's fail-safe direction, not
        // the default — and that direction is per-switch (R7-F2). An empty
        // MINERTIM_VERIFY_SHARES= reaches this path with no typo at all.
        unsafe { std::env::set_var(VAR, "wat") };
        assert!(!parse_switch(&args(&[]), "--x", VAR, true, false));
        assert!(parse_switch(&args(&[]), "--x", VAR, true, true));
        unsafe { std::env::set_var(VAR, "") };
        assert!(parse_switch(&args(&[]), "--x", VAR, true, true), "empty env must not disarm");

        unsafe { std::env::remove_var(VAR) };
        assert!(parse_switch(&args(&[]), "--x", VAR, true, false), "default applies once unset");
    }

    #[test]
    fn verify_shares_fails_safe_on_because_it_is_a_safety_net() {
        // Real switch, real fail-safe direction: ON, because it is a safety net.
        assert!(parse_verify_shares(&args(&[])));
        assert!(!parse_verify_shares(&args(&["--verify-shares", "off"])));
        assert!(parse_verify_shares(&args(&["--verify-shares=yes"])));
        // R7-F2: a typo must NOT switch the safety net off.
        assert!(parse_verify_shares(&args(&["--verify-shares", "nonsense"])));
        assert!(parse_verify_shares(&args(&["--verify-shares"])));
        // ...whereas the native-loop switch fails safe the other way.
        assert!(!parse_native_loop(&args(&["--native-loop", "nonsense"])));
    }
}
