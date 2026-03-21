# CLAUDE.md - MinerTim

Android Monero (XMR) CPU mining app. Kotlin UI/services + Rust native mining engine via JNI. Designed for older Android devices with thermal/battery protection.

## Build & Run

**Environment variables required:**
```bash
export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/26.1.10909125"
```

**Commands:**
```bash
./gradlew assembleDebug          # Build debug APK (auto-builds Rust)
./gradlew assembleRelease        # Build release APK
./gradlew test                   # Unit tests
./gradlew connectedAndroidTest   # Instrumentation tests
./gradlew clean                  # Clean build artifacts

# Rust only (fast iteration)
cd app/src/main/rust && cargo check                      # Type-check (host target)
cd app/src/main/rust && cargo ndk -t arm64-v8a -o ../jniLibs build --release  # Single ABI
```

**Prerequisites:** Rust 1.94+ via rustup, `cargo-ndk`, Android SDK (platform 35, NDK 26.1), Java 17.
Rust Android targets: `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`

## Versions

| Component | Version |
|---|---|
| AGP | 9.1.0 (Kotlin built-in, no separate kotlin-android plugin) |
| Gradle | 9.4.1 |
| compileSdk / targetSdk | 35 |
| minSdk | 21 (Android 5.0) |
| Rust edition | 2021 |
| jni crate | 0.22.4 |
| Java compatibility | 17 |

## Project Structure

```
app/src/main/
├── java/com/minertim/
│   ├── MainActivity.kt              # Main UI (Material Design 3, ViewBinding, bound service)
│   ├── mining/
│   │   ├── MiningCore.kt            # JNI declarations — loads libminertim.so
│   │   └── MiningService.kt         # Foreground service (WakeLock, notification, coroutines)
│   ├── thermal/
│   │   └── ThermalManager.kt        # CPU temp + battery monitoring (reads /sys/class/thermal/)
│   ├── config/
│   │   └── MiningConfig.kt          # SharedPreferences with AES-256 wallet encryption
│   └── security/
│       └── SecurityValidator.kt     # Address validation, pool whitelist, input sanitization
├── rust/                             # Native mining engine
│   ├── Cargo.toml                   # jni 0.22, serde_json, android_logger
│   └── src/
│       ├── lib.rs                   # JNI entry point (9 exported functions)
│       ├── miner.rs                 # Worker thread pool, hashrate tracking, share submission
│       ├── pool_connection.rs       # Stratum protocol: TCP socket, JSON-RPC, keepalive
│       └── randomx.rs              # RandomX VM: Blake2b, SipHash, program gen/exec
├── res/
│   ├── layout/activity_main.xml    # Material CardView layout
│   ├── mipmap-anydpi-v26/          # Adaptive icons (requires SDK 26+)
│   ├── values/                     # strings.xml, colors.xml, themes.xml
│   ├── drawable/                   # ic_play_arrow, ic_stop, ic_mining_notification
│   └── xml/                        # backup_rules, data_extraction_rules
└── AndroidManifest.xml             # Permissions, MainActivity, MiningService (foregroundServiceType=dataSync)
```

## Architecture

### Layer Diagram
```
┌─────────────────────────────────────────────┐
│  MainActivity (UI)                          │
│  ViewBinding, lifecycleScope, ServiceConn   │
├─────────────────────────────────────────────┤
│  MiningService (Foreground Service)         │
│  WakeLock, Notification updates (5s),       │
│  thermal throttling callback                │
├──────────────────┬──────────────────────────┤
│  ThermalManager  │  MiningConfig            │
│  /sys/thermal    │  SharedPrefs + AES-256   │
│  BatteryManager  │  SecurityValidator       │
├──────────────────┴──────────────────────────┤
│  MiningCore.kt (JNI boundary)               │
│  System.loadLibrary("minertim")             │
├─════════════════════════════════════════════┤
│  Rust: lib.rs → miner.rs                   │
│         ├── pool_connection.rs (TCP/JSON)   │
│         └── randomx.rs (hash computation)   │
└─────────────────────────────────────────────┘
```

### Threading Model
- **Main thread:** Android UI (MainActivity)
- **Default dispatcher:** MiningService coroutine scope (stats updates every 5s, thermal throttle recovery after 30s)
- **IO dispatcher:** ThermalManager monitoring loop (every 5s, reads sysfs + battery)
- **Rust std::thread:** N mining worker threads (configurable 1–hardware_max), pool connection worker thread

### JNI Interface (MiningCore.kt ↔ lib.rs)
`MiningCore.kt` declares `external fun` methods. `lib.rs` exports matching `Java_com_minertim_mining_MiningCore_*` symbols.

| Kotlin | Rust | Purpose |
|---|---|---|
| `initializeMiner(pool, wallet, threads)` | Creates `Miner`, connects to pool | Returns false on invalid input or connection failure |
| `startMining()` / `stopMining()` | Spawns/joins worker threads | Atomic `MINING_ACTIVE` flag guards state |
| `getHashrate()` | `total_hashes / elapsed_seconds` | Updated by thread 0 every 5s |
| `getAcceptedShares()` / `getRejectedShares()` | Atomic counters | Incremented on pool submit response |
| `isMining()` | Reads `MINING_ACTIVE` AtomicBool | |
| `setThreadCount(n)` | Clamped to hardware max | Only while not mining |

