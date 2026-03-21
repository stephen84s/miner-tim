# MinerTim - Android Monero Miner

CPU-based Monero (XMR) mining app for Android with thermal and battery protection. Built with Kotlin + Rust.

> **Disclaimer:** Mining on mobile devices will generate heat, drain battery, and is generally unprofitable. This project is for educational purposes and Monero network support. Use on devices you own, at your own risk.

## Requirements

### Device
- Android 5.0+ (API 21)
- ARM, ARM64, x86, or x86_64 architecture
- 2+ CPU cores, 2GB+ RAM recommended

### Development Machine
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

| Command | Description |
|---|---|
| `./gradlew assembleDebug` | Debug APK (auto-builds Rust) |
| `./gradlew assembleRelease` | Release APK |
| `./gradlew test` | Run unit tests |
| `./gradlew connectedAndroidTest` | Run instrumentation tests (device required) |
| `./gradlew clean` | Clean all build artifacts |
| `cd app/src/main/rust && cargo check` | Quick Rust type-check (host target) |

## Deploy to Device

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
│       │       ├── miner.rs            # Mining workers
│       │       ├── pool_connection.rs  # Stratum protocol
│       │       └── randomx.rs          # RandomX hash algorithm
│       ├── res/                        # Layouts, themes, drawables
│       └── AndroidManifest.xml
├── build.gradle                        # Root: AGP 9.1.0 plugin
├── settings.gradle                     # Repository config
├── gradle/wrapper/                     # Gradle 9.4.1 wrapper
└── CLAUDE.md                           # Detailed architecture docs
```

## Architecture

**Kotlin** handles the Android UI, services, configuration, and security validation. **Rust** handles the performance-critical mining engine, pool communication, and RandomX hashing — connected via JNI.

```
UI (MainActivity) -> Service (MiningService) -> JNI (MiningCore.kt)
                                                    |
                                            Rust (libminertim.so)
                                            ├── miner.rs (thread pool)
                                            ├── pool_connection.rs (Stratum TCP)
                                            └── randomx.rs (hash computation)
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

## License

MIT
