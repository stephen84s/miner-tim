# REVIEW_PR24.md

## Round 3

Fresh reviewer. Scope: the round-2 fixes, `git diff d97b0b2..HEAD` (`8fcfadc`,
`8a03fa8`) on `chore/mutation-testing` at `8a03fa8`. Rounds 1-2: `git show
75e178d:REVIEW_PR24.md`.

### Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | `mutants.sh` control flow — every path executed | done |
| 2 | Anchored `EQUIVALENT` exclusion | done |
| 3 | Task board renders (whole file, GFM) | pending |
| 4 | AUDIT/CLAUDE claims re-derived | pending |
| 5 | clippy + suite | pending |
| 6 | Working tree restored | pending |

### Priority 1 — control flow, executed not read

Every path run against `8a03fa8`. Commands and observed results:

| Path | Command | Observed |
|---|---|---|
| success | `./scripts/mutants.sh hex_decode 'hex::'` | `20 mutant(s) ... after exclusions`; `20 mutants tested in 30s: 18 caught, 2 unviable`; exit 0 |
| R1/R2 repro — `-E` empties the set | `./scripts/mutants.sh 'replace \| with \^ in hex_decode' 'hex::'` | "no mutants match ... **after exclusions**", **exit 3** (round 2 saw "1 mutant(s)" then 0 tested, exit 0). Fixed. |
| `-F` matches nothing | `./scripts/mutants.sh zzz_no_such_function 'hex::'` | exit 3, same diagnostic |
| malformed `-F` | `./scripts/mutants.sh 'hex_decode(' 'hex::'` | exit 3 **and prints** cargo-mutants' `regex parse error: unclosed group` (R2-F6 fixed; previously exit 1, silent) |
| unrelated cargo-mutants failure | appended `this is not rust;` to `src/hex.rs` | exit 3 + `failed to parse src/hex.rs / expected !` |
| **test filter matches nothing** | `./scripts/mutants.sh hex_decode zzz_no_such_test` | `20 mutants tested in 31s: 18 missed, 2 unviable`, **exit 2** — red. This repo's signature defect (a filter matching nothing reported as success) does *not* go green here. |
| `MUTANTS_TIMEOUT=0.001` | as above | exit 4, `cargo test failed in an unmutated tree, so no mutants were tested` |
| 0 / 1 argument | — | usage text, exit 2 |
| args with spaces / regex metachars | `'replace \| with \^ in hex_decode'` | quoted correctly throughout; no word-splitting or globbing observed |

`wc -l` boundary: `--list` emits a trailing newline at n=1 (`od -c` confirmed),
so a single-mutant scope counts 1, not 0.

`mktemp` cleanup: `$TMPDIR` entry count identical before/after the two error
paths (385/385); all three `exit 3` paths `rm -f` both files, and by the time
the real run starts both are gone. No `trap` (0 matches), so a signal inside the
~0.3 s `--list` window leaks up to two empty files. **Nit only.**

The `LIST_STATUS=$?`-after-`wc` and `set -e` bugs round 2 hit are both genuinely
gone: status is captured with `|| LIST_STATUS=$?` on the cargo call itself, and
the list is written to a file before counting.

**Verdict on priority 1: sound.** The gate can still go red, and does, for both
of the failure modes it exists to catch.

#### R3-F1 (minor) — exit code 3 collides with cargo-mutants' own

`scripts/mutants.sh` claims 3 = "nothing would be tested / listing failed", and
2 = "real survivors". But line 53 exits **2** for a *usage* error, and the final
line passes cargo-mutants' status through unmodified. From
`cargo-mutants-27.1.0/src/exit_code.rs`: `Success=0, Usage=1, FoundProblems=2,
Timeout=3, BaselineFailed=4`. So **3 is ambiguous** — pre-flight "nothing to
test" versus "a mutant timed out" (i.e. the 300 s default was too low), which is
exactly the case the header comment's timeout discussion is about; and 2 is
ambiguous between usage error and survivors. Confirmed empirically:
`MUTANTS_TIMEOUT=0.001` exits 4, and the usage path exits 2. Low impact while
the job is advisory (CI only reads zero/non-zero), but the taxonomy as
documented is not what the script produces.

#### R3-F2 (minor) — orphaned duplicate comment block, lines 64-76

The old "A filter that matches nothing must FAIL" block stayed where it was when
its code moved down; the same text is now repeated verbatim at lines 109-119,
and 64-76 now sits immediately above the *`EQUIVALENT`* comment with no blank
line, reading as if it introduced it. This is the repo's documented "doc
comments orphaned by splicing new code under an existing one" failure mode,
landing inside the fix for the previous round's finding.

#### R3-F3 (nit) — stale line reference inside the new comment

Line 130: "`set -e` is on (line 42)". It is line **45** (`set -euo pipefail`).
Stale cross-reference introduced by the commit that added the comment.

### Priority 2 — the anchored exclusion: correct

`EQUIVALENT='^src/hex\.rs:[0-9]+:[0-9]+: replace \| with \^ in hex_decode$'`.
- `-E`/`--exclude-re` is documented as "matched against the names shown by
  `--list`", and `--list` names are exactly `file:line:col: text` (verified).
- Excludes **exactly one**: 21 mutants with `-F hex_decode`, 20 with `-E`; the
  `diff` shows the single removed line is `src/hex.rs:42:44: replace | with ^ in
  hex_decode`.
- The file anchor is load-bearing, not decorative: substituting `src/nope\.rs`
  in the same pattern excludes **nothing** (21), so the anchor really binds.
- Wildcarded line/col is a defensible trade and is argued in the comment. Noted
  limit: it does not fail closed if the *code* changes meaning while the
  description stays the same (e.g. the nibbles stop being disjoint) — the
  exclusion would keep silencing a no-longer-equivalent mutant. The anchor
  catches a move to another file, not a change in semantics at the same site.
