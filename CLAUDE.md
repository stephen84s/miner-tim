# AI Agent Protocol / Project Manager

> **Note to AI:** This section defines your operational logic. You are the **Project Manager** and **Lead Engineer**. Follow these rules strictly.

## Identity & Mandate
- **Role:** Project Manager & Lead Engineer.
- **Mandate:** Execute tasks, verify code health, and maintain the `AUDIT.md` ledger.
- **Constraint:** **No implementation is complete until it is committed to `AUDIT.md`.**

## Operational Protocol
1.  **Task Analysis:** Break user requests into atomic steps.
2.  **Execution:** Implement changes in the repository.
3.  **Audit:** **Immediately** after implementation, append a detailed entry to `AUDIT.md`.
4.  **Status Update:** Update the `Current Task` table at the top of this file (`CLAUDE.md`) to reflect the new state. Do not leave tasks "Active" if they are completed.
5.  **Review:** Before replying "Done", verify `make check` and `make test` passed.

## Current Task Board
| Status | Task ID | Description |
| :--- | :--- | :--- |
| **Completed** | **SYS-01** | **Agent Initialization.** Establishing management protocol. |
| **Completed** | **NET-01** | **Pool robustness.** Fixed TLS double-session receiver bug; added keepalive, auto-reconnect/relogin, 8-byte target support. Clippy backlog cleared (63→0). |
| **Pending** | - | **Awaiting User Task** |

---

# CLAUDE.md - MinerTim
Monero (XMR) CPU miner for macOS (Apple Silicon). Pure Rust — no C/FFI dependencies. aarch64 JIT compiler, pipelined hashing, full RandomX dataset mode.

## Build & Run

```bash
make build        # Release binary (target-cpu=native via .cargo/config.toml)
make run          # Build + run (reads mining.conf)
make test         # Rust unit tests (87 vectors)
make check        # Quick type-check
make clean        # cargo clean
```

**CLI configuration:** Copy `mining.conf.example` to `mining.conf` and set `POOL`, `WALLET`, `THREADS`.

```bash
make run POOL=pool.supportxmr.com:443 WALLET=<addr> THREADS=12
./target/release/minertim pool.supportxmr.com:443 <wallet> 12
```

**Prerequisites:** Rust 1.94+ via rustup.

## Versions

| Component | Version |
|---|---|
| Rust edition | 2021 |
| serde_json | 1.0 |
| rustls | 0.23 |
| env_logger | 0.11 |

## Project Structure

```
src/
├── lib.rs                  # Crate root — pub mod declarations
├── bin/minertim.rs         # CLI entry point (args, env_logger, Ctrl+C, stats loop)
├── hex.rs                  # Shared hex_encode / hex_decode utilities
├── miner.rs                # Miner struct, worker thread pool, hashrate tracking
├── pool_connection.rs      # Stratum TCP/TLS, JSON-RPC 2.0, keepalive
└── randomx/
    ├── mod.rs              # Module exports; jit gated on target_arch = "aarch64"
    ├── vm.rs               # RandomXVm: program execution, JIT dispatch, pipelining
    ├── blake2b.rs          # Blake2b (256 and 512 bit)
    ├── blake2gen.rs        # Blake2 generator for key/program derivation
    ├── soft_aes.rs         # Software AES (4-round, no intrinsics)
    ├── aes_hash.rs         # fillAes1Rx4, hashAes1Rx4, hash_and_fill_aes_1rx4
    ├── argon2d.rs          # Argon2d cache init (256 MiB, 3 passes, 1 lane)
    ├── superscalar.rs      # SuperscalarHash program generation
    ├── dataset.rs          # Dataset item computation; SharedDatasetCache (Arc<Mutex>)
    ├── tests.rs            # 87 test vectors
    └── jit/                # aarch64 JIT (macOS only at runtime; module always compiled)
        ├── mod.rs          # Re-exports JitCompiler
        ├── memory.rs       # JitMemory: mmap MAP_JIT, pthread_jit_write_protect_np
        ├── aarch64.rs      # ARM64 instruction emitter (Emitter + reg constants)
        └── compiler.rs     # BytecodeInstruction[256] → ARM64; JitFn type alias
```

## Architecture

### Threading Model
- **Main thread:** CLI args, env_logger init, Ctrl+C handler, stats print loop (10s)
- **Rust std::thread:** 1 pool connection worker + N mining worker threads