### Rust JNI Pattern (jni 0.22)
The crate split `JNIEnv` into `EnvUnowned` (FFI-safe) and `Env` (full API). Native methods receive `EnvUnowned` and use:
```rust
unowned_env.with_env(|env| -> Result<T> {
    // use env.new_string(), jstring.mutf8_chars(env), etc.
    Ok(value)
}).resolve::<ThrowRuntimeExAndDefault>()
```
Functions that don't need `Env` (getHashrate, isMining, etc.) take `_env: EnvUnowned` and ignore it.

Global state: `static MINER: Mutex<Option<Miner>>` and `static MINING_ACTIVE: AtomicBool`.

### Mining Flow
1. User taps start → `MiningService.startMining()` checks thermal/battery via `ThermalManager`
2. Calls `MiningCore.initializeMiner()` → Rust creates `PoolConnection`, TCP connects, sends Stratum `login` JSON-RPC
3. Pool connection worker thread receives job notifications (blob + target + difficulty)
4. `MiningCore.startMining()` → Rust spawns N worker threads, each:
   - Initializes local RandomX VM (dataset + cache from key)
   - Loops: get work → set nonce at blob[39..47] → compute RandomX hash → compare to target
   - On share found: `pool.submit_share()` sends JSON-RPC `submit`
   - Nonces interleaved: `nonce += thread_count` to avoid overlap
5. `ThermalManager` monitors every 5s; if temp/battery exceeded, invokes throttle callback → stops mining for 30s cooldown

### Stratum Protocol (pool_connection.rs)
- TCP with newline-delimited JSON-RPC 2.0
- Login: `{"method":"login", "params":{"login":"<wallet>", "pass":"android", "agent":"MinerTim/1.0", "algo":"rx/0"}}`
- Pool sends `job` notifications with `blob` (168 hex = 84 bytes), `target` (8 hex = 4 bytes), `job_id`
- Share submit: `{"method":"submit", "params":{"job_id":"...", "nonce":"...", "result":"..."}}`
- Keepalive every 60s

### Security (SecurityValidator.kt)
- Monero address regex: mainnet `4...` (95 chars), testnet `9/A/B...`, stagenet `5...`, integrated `4...` (106 chars)
- Pool whitelist: 12 known pools (supportxmr, nanopool, hashvault, etc.) — non-whitelisted pools get HIGH risk
- Input sanitization: rejects `<script`, `javascript:`, `eval(`, path traversal, `file://`
- `ValidationResult` includes `isValid`, `errorMessage`, `warningMessage`, `riskLevel` (LOW/MEDIUM/HIGH/CRITICAL)

### Configuration (MiningConfig.kt)
SharedPreferences file: `mining_config_secure`. Wallet address encrypted with AES-256-ECB (key stored Base64 in prefs).

| Key | Default | Range |
|---|---|---|
| `pool_address` | `pool.supportxmr.com:443` | Must contain `:port` |
| `wallet_address` | (empty) | 95–106 chars, encrypted |
| `thread_count` | 2 | 1 to availableProcessors() |
| `max_cpu_temp` | 75.0°C | 40–90 |
| `min_battery_level` | 20% | 5–95 |
| `mining_intensity` | 50 | 1–100 |
| `auto_start` | false | |
| `wifi_only` | true | |

## Gradle Build Integration

The `buildRust` Exec task in `app/build.gradle` runs `cargo ndk` for all 4 ABIs, outputting `.so` files to `app/src/main/jniLibs/{abi}/libminertim.so`. It hooks into Gradle via:
```groovy
tasks.whenTaskAdded { task ->
    if (task.name.startsWith('merge') && task.name.endsWith('JniLibFolders')) {
        task.dependsOn buildRust
    }
}
```
NDK path is resolved from `ANDROID_NDK_HOME` env var, falling back to `$ANDROID_HOME/ndk/26.1.10909125`.

## Android Permissions
`INTERNET` (pool), `ACCESS_NETWORK_STATE` (network type), `WAKE_LOCK` (prevent sleep), `FOREGROUND_SERVICE` (background mining), `BATTERY_STATS`, `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`, `ACCESS_WIFI_STATE`.

## Conventions
- **Kotlin:** camelCase properties, PascalCase classes
- **Rust:** snake_case functions/variables, PascalCase types, UPPER_SNAKE_CASE consts
- **XML resources:** snake_case (`activity_main`, `ic_play_arrow`)
- **Logging:** Android Log API with TAG constants (Kotlin), `log` crate with `android_logger` (Rust)

## Known Issues / Legacy
- AES encryption uses ECB mode (`AES/ECB/PKCS5Padding`) which is not semantically secure. Should migrate to GCM.
