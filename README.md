# MinerTim - Android Monero Miner

CPU-based Monero (XMR) miner for Android and macOS/Linux. Android app with thermal and battery protection (Kotlin UI + Rust JNI). Native CLI binary for desktop mining. Pure Rust mining engine — no C/FFI dependencies.

> **Disclaimer:** Mining on mobile devices will generate heat, drain battery, and is generally unprofitable. This project is for educational purposes and Monero network support. Use on devices you own, at your own risk.

## Requirements

### Android Device
- Android 5.0+ (API 21)
- ARM, ARM64, x86, or x86_64 architecture
- 2+ CPU cores, 2GB+ RAM recommended

### Desktop (CLI)
- **macOS** (tested) or Linux
- **Rust 1.94+** via [rustup](https://rustup.rs)

### Development (Android builds)
- **macOS** (tested), Linux should also work
- **Java 17** (e.g. Temurin via SDKMAN)
- **Rust 1.94+** via [rustup](https://rustup.rs)
- **Android SDK** with NDK 26.1
- **cargo-ndk** for cross-compiling Rust to Android

## Setup

### 1. Install Rust and Android Targets

```bash
# Install rustup if you don't have it
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Add Android cross-compilation targets
rustup target add \
  aarch64-linux-android \
  armv7-linux-androideabi \
  i686-linux-android \
  x86_64-linux-android

# Install cargo-ndk
cargo install cargo-ndk
```

### 2. Install Android SDK and NDK

If you don't have Android Studio:

```bash
# macOS (via Homebrew)
brew install --cask android-commandlinetools

# Create SDK directory and install components
export ANDROID_HOME="$HOME/Library/Android/sdk"
mkdir -p "$ANDROID_HOME"

yes | sdkmanager --sdk_root="$ANDROID_HOME" \
  "platform-tools" \
  "platforms;android-35" \
  "build-tools;36.0.0" \
  "ndk;26.1.10909125"
```

### 3. Set Environment Variables

Add to your `~/.zshrc` (or `~/.bashrc`):

```bash
export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/26.1.10909125"
export PATH="$ANDROID_HOME/platform-tools:$PATH"
```

Then reload: `source ~/.zshrc`

### 4. Clone and Build

```bash
git clone <repo-url> miner-tim
cd miner-tim
./gradlew assembleDebug
```

The Gradle build automatically cross-compiles the Rust native library for all 4 Android ABIs before packaging.

**Output:** `app/build/outputs/apk/debug/app-debug.apk` (~9MB)

## Build Commands

All commands are available via `make help`.

| Command | Description |
|---|---|
| `make build` | Debug APK (auto-builds Rust) |
| `make release` | Release APK |
| `make test` | Run unit tests |
| `make rust-check` | Quick Rust type-check (host target) |
| `make rust-test` | Run Rust unit tests |
| `make cli` | Build CLI miner binary (native macOS/Linux) |
| `make cli-run` | Build + run CLI miner (reads `mining.conf`) |
| `make clean` | Clean all build artifacts |

## Desktop CLI Mining

The Rust mining engine can run natively on macOS/Linux without Android.

### Quick Start

```bash
# 1. Configure
cp mining.conf.example mining.conf
# Edit mining.conf — set WALLET to your Monero address

# 2. Build and run
make cli-run

# Binary output: app/src/main/rust/target/release/minertim
```

### Configuration

Create `mining.conf` from the example:

```bash
cp mining.conf.example mining.conf
```

```ini
# mining.conf
POOL=pool.supportxmr.com:443
WALLET=4...your_monero_address
THREADS=2
```

Values can also be overridden on the command line:

```bash
make cli-run POOL=pool.hashvault.pro:443 WALLET=4...addr THREADS=4
```

Or run the binary directly:

```bash
cd app/src/main/rust
cargo build --release --bin minertim
./target/release/minertim pool.supportxmr.com:443 <wallet> 4
```

### Build Output

| Target | Path |
|---|---|
| CLI binary | `app/src/main/rust/target/release/minertim` |
| Debug APK | `app/build/outputs/apk/debug/app-debug.apk` |
| Release APK | `app/build/outputs/apk/release/app-release.apk` |

### Distributing the Binary

The CLI binary is self-contained (pure Rust, no C dependencies). To share it:

1. Build: `make cli`
2. Binary is at `app/src/main/rust/target/release/minertim`
3. For macOS distribution, codesign to avoid Gatekeeper warnings:
   ```bash
   codesign -s - app/src/main/rust/target/release/minertim  # ad-hoc (local use)
   ```

## Deploy to Device (Android)

### USB (ADB)

```bash
# Enable Developer Options and USB Debugging on your Android device, then:
adb devices                  # Verify device is connected
adb install app/build/outputs/apk/debug/app-debug.apk

# Or build + install in one step:
./gradlew installDebug
```

### Wireless ADB (Android 11+)

```bash
# On device: Settings > Developer Options > Wireless debugging > Pair
adb pair <ip>:<port>         # Enter pairing code
adb connect <ip>:<port>
./gradlew installDebug
```

### View Logs

```bash
adb logcat -s MinerTim:V Miner:V PoolConnection:V Crypto:V ThermalManager:V
```

## App Usage

### First-Time Setup

1. **Get a Monero wallet** — use the official Monero GUI/CLI wallet or a mobile wallet like Cake Wallet. Copy your address (95 characters, starts with `4`).

2. **Open MinerTim** and configure:
   - **Wallet address** — your Monero address
   - **Pool address** — e.g. `pool.supportxmr.com:443`
   - **Thread count** — start with `cores - 1` (e.g. 3 on a quad-core)
   - **Temperature limit** — default 75°C is safe for most devices
   - **Battery minimum** — default 20%, mining pauses below this

3. **Tap Start Mining** — the app runs as a foreground service with a persistent notification showing hashrate.

### Supported Pools

These pools are pre-whitelisted and connect without warnings:

| Pool | Address |
|---|---|
| SupportXMR | `pool.supportxmr.com:443` |
| XMRPool.eu | `xmrpool.eu:443` |
| Nanopool | `xmr.nanopool.org:14433` |
| HashVault | `pool.hashvault.pro:443` |
| MoneroHash | `monerohash.com:443` |
| XMRPool.net | `xmrpool.net:443` |

Other Stratum-compatible pools will work but show a risk warning.

### Safety Features

- **Thermal throttling** — automatically pauses mining if CPU temperature exceeds the configured limit, resumes after 30s cooldown
- **Battery protection** — stops mining when battery drops below minimum level
- **WakeLock** — keeps CPU awake during mining (foreground service)
- **WiFi-only mode** — prevents mining on metered mobile data

### Expected Performance

| Device Type | Hashrate |
|---|---|
| Older phones (2-4 cores) | 5-20 H/s |
| Mid-range (6-8 cores) | 20-50 H/s |
| Flagship | 40-80 H/s |

Mobile mining is not profitable. Electricity costs will exceed earnings.

## Project Structure

```
miner-tim/
├── app/
│   ├── build.gradle                    # AGP 9.1.0, dependencies, Rust build task
│   └── src/main/
│       ├── java/com/minertim/
│       │   ├── MainActivity.kt         # UI (Material Design 3, ViewBinding)
│       │   ├── mining/
│       │   │   ├── MiningCore.kt       # JNI interface to Rust
│       │   │   └── MiningService.kt    # Foreground service
│       │   ├── thermal/
│       │   │   └── ThermalManager.kt   # CPU temp & battery monitoring
│       │   ├── config/
│       │   │   └── MiningConfig.kt     # Encrypted config (AES-256)
│       │   └── security/
│       │       └── SecurityValidator.kt # Input validation & pool whitelist
│       ├── rust/                        # Native mining engine
│       │   ├── Cargo.toml
│       │   └── src/
│       │       ├── lib.rs              # JNI bridge
│       │       ├── bin/minertim.rs     # CLI binary entry point
│       │       ├── miner.rs            # Mining workers
│       │       ├── pool_connection.rs  # Stratum protocol (TCP/TLS)
│       │       └── randomx/            # Pure Rust RandomX (light mode)
│       ├── res/                        # Layouts, themes, drawables
│       └── AndroidManifest.xml
├── Makefile                            # Build targets (make help)
├── mining.conf.example                 # CLI config template
├── build.gradle                        # Root: AGP 9.1.0 plugin
├── settings.gradle                     # Repository config
├── gradle/wrapper/                     # Gradle 9.4.1 wrapper
└── CLAUDE.md                           # Detailed architecture docs
```

## Architecture

**Kotlin** handles the Android UI, services, configuration, and security validation. **Rust** handles the performance-critical mining engine, pool communication, and RandomX hashing. The Rust crate builds as both a JNI library (Android) and a native CLI binary (macOS/Linux).

```
Android:  UI (MainActivity) -> Service (MiningService) -> JNI (MiningCore.kt)
                                                              |
Desktop:  CLI (bin/minertim.rs) ─────────────────────────────┘
                                                              |
                                                  Rust (miner.rs)
                                                  ├── pool_connection.rs (Stratum TCP/TLS)
                                                  └── randomx/ (pure Rust, light mode)
```

For detailed architecture documentation, see [CLAUDE.md](CLAUDE.md).

## Troubleshooting

**Build fails with "cargo: command not found"**
— Ensure `source "$HOME/.cargo/env"` is in your shell profile, or run it before building.

**Build fails with "NDK not found"**
— Set `ANDROID_NDK_HOME` environment variable. The Gradle build expects it at `$ANDROID_HOME/ndk/26.1.10909125`.

**Build fails with Kotlin errors**
— Run `./gradlew clean` then rebuild. AGP 9.1.0 bundles Kotlin — do not add a separate Kotlin plugin.

**Mining won't start**
— Check: wallet address is 95+ chars starting with `4`, pool address includes `:port`, device has internet, temperature and battery are within limits.

**No shares after extended mining**
— Normal for low hashrate devices. At 10 H/s it can take hours to find a share depending on pool difficulty.

## Acknowledgements

- **[tevador/RandomX](https://github.com/tevador/RandomX)** — The RandomX proof-of-work algorithm and its C++ reference implementation. The Rust mining engine (`randomx/`) is a port of the reference implementation, following the same algorithmic structure for the VM, AES hashing, Argon2d cache, Blake2b, SuperscalarHash, and dataset generation. RandomX is licensed under BSD 3-Clause.

- **[XMRig](https://github.com/xmrig/xmrig)** — The aarch64 JIT compiler design (register allocation scheme, JIT memory management approach, and scratchpad/dataset prefetch strategy) was informed by XMRig's RandomX JIT implementation. XMRig is licensed under GPL-3.0.

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE).
