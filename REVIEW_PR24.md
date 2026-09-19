# REVIEW_PR24 — round 1, independent

PR #24 "Adopt mutation testing; move break-testing into the author's protocol"
Base `main` (`f2abc1e`), head `e358581`. Reviewer spawned cold.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Advisory job cannot block a merge | DONE |
| 2 | `scripts/mutants.sh` works as documented | DONE |
| 3 | Cost claim honest / like-for-like | DONE |
| 4 | Equivalent-mutant claim + the reasoning from it | DONE |
| 5 | Rule text — behaviour change or intention | DONE |
| 6 | AUDIT PROC-06 / CLAUDE.md row vs the diff | DONE |
| 7 | Nothing else broken (`verify-jit.sh`) | DONE |
| 8 | clippy + test suite | DONE |

---

## Findings

### F1 (MAJOR) — 102 files / 1.5 MB of cargo-mutants scratch output committed

`git diff origin/main...HEAD --stat` is **107 files, 15,816 insertions**. Of
those, **102 files are tool output**: `mutants.out/` (788 KB) and
`mutants.out.old/` (788 KB) — the second being nothing but the previous run,
which `cargo-mutants` rotates automatically. They contain `outcomes.json`
(1,425 lines), per-mutant build logs, and a byte-level diff per mutant.

`.gitignore` has **no `mutants.out` entry**, so this is not a one-off slip: it
recurs on every future run.

This is LEDGER-01 repeating four days later. That entry removed 530 KB of
`REVIEW_*.md` from the tree, on the grounds that it was "larger than the entire
Rust source (504 KB)". This PR adds 1.5 MB of a strictly less durable artifact,
and the entry that records the change does not mention it.

**Operational consequence.** `scripts/mutants.sh` offers no `--output` control
and `cargo mutants` defaults to writing `mutants.out/` in the working directory.
So the tool the PR asks the author to *prefer over judgement* dirties two
tracked paths every single time it is run, and rotates the older one. Any author
following the new `CLAUDE.md` rule ends with a dirty tree and a spurious 800 KB
diff to discard.

### F2 (MAJOR) — the "Files changed" list in `AUDIT.md` PROC-06 is false

PROC-06 states:

> **Files changed:** `CLAUDE.md`, `.claude/agents/_shared-context.md`,
> `scripts/mutants.sh`, `.github/workflows/ci.yml`, `AUDIT.md` (this entry).

Five files. The diff touches **107**. The `CLAUDE.md` task-board row omits them
too, as does the PR body.

This is a false claim in the permanent, append-only ledger, in the entry whose
entire subject is claims recorded as true at the moment they were false. Per
`CLAUDE.md` step 0 an entry on an unmerged branch may still be edited in place,
so this is correctable without an appended note.

It also survived a deliberate accuracy pass. Both artifact directories landed in
`fc35c12` (107 files, 15,808 insertions). The branch's second commit, `e358581`
"docs: name the tree the mutant count was measured in", exists *only* to correct
a figure in this very entry — and touched `AUDIT.md` and `CLAUDE.md` alone. So
the author re-read the entry for accuracy and the 1.5 MB of scratch beside it
was invisible.

### F3 (MAJOR) — the advisory job is red today and is red forever, and its
redness carries no information

Live evidence on head `e358581`:

* check run `mutation testing (advisory, hex::)` — **conclusion `failure`**
* workflow run `35334408664` (CI) — **conclusion `success`**
* job log: `21 mutants tested in 44s: 1 missed, 18 caught, 2 unviable`
  then `##[error]Process completed with exit code 2.`

`cargo-mutants` exits 2 when a mutant survives, so the script exits 2, so the
job fails. The survivor is the `|`→`^` mutant, which the PR itself proves is
**equivalent and therefore unkillable by any test** (F4). The job is therefore
red on this PR and will be red on every future PR until the scope changes.

The PR's own argument against making it required is:

> A gate that fails on equivalent mutants is a check people learn to ignore

— and it then ships exactly that, one severity down. `continue-on-error: true`
keeps the *workflow* green, but the *check run* is red, so the PR page shows
"Some checks were not successful" on every PR from here on.

