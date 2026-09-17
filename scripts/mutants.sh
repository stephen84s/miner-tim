#!/usr/bin/env bash
#
# Mutation testing, scoped so it is fast enough to actually run.
#
# WHY THIS EXISTS
#
# This project's recurring defect is not broken code — it is tests that look
# like they cover something and do not. Reviewers caught eight instances; in one
# case it took three attempts to write a test that failed when the defect was
# reintroduced, and the first two passed against the bug while being described
# in the audit log as "break-tested".
#
# The cause is always the same: break-testing by hand means *choosing* a
# mutation, and a mutation the test catches for an unrelated reason proves
# nothing. `cargo-mutants` does not choose — it tries them all.
#
# COST, MEASURED
#
# Scope is everything. Unscoped, one function took 28 minutes for 8 mutants,
# because every mutant re-ran the whole 48-second suite. Scoping the *tests* as
# well as the mutants took 21 mutants to 33 seconds. Always pass both.
#
#   ./scripts/mutants.sh hex_decode 'hex::'
#   ./scripts/mutants.sh take_complete_lines 'pool_connection::'
#
# READING THE RESULT
#
# A MISSED mutant is one your tests did not kill. That is usually a real gap —
# but not always. Some mutants are *equivalent*: the mutated program behaves
# identically, so no test can kill it. The first run of this script found
# `replace | with ^` in `hex_decode`, which is equivalent, because the high
# nibble occupies bits 4-7 and the low nibble bits 0-3 — disjoint, so OR and XOR
# agree on all 256 inputs.
#
# So a survivor is a question, not a verdict. Work out whether the mutant is
# genuinely equivalent before writing a test to chase it, and if it is, say so
# in the audit entry rather than contorting a test around it.
set -euo pipefail

if [ $# -lt 2 ]; then
    echo "usage: $0 <function-regex> <test-filter>" >&2
    echo "  e.g. $0 hex_decode 'hex::'" >&2
    echo >&2
    echo "Both arguments matter. Omitting the test filter makes every mutant" >&2
    echo "run the full suite, which measured 28 minutes for 8 mutants." >&2
    exit 2
fi

FN="$1"
TESTS="$2"

command -v cargo-mutants >/dev/null || {
    echo "cargo-mutants is not installed: cargo install cargo-mutants --locked" >&2
    exit 127
}

echo "mutants: functions matching /$FN/, tested with '$TESTS'"
cargo mutants -F "$FN" --timeout 120 -- --lib "$TESTS"
