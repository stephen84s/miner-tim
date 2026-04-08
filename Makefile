# Load config if present (copy mining.conf.example to mining.conf)
-include mining.conf

.PHONY: build release test clean install run logs rust-check rust-build rust-test cli cli-run help

# Default target
help:
	@echo "MinerTim Build Commands"
	@echo "======================"
	@echo ""
	@echo "  make build        - Build debug APK (includes Rust cross-compilation)"
	@echo "  make release      - Build release APK"
	@echo "  make test         - Run unit tests"
	@echo "  make test-device  - Run instrumentation tests (device required)"
	@echo "  make clean        - Clean all build artifacts"
	@echo "  make install      - Build and install debug APK on connected device"
	@echo "  make run          - Build, install, and launch the app"
	@echo "  make logs         - View app logs via adb logcat"
	@echo "  make rust-check   - Quick Rust type-check (host target)"
	@echo "  make rust-build   - Build Rust for single ABI (arm64-v8a)"
	@echo "  make rust-test    - Run Rust unit tests"
	@echo "  make cli          - Build CLI miner binary (native macOS)"
	@echo "  make cli-run      - Run CLI miner (usage: make cli-run POOL=host:port WALLET=addr [THREADS=N])"
	@echo ""

# Android builds
build:
	./gradlew assembleDebug

release:
	./gradlew assembleRelease

test:
	./gradlew test

test-device:
	./gradlew connectedAndroidTest

clean:
	./gradlew clean
	cd app/src/main/rust && cargo clean

# Device deployment
install: build
	adb install -r app/build/outputs/apk/debug/app-debug.apk

run: install
	adb shell am start -n com.minertim/.MainActivity

logs:
	adb logcat -s MinerTim:V Miner:V PoolConnection:V Crypto:V ThermalManager:V

# Rust-only targets (fast iteration)
rust-check:
	cd app/src/main/rust && cargo check

rust-build:
	cd app/src/main/rust && cargo ndk -t arm64-v8a -o ../jniLibs build --release

rust-test:
	cd app/src/main/rust && cargo test

# CLI miner (native macOS/Linux) — defaults from mining.conf
POOL ?= pool.supportxmr.com:443
WALLET ?=
THREADS ?= 2

cli:
	cd app/src/main/rust && RUSTFLAGS="-C target-cpu=native" cargo build --release --bin minertim

cli-run: cli
ifndef WALLET
	$(error WALLET is required. Usage: make cli-run POOL=host:port WALLET=your_address THREADS=4)
endif
	cd app/src/main/rust && ./target/release/minertim $(POOL) $(WALLET) $(THREADS)