The load-bearing consequence: **a permanently-red check cannot report anything.**
Nobody can distinguish "a new survivor appeared in a future change" from "the
usual `|`/`^`" without opening the log. The stated purpose of the job — to make
a missing break-test visible — cannot be discharged by a signal that is
constant.

**The job's scope also excludes every defect it was created for.** All three
motivating examples are in `pool_connection.rs` / TLS-verifier territory — the
all-zeros fingerprint, the flooding socket the test server dropped, the
third-reconnect assertion. The job mutates `hex_decode` and nothing else, so it
could not in principle have caught any of them. Combined with the constant
redness, the job as configured reports nothing about anything it was built for.

Nothing in the PR body, `AUDIT.md` PROC-06, or the `ci.yml` comment says the job
is red. PROC-06's Verification paragraph says only "reports 18 caught / 1 missed
/ 2 unviable", omitting the exit status and the resulting check conclusion.

`advisory vs required` is presented as the only axis. It is not:
`cargo-mutants` supports suppressing a known-equivalent mutant, after which a
red job would mean something. That option is not discussed.

### F4 (verified, no defect) — the equivalent-mutant claim is correct

Checked exhaustively and independently, not by reading the PR:

* all 256 `(hi, lo)` nibble pairs give `(hi<<4)|lo == (hi<<4)^lo` — 0
  disagreements;
* the premise holds because `nibble()` can only return `0..=15` (`b'9'-b'0'=9`,
  `b'f'-b'a'+10=15`), so the two operands never share a set bit.

The claim is sound. What follows from it is F3.

### F7 (verified, no defect) — `scripts/verify-jit.sh` untouched

`git diff origin/main...HEAD -- scripts/verify-jit.sh` is empty;
`EXPECTED_PASSES=92` present at line 65 and asserted at line 110. The five
required checks were all green on head `e358581`, including both `jit-*` jobs.

### Priority 1 — the advisory job cannot block a merge (confirmed)

* Live branch protection required contexts are exactly the five:
  `lint (clippy, x86_64 linux)`, `audit (cargo-audit / RustSec)`,
  `test (cargo test --release, x86_64 linux)`,
  `jit-macos (aarch64 darwin, make verify-jit)`,
  `jit-linux-arm (aarch64 linux, scripts/verify-jit.sh)`.
  `strict: true`, `enforce_admins: true`. The new job's name,
  `mutation testing (advisory, hex::)`, is **not** among them.
* `continue-on-error: true` is set at job level (ci.yml:290) — empirically the
  workflow run concluded `success` with the job red.
* **Nothing shared.** The `mutants` job has no `actions/cache` step at all, so
  no key can collide with `target-lint`/`target-test`/`cargo-*`. No `needs:`.
  It reads workflow-level `env` and the workflow-level `concurrency` group
  read-only; adding a job to the group changes nothing for the other five.
* CI-03 respected: no new trigger — `pull_request` + `workflow_dispatch` only.

### F5 (MINOR) — "a permanent fix" overstates what was built

*(The substantive half of this finding — that the job's scope excludes every
defect it cites — is folded into F3, where it belongs. What is left here is a
claim-accuracy problem of the same class as F12.)*

The PR opens "A permanent fix for the defect this session produced repeatedly";
PROC-06 says "Fixed three ways". Inventory what actually lands:

| Part | What it is | Enforces anything? |
|---|---|---|
| `CLAUDE.md` bullet | prose | no |
| `_shared-context.md` sharpening | prose | no |
| `scripts/mutants.sh` | opt-in, run only by an author who remembers | no |
| CI job | runs `hex::` only | see below |

**The advisory job's scope excludes all three named motivating defects.** Every
one is in `pool_connection.rs` / TLS-verifier territory — the all-zeros
fingerprint (verifier wiring), the flooding socket the test server dropped, the
third-reconnect assertion. The job mutates `hex_decode` and nothing else. It
could not, even in principle, have caught any of them.

So the new mechanism is: two prose rules, plus a script nobody is obliged to
run, plus a check that is permanently red on one 30-line pure function. Issue
**#19** — open, filed by this same repo — names exactly this pattern:

