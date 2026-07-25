//! RandomX hash microbenchmark.
//!
//! Guards against hashrate regressions in the performance-critical VM/JIT
//! path — the project has a documented history of large (~36%) regressions
//! from seemingly innocuous changes, and this gives them a number instead of
//! a guess. Run with `make bench` (or `cargo bench`).
//!
//! Light mode is used deliberately: it needs no 2 GiB dataset, so the bench
//! is self-contained and fast to start, while still exercising the full
//! per-hash pipeline (Blake2b, AES fill, 8 JIT-compiled program chains,
//! SuperscalarHash dataset-item computation).

use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};

use minertim::randomx::vm::RandomXVm;

fn bench_hash(c: &mut Criterion) {
    // Fixed key + blob so runs are directly comparable over time.
    let key = b"MinerTim benchmark seed hash\0\0\0\0";
    let blob = [0x42u8; 76];

    let mut vm = RandomXVm::new(key);
    vm.prepare_scratchpad(&blob);

    let mut group = c.benchmark_group("randomx");
    // Each hash is hundreds of ms, so use flat sampling (one measurement per
    // sample) and a low sample count to keep total bench time bounded.
    group.sampling_mode(SamplingMode::Flat);
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(16));
    group.bench_function("hash_pipelined_light", |b| {
        b.iter(|| black_box(vm.calculate_hash_pipelined(black_box(&blob))));
    });
    group.finish();
}

criterion_group!(benches, bench_hash);
criterion_main!(benches);
