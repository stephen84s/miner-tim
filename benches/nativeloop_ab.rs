//! Paired A/B benchmark: native-loop JIT vs the per-iteration body JIT.
//!
//! Run with: `cargo bench --bench nativeloop_ab -- [threads] [pairs] [hashes_per_round]`
//!
//! # Why this harness exists
//!
//! AUDIT.md 2026-08-29 established that comparing two *binaries* on this machine
//! has a within-version spread of 11-19%, so it cannot resolve anything below
//! roughly 10%. That finding is about between-process comparison, where the two
//! arms are separated in time by a whole mining run and therefore by a different
//! thermal state. This harness attacks that directly:
//!
//! * **Both arms live in one process**, sharing one `Arc<RandomXDataset>`, so
//!   there is no second dataset build and no second thermal ramp.
//! * **Rounds alternate A-B-B-A**, so a linear drift in machine state over a
//!   pair cancels rather than accumulating into the difference.
//! * **Paired differences** are the statistic, not two independent means. The
//!   noise that dominates the two-binary comparison is drift shared by both
//!   arms of a pair, and differencing removes it.
//!
//! # Why two VMs and not one with a toggled flag
//!
//! Each `RandomXVm` owns a `JitCompiler` with its own MAP_JIT region. Toggling
//! `set_native_loop` on a single VM would rewrite that one region on every
//! switch and re-invalidate icache, and the two arms emit blobs of very
//! different sizes — so the measurement would include icache/iTLB residency
//! differences on top of the change under test. Two VMs, each with its flag
//! fixed at construction, keeps the thermal pairing without that confound.
//!
//! # Correctness, for free
//!
//! Both arms are fed the identical blob sequence from an identical starting
//! scratchpad, so every hash they produce must be bit-identical. The harness
//! checksums every round and asserts on all of them once the threads have
//! joined — not inside the loop, because a panic between two barrier waits
//! strands every sibling in `Barrier::wait()` and hangs the process. That is a far broader correctness check than the
//! unit tests give: the known-answer tests pin exactly one program stream, while
//! this exercises thousands of real nonces and therefore thousands of distinct
//! RandomX programs, entropy values and `dataset_offset`s.

use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use minertim::randomx::dataset::RandomXDataset;
use minertim::randomx::vm::RandomXVm;

const KEY: &[u8] = b"benchmark key 000";

/// One measured round: `hashes` pipelined hashes on `vm`, returning elapsed
/// seconds and a checksum of every hash produced.
fn round(vm: &mut RandomXVm, blob: &mut [u8], nonce: &mut u32, hashes: usize) -> (f64, u64) {
    let t = Instant::now();
    let mut checksum: u64 = 0;
    for _ in 0..hashes {
        *nonce = nonce.wrapping_add(1);
        blob[39..43].copy_from_slice(&nonce.to_le_bytes());
        let h = vm.calculate_hash_pipelined(blob);
        // Fold the whole hash in so a divergence anywhere is caught.
        for c in h.chunks_exact(8) {
            checksum = checksum
                .rotate_left(7)
                .wrapping_add(u64::from_le_bytes(c.try_into().unwrap()));
        }
    }
    (t.elapsed().as_secs_f64(), checksum)
}

/// Mean, and the half-width of a 95% CI, of a paired-difference sample.
fn mean_ci95(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    if xs.len() < 2 {
        return (mean, f64::NAN);
    }
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    // t(0.975) by degrees of freedom. `pairs` is a CLI argument, so a single
    // hardcoded value is wrong whenever the caller asks for fewer rounds — at
    // n=6 a flat 2.09 understates the interval by roughly a fifth.
    let df = xs.len() - 1;
    let t = match df {
        1 => 12.706, 2 => 4.303, 3 => 3.182, 4 => 2.776, 5 => 2.571,
        6 => 2.447, 7 => 2.365, 8 => 2.306, 9 => 2.262, 10 => 2.228,
        11 => 2.201, 12 => 2.179, 13 => 2.160, 14 => 2.145, 15 => 2.131,
        16 => 2.120, 17 => 2.110, 18 => 2.101, 19 => 2.093,
        // Buckets take the value for the LOWEST df in the range, so the
        // interval errs wide. Taking the highest (2.045/2.001/1.96) made every
        // df below the top of a bucket anti-conservative — including the
        // default run, which is n=24 => df=23, where t is 2.069 not 2.045.
        20..=29 => 2.086, 30..=59 => 2.042, _ => 2.000,
    };
    (mean, t * (var / n).sqrt())
}

