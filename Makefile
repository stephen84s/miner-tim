-include mining.conf

.PHONY: build run bench test clean check audit dist release help

POOL    ?= pool.supportxmr.com:443
WALLET  ?=
# THREADS unset by default: the binary auto-detects the performance-core count.
THREADS ?=
# DONATE_LEVEL unset by default: the binary uses its built-in default (5%).
DONATE_LEVEL ?=

VERSION   := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
DIST_NAME := minertim-$(VERSION)-macos-arm64

help:
	@echo "MinerTim - Monero CLI miner for macOS (Apple Silicon)"
	@echo ""
	@echo "  make build            Build release binary (target-cpu=native, local only)"
	@echo "  make run              Build and run (requires WALLET=...)"
	@echo "  make bench            Run the RandomX hash benchmark"
	@echo "  make test             Run Rust unit tests"
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
	./target/release/minertim $(POOL) $(WALLET) $(THREADS) $(if $(DONATE_LEVEL),--donate-level $(DONATE_LEVEL),)

bench:
	cargo bench

test:
	cargo test

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
