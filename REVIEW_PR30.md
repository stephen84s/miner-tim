# Review: PR #30 (fix/help-wording), round 1

## Coverage ledger

| Item | Status |
|---|---|
| AUDIT.md `DOC-03` / PR body claims vs diff | Checked |
| Test coverage of `native_loop_disabled_warning` | Checked, break-tested |
| `CLAUDE.md` env-var correction, independently verified | Checked (ran binary) |
| `--help` output | Checked (ran binary) |
| Task-board / AUDIT.md record format | Checked (GitHub markdown API) |
| `cargo test --release`, `cargo clippy` | Both run, clean |

## What was verified and checked out

- **`--verify-shares` already in the synopsis** (item 1, "already fixed"): confirmed
  present at `src/bin/minertim.rs:21` on `origin/main` *before* this branch. The
  "no change needed" claim is true — nothing in the diff touches that line.
- **Env-var claim, reproduced by running the binary**, not just reading:
  `NATIVE_LOOP=off ./minertim 127.0.0.1:1 <wallet> 1` → `Native-loop JIT: on
  (requested)` (ignored). `MINERTIM_NATIVE_LOOP=off ...` → `off (requested)` +
  the disabled-warning line (works). Matches the PR body and `AUDIT.md` exactly.
  Grepped the rest of the repo (`README.md`, `Makefile`, `mining.conf.example`,
  `src/miner.rs`, `AUDIT.md`) for the same wrong claim — only `CLAUDE.md` had
  it; everywhere else already frames the bare names as `mining.conf` keys.
- **`--help` output**: synopsis, examples, and the updated "Switch values"
  paragraph (both `--native-loop "$VAR"` and `MINERTIM_NATIVE_LOOP="$VAR"`
  forms) match the diff byte for byte.
- **Break-test of `native_loop_disabled_warning`, reproduced**: mutated the
  non-aarch64 arm to return the aarch64 advice string → `cargo test --release
  --bin minertim native_loop_warning_is_target_aware` fails at exactly
  `minertim.rs:868` with the message the PR describes. Restored from a `cp`
  copy, `cmp` byte-identical, `git status --short` clean afterward.