fn median(xs: &[f64]) -> f64 {
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n.is_multiple_of(2) {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    } else {
        v[n / 2]
    }
}

/// Per-pair `(ca, cb, cc, cd)` checksums: the A, B, B, A rounds in order.
type Checks = Vec<(u64, u64, u64, u64)>;
/// What one thread's A/B phase yields: baseline rates, native rates, checksums.
type PhaseOut = (Vec<f64>, Vec<f64>, Checks);

/// Run `pairs` A-B-B-A blocks on one thread and return the per-round hashrate
/// of each arm, as (baseline, native).
fn ab_phase(
    dataset: &Arc<RandomXDataset>,
    tid: usize,
    pairs: usize,
    hashes: usize,
    barrier: Option<&Barrier>,
) -> PhaseOut {
    // Wait before a round, never during one: `round()` takes its own
    // `Instant::now()` as its first statement, so a barrier here is outside
    // every timed region.
    let sync = || {
        if let Some(b) = barrier {
            b.wait();
        }
    };
    let mut base_vm = RandomXVm::new_full(KEY, dataset.clone());
    // BOTH arms set the flag explicitly. Relying on the constructor default for
    // the baseline is what broke this harness once already: it was written when
    // the default was `false`, then stage D flipped the default to `true` in the
    // same commit, so the "baseline" silently became a second native-loop arm
    // and the benchmark measured the native loop against itself (~-0.02%).
    // Never infer an arm from a default that another commit can move.
    base_vm.set_native_loop(false);
    let mut nat_vm = RandomXVm::new_full(KEY, dataset.clone());
    nat_vm.set_native_loop(true);

    // 76-byte Monero-shaped blob; both arms get the identical sequence.
    let mut base_blob = vec![0u8; 76];
    base_blob[0] = 16;
    base_blob[43..47].copy_from_slice(&(tid as u32).to_le_bytes());
    let mut nat_blob = base_blob.clone();

    base_vm.prepare_scratchpad(&base_blob);
    nat_vm.prepare_scratchpad(&nat_blob);
    let (mut base_nonce, mut nat_nonce) = (0u32, 0u32);

    // Warm-up round on each arm, discarded: first-touch page faults on the
    // scratchpad and the initial JIT compile land here rather than in the data.
    sync();
    round(&mut base_vm, &mut base_blob, &mut base_nonce, hashes.min(32));
    sync();
    round(&mut nat_vm, &mut nat_blob, &mut nat_nonce, hashes.min(32));

    let mut base_rates = Vec::with_capacity(pairs * 2);
    let mut nat_rates = Vec::with_capacity(pairs * 2);
    let mut checks: Checks = Vec::with_capacity(pairs);

    for _ in 0..pairs {
        // A-B-B-A: a drift linear in time contributes equally to both arms.
        sync();
        let (ta, ca) = round(&mut base_vm, &mut base_blob, &mut base_nonce, hashes);
        sync();
        let (tb, cb) = round(&mut nat_vm, &mut nat_blob, &mut nat_nonce, hashes);
        sync();
        let (tc, cc) = round(&mut nat_vm, &mut nat_blob, &mut nat_nonce, hashes);
        sync();
        let (td, cd) = round(&mut base_vm, &mut base_blob, &mut base_nonce, hashes);

        // Deliberately NOT asserted here. A panic between two `sync()` calls
        // leaves every sibling blocked on `Barrier::wait()` forever — no
        // timeout, no poisoning — so the process hangs with the panic message
        // already printed and has to be killed. Checksums are collected and
        // checked after the join instead, where a failure is loud and the
        // harness still exits.
        //
        // This removes the *expected* panic from the barriered region, not every
        // possible one: an index panic, an unwrap or an arithmetic overflow in
        // here would still strand the siblings. Nothing in this loop currently
        // does any of those, but keep it that way.
        checks.push((ca, cb, cc, cd));

        let h = hashes as f64;
        base_rates.push(h / ta);
        nat_rates.push(h / tb);
        nat_rates.push(h / tc);
        base_rates.push(h / td);
    }

    (base_rates, nat_rates, checks)
}

