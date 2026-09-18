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
#   ./scripts/mutants.sh 'DonationSchedule::level' 'donate::'
#
# Note cargo-mutants builds and runs in DEBUG, where this tree's lib suite takes
# ~192 s — not the 48 s release figure. The default timeout here is 300 s for
# that reason; override with MUTANTS_TIMEOUT if a scope needs longer.
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

# A filter that matches nothing must FAIL, not pass quietly.
#
# `cargo-mutants` prints "No mutants found under the active filters" and exits
# **0**. That is the repo's signature defect — a check that reports success
# having verified nothing — shipping inside the fix for it. `verify-jit.sh`, in
# this same directory, asserts an exact test count for precisely this reason;
# this script did not, and review found its own second documented example
# (`take_complete_lines`, which lives on another branch) returned 0 mutants and
# exited 0.
#
# The realistic trigger is a rename: the CI job names one function, someone
# renames it, and the job flips from red to green. The break would read as an
# improvement.
LIST=$(cargo mutants -F "$FN" --list 2>/dev/null | wc -l | tr -d ' ')
if [ "$LIST" -eq 0 ]; then
    echo "error: no mutants match /$FN/ — nothing would be tested." >&2
    echo "  A filter matching nothing exits 0 in cargo-mutants, so this would" >&2
    echo "  otherwise report success having checked nothing. Has the function" >&2
    echo "  been renamed, or is it on a different branch?" >&2
    exit 3
fi

echo "mutants: $LIST mutant(s) matching /$FN/, tested with '$TESTS'"
# Known-equivalent mutants are excluded by regex, with the reason recorded here.
# Without this the run exits 2 on a mutant nothing can ever kill, so the CI job
# is red on day one and red forever — and a red check that never changes carries
# no information, which is the very failure this PR argues against while shipping
# it. A *new* survivor should be the thing that turns it red.
#
# Each entry needs a justification, not just a silenced line:
#
#   replace | with ^ in hex_decode
#     `(hi << 4) | lo` and `(hi << 4) ^ lo` agree on all 256 nibble pairs,
#     because the high nibble occupies bits 4-7 and the low nibble bits 0-3 —
#     disjoint, so no test can distinguish them. Verified exhaustively.
EQUIVALENT='replace \| with \^ in hex_decode'

cargo mutants -F "$FN" -E "$EQUIVALENT" --timeout "${MUTANTS_TIMEOUT:-300}" -- --lib "$TESTS"