- **`scripts/mutants.sh` blind spot, reproduced**: `cargo test --lib
  native_loop_warning_is_target_aware` → 0 run, 161 filtered out (silent
  success on a name that doesn't exist in `--lib`). `cargo test --bin minertim
  <same>` → 1 run, passes. Confirms Finding 6 exactly.
- **Task-board table, checked with GitHub's own renderer**
  (`gh api --method POST /markdown -f mode=gfm`): branch renders as 3
  `<table>` elements (task board, platform coverage, versions), same as
  `origin/main`; task-board table is 36 `<tr>` on the branch vs 35 on `main` —
  exactly one added row, no split. Matches the PR body's "1 table, 36 rows
  against main's 35" claim.
- **`cargo test --release`**: 159 lib passed / 2 ignored, 19 bin passed
  (18 + the new one) — matches `AUDIT.md` exactly.
- **`cargo clippy --all-targets --release -- -D warnings`**: clean.
- `AUDIT.md`'s "lines 421–427" citation for the old Runtime-switches text
  checked against `origin/main:CLAUDE.md` — exact match.

## Findings

**Minor 1 — `AUDIT.md` Finding 5 misattributes the `std::env::var` call and
overstates it as unique.** It reads: *"`parse_native_loop` makes the only
`std::env::var` call and reads exactly `"MINERTIM_NATIVE_LOOP"` /
`"MINERTIM_VERIFY_SHARES"`."* Reading the source: the call is in the shared
`parse_switch` helper (`minertim.rs:401`), not in `parse_native_loop` itself,
which only ever passes `"MINERTIM_NATIVE_LOOP"` (`parse_verify_shares` passes
the other name through the same helper). It is also not the *only*
`std::env::var` call in the binary — `minertim.rs:284` reads
`MINERTIM_TLS_FINGERPRINT` separately. The PR body's own wording ("`parse_switch`
makes the only `std::env::var` call **on that path**") is accurate; the
`AUDIT.md` entry lost that precision in the rewrite. Not misleading about
behaviour (the empirical claim — bare names are ignored — is correct and
reproduced above), but `AUDIT.md` is supposed to be exact, and this is the kind
of small drift the project has flagged before.

**Minor 2 — `AUDIT.md`'s "Files changed" line says "no changes to line
counts," which is false.** `src/bin/minertim.rs` grew by a net 55 lines
(824 → 879, `+59/-4` per `git diff --numstat`). The intended claim was almost
certainly "no changes to *existing* function signatures," which is true, but as
written it asserts something the diff contradicts.

**Major — the wiring between `main()` and `native_loop_disabled_warning` is
untested, and a break-test of it passes clean on the shipping platform.** I
mutated the call site (`native_loop_disabled_warning(cfg!(target_arch =
"aarch64"))` → `native_loop_disabled_warning(!cfg!(target_arch = "aarch64"))`
at `minertim.rs:139`) and reran the full bin suite: all 19 tests, including
`native_loop_warning_is_target_aware`, still pass. Restored and `cmp`
byte-identical afterward. This is the same shape flagged in the task brief
(PR #22, "a test held a helper while the mutation targeted the wiring") —
`native_loop_warning_is_target_aware` genuinely tests the two arms of the
extracted function, but nothing tests that `main` calls it with the right
argument. On this host (aarch64 macOS — the shipping target), that mutation
makes an operator who runs `--native-loop off` see "Native-loop JIT
unavailable on this architecture (aarch64 only)" instead of the actionable
advice to unset the flag. That is not a cosmetic slip: it is a silent
reintroduction, on the shipping platform, of the exact defect issue #3 item 3
exists to fix ("the warning gives non-actionable advice on the wrong
architecture") — just triggered by a wiring mistake instead of a hardcoded
string. No test would catch it. It is not a blocker (no wrong hashes, no
disabled safety net — `--native-loop` and `--verify-shares` themselves are
unaffected; only this one advisory log line would lie), but it is silently
wrong behaviour with no test standing between it and a release, which is why
it is rated major rather than minor.

The PR's own "Not established" section says the `cfg!(target_arch =
"aarch64")` argument at the call site "is not itself covered by a test" — true
but weaker than what I found. It does not say a wiring mutation was tried and
passed; that is a stronger, empirical result this review adds, not something
already disclosed.

**Observation, not a finding** — the new `DOC-03` task-board row is a
one-line pointer ("See `AUDIT.md` for details") rather than the multi-sentence
summary every other row carries. This deviates from house style, but
`LEDGER-01` already flagged the task board as 46% of `CLAUDE.md`'s size, so a
terser row is plausibly the right direction rather than a regression. Not
flagged as a defect.

**Checked and confirmed accurate** — `AUDIT.md`'s parenthetical "`cargo run
--release -- --help`: ... Exit 1 is expected (`args.len() < 3` triggers the
help pathway to exit 1)." Ran `./target/release/minertim --help` directly and
checked `$?`: exits 1. (`args.len()` is 2 for that exact invocation — program
name plus `--help` — which is `< 3`, matching the code path cited.) Also
confirmed `./target/release/minertim` with no args exits 1 the same way.

**Pattern across the three `AUDIT.md` findings above**: Minor 1, Minor 2, and
the Major all share one shape — the `AUDIT.md` entry (not the diff, not the
behaviour) claims slightly more, or slightly other, than what the source and
tests actually support: a helper misattributed and an "only" that isn't, a
"no changes to line counts" that is false by 55 lines, and a coverage gap
stated as "not covered by a test" when the stronger and more damaging fact —
that a wiring mutation reintroducing the PR's own target defect passes the
suite clean — was available to state and wasn't. This is this repo's named
recurring defect (entries claiming more than the diff/tests support), so it is
named here as one pattern rather than three unrelated nits.

## Not verified / out of scope

- The non-aarch64 arm of `native_loop_disabled_warning` has never executed on
  real non-aarch64 hardware, only by passing `false` directly in the test and
  by hand-inverting the flag on this aarch64 host. `AUDIT.md` already discloses
  this; I have nothing to add beyond confirming the disclosure is accurate.
- Did not re-run `make verify-jit` — this PR touches no code under
  `src/randomx/jit/`, `vm.rs`'s native-loop path, or `benches/`, so per this
  agent's scope note that gate is not this round's job; nothing in the diff
  would trigger it regardless (no JIT/emitter/vm.rs changes).

## Scope note

This diff touches only `src/bin/minertim.rs`, `CLAUDE.md`, and `AUDIT.md`. It
does not touch `src/randomx/jit/`, the emitter, `vm.rs`'s native-loop path,
`benches/`, `.github/workflows/`, `Makefile`, `scripts/`, or
`.cargo/config.toml`. Squarely `pr-reviewer` scope; no handoff needed.

## Verdict

**MERGEABLE, no blockers.** One major, two minors. The major is a test-coverage
gap, not a wrong-behaviour-in-the-diff finding: the code as shipped is correct
(verified by running it on real aarch64 hardware, both switch states), but
nothing guards the one line of wiring that connects `main()` to the
architecture-aware warning, and a mutation of exactly that wiring silently
reintroduces the defect issue #3 item 3 was filed to fix. Recommend a test
before merge — even an assertion that `cfg!(target_arch = "aarch64")` (not its
negation) is what reaches the call site would close it — but this is a
judgment call for the lead: the code that ships today is right, and every
five-nines of this project's actual safety-critical switches
(`--native-loop`, `--verify-shares`) is unaffected.

**ACTIONABLE:** yes, on all three findings. The major needs a test or an
explicit, stronger acknowledgment in `AUDIT.md`/the PR body than the current
"not covered by a test" (which understates what this review found: the
mutation was tried and passes). Minor 1 and Minor 2 are one-sentence fixes to
the `AUDIT.md` entry. This entry has **not** merged to `main` yet, so per
`CLAUDE.md`'s correction rule it may be edited in place rather than appended
to.