> The new rule describes an intention with nothing enforcing it — **the
> recurrence being fixed came from a rule followed exactly as written.**

The PR's defence is that the tool is the enforcement and the prose only the
pointer. That does not hold on this configuration: nothing makes a missing
break-test visible. Declaring the defect permanently fixed in the append-only
ledger is the same false-completion pattern the entry is about.

The prose itself is good — `CLAUDE.md`'s new bullet is specific, names the
failure mode, and quotes three real examples. As documentation it is an
improvement. As a *fix* it is the thing #19 says does not work, and the entry
should say so rather than claim closure.

### F6 (MINOR) — script exit 2 collides with cargo-mutants' own exit 2

`scripts/mutants.sh` exits 2 for a usage error, and `cargo-mutants` exits 2 for
"mutants survived" (measured: `./scripts/mutants.sh hex_decode 'hex::'` exits
**2**). A typo'd argument and a genuine survivor are indistinguishable by exit
code. Harmless while advisory; not harmless if the job is ever promoted, which
the entry explicitly leaves open. `$# -lt 2` also silently ignores extra
arguments (`./scripts/mutants.sh --help` prints usage, which is fine by
accident).

### F8 (MINOR) — `cargo install cargo-mutants --locked` is unpinned

ci.yml:299. It resolved to 27.1.0 and rebuilt from source in 1m06s with no
cache. A future release can change the generated mutant set, or raise its MSRV
above the deliberately pinned `RUST_VERSION: 1.97.1`. The ci.yml comment calls
the job "a standing demonstration that the tooling works"; an unpinned installer
undercuts that, and every other toolchain input in this workflow is pinned on
purpose. `timeout-minutes: 20` against a ~2 min observed run is ample headroom.

### F9 (MINOR) — no `make mutants` target

Every other verification tool in the repo is reachable from the `Makefile`
(`verify-jit`, `verify-jit-linux`, `audit`, `test`, `bench`). `mutants.sh` is
not, and is absent from `README.md`. Its only pointer is the `CLAUDE.md` prose
the PR argues is insufficient on its own.

### F10 (NIT) — 3,196 is an aarch64 figure quoted without its platform

Reproduced exactly on this Mac (`arm64`): **3,196** total, `randomx/vm.rs`
**855**, `randomx/jit/aarch64.rs` **710**, `src/pool_connection.rs` **88**. All
four quoted figures check out, including the corrected 88.

Caveat the entry does not give: 967 of those 3,196 (30%) are in `jit/aarch64.rs`
+ `jit/compiler.rs`, which are `cfg`'d out on x86_64 — so the `ubuntu-24.04`
runner could never test them even if the job were widened crate-wide. The scale
figure is right; what it would cost to act on it is platform-split.

### F11 (MAJOR, most actionable) — the advisory gate has a silent-green mode, and
the script's own second documented example already hits it

`scripts/mutants.sh` asserts nothing about how many mutants it found.
**Break-tested, twice:**

```
$ ./scripts/mutants.sh hex_decodeX 'hex::'        # simulates a rename
mutants: functions matching /hex_decodeX/, tested with 'hex::'
Found 0 mutants to test
 WARN No mutants found under the active filters
EXIT=0                                            # <-- green
```

A function regex that matches nothing exits **0**. In CI that is a green job
that checked nothing — the exact defect `_shared-context.md` lists
("A test's `#[ignore]`/filter matched nothing, so libtest reported success") and
the exact reason `scripts/verify-jit.sh`, in this same directory, carries
`EXPECTED_PASSES=92` and fails on an unexpected count. The new script was
written without that lesson applied.

This is not hypothetical. **The script's own second documented example is
already in that state on this branch:**

```
$ ./scripts/mutants.sh take_complete_lines 'pool_connection::'
Found 0 mutants to test
 WARN No mutants found under the active filters
real 0.39
```

`take_complete_lines` does not exist in `src/pool_connection.rs` on `main` or on
this branch (`grep -c` → 0); it is on the in-flight `security/bound-recv-buffer`
branch. So the header's worked example, and the 28-minute row of the cost table,
both refer to a function that is not in this tree — and running the example as
written reports success in 0.39 s having tested nothing.

