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
| 3 | Task board renders (whole file, GFM) | done |
| 4 | AUDIT/CLAUDE claims re-derived | done |
| 5 | clippy + suite | done |
| 6 | Working tree restored | done |

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

#### R3-F4 (minor, ACTIONABLE) — a scope whose mutants are all *unviable* still exits 0

The guard counts **listed** mutants, not mutants that were actually built and
killed. cargo-mutants exits **0** when every mutant in scope fails to compile.
Demonstrated on this head:

```
$ ./scripts/mutants.sh 'replace \+ with \* in hex_decode' 'hex::'
mutants: 2 mutant(s) matching /replace \+ with \* in hex_decode/ after exclusions, tested with 'hex::'
2 mutants tested in 9s: 2 unviable
 WARN No mutants were viable: ...
$ echo $?
0
```

(The two are `src/hex.rs:34:42` and `35:42`, `replace + with *`, taken from
`mutants.out/unviable.txt` of the real run — they are genuinely unviable, not
contrived.)

That is the same shape as R1-F11 and R2-F1 — the script reports a mutant count
and success while nothing was verified — now a third time in this PR, one layer
further out. Narrower than its predecessors, and that is why this is a minor
rather than a major: it needs **every** mutant in scope to be unviable (the CI
scope has 18 viable of 20), cargo-mutants does print a `WARN`, and the job is
advisory so it can neither block nor unblock a merge. The realistic trigger is
not a rename — that path exits 3 correctly — but a signature change that makes
the whole function's mutants fail to compile.

Actionable either way: assert on the outcome, not the listing (the run's
`mutants.out/caught.txt` / `missed.txt` are right there), or record it in
`AUDIT.md` as a known limit of the guard rather than leaving the entry's
"nothing would be tested must FAIL" claim reading as complete.

#### R3-F5 (nit) — unpinned `cargo-mutants` in CI

`cargo install cargo-mutants --locked` takes whatever is current. The script's
correctness depends on `--list` emitting exactly one `file:line:col: text` line
per mutant and on the exit-code taxonomy; a release that adds a header line to
`--list` shifts the count silently. Reviewed against 27.1.0 locally; CI may run
something else. Consistent with the repo's existing `cargo-audit` practice, so a
nit, not a finding against this PR alone.

### Priority 3 — task board: fixed, verified on the whole file

Rendered `git show <rev>:CLAUDE.md` in full through `POST /markdown`, `mode:
gfm`, three revisions:

| Revision | `<tr>` | `<table>` | `<th>` | `<td>` |
|---|---|---|---|---|
| `main` | 44 | 3 | 11 | 117 |
| `77246b39` (the merge round 2 faulted) | **42** | — | — | — |
| `HEAD` (`9512b11`, board identical to `8a03fa8`) | **45** | 3 | 11 | 120 |

Delta `main` → `HEAD` is **exactly +1 row and +3 cells** — one new three-column
row, not a re-flow. Extracting the task IDs from the rendered cells gives 27 on
`main` and 28 on `HEAD`, `diff` showing the single addition `PROC-06`. The other
two tables (Platform coverage, Versions) still render as tables — `<table>` and
`<th>` counts are unchanged. No `<p>| **Completed**` paragraphs anywhere.

The round-2 account also checks out at its details: three blank lines inside the
task board in the merge tree, **zero** in either parent (`99854a9b`,
`e07a9a20`) and **zero** at `HEAD`; and `77246b39` is indeed a merge
(`Merge remote-tracking branch 'origin/main' into chore/mutation-testing`).
42 vs 44 is confirmed by rendering, not asserted.

### Priority 4 — the record

