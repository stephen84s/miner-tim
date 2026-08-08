# MinerTim - Monero CLI Miner

CPU-based Monero (XMR) miner optimised for macOS (Apple Silicon). Pure Rust mining engine — no C/FFI dependencies. Stratum TCP/TLS pool support. aarch64 JIT compiler for maximum hashrate on M-series Macs.

> **MinerTim is a direct translation of [XMRig](https://github.com/xmrig/xmrig)'s
> RandomX miner into Rust, produced with AI assistance.** It is not an independent
> design — see [Acknowledgements](#acknowledgements).

## Requirements

- **macOS** on Apple Silicon (M1/M2/M3)
- **Rust 1.97+** via [rustup](https://rustup.rs)

Linux/x86_64 also supported (interpreter fallback, no JIT).

## Quick Start

```bash
# 1. Configure
cp mining.conf.example mining.conf
# Edit mining.conf — set WALLET to your Monero address

# 2. Build and run
make run
```

## Build Commands

| Command | Description |
|---|---|
| `make build` | Build release binary |
| `make run` | Build and run (reads `mining.conf`) |
| `make test` | Run Rust unit tests |
| `make check` | Quick type-check |
| `make clean` | Remove build artifacts |

Override config on the command line:

```bash
make run POOL=pool.hashvault.pro:443 WALLET=4...addr THREADS=12
```

Or run the binary directly:

```bash
./target/release/minertim pool.supportxmr.com:443 <wallet> 12
```

## Configuration

Create `mining.conf` from the example:

```bash
cp mining.conf.example mining.conf
```

```ini
POOL=pool.supportxmr.com:443
WALLET=4...your_monero_address
THREADS=12
```

## Donation (donate-level)

Like [XMRig](https://xmrig.com/docs/miner/donate), MinerTim donates a small,
configurable fraction of mining time. This is **on by default and disclosed** —
it is logged at startup every run.

- **Default: 5%** of mining time, split **50/50**: 2.5% to the MinerTim author and
  2.5% to **XMRig** (since MinerTim is an AI-assisted Rust translation of XMRig,
  a share goes upstream).
- **Configurable** down to a **minimum of 1%**:

  ```bash
  ./target/release/minertim <pool> <wallet> --donate-level 1
  # or in mining.conf:  DONATE_LEVEL=1
  ```

- Going **below 1% (or to zero) is intentionally not possible at runtime** — it
  requires editing `MIN_DONATE_LEVEL` in `src/donate.rs` and recompiling.

Mechanically, the miner briefly switches the pool login to each donation address
on a rolling 100-minute cycle (the same model XMRig uses). The donation
addresses live in [`src/donate.rs`](src/donate.rs).

## Project Structure

```
miner-tim/
├── Cargo.toml                  # Dependencies: serde_json, rustls, env_logger, ctrlc
├── .cargo/config.toml          # target-cpu=native for aarch64-apple-darwin
├── src/
│   ├── lib.rs                  # Crate root — re-exports modules
│   ├── bin/minertim.rs         # CLI entry point (args, Ctrl+C, stats loop)
│   ├── hex.rs                  # Shared hex encoding/decoding
│   ├── miner.rs                # Worker thread pool, hashrate tracking, share submission
│   ├── pool_connection.rs      # Stratum protocol: TCP/TLS, JSON-RPC 2.0, keepalive
│   └── randomx/                # Pure Rust RandomX implementation (rx/0)
│       ├── mod.rs
│       ├── vm.rs               # RandomX VM: program execution, hash computation, pipelining
│       ├── blake2b.rs          # Blake2b hash
│       ├── blake2gen.rs        # Blake2 generator for key/program derivation
│       ├── soft_aes.rs         # Software AES
│       ├── aes_hash.rs         # AES-based hash functions (fillAes1Rx4, hashAes1Rx4)
│       ├── argon2d.rs          # Argon2d cache initialisation (256 MiB)
│       ├── superscalar.rs      # SuperscalarHash program generation
│       ├── dataset.rs          # Dataset item computation from cache (2 GiB)
│       ├── tests.rs            # 87 test vectors
│       └── jit/                # aarch64 JIT compiler
│           ├── mod.rs
│           ├── memory.rs       # MAP_JIT memory, W^X toggle (macOS)
│           ├── aarch64.rs      # ARM64 instruction emitter
│           └── compiler.rs     # BytecodeInstruction[256] → native ARM64
├── Makefile
├── mining.conf.example
└── CLAUDE.md                   # Architecture reference for AI sessions
```

## Architecture

```
CLI (bin/minertim.rs)
  └── Miner (miner.rs)
        ├── PoolConnection (pool_connection.rs)  — Stratum TCP/TLS
        └── Worker threads × N
              └── RandomXVm (randomx/vm.rs)
                    ├── Full dataset (2 GiB, shared across workers via Arc)
                    ├── JIT compiler (aarch64) — compiles each program to native ARM64
                    └── Interpreter fallback (non-aarch64)
```

### Mining Flow

1. `Miner::initialize()` — connects to pool, sends Stratum `login`
2. Pool sends job (blob + target + job_id)
3. Thread 0 generates shared 2 GiB RandomX dataset (~46s on M2 Max)
4. All workers start: `prepare_scratchpad` → pipelined `calculate_hash_pipelined` loop
5. Each worker writes nonce at `blob[39..43]`, computes hash, checks against target
6. Share found → `pool.submit_share()` sends Stratum `submit`
7. New job from pool → workers pick it up atomically via `Arc<Mutex<Option<Arc<Job>>>>`

### JIT Compiler (aarch64)

Active on macOS aarch64. For each of the 8 RandomX program chains per hash:

1. `JitCompiler::compile()` translates `BytecodeInstruction[256]` to ARM64 machine code
2. Stored in a MAP_JIT region with W^X toggle (`pthread_jit_write_protect_np`)
3. Executed directly via function pointer — ~3× faster than interpreter

### Pipelined Hashing

`calculate_hash_pipelined` overlaps the AES scratchpad fill for the *next* input with the final Blake2b hash of the *current* output, hiding dataset-read latency.

### Stratum Protocol

- Newline-delimited JSON-RPC 2.0 over TCP (TLS via rustls)
- Login: `{"method":"login", "params":{"login":"<wallet>", "pass":"x", "agent":"MinerTim/1.0", "algo":"rx/0"}}`
- Pool sends `job` with `blob` (168 hex), `target` (8 hex), `job_id`
- Submit: `{"method":"submit", "params":{"job_id":"...", "nonce":"...", "result":"..."}}`
- Keepalive ping every 60s

## Performance

Measured on **Apple M2 Max** (11 threads — the default `cores − 1`), **plugged in
with macOS Low Power Mode OFF** — the power state matters a lot (see notes):

| Metric | Value |
|---|---|
| Hardware | Apple M2 Max (8 performance + 4 efficiency cores), 32 GB RAM |
| Dataset init | ~45s (one-time, ~2 GiB, before hashing begins) |
| Peak 1m hashrate | ~5,010 H/s |
| Sustained (1-hour average) | ~4,925 H/s |
| Stale-share rejects | ~0% (11 threads) vs ~15% (all 12) |
| On battery / Low Power Mode | ~3,800 H/s (≈20% lower) |
| Optimisation flags | `target-cpu=native`, LTO, `codegen-units=1` |

Measured over continuous 1-hour runs: **11 threads** → ~4,925 H/s avg, 5,013 peak,
**0 rejected shares**; **all 12 cores** → ~4,960 H/s avg (no faster) but ~15%
rejected. 11 threads earns ~18% more paid shares — see the acceptance note below.

**Notes**

- **Power state dominates.** On battery with Low Power Mode enabled, macOS caps
  CPU clocks and hashrate drops ~20%. For peak hashrate, plug in and turn Low
  Power Mode off. Sustained figures also sit a few percent under the cold-start
  peak once the chip heat-soaks under continuous load.
- **Leave one core free — this is the sweet spot.** RandomX is memory-bound, so
  the last thread adds almost nothing: on the M2 Max, 11 threads (4,925 H/s) and
  all 12 (4,960 H/s) have the same raw hashrate. But mining on *all* cores starves
  the pool receiver thread, so ~15% of shares are rejected as stale; 11 threads
  gives **0% rejects**, i.e. ~18% more accepted (paid) shares. The default is
  therefore `cores − 1` (11 here). Using 8 P-cores only would leave ~1,600 H/s on
  the table — the efficiency cores do contribute.
- **`target-cpu` barely affects hashrate.** The RandomX inner loop is JIT-compiled
  to ARM64 at runtime, so `native` and the portable `apple-m1` build (used by
  `make dist`) measure the same; `native` only helps the non-JIT support code.
- Hashrate is a rolling average that includes the ~45s dataset-init dead time at
  startup, so the `1m`/`5m`/`10m` figures read low for the first minute or two
  and then flatten — that is the average catching up, not the CPU ramping.
- **Share acceptance.** If you mine on *every* core, the pool receiver thread is
  CPU-starved, the current job goes stale, and shares are rejected as "Invalid
  job id" (~15% over a 1-hour run, even with the receiver at raised scheduling
  priority — the priority hint helps only marginally). The reliable fix is the
  default `cores − 1`, which measured **0 rejects** over a full hour. Prefer that
  over `THREADS=<all cores>`.

## Distribution

The binary is self-contained (pure Rust, no dynamic C dependencies).

```bash
make build
# Binary: target/release/minertim

# Ad-hoc codesign for local distribution (avoids Gatekeeper warning):
codesign -s - target/release/minertim
```

## Acknowledgements

**MinerTim is a direct, AI-assisted translation of [XMRig](https://github.com/xmrig/xmrig)
into Rust.** Its RandomX engine — the Argon2d cache, SuperscalarHash generation,
dataset/scratchpad construction, AES and Blake2 routines, the VM execution model,
and the aarch64 JIT (register allocation, MAP_JIT memory handling, prefetch
strategy) — all follow XMRig's C++ line by line. The Rust in this repository is a
re-expression of that prior work, generated with AI assistance; it is not an
independent implementation.

- **[XMRig](https://github.com/xmrig/xmrig)** — the miner this project is
  translated from. Mature, battle-tested, and multi-platform; if you mine
  seriously, use XMRig. GPL-3.0. All the hard design work is theirs.
- **[tevador/RandomX](https://github.com/tevador/RandomX)** — the RandomX
  proof-of-work algorithm and C++ reference implementation that XMRig itself is
  built on. BSD 3-Clause.

Because MinerTim is a translation of GPL-3.0 XMRig code, it is a derivative work
and is distributed under the same [GPL-3.0](LICENSE) license.

## License

[GNU General Public License v3.0](LICENSE)