Interaction with F3 that makes it worse. The CI job hardcodes `hex_decode`, so
the realistic trigger is not a typo in the workflow but a **rename or move of
the function** — and `src/hex.rs` is 60 lines that were rewritten from scratch
this same session. The job is red *today* because of the equivalent mutant; the
moment `hex_decode` is renamed it flips to **green**. A break would read as an
improvement.

The other direction fails loudly, which is correct: a test filter matching
nothing (`'hexXX::'`) gives `19 missed, 2 unviable` and exit 2.

### F12 (MAJOR) — "The full lib suite is 48 s" does not reproduce; it is a
release figure explaining a debug cost

`cargo-mutants` builds and tests in the **debug** profile. Measured on this Mac,
in this worktree:

```
$ cargo test --lib
test result: ok. 149 passed; 0 failed; 2 ignored ... finished in 191.87s
real 192.40
```

**192 s, not 48 s** — 4× out. 48 s matches MEM-01's *release* `--lib` figure
(94 s → 50 s), a different profile from the one the tool uses.

The consequence for the headline table: the entry explains 28 min for 8 mutants
as "every mutant pays the 48-second suite", which is 8 × 48 s = **6.4 min** —
it accounts for under a quarter of the number it is offered to explain. At the
measured 192 s it is 8 × 192 s ≈ 25.6 min, which does reconcile with 28 min once
builds are added.

So the *conclusion* — scope dominates, pass both arguments — survives and is
independently supported by my measurement. The stated mechanism is wrong by 4×,
and quoted from the wrong profile. Same shape as the retracted "~8 minutes"
`jit-macos` claim and the 2.7× RSS overstatement.

Two further caveats on the table, which is presented as though scope were the
only variable:

* the rows are **different functions in different modules** (8 mutants in
  `pool_connection.rs` vs 21 in `hex.rs`), so it is not a controlled comparison;
* `-- --lib <filter>` narrows the **build** as well as the tests (no bins, no
  `benches/`, which here is criterion plus the A/B harness), so part of the win
  is build scope, not test scope.

**The scoped figure does reproduce exactly:** `./scripts/mutants.sh hex_decode
'hex::'` → `21 mutants tested in 32s: 1 missed, 18 caught, 2 unviable`,
`real 33.29`. 33 s confirmed.

### F13 (MINOR) — the hardcoded `--timeout 120` is below the tree's own baseline

`mutants.sh` hardcodes `--timeout 120`, undocumented in its header. Measured:
an unscoped `cargo mutants -F hex_decode --timeout 120` aborts with

```
TIMEOUT  Unmutated baseline in 10s build + 120s test
ERROR cargo test failed in an unmutated tree, so no mutants were tested
```

because the debug suite needs 192 s. The entry anticipates widening scope to
`miner.rs` or `vm.rs`, "modules with slow tests" — at which point this constant
aborts the run rather than scaling. It fails loudly, which is the right
direction, but the value is already wrong for this repo's baseline.


---

## Priority 2 — does `scripts/mutants.sh` work as documented?

Mode `100755`, trailing newline present, `set -euo pipefail` set before any
logic. Argument handling verified by running it:

| Invocation | Result |
|---|---|
| no args | usage to stderr, exit **2** |
| one arg | usage to stderr, exit **2** |
| `PATH` without `cargo-mutants` | clear message, exit **127** |
| `hex_decode 'hex::'` | 21 mutants / 32 s, exit **2** (survivor) |
| `hex_decodeX 'hex::'` | **0 mutants, exit 0** — F11 |
| `take_complete_lines 'pool_connection::'` (its own example) | **0 mutants, exit 0** — F11 |

Quoting is correct: `"$FN"` and `"$TESTS"` are both quoted, so a regex with
spaces survives. `command -v ... || { ...; exit 127; }` is not defeated by
`set -e`. The generated invocation
`cargo mutants -F "$FN" --timeout 120 -- --lib "$TESTS"` is valid — `--lib` and
the positional filter reach `cargo test` correctly, proven by 18 caught mutants.

## Priority 8 — clippy and tests, run here

