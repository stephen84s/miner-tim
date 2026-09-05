# MinerTim — a Monero miner for Apple Silicon Macs

A CPU miner for Monero (XMR), written entirely in Rust, tuned for M-series Macs.

> **This is a translation of [XMRig](https://github.com/xmrig/xmrig) into Rust,
> written with AI assistance.** It is not an independent design. If you mine
> seriously, use XMRig — it is mature, tested by many people, and runs
> everywhere. See [Acknowledgements](#acknowledgements).

## What you need

- A Mac with Apple Silicon (M1/M2/M3)
- [Rust](https://rustup.rs) 1.97 or newer
- About 3 GB of free memory while mining

## Getting started

```bash
cp mining.conf.example mining.conf   # then put your wallet address in it
make run
```

That is the whole setup. The first run spends about 45 seconds building a 2 GB
table in memory before it starts hashing — that is normal and happens once per
run.

## Configuration

`mining.conf` holds the settings:

```ini
POOL=pool.supportxmr.com:443
WALLET=4...your_monero_address
THREADS=                 # blank = one fewer than your core count (recommended)
DONATE_LEVEL=5
NATIVE_LOOP=             # blank = on
VERIFY_SHARES=           # blank = on
```

You can override any of them on the command line:

```bash
make run POOL=pool.hashvault.pro:443 WALLET=4...addr THREADS=12
```

Or run the built binary directly:

```bash
./target/release/minertim pool.supportxmr.com:443 <wallet> 12
```

### The two switches

Both are on by default and both exist as safety valves. You should not normally
need to touch them.

- **`NATIVE_LOOP`** — turns on the faster version of the mining inner loop
  (about 7% more hashes). If a future change ever breaks it, turning this off
  falls back to the slower version that has been in use much longer.
- **`VERIFY_SHARES`** — before sending a winning share to the pool, the miner
  recomputes it a second way and compares. If the two disagree, the share is
  thrown away rather than sent. This costs about 0.005% of mining time and
  protects against a bug silently producing wrong results.

These two are linked: verification works by comparing the fast path against the
slower one, so if you turn `NATIVE_LOOP` off there is nothing left to compare
against and verification switches off too. The miner tells you at startup which
are actually running, not just what you asked for.

If you set either to something the miner does not understand, it says so at
startup and picks the safe option rather than guessing.

## Commands

| Command | What it does |
|---|---|
| `make run` | Build and start mining, reading `mining.conf` |
| `make build` | Build the release binary only |
| `make test` | Run the test suite |
| `make verify-jit` | Run the machine-code tests on this Mac |
| `make verify-jit-linux` | Run the same tests under Linux on ARM (needs colima) |
| `make check` | Fast type-check, no binary |
| `make clean` | Delete build output |

## How fast is it

Measured on an **Apple M2 Max** (8 performance + 4 efficiency cores, 32 GB RAM),
plugged in, with Low Power Mode off:

| | |
|---|---|
| Peak hashrate | ~5,010 H/s |
| Sustained over an hour | ~4,925 H/s |
| Startup delay | ~45 seconds to build the 2 GB table |
| On battery, Low Power Mode on | ~3,800 H/s (about 20% slower) |

Three things matter more than they look:

**Leave one core free.** Using every core does not mine faster — 11 threads
measured 4,925 H/s and all 12 measured 4,960 H/s — but it starves the thread that talks
to the pool, so roughly 15% of your shares arrive too late and are rejected. At
11 threads, none were rejected over a full hour. That is about 18% more shares
you actually get paid for, which is why one-fewer-than-your-cores is the
default.

**Stay plugged in.** On battery with Low Power Mode on, macOS slows the CPU and
you lose about 20%.

**The first minute reads low.** The hashrate shown is an average that includes
the 45-second startup, so it climbs for a minute or two before settling. Nothing
is warming up — the average is just catching up.

## Which platforms are tested

| Platform | What runs | Tested by |
|---|---|---|
| macOS on Apple Silicon | Machine-code (JIT) path — this is the target | Automatically, on every change |
| Linux on ARM | Same machine-code path, tests only | Automatically, on every change |
| Linux/macOS on Intel | Slower fallback path only | Automatically, on every change |

The miner writes ARM64 machine code at runtime and jumps into it — that is where
the speed comes from, and it is also the riskiest part of the project. A mistake
there does not crash; it quietly produces wrong answers, and the pool rejects
your shares. So that code is tested on real Apple Silicon and real ARM Linux
every time anything changes, in two build modes, and a failure blocks the change.

The Intel tests are still useful — they cover the pool connection, the mining
loop and the dependency security audit — but they cannot say anything about the
machine-code path, because it is not even compiled on Intel.

You can run the same checks yourself with `make verify-jit`.

## Donations

Like [XMRig](https://xmrig.com/docs/miner/donate), MinerTim mines for its authors
a small part of the time. This is on by default and printed at startup every run.

- **Default 5%** of mining time, split evenly: 2.5% to the MinerTim author and
  2.5% to XMRig, since this project is a translation of theirs.
- You can lower it to **1%**:

  ```bash
  ./target/release/minertim <pool> <wallet> --donate-level 1
  # or in mining.conf:  DONATE_LEVEL=1
  ```

- Going below 1% is deliberately not possible without editing
  [`src/donate.rs`](src/donate.rs) and rebuilding.

It works by briefly logging into the pool with a different wallet address on a
rolling 100-minute cycle — the same approach XMRig uses.

## How it works

```
CLI (src/bin/minertim.rs)
  └── Miner (src/miner.rs)
        ├── Pool connection (src/pool_connection.rs) — Stratum over TCP/TLS
        └── Worker threads × N
              └── RandomX engine (src/randomx/)
                    ├── 2 GB dataset, built once and shared by all workers
                    ├── JIT compiler (Apple Silicon / ARM Linux)
                    └── Interpreter fallback (everything else)
```

Mining is a loop: the pool sends a job, each worker tries different nonces, and
anything that beats the target gets sent back as a share.

**The JIT.** RandomX generates a fresh random program for every hash. Rather than
interpreting those instructions one at a time, MinerTim translates them into
ARM64 machine code and runs them directly. It goes further and emits the whole
2048-iteration loop as machine code, instead of returning to Rust between
iterations — worth about 7% (measured over paired A/B runs; the same work, in the
same process, alternating between the two versions).

**Pipelining.** While finishing the current hash, the miner is already preparing
memory for the next one, so waiting on memory overlaps with useful work.

**Talking to the pool.** Standard Stratum: newline-separated JSON-RPC over TCP,
optionally wrapped in TLS. Login, receive jobs, submit shares, ping every 60
seconds to stay connected.

## Layout

```
src/
├── bin/minertim.rs      Command line, startup, statistics
├── miner.rs             Worker threads, hashrate, share submission and checking
├── pool_connection.rs   Stratum protocol, TCP/TLS
├── hex.rs               Hex encoding helpers
└── randomx/             The RandomX algorithm, in pure Rust
    ├── vm.rs            Runs the generated programs; picks JIT or interpreter
    ├── dataset.rs       Builds and reads the 2 GB dataset
    ├── argon2d.rs       Builds the 256 MB cache the dataset comes from
    ├── superscalar.rs   Generates the programs used to build the dataset
    ├── aes_hash.rs      AES-based memory filling and hashing
    ├── soft_aes.rs      AES without hardware instructions
    ├── blake2b.rs       Blake2b hashing
    ├── blake2gen.rs     Derives programs and keys from a seed
    ├── tests.rs         Known-answer tests and JIT-vs-interpreter comparisons
    └── jit/             Writes ARM64 machine code at runtime
        ├── compiler.rs  Turns RandomX instructions into ARM64
        ├── aarch64.rs   Encodes individual ARM64 instructions
        └── memory.rs    Gets executable memory (differs on macOS and Linux)

benches/                 Performance comparisons
scripts/verify-jit.sh    The machine-code test gate
.github/workflows/       Automated testing
```

`AUDIT.md` is a running log of every change and why it was made. `CLAUDE.md` is
the reference for AI coding sessions.

## Building a copy to share

```bash
make dist
```

This builds a version that runs on any M-series Mac, rather than tuning to the
one that built it, and produces a `.tar.gz` with checksums. See
[RELEASING.md](RELEASING.md).

To avoid a Gatekeeper warning on your own machine:

```bash
codesign -s - target/release/minertim
```

## Acknowledgements

**MinerTim is a direct, AI-assisted translation of
[XMRig](https://github.com/xmrig/xmrig) into Rust.** The RandomX engine — the
cache and dataset construction, the AES and Blake2 routines, the way the virtual
machine executes programs, and the ARM64 JIT including its register allocation
and memory handling — follows XMRig's C++ closely. The Rust here is a
re-expression of that work, not an independent implementation.

- **[XMRig](https://github.com/xmrig/xmrig)** — the miner this is translated
  from. GPL-3.0. All the hard design work is theirs.
- **[tevador/RandomX](https://github.com/tevador/RandomX)** — the RandomX
  algorithm and reference implementation XMRig itself builds on. BSD 3-Clause.

Because this is a translation of GPL-3.0 code, it is a derivative work and
carries the same licence.

## License

[GNU General Public License v3.0](LICENSE)