### Mining Flow
1. `Miner::initialize(pool, wallet, threads)` — creates `PoolConnection`, TCP/TLS connects, sends Stratum `login`
2. Pool sends `job` (blob + target + job_id)
3. `Miner::start()` — spawns N workers; `dataset_cache = Arc::new(Mutex::new(None))`
4. Thread 0 calls `get_or_generate_dataset()` — generates 2 GiB dataset (~46s M2 Max); other threads wait on the same mutex
5. Each worker: `RandomXVm::new_full(seed, dataset)` → `prepare_scratchpad(blob)` → loop `calculate_hash_pipelined(next_blob)`
6. On hash ≤ target: `pool.submit_share(job_id, nonce_hex, hash_hex)`
7. Nonces interleaved: `nonce += thread_count`
8. New job from pool: worker picks it up via `pool.get_work()` → reinitialises VM if seed changed

### Pipelined Hashing (`vm.rs`)
`calculate_hash_pipelined(next_input)` overlaps work:
1. Runs 8 program chains on current scratchpad (JIT or interpreter)
2. Simultaneously calls `hash_and_fill_aes_1rx4` — hashes current scratchpad and fills new scratchpad for `next_input`
3. Returns the current hash; new scratchpad is ready for the next call

`prepare_scratchpad(input)` must be called once before entering the pipeline loop.

### JIT Compiler (`jit/compiler.rs`)
Active on aarch64. `JitCompiler::compile(bytecode)`:
1. Emits ARM64 prologue: saves callee-saved regs, loads nreg/scratchpad/config pointers
2. Translates each `BytecodeInstruction` to ARM64 via `emit_*` functions
3. Emits epilogue: restores regs, returns
4. Writes to `JitMemory` (MAP_JIT region), toggles W^X via `pthread_jit_write_protect_np`
5. `get_fn()` returns the function pointer; called as `f(nreg, scratchpad, config)`

**Register allocation:**
- `r[0..7]` → `x8..x15`; scratchpad → `x16`; e_mask → `x19/x20`; nreg ptr → `x21`
- FP: `f[0..3]` → `d0–d7`; `e[0..3]` → `d8–d15`; `a[0..3]` → `d16–d23`; FSCAL mask → `d24`

**CBRANCH:** `ibc.target` is `i16`. Cast to `i32` before `+1` to avoid overflow. Out-of-bounds target → fall through (no branch emitted).

### Stratum Protocol (`pool_connection.rs`)
- Newline-delimited JSON-RPC 2.0 over TCP; TLS via rustls + webpki-roots
- Login: `{"method":"login","params":{"login":"<wallet>","pass":"x","agent":"MinerTim/1.0","algo":"rx/0"}}`
- Job: `{"blob":"<168hex>","target":"<8hex>","job_id":"..."}`
- Submit: `{"method":"submit","params":{"job_id":"...","nonce":"<8hex>","result":"<64hex>"}}`
- Keepalive: `{"method":"keepalived"}` every 60s

### Dataset & Cache (`dataset.rs`)
`SharedDatasetCache = Arc<Mutex<Option<DatasetCache>>>`. `DatasetCache` holds `seed_hash` + `Arc<RandomXDataset>`. Thread 0 generates; others call `get_or_generate_dataset()` which waits on the mutex, then clones the `Arc`.

### Optimisation Flags
`.cargo/config.toml` sets `rustflags = ["-C", "target-cpu=native"]` for `aarch64-apple-darwin`. `Cargo.toml` release profile: `lto=true`, `opt-level=3`, `codegen-units=1`, `strip=true`.

## Conventions
- **Rust:** `snake_case` functions/variables, `PascalCase` types, `UPPER_SNAKE_CASE` consts
- **Logging:** `env_logger` with `RUST_LOG=info` (default); structured with module path
- **Error handling:** `Result<T, String>` at pool boundaries; panics only for programmer errors

## AI Session Audit Requirement

For any AI-assisted implementation session:

- Maintain an audit log in repository root: `AUDIT.md`.
- Append an entry for each implementation batch that changes repo-tracked files.
- Each entry should include:
  - request/goal summary,
  - files changed,
  - behaviour/API changes,
  - verification performed (build/tests/runtime checks),
  - notable assumptions or constraints.
- Do not delete prior audit history; append chronologically.