```
cargo clippy --all-targets --release -- -D warnings   # clean
cargo test --release --locked                          # 149 lib + 18 bin, 0 failed, 2 ignored
```

No `src/` file is touched by this PR, so this is a regression check only. (Side
note for F12: release `--lib` took **101.5 s** on this machine, so 48 s does not
reproduce in either profile here.)

## Workflow structure (priority 1, mechanical)

`ci.yml` parses. `jobs` = `lint, audit, test, mutants`. Triggers unchanged:
`pull_request` + `workflow_dispatch` (CI-03 respected). The `mutants` job has no
`needs:`, no step-level `if:`, and **no `actions/cache` step at all** — so no
cache key can collide with `target-lint` / `target-test` / `cargo-*`. It reads
workflow-level `env` and `concurrency` read-only. It cannot affect the other
five.

Three independent proofs it cannot block a merge:
1. absent from the five required contexts on the live protection API;
2. `continue-on-error: true` — the CI workflow run concluded **success** with
   the job red;
3. `gh api .../pulls/24` → `mergeable: true, mergeable_state: "unstable"` —
   "unstable" is the state meaning a non-required check is red and the PR is
   still mergeable. Empirical, not inferred.

## What I could not verify

* The **28-minute** figure was not reproduced directly (F12 reconciles it
  arithmetically from the measured 192 s debug suite, which is indirect).
* Behaviour of `cargo install cargo-mutants --locked` against a future release.
* Whether `cargo mutants --list` would enumerate the 967 JIT mutants on x86_64;
  the 3,196 figure was reproduced on aarch64 only.

## Verdict

**NOT MERGEABLE.** No blockers — nothing here can produce a wrong hash, and the
five required checks are green and untouched. But five majors, and two of them
reproduce the exact defect class this PR was written to eliminate.

