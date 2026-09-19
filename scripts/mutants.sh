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
# **~190 s** — not the 48 s release figure. Four measurements: 188.0, 189.6,
# 191.9, 192.6 s. Quote the range, not a single run: an earlier version of this
# comment said 192 s while the AUDIT entry said 188 s, two figures for one
# quantity inside one change (R2-F5). The default timeout here is 300 s for
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
# Known-equivalent mutants are excluded by regex, with the reason recorded here.
# Without this the run exits 2 on a mutant nothing can ever kill, so the CI job
# is red on day one and red forever — and a red check that never changes carries
# no information, which is the very failure this PR argues against while shipping
# it. A *new* survivor should be the thing that turns it red.
#
# Each entry needs a justification, not just a silenced line:
#
#   src/hex.rs — replace | with ^ in hex_decode
#     `(hi << 4) | lo` and `(hi << 4) ^ lo` agree on all 256 nibble pairs,
#     because the high nibble occupies bits 4-7 and the low nibble bits 0-3 —
#     disjoint, so no test can distinguish them. Verified exhaustively.
#
# The pattern is **anchored on the file and the whole description**, not a bare
# substring. `-E` matches against cargo-mutants' full "file:line:col: text"
# line, so an unanchored 'replace \| with \^ in hex_decode' would also silence
# that mutation appearing in any other file, or in any function whose name
# merely contains `hex_decode`, on every invocation of this script. The line
# and column stay wildcards so ordinary edits above it do not turn the job red
# for no reason; the file and the exact operator do not (R2-F3).
EQUIVALENT='^src/hex\.rs:[0-9]+:[0-9]+: replace \| with \^ in hex_decode$'

# A filter that leaves nothing to test must FAIL, not pass quietly.
#
# Count with **the same -F and -E the real run uses**. Counting with -F alone
# was this script's own version of the defect it exists to catch: the exclusion
# could remove the last surviving mutant, the run would test zero and exit 0,
# and the line printed below would claim a mutant had been tested. Round 2
# reproduced exactly that — `./scripts/mutants.sh 'replace \| with \^ in
# hex_decode' 'hex::'` announced "1 mutant(s)" and then tested none, exit 0
# (R2-F1).
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
#
# stderr is captured rather than discarded: a bad -F regex or a broken build
# used to exit 1 printing nothing at all, which tells the operator less than
# the failure it is reporting (R2-F6).
# The list goes to a file before it is counted. Piping straight into `wc -l`
# would put cargo-mutants' exit status behind `wc`'s, which is always 0 — so a
# failed list would have counted as an empty one and been reported as the wrong
# error (or, in the bad-regex case, as nothing at all).
LIST_ERR=$(mktemp)
LIST_OUT=$(mktemp)
# `|| LIST_STATUS=$?` rather than a bare call: `set -e` is on (line 42), so a
# non-zero exit would otherwise kill the script here and the diagnostic below
# would never run — which is what made a bad -F regex exit 1 silently.
LIST_STATUS=0
cargo mutants -F "$FN" -E "$EQUIVALENT" --list >"$LIST_OUT" 2>"$LIST_ERR" || LIST_STATUS=$?
LIST=$(wc -l <"$LIST_OUT" | tr -d ' ')
rm -f "$LIST_OUT"
if [ "$LIST_STATUS" -ne 0 ]; then
    echo "error: could not list mutants (cargo-mutants exited $LIST_STATUS):" >&2
    sed 's/^/  /' "$LIST_ERR" >&2
    rm -f "$LIST_ERR"
    exit 3
fi
if [ "$LIST" -eq 0 ]; then
    echo "error: no mutants match /$FN/ after exclusions — nothing would be tested." >&2
    echo "  A filter matching nothing exits 0 in cargo-mutants, so this would" >&2
    echo "  otherwise report success having checked nothing. Has the function" >&2
    echo "  been renamed, is it on a different branch, or did EQUIVALENT below" >&2
    echo "  swallow the last one?" >&2
    sed 's/^/  /' "$LIST_ERR" >&2
    rm -f "$LIST_ERR"
    exit 3
fi
rm -f "$LIST_ERR"

echo "mutants: $LIST mutant(s) matching /$FN/ after exclusions, tested with '$TESTS'"

cargo mutants -F "$FN" -E "$EQUIVALENT" --timeout "${MUTANTS_TIMEOUT:-300}" -- --lib "$TESTS"
