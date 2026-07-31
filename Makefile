-include mining.conf

.PHONY: build run bench test clean check audit help

POOL    ?= pool.supportxmr.com:443
WALLET  ?=
# THREADS unset by default: the binary auto-detects the performance-core count.
THREADS ?=
# DONATE_LEVEL unset by default: the binary uses its built-in default (5%).
DONATE_LEVEL ?=

help:
	@echo "MinerTim - Monero CLI miner for macOS (M2 Max optimised)"
	@echo ""
	@echo "  make build            Build release binary"
	@echo "  make run              Build and run (requires WALLET=...)"
	@echo "  make bench            Run the RandomX hash benchmark"
	@echo "  make test             Run Rust unit tests"
	@echo "  make check            Quick type-check"
	@echo "  make audit            Scan dependencies for known vulnerabilities"
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

clean:
	cargo clean