/// Panic if any pair's two arms disagreed. Called only after every thread has
/// joined, so it can never strand a sibling inside `Barrier::wait()`.
fn assert_arms_agree(label: &str, checks: &Checks) {
    for (i, (ca, cb, cc, cd)) in checks.iter().enumerate() {
        assert_eq!(
            (ca, cc),
            (cb, cd),
            "{label}: native loop and body JIT produced different hashes in pair \
             {i} — this is a correctness failure, not a benchmark result"
        );
    }
}

/// Across-thread spread within a round, per arm.
///
/// This exists to check the barrier did not trade one bias for another. Under a
/// barrier a round's wall time is max-over-threads, so threads that finish early
/// idle in the tail and the memory pressure late in a round is lower than early.
/// That is harmless only while the two arms idle by *similar* amounts. If one
/// arm's threads are consistently more spread out, the tail idle is asymmetric —
/// a function of the very difference being measured, which is the original
/// defect one level down. Then the barrier is the wrong fix and the aggregate
/// should be dropped in favour of the per-thread paired diffs.
///
/// Reported three ways, because each answers a different question:
///   * the **level** per arm — a 5% spread and a 40% spread have very different
///     implications for how much tail idle there is to be asymmetric about;
///   * the **paired difference across rounds, with a CI** — the two arms are
///     measured in the same round sequence, so this is a real within-run test
///     rather than the difference of two point estimates;
///   * mean *and* median, so one straggling round cannot carry the verdict.
fn spread_report(results: &[(Vec<f64>, Vec<f64>)]) {
    // Range over mean, per round, across threads. Deliberately NOT called a
    // coefficient of variation: range/mean is not sd/mean. It is used because
    // the quantity of interest is how far the slowest thread lags the fastest —
    // that lag is the tail idle — not the dispersion of the whole set.
    fn range_over_mean(per_thread: &[&Vec<f64>]) -> Vec<f64> {
        let rounds = per_thread[0].len();
        let mut out = Vec::with_capacity(rounds);
        for i in 0..rounds {
            let vals: Vec<f64> = per_thread.iter().map(|t| t[i]).collect();
            let mean = vals.iter().sum::<f64>() / vals.len() as f64;
            let lo = vals.iter().cloned().fold(f64::INFINITY, f64::min);
            let hi = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            if mean > 0.0 {
                out.push((hi - lo) / mean * 100.0);
            }
        }
        out
    }

    let base: Vec<&Vec<f64>> = results.iter().map(|r| &r.0).collect();
    let nat: Vec<&Vec<f64>> = results.iter().map(|r| &r.1).collect();
    let b = range_over_mean(&base);
    let n = range_over_mean(&nat);
    let b_mean = b.iter().sum::<f64>() / b.len() as f64;
    let n_mean = n.iter().sum::<f64>() / n.len() as f64;

    // Paired across rounds: round i of both arms sat in the same run, under the
    // same thermal and scheduling conditions.
    let paired: Vec<f64> = n.iter().zip(b.iter()).map(|(x, y)| x - y).collect();
    let (d_mean, d_ci) = mean_ci95(&paired);

    println!("\n=== across-thread spread within a round (barrier tail-idle check) ===");
    println!("  body JIT     : range/mean  mean {b_mean:5.2}%   median {:5.2}%", median(&b));
    println!("  native loop  : range/mean  mean {n_mean:5.2}%   median {:5.2}%", median(&n));
    println!(
        "  asymmetry    : {d_mean:+.2} pp  (95% CI {:+.2} .. {:+.2} pp, paired over n={} rounds)",
        d_mean - d_ci,
        d_mean + d_ci,
        paired.len()
    );
    let verdict = if (d_mean - d_ci) > 0.0 || (d_mean + d_ci) < 0.0 {
        "ASYMMETRIC within this run — see note"
    } else {
        "no asymmetry detectable within this run (CI includes 0)"
    };
    println!("  verdict      : {verdict}");
    println!(
        "  note         : a within-run test. One run's verdict does not establish\n\
         \x20                the absence of a systematic effect: this figure has been\n\
         \x20                seen with either sign across runs, moving by more between\n\
         \x20                runs than a single run's interval spans. Treat a\n\
         \x20                consistent sign over several runs, not one CI, as the\n\
         \x20                evidence. AUDIT.md's BENCH-02 entry carries the running\n\
         \x20                record; do not hardcode observations here, where they go\n\
         \x20                stale the moment the next run finishes."
    );
}