| | |
|---|---|
| **Blockers** | none |
| **Majors** | F1 (102 files / 1.5 MB of tool scratch committed, `.gitignore` not updated), F2 (PROC-06 "Files changed" names 5, diff touches 107, and survived a correction commit), F3 (advisory job permanently red, redness carries no information, and its scope excludes all three defects it cites), F11 (silent-green mode; the script's own example already hits it), F12 ("48 s" is a release figure, 4× out) |
| **Minors** | F5 ("permanent fix" overstates it), F6 (exit-code collision), F8 (unpinned installer), F9 (no `make mutants`), F13 (`--timeout 120` below the tree's baseline) |
| **Nits** | F10 (3,196 is aarch64-only) |
| **Verified sound** | F4 (equivalent-mutant claim, checked exhaustively), F7 (`verify-jit.sh` untouched, `EXPECTED_PASSES=92` intact), all four mutant counts (3196/855/710/88), the 33 s scoped figure, priority-1 non-blocking |

**ACTIONABLE — yes.** In order:

1. **F11** — assert the mutant **count** in `scripts/mutants.sh` (a minimum, or
   exact as `verify-jit.sh` does with `EXPECTED_PASSES=92`); a count is the only
   thing that distinguishes "found nothing" from "found everything and killed
   it". Fix or remove the `take_complete_lines` example, which finds nothing on
   this branch. This is the
   repo's signature failure mode shipping inside the fix for it.
2. **F1 / F2** — `git rm -r mutants.out mutants.out.old`, add `mutants.out*` to
   `.gitignore`, and correct PROC-06's "Files changed". The entry is still on an
   unmerged branch, so it may be edited in place.
3. **F12** — replace 48 s with the measured debug figure, or drop the mechanism
   sentence; label the table's two rows as different functions.
4. **F3** — either suppress the known-equivalent mutant (`.cargo/mutants.toml`
   `exclude_re`, or `-E`) so a red job means something, or state plainly in
   `ci.yml`, PROC-06 and the PR body that the job is red now and stays red.
5. **F5 / F3** — soften "permanent fix" to what was actually built, and state
   that the job's scope excludes all three cited defects. Relates to open
   issue #19.

Reviewer note: the tree was dirtied twice by running `mutants.sh` as documented
and restored both times with `git checkout -- mutants.out mutants.out.old &&
git clean -fd` those two paths; `git status` clean apart from this ledger. No
production file was mutated — the break tests in F11 used filter arguments, not
source edits.

---

# Round 2 (fresh reviewer, cold; head `d97b0b2`)

Scope: the two fix commits `99854a9`, `d97b0b2` plus the merge `77246b3`.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Can the gate still go red? | done — break-tested, exit 2 |
| 2 | Silent green (R1 F11) closed? | **done — NOT fully closed, R2-F1** |
| 3 | EQUIVALENT exclusion narrow? | done — R2-F3 |
| 4 | `mutants.out/` untracked+ignored | done — clean |
| 5 | Count claims (3196 / 88 / 95) | done — all reproduce |
| 6 | `MUTANTS_TIMEOUT` default | done — R2-F5 |
| 7 | Merge vs rebase, nothing lost | **done — R2-F2** |
| 8 | Required contexts / advisory job | done — 5 required, mutants not among them |
| 9 | clippy, six checks | done |

## Findings

### R2-F1 (MAJOR) — the silent-green fix is one step short: the guard counts mutants *before* the exclusion, so an all-excluded scope still reports success having tested nothing

`scripts/mutants.sh:74` counts with `-F` only; the run at line 98 adds
`-E "$EQUIVALENT"`. Nothing asserts that anything survived the exclusion.

Reproduced on this head:

```
$ ./scripts/mutants.sh 'replace \| with \^ in hex_decode' 'hex::'
mutants: 1 mutant(s) matching /replace \| with \^ in hex_decode/, tested with 'hex::'
Found 0 mutants to test
 WARN No mutants found under the active filters
EXIT=0
```

Exit 0, zero mutants tested, and the script's own line *claims* one was. This is
R1-F11 surviving in its fix, in the narrower form the fix itself created: the
guard was added for the empty-`-F` case and the exclusion opens a second door to
the same room. It is also the repo's signature defect (a check reporting success
having verified nothing) shipping inside the fix for it, for the second time in
one PR.

Realistic trigger: the exclusion list is invited to grow ("Each entry needs a
justification"), and any future scope whose mutants are all excluded goes green
silently. The fix is one character of scope — count with `-F` **and** `-E`, i.e.
`cargo mutants -F "$FN" -E "$EQUIVALENT" --list`, which returns 20 here.

### R2-F2 (MAJOR) — the merge broke the `CLAUDE.md` task board; three rows now render outside the table, and the AUDIT entry records the merge as a clean pure-append

`git blame` attributes lines 211, 213 and 215 of `CLAUDE.md` — three **blank
lines** inside the task-board table — to the merge commit `77246b39`. Neither
parent has them (`main` has SEC-02 / FIX-01 / Pending contiguous).

A blank line terminates a GFM table. Confirmed against GitHub's own renderer
(`POST /markdown`, mode `gfm`, lines 179-216): the table ends after **SEC-02**,
and `FIX-01`, **`PROC-06` — the row this PR adds** — and `Pending | Awaiting
User Task` render as literal paragraphs with visible pipes.

This is DOC-02 round 3 repeating: a documentation edit silently destroying the
structure of the very table it edits. It also contradicts the new AUDIT
paragraph, which argues the merge was safe because "the conflicts were a pure
append in two documents". The append was not clean, and the entry does not say
so.

### R2-F3 (MINOR) — the EQUIVALENT exclusion is broader than the mutant it documents, global to every invocation, and has no expiry

`EQUIVALENT='replace \| with \^ in hex_decode'` is matched against the `--list`
name, which is `src/hex.rs:42:44: replace | with ^ in hex_decode`. The regex is
unanchored and carries no file or line, so it excludes:

* **any future** `| → ^` mutant anywhere in `hex_decode`, not just the one at
  42:44 that the justification proves equivalent — a second `|` added to that
  function where OR and XOR differ would be silenced on arrival;
* the same mutant in any function whose name *contains* `hex_decode`;
* on every invocation of the script, including
  `./scripts/mutants.sh 'DonationSchedule::level' 'donate::'`, since the
  exclusion is not scoped to the run.

Anchoring on `^src/hex\.rs:42:44: ` would pin it to the justified mutant and
would fail closed (re-appearing as a survivor) the moment the line moves.
Secondary: the comment reads as a list ("Each entry needs a justification") but
the mechanism holds exactly one regex; a second entry has to be alternated into
the same string.

### R2-F4 (MINOR) — the script's headline count is the pre-exclusion count

`mutants: 21 mutant(s) ... tested with 'hex::'` immediately above
`Found 20 mutants to test`. Same root cause as R2-F1; it is the visible symptom
that was available and not read.

### R2-F5 (MINOR) — the debug-suite figure disagrees with itself inside this PR

`AUDIT.md` PROC-06 says the debug lib suite measures **188 s** here and derives
8 x 188 s = 25.1 min; `scripts/mutants.sh:27` says **~192 s**; R1-F12 measured
191.87 s. Neither figure is sourced. Both support the conclusion (scope
dominates), so this is accuracy only — but this repo retracts figures for a
living and the two numbers are in the same change.

### R2-F6 (MINOR) — a bad `-F` regex fails with no message at all

`2>/dev/null` on line 74 discards `cargo-mutants`' diagnostics, and `set -e`
then aborts on the pipeline. `./scripts/mutants.sh '[' 'hex::'` exits **1**
printing nothing. Fails closed, which is the right direction, but the operator
is told nothing — including in the case that matters, a broken build making
`--list` fail.

## Verified, no defect

* **The gate can go red.** Break-tested: `#[ignore]` on `round_trips`,
  `decodes_either_case_and_encodes_lower` and `every_byte_round_trips` in
  `src/hex.rs` gave `20 mutants tested in 30s: 13 missed, 5 caught, 2 unviable`,
  **exit 2**. `src/hex.rs` restored from a pre-edit copy; tree clean.
* **Empty filter fails.** `./scripts/mutants.sh no_such_function_xyz 'hex::'` →
  exit **3** with the explanatory message. Usage error → exit 2.
* **Green path reproduces.** `./scripts/mutants.sh hex_decode 'hex::'` →
  `20 mutants tested in 32s: 18 caught, 2 unviable`, exit 0, `real 33.0s`. The
  33 s claim holds.
* **R1-F1 closed.** `git ls-files | grep -i mutant` → `scripts/mutants.sh` only.
  `.gitignore` carries `mutants.out/` and `mutants.out.old/` with the reason.
* **R1-F2 closed.** The Files-changed list names six files; the diff against
  `main` touches exactly those six plus this ledger.
* **Counts reproduce** (arm64, this tree): total **3,196**;
  `src/pool_connection.rs` **88** here (src is byte-identical to `main`) and
  **95** on `security/bound-recv-buffer`, counted from `git archive` of that
  branch. The tree is now named in the entry.
* **Advisory wiring.** Live protection requires exactly the five contexts;
  `mutation testing (advisory, hex::)` is not among them. All six checks are
  SUCCESS on `d97b0b2`; PR is `MERGEABLE` / `CLEAN`.
* **The merge lost nothing else.** `git diff main HEAD -- src/ Cargo.toml
  Cargo.lock` is empty (FIX-01's `donate.rs` preserved byte-identical), and
  `git diff 99854a9 HEAD` over `ci.yml`, `scripts/mutants.sh`, `.gitignore` and
  `_shared-context.md` is empty. Only `CLAUDE.md` was damaged (R2-F2).
* **The rebase account is corroborated.** The reflog shows two
  `rebase (start): checkout origin/main` / `rebase (abort): returning to
  refs/heads/chore/mutation-testing` pairs at 10:20 and 10:21, both returning to
  `99854a9`, and the branch was exactly one commit behind (`e07a9a2`). The
  narrative matches the record.

## Could not verify

* What a **failed** `continue-on-error: true` job reports as its check-run
  conclusion. If GitHub reports SUCCESS, the advisory job is not merely
  non-blocking but invisible, and a new survivor would never be noticed —
  which is the whole value proposition. Settling it needs a deliberately red
  run pushed to a PR; not done.
* The unscoped `28 min / 8 mutants` row: `take_complete_lines` lives on another
  branch, so it was not re-measured here.
