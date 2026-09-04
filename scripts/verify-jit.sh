#!/usr/bin/env bash
#
# The aarch64 JIT gate — the tests CI structurally cannot run.
#
# GitLab's shared runners are x86_64 Linux, where `randomx::jit` is cfg'd out of
# the build entirely (`src/randomx/mod.rs`). A fully green pipeline therefore
# says *nothing* about emitted ARM64, which is the code that decides whether a
# share is accepted. This script is that missing coverage, run by a human:
#
#   make verify-jit         — this script on the Apple Silicon host
#   make verify-jit-linux   — this script inside a native linux/arm64 container
#
# Issue #2 tracks the gap; issue #9 tracks moving it into GitHub Actions, which
# gives public repos free `macos-14` and `ubuntu-24.04-arm` runners.
#
# It is a HARD gate: any failing test, or a test count that does not match the
# expectation below, exits non-zero.

set -uo pipefail

# ---------------------------------------------------------------------------
# What runs, and why these filters
# ---------------------------------------------------------------------------
# Substring filters over `cargo test --lib` (libtest ORs multiple filters).
# Derived from `cargo test --release --lib -- --list`, not guessed.
#
#   randomx::jit::                  the emitter, compiler and JIT-memory unit
#                                   tests (66) — encodings checked bit-for-bit
#   native_loop_diff_tests::        differential: the emitted native loop vs the
#                                   interpreter, from byte-identical state (4)
#   full_hash_tests::               known-answer vectors and the native-loop
#                                   guards, end-to-end through the JIT
#                                   (15 + 1 ignored 2 GiB light-vs-full test)
#   full_hash_v2_tests::            RandomX v2 known-answer vectors (2)
#   v2_jit_tests::                  v2 through the JIT + the share-cache key (2)
#   randomx::vm::native_loop        the guards that decide whether the native
#                                   loop is used at all (3)
#
# LOAD-BEARING, do not trim: `full_mode_v1_vm_reports_the_native_loop_effective`
# (inside full_hash_tests::) is the only test that hard-requires a *successful*
# JIT allocation. Every known-answer vector still passes when allocation fails,
# because the interpreter fallback returns the same hash — that is issue #4's
# shape. Without this one test the gate can be green on an inert JIT.
JIT_FILTERS=(
  randomx::jit::
  randomx::tests::native_loop_diff_tests::
  randomx::tests::full_hash_tests::
  randomx::tests::full_hash_v2_tests::
  randomx::tests::v2_jit_tests::
  randomx::vm::native_loop
)

# Exact pass count. libtest exits 0 when a filter matches nothing, so without
# this the gate would go green after a module rename silently emptied it.
# Update deliberately, in the same commit that adds or removes a test.
# 66 jit unit + 4 differential + 15 known-answer/guard + 2 v2 + 2 v2-jit + 3 vm.
EXPECTED_PASSES=92

# ---------------------------------------------------------------------------
# Host guard: on x86_64 every test above is cfg'd out and libtest exits 0.
# ---------------------------------------------------------------------------
arch="$(uname -m)"
case "$arch" in
  arm64 | aarch64) ;;
  *)
    echo "verify-jit: host is $arch, not aarch64." >&2
    echo "verify-jit: the JIT does not exist on this architecture — there is" >&2
    echo "            nothing to gate here, and a pass would be meaningless." >&2
    exit 1
    ;;
esac

echo "verify-jit: $(uname -s) $arch, $(rustc --version)"

fail=0

# run_group <label> <profile-flag-or-empty>
run_group() {
  local label="$1" profile="$2"
  local log
  log="$(mktemp "${TMPDIR:-/tmp}/verify-jit.XXXXXX")"

  echo
  echo "=== $label ==="
  if [ -n "$profile" ]; then
    cargo test "$profile" --locked --lib -- "${JIT_FILTERS[@]}" 2>&1 | tee "$log"
  else
    cargo test --locked --lib -- "${JIT_FILTERS[@]}" 2>&1 | tee "$log"
  fi
  local status=${PIPESTATUS[0]}

  local passed
  passed="$(grep -E '^test result:' "$log" | tail -1 |
            sed -E 's/^test result: [a-zA-Z]+\. ([0-9]+) passed.*/\1/')"
  rm -f "$log"

  if [ "$status" -ne 0 ]; then
    echo "verify-jit: FAIL — $label exited $status" >&2
    fail=1
    return
  fi
  if [ "$passed" != "$EXPECTED_PASSES" ]; then
    echo "verify-jit: FAIL — $label ran '$passed' tests, expected $EXPECTED_PASSES." >&2
    echo "            Either a test was added or removed (update EXPECTED_PASSES" >&2
    echo "            in scripts/verify-jit.sh) or a filter no longer matches" >&2
    echo "            anything, which libtest reports as success." >&2
    fail=1
    return
  fi
  echo "verify-jit: OK — $label, $passed passed"
}

# ---------------------------------------------------------------------------
# 1. Debug profile — the only profile in which `debug_assert!` executes.
# ---------------------------------------------------------------------------
# `make test` is debug but is not this gate, and every recorded JIT result has
# been release, where `debug_assert!` is compiled out. The guards added for the
# native loop — the imm12/imm7 encoding ranges in `jit/aarch64.rs`, the CBRANCH
# forward-target rule, the CBZ zero-iteration patch range and the back-branch
# imm19 range in `jit/compiler.rs` — had therefore never executed in the
# profile that gets cited as evidence. Issue #6 / issue #2 mitigation 2.
#
# The whole set runs here, not a subset: it was measured at ~308 s on an idle
# M2 Max against 177 s for the JIT-unit-plus-differential subset, and the
# known-answer vectors push ~80 further real programs through the same
# assertions. Cheap
# enough that "which profile is authoritative" needs no caveat — both run.
run_group "debug profile (debug_assert! live)" ""

# ---------------------------------------------------------------------------
# 2. Release profile — what the miner ships, and what every number refers to.
# ---------------------------------------------------------------------------
# Memory: the binary builds ONE 2 GiB dataset (issue #7 removed the second).
# Max RSS on an M2 Max for this filtered set in the debug profile: 6.77 GB
# before that change, 4.50 GB after. Both figures are the *test binary*
# measured directly (`/usr/bin/time -l target/debug/deps/minertim-* <filters>`),
# not `make verify-jit`, which adds cargo and rustc on top — reproducing it
# through the target will read higher. Peak scales with libtest's
# --test-threads, which defaults to the core count, because each concurrent
# test may hold its own 256 MiB Argon2d cache; pass --test-threads to trade
# wall time for headroom on a small runner. Release is authoritative for hash
# values and for anything quoted as a measurement; debug is authoritative for
# the assertions.
run_group "release profile (shipping profile)" "--release"

echo
if [ "$fail" -ne 0 ]; then
  echo "verify-jit: GATE FAILED on $(uname -s) $arch" >&2
  exit 1
fi
echo "verify-jit: GATE PASSED on $(uname -s) $arch — $EXPECTED_PASSES tests, debug + release"
echo "verify-jit: paste these lines into the MR description (issue #2 mitigation 3)"