| Claim | Re-derived | Result |
|---|---|---|
| debug lib suite ~190 s (188.0, 189.6, 191.9, 192.6) | `cargo test --lib`, warm build, twice, `caffeinate -i` | **188.72 s and 189.23 s** — inside the quoted range. Reproduces. |
| `hex_decode 'hex::'` → 20 mutants, 18 caught, 2 unviable, 31 s | re-run | **exact**: "20 mutants tested in 30s: 18 caught, 2 unviable", wall 31.4 s |
| `'DonationSchedule::level' 'donate::'` → 2 mutants, 2 caught, 11 s | re-run | 2 mutants, 2 caught, cargo-mutants "10s", wall **10.9 s** → 11 s. Reproduces. (CLAUDE.md's FIX-01 says 12 s for the same command; that is a second run, not a second figure for one measurement, so it is not R2-F5 recurring — but after an entry that argues for quoting a range, two single-run seconds in two files is an odd note to end on.) |
| R2-F7: on `fc35c12` the advisory job reads `conclusion=failure` while the five required contexts gate | `gh api .../commits/fc35c12/check-runs` and `.../branches/main/protection` | **Confirmed.** `mutation testing (advisory, hex::) \| completed \| failure`. Required contexts are exactly the five job names; the mutants job is not among them. `strict: true`, `enforce_admins: true`. |
| "break-tested by `#[ignore]`-ing three `hex.rs` tests → 4 missed" | attempted | **Not reproducible as written** — the three tests are not named. Ignoring `round_trips`, `decodes_either_case_and_encodes_lower` and `every_byte_round_trips` gives **13 missed, 5 caught, 2 unviable, exit 2**. The substance (real survivors ⇒ exit 2) holds; the specific number cannot be checked. Nit. |
| merge "lost nothing", damage purely additive | blank-line counts across both parents and the merge; diff of the named files | consistent with what I checked |

### Priority 5 — build and suite

- `cargo clippy --all-targets --release -- -D warnings` → **exit 0**, no warnings.
- `cargo test --lib` (debug) → **150 passed, 0 failed, 2 ignored**, twice.
- `cargo test --bins` → **18 passed, 0 failed**.
- `mutants.out/` and `mutants.out.old/` confirmed ignored (`git check-ignore -v`
  names `.gitignore:48` and `:49`), and both exist after my runs without
  dirtying the tree.

### Working tree

`src/hex.rs` mutated twice (a syntax error, then three `#[ignore]`s) and
restored from a `cp` each time; `cmp` against the pre-mutation copies passes.
`scripts/mutants.sh` `cmp`-identical to its committed form. `git status
--porcelain` is empty apart from this ledger.

### Not verified

- The `mutants` job has never been observed on a **fresh** `ubuntu-24.04`
  runner in this form: `cargo install cargo-mutants` from source plus the run,
  inside `timeout-minutes: 20`. `fc35c12` shows it completing and failing, so it
  fits there, but the install is unpinned and the margin is unmeasured.
- Behaviour under a cargo-mutants version other than 27.1.0.
- Signal-handling cleanup (no `trap`); reasoned, not raced.

### Verdict — MERGEABLE

No blockers, no majors. Round 2's major (R2-F1) is genuinely closed and its
reproducer now exits 3; the two bugs the author hit inside that fix are both
gone, and I could not make the script report success while testing nothing along
any path I exercised except R3-F4. The anchored exclusion is exactly right and
its anchor is load-bearing. The task board renders +1 row and nothing else.
Every quoted number reproduced.

**ACTIONABLE: R3-F4 only** — either assert on the outcome rather than the
listing, or record the all-unviable case in `AUDIT.md` as a known limit, so the
entry does not read as if the silent-green class were fully closed. R3-F1 to
R3-F3 and R3-F5 are tidying: the exit-code taxonomy collides with
cargo-mutants' own (2 = usage *and* survivors; 3 = "nothing to test" *and*
Timeout), lines 64-76 are an orphaned duplicate of 109-119, and line 130 cites
"line 42" for a `set -e` that is on line 45.

### Addendum — record checks the first pass left open

- **PR body (#24) is current, not stale.** `gh pr view 24` shows it updated
  2026-09-19T08:11Z and carrying the full round-2 account: R2-F1 and its
  reproducer, the two silences found while fixing it, the task-board damage with
  the 42-vs-44 figure, the anchored `EQUIVALENT`, the ~190 s reconciliation and
  R2-F8. "The PR body was never updated" — found in three previous round 2s —
  does **not** recur here.
- **"The merge lost nothing" is now derived, not asserted.**
  `git diff 99854a9b..77246b39 | grep -c '^-[^-]'` → **0**; same against
  `e07a9a20` → **0**. No deletions in either direction; the damage was purely
  additive, as claimed.
- **R2-F7's framing holds at the detail level too.** On `fc35c12` the job failed
  on a *real* survivor, not a timeout or an install error: the step log reads
  `21 mutants tested in 46s: 1 missed, 18 caught, 2 unviable` with the missed one
  being `src/hex.rs:42:44: replace | with ^`, and `Process completed with exit
  code 2`.
- **`timeout-minutes: 20` has margin** — closing one of my "not verified" items.
  That job ran 10:18:42 → 10:20:47, **2 min 05 s**, of which `cargo install
  cargo-mutants` was 67 s and the gate itself 47 s. Roughly a 10x margin, on an
  unpinned install.
- **R3-F6 (nit).** The cost table quoted in the PR body, `CLAUDE.md`'s PROC-06
  row, the `AUDIT.md` entry and `mutants.sh`'s header all say the scoped run is
  **21 mutants / 33 s**. That count predates `EQUIVALENT`: no invocation of the
  current script can produce 21, and the documented command now gives **20 in
  31 s** here and 46 s on `ubuntu-24.04`. The figures are internally consistent
  with each other and with the pre-exclusion state, so this is a stale
  measurement rather than a contradiction — but R2-F4 was closed as "the script
  now prints 20 where it printed 21", and four other places still print 21.
- **R3-F1's consequence, stated concretely.** cargo-mutants' `Timeout = 3` means
  **a mutant that survives by hanging** — a genuine uncaught mutant, and the
  worst kind — exits with the same code the script documents as "your filter
  matched nothing". An operator following the documentation goes looking for a
  rename instead of a survivor. Severity unchanged (minor); the misdiagnosis is
  the point.
- **Why R3-F4 is worth recording even though it stays minor.** The advisory
  status makes it harmless at the merge gate, but `CLAUDE.md` now points the
  **author** at this same script for local break-testing, and there
  cargo-mutants' `WARN No mutants were viable` on stdout is the only signal —
  no red check behind it, and the script's own line above it says a mutant was
  tested.

Verdict unchanged: **MERGEABLE**, no blockers, no majors, **ACTIONABLE: R3-F4**
(and, trivially, R3-F6 if the 21/33 s figures are to match what the script now
does).