fn report(label: &str, base: &[f64], nat: &[f64]) {
    let diffs: Vec<f64> = base
        .iter()
        .zip(nat.iter())
        .map(|(b, n)| (n - b) / b * 100.0)
        .collect();
    let (mean_d, ci) = mean_ci95(&diffs);
    let base_mean = base.iter().sum::<f64>() / base.len() as f64;
    let nat_mean = nat.iter().sum::<f64>() / nat.len() as f64;

    println!("\n=== {label} ===");
    println!("  body JIT     : mean {base_mean:8.1} H/s   median {:8.1}", median(base));
    println!("  native loop  : mean {nat_mean:8.1} H/s   median {:8.1}", median(nat));
    println!("  paired diff  : {mean_d:+.2}%  (95% CI {:+.2}% .. {:+.2}%, n={})",
             mean_d - ci, mean_d + ci, diffs.len());
    let verdict = if (mean_d - ci) > 0.0 {
        "native loop FASTER (CI excludes 0)"
    } else if (mean_d + ci) < 0.0 {
        "native loop SLOWER (CI excludes 0)"
    } else {
        "NO MEASURABLE DIFFERENCE (CI includes 0)"
    };
    println!("  verdict      : {verdict}");
    print!("  per-pair-diff:");
    for d in &diffs {
        print!(" {d:+.1}%");
    }
    println!();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pos: Vec<&String> = args[1..].iter().filter(|a| !a.starts_with('-')).collect();
    let threads: usize = pos.first().and_then(|s| s.parse().ok()).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1))
            .unwrap_or(4)
    });
    let pairs: usize = pos.get(1).and_then(|s| s.parse().ok()).unwrap_or(12);
    let hashes: usize = pos.get(2).and_then(|s| s.parse().ok()).unwrap_or(256);

    eprintln!("building dataset (~45 s)...");
    let t = Instant::now();
    let vm_light = RandomXVm::new(KEY);
    let (cache, programs) = vm_light.cache_and_programs();
    let dataset = Arc::new(RandomXDataset::generate(
        cache,
        programs,
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4),
    ));
    drop(vm_light);
    eprintln!(
        "dataset ready in {:.1} s; cooling 30 s so the all-core build does not \
         bleed into phase 1",
        t.elapsed().as_secs_f64()
    );
    std::thread::sleep(Duration::from_secs(30));

    // Phase 1: single thread. Most sensitive — no memory-bandwidth contention
    // between threads to swamp a per-iteration change.
    eprintln!("phase 1: 1 thread, {pairs} A-B-B-A pairs x {hashes} hashes/round");
    let (b1, n1, c1) = ab_phase(&dataset, 0, pairs, hashes, None);
    assert_arms_agree("1 thread", &c1);
    report("1 thread", &b1, &n1);

    if threads > 1 {
        eprintln!("cooling 30 s before phase 2");
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("phase 2: {threads} threads, {pairs} pairs x {hashes} hashes/round");

        // Every thread runs its own A-B-B-A schedule in lockstep-ish fashion, so
        // both arms see the same memory-bandwidth pressure. This is the
        // configuration the miner actually runs in, where the dataset reads may
        // dominate anything the loop does.
        let barrier = Barrier::new(threads);
        let joined: Vec<PhaseOut> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..threads)
                .map(|tid| {
                    let ds = dataset.clone();
                    let bar = &barrier;
                    s.spawn(move || ab_phase(&ds, tid, pairs, hashes, Some(bar)))
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });

        // Every thread has joined, so asserting here cannot strand a sibling
        // inside `Barrier::wait()`.
        for (tid, (_, _, checks)) in joined.iter().enumerate() {
            assert_arms_agree(&format!("thread {tid}"), checks);
        }
        let results: Vec<(Vec<f64>, Vec<f64>)> =
            joined.into_iter().map(|(b, n, _)| (b, n)).collect();

        // Sum per-round rates across threads. Round i *starts* at the same
        // moment on every thread — that much the barrier above does enforce,
        // where it used to be an assumption that held only while both arms ran
        // at equal speed (GitHub #5). Without it the threads drifted out of
        // phase once the arms differed, so each arm's rounds partly overlapped
        // the other arm's rounds on sibling threads and the arms blended.
        //
        // Two things it does NOT give, both understated in the first version of
        // this comment:
        //   * a common *duration*. Threads still finish at different times, so
        //     summing rates (Sum h/T_t = n*h/HM(T)) still assumes equal windows.
        //     Measured at <=0.03 pp here, but it is an approximation, not an
        //     identity.
        //   * independence. The 24 rounds share one machine, and the barrier
        //     *increases* cross-thread coupling by tying every thread's window
        //     to the slowest. So this aggregate's n=24 overstates independence.
        //     The per-thread paired diffs below are the conservative reading and
        //     are the authoritative number; this one is reported for comparison.
        let rounds = results[0].0.len();
        let mut base_agg = vec![0.0; rounds];
        let mut nat_agg = vec![0.0; rounds];
        for (b, n) in &results {
            for i in 0..rounds {
                base_agg[i] += b[i];
                nat_agg[i] += n[i];
            }
        }
        spread_report(&results);
        report(&format!("{threads} threads (aggregate)"), &base_agg, &nat_agg);

        // Per-thread paired differences — **the authoritative number**. Each
        // thread's own A-B-B-A pairing is valid whatever the other threads are
        // doing, which the aggregate's round-level pairing is not.
        //
        // Not "independent", though: the barrier ties every thread's round
        // window to the slowest, so if anything this change *increases* the
        // coupling between these units. The honest claim is exchangeability —
        // the threads are interchangeable draws from one machine — not
        // independence, and the CI should be read with that in mind. It is
        // still the conservative of the two, and it is the one to quote.
        let per_thread: Vec<f64> = results
            .iter()
            .map(|(b, n)| {
                let d: Vec<f64> = b
                    .iter()
                    .zip(n.iter())
                    .map(|(x, y)| (y - x) / x * 100.0)
                    .collect();
                d.iter().sum::<f64>() / d.len() as f64
            })
            .collect();
        let (pt_mean, pt_ci) = mean_ci95(&per_thread);
        println!("\n=== {threads} threads (per-thread paired diffs — AUTHORITATIVE) ===");
        println!(
            "  mean of per-thread means: {pt_mean:+.2}%  (95% CI {:+.2}% .. {:+.2}%, n={})",
            pt_mean - pt_ci,
            pt_mean + pt_ci,
            per_thread.len()
        );
        print!("  per-thread   :");
        for d in &per_thread {
            print!(" {d:+.1}%");
        }
        println!();
    }
}
