-include mining.conf

.PHONY: build run bench test verify-jit verify-jit-linux clean check audit dist release help

POOL    ?= pool.supportxmr.com:443
WALLET  ?=
# THREADS unset by default: the binary auto-detects the performance-core count.
THREADS ?=
# DONATE_LEVEL unset by default: the binary uses its built-in default (5%).
DONATE_LEVEL ?=
# NATIVE_LOOP unset by default: the binary uses its built-in default (on).
# Set to "off" to fall back to the per-iteration body JIT without rebuilding —
# see mining.conf.example.
NATIVE_LOOP ?=
# VERIFY_SHARES unset by default: the binary uses its built-in default (on).
VERIFY_SHARES ?=

VERSION   := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
DIST_NAME := minertim-$(VERSION)-macos-arm64

help:
	@echo "MinerTim - Monero CLI miner for macOS (Apple Silicon)"
	@echo ""
	@echo "  make build            Build release binary (target-cpu=native, local only)"
	@echo "  make run              Build and run (requires WALLET=...)"
	@echo "  make bench            Run the RandomX hash benchmark"
	@echo "  make test             Run Rust unit tests (debug; NOT the JIT gate)"
	@echo "  make verify-jit       aarch64 JIT gate on this Mac (mandatory before"
	@echo "                        any MR touching src/randomx/jit/)"
	@echo "  make verify-jit-linux The same gate under native linux/arm64 (colima)"
	@echo "  make check            Quick type-check"
	@echo "  make audit            Scan dependencies for known vulnerabilities"
	@echo "  make dist             Build a portable release tarball + SHA256SUMS"
	@echo "  make release          Tag v$(VERSION) and push (triggers the CI release)"
	@echo "  make clean            Remove build artifacts"
	@echo ""
	@echo "  make run POOL=host:port WALLET=addr THREADS=8"

build:
	cargo build --release

run: build
ifndef WALLET
	$(error WALLET is required. Usage: make run POOL=host:port WALLET=your_address THREADS=N)
endif
	./target/release/minertim $(POOL) $(WALLET) $(THREADS) $(if $(DONATE_LEVEL),--donate-level $(DONATE_LEVEL),) $(if $(NATIVE_LOOP),--native-loop $(NATIVE_LOOP),) $(if $(VERIFY_SHARES),--verify-shares $(VERIFY_SHARES),)

bench:
	cargo bench

# Debug profile, whole suite. This is NOT the JIT gate: on x86_64 it cannot
# execute one emitted ARM64 instruction, and even on Apple Silicon it is the
# broad regression net rather than the targeted aarch64 evidence. Use
# `make verify-jit` for that.
test:
	cargo test

# ---------------------------------------------------------------------------
# The aarch64 JIT gate (issue #2). CI cannot run any of this — GitLab's shared
# runners are x86_64 Linux, where `randomx::jit` is cfg'd out of the build, so a
# green pipeline says nothing about emitted ARM64. Both targets are hard gates:
# non-zero exit on any failing test or an unexpected test count.
#
# Mandatory before any MR that touches src/randomx/jit/ (or vm.rs's native-loop
# path); paste the final PASS lines into the MR description. Issue #9 tracks
# replacing this with GitHub Actions, which gives public repos free `macos-14`
# and `ubuntu-24.04-arm` runners.
verify-jit:
	@./scripts/verify-jit.sh

# Expect ~15 minutes on a 4-vCPU colima VM (debug ~12 min, release ~3 min): the
# debug profile generates both 2 GiB test datasets unoptimised. It is not hung.
#
# Pinned image: the JIT is compiler- and libc-sensitive, and 1.97.1 is what
# every recorded Linux aarch64 result on this branch used. The named volumes
# keep the container's CARGO_TARGET_DIR and registry off the host `target/`,
# which holds macOS artifacts, while still making re-runs incremental.
JIT_LINUX_IMAGE  := rust:1.97.1
JIT_LINUX_TARGET := minertim-jit-linux-target
JIT_LINUX_CARGO  := minertim-jit-linux-cargo

verify-jit-linux:
	@command -v docker >/dev/null 2>&1 || { \
		echo "verify-jit-linux: no docker CLI on PATH."; \
		echo "  brew install colima docker && colima start --arch aarch64 --cpu 4 --memory 8"; \
		exit 1; }
	@docker info >/dev/null 2>&1 || { \
		echo "verify-jit-linux: the docker daemon is unreachable — colima is probably not running."; \
		echo "  colima start --arch aarch64 --cpu 4 --memory 8"; \
		echo "Refusing to skip: skipping silently is exactly the failure mode this gate exists to prevent."; \
		exit 1; }
	@a=$$(docker info --format '{{.Architecture}}'); \
		[ "$$a" = "aarch64" ] || { \
			echo "verify-jit-linux: the docker daemon reports $$a, not aarch64."; \
			echo "  A linux/arm64 container would run under qemu emulation, which does not"; \
			echo "  execute the host's real ARM64 — the result would prove nothing."; \
			echo "  colima delete && colima start --arch aarch64 --cpu 4 --memory 8"; \
			exit 1; }
	docker run --rm --platform linux/arm64 \
		-v "$(CURDIR)":/src:ro \
		-v $(JIT_LINUX_TARGET):/target \
		-v $(JIT_LINUX_CARGO):/usr/local/cargo/registry \
		-e CARGO_TARGET_DIR=/target \
		-w /src $(JIT_LINUX_IMAGE) \
		bash -c 'echo "container: $$(uname -m), $$(nproc) cpu, $$(awk "/MemTotal/ {printf \"%.1f GiB\", \$$2/1048576}" /proc/meminfo)"; exec ./scripts/verify-jit.sh'

check:
	cargo check

# Scan the locked dependency tree against the RustSec advisory database.
# Installs cargo-audit on first use if it is missing.
audit:
	@command -v cargo-audit >/dev/null 2>&1 || cargo install cargo-audit --locked
	cargo audit

# Portable release build. Overrides the local target-cpu=native (see
# .cargo/config.toml) with apple-m1 — a baseline that runs on every Apple
# Silicon Mac (M1 and newer), so the artifact won't SIGILL on other machines.
dist:
	@echo "Building portable release $(VERSION) (target-cpu=apple-m1)..."
	@RUSTFLAGS="-C target-cpu=apple-m1" cargo build --release
	@rm -rf dist/$(DIST_NAME) && mkdir -p dist/$(DIST_NAME)
	@cp target/release/minertim README.md LICENSE mining.conf.example dist/$(DIST_NAME)/
	@tar -C dist -czf dist/$(DIST_NAME).tar.gz $(DIST_NAME)
	@rm -rf dist/$(DIST_NAME)
	@cd dist && shasum -a 256 $(DIST_NAME).tar.gz > SHA256SUMS
	@echo "→ dist/$(DIST_NAME).tar.gz"
	@cat dist/SHA256SUMS

# Tag the current commit and push it; the CI 'release' job then creates the
# GitLab Release. Attach the dist tarball per RELEASING.md.
release:
	@test -z "$$(git status --porcelain)" || (echo "Working tree not clean; commit first." && exit 1)
	git tag -a v$(VERSION) -m "MinerTim v$(VERSION)"
	git push origin v$(VERSION)

clean:
	cargo clean
	rm -rf dist
