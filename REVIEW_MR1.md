# Review: MR !1 — JIT native iteration loop (open state)

Branch `feat/jit-native-loop` → `main`. Rounds 1–12 complete.
Full transcripts of every round: **`REVIEW_MR1_ARCHIVE.md`** (~2900 lines).

> **Read this file, not the archive.** The archive is 175 KB. Consult it only to
> look up one specific finding by its ID. Loading it whole is what ended the
> previous reviewer's run — its context reached 560k tokens and round 13 could
> not start. Findings live here in one line each; the archive holds the reasoning
> if you need it.

## Standing protocol — read this first if you are resuming cold

1. Write findings to this file **as you go** — after each finding, not at the
   end. Assume you can be killed at any moment.
2. Update the round's coverage ledger **before** starting each item and again
   when it is done, so an interruption leaves an accurate picture.
3. `git add REVIEW_MR1.md && git commit` periodically. **This file only.** Never
   commit anything else, never amend, never push.
4. Each round has a `## Round N brief` section stating scope and questions. If
   you are resuming, that brief is your instructions — the requester's message
   is not available to you.
5. Verify claims by reading and running, not by trusting commit messages or the
   requester's summary. Several rounds found defects in fixes whose commit
   messages described them as correct.
6. Do not fix anything. Review only.
7. Finish by setting Status, filling "remaining work", committing, and stating
   plainly whether the MR is mergeable.
8. **Do not read `REVIEW_MR1_ARCHIVE.md` in full.** `grep` it for a finding ID.

**Resume procedure:** find the last `## Round N coverage ledger` below, take the
first row not marked DONE, and continue from there.

## Status after round 13 (`2a6b5fa..593a410`)

Round 13 COMPLETE. No blockers, no majors. Three findings: R13-F1 (MINOR — the
startup line and `worker_loop` disagree about share verification on non-aarch64),
R13-F2 (MINOR — the line still over-reports when the JIT itself is unavailable on
aarch64), R13-F3 (TRIVIAL — three `--help`/warning wording clauses). Both round-12 minors verified
fixed. **Mergeable as of `593a410`.**

Rounds 5–13 found nothing that can produce a wrong hash, a withheld valid share,
or an out-of-bounds access.

## Open items — all tracked on GitLab

Every deferred finding now has an issue, so this ledger is no longer the only
record. Filed 2026-09-03.

| ID | Issue | Item |
|---|---|---|
| R13-F1 | [#3](https://gitlab.com/stephen84s/miner-tim/-/issues/3) | Startup line and `miner.rs:549` disagree on non-aarch64 — reports `off`, verifies anyway. Introduced by `593a410`. |
| R13-F2 | [#4](https://gitlab.com/stephen84s/miner-tim/-/issues/4) | Silent `MAP_JIT` fallback: `.ok()` at `vm.rs:1681,1714` swallows the error, line still says `on`, verifier compares the interpreter against itself. **Reachable on the shipping platform.** |
| R13-F3 | [#5](https://gitlab.com/stephen84s/miner-tim/-/issues/5) | `--help` synopsis omits `--verify-shares`; two other wording carry-overs. |
| R5-F2 | [#6](https://gitlab.com/stephen84s/miner-tim/-/issues/6) | `make test` runs debug; AUDIT verification ran release, so the `debug_assert` nets never ran in the verified profile. |
| R5-F4 | [#7](https://gitlab.com/stephen84s/miner-tim/-/issues/7) | Two 2 GiB `LazyLock` test datasets, ~4.5 GiB peak — may block contributors on 16 GB machines. |
| R5-F6 | [#8](https://gitlab.com/stephen84s/miner-tim/-/issues/8) | Multi-thread bench phase has no barrier; dilutes rather than inflates, but the aggregate CI is too narrow. |
| R5-F7 | [#1](https://gitlab.com/stephen84s/miner-tim/-/issues/1) | 8 redundant FMOVs per iteration in the f-load path. Opportunity, not a defect. |
| — | [#2](https://gitlab.com/stephen84s/miner-tim/-/issues/2) | ARM64 / multi-platform CI. **The one item not to leave open indefinitely** — and doubly earned: R13-F1 exists precisely because no non-aarch64 build is ever run, only linted. |
| — | — | `worker_loop` verifier glue (3 lines): re-check whether the `ShareVerdict`/`classify_share` extraction closed this. |

## Closed findings — index only

Reasoning for each is in `REVIEW_MR1_ARCHIVE.md`; grep the ID.

**Round 5 (initial):** F1 (MAJOR — A/B benchmark measured the native loop against
itself; +9.01% retracted), F2, F3, F4, F5, F6, F7.
**Round 6 (`4a4f5ca..`):** R6-F1 (CBZ range assert 2x too loose — imm19 is
signed), R6-F2 (t-table buckets anti-conservative), R6-F3 (empty `--native-loop`
silently ignored), R6-F4 (MAJOR — published CI narrower than run-to-run
reproducibility), R6-Q1 (fail-safe direction).
**Round 7 (`95e0c9a..`):** R7-F1 (MAJOR — 256 MiB Argon2d cache per full-mode VM,
never read; ~2.75 GiB at 11 threads), R7-F2 (fail-safe **inverted** for
`--verify-shares`), R7-F3, R7-F4, R7-F5, R7-F6, R7-Q1.
**Round 8:** R8-F1, R8-F2 (`set_var` racing `getenv`).
**Round 9 (`a8589c8..0df35e9`):** R9-F1 … R9-F7.
**Round 10 (`0df35e9..726bb21`):** R10-F1, R10-F2 (MAJOR — an empty value
*erased* an explicit `off`, silently re-enabling the native loop).
**Round 11 (`726bb21..29e3ed8`):** R11-F1 … R11-F4.
**Round 12 (`29e3ed8..2a6b5fa`):** R12-F1 (startup line reports *requested*, not
*effective*, state; invisible under `RUST_LOG=warn`), R12-F2 (`--help` note
formatted as a flag entry). Both verified fixed in round 13.

**Round 13 (`2a6b5fa..593a410`) — OPEN, not yet fixed.** Full text is below in
this file, not in the archive: R13-F1 (MINOR — report says
`share verification: off` on non-aarch64 while `miner.rs:549` enables the
verifier), R13-F2 (MINOR — line reports `Native-loop JIT: on` when
`JitCompiler::new()` failed on aarch64; the verifier then compares the
interpreter against itself), R13-F3 (TRIVIAL — `--help` omits `--verify-shares`
from the synopsis, the empty-value example is flag-only, and the native-loop
warning gives non-actionable advice on non-aarch64).

## Standing lesson

Every round from 5 onward found a real defect **in the fix written for the
previous round's finding** — including a benchmark measuring a path against
itself, a signed/unsigned range assert 2x too loose, and an inverted fail-safe on
a safety switch. Treat each round's fixes as unreviewed code, not as corrections.

---

# Round 13 — `2a6b5fa..593a410`

## Round 13 brief

Scope: the two commits fixing R12-F1 and R12-F2, in `src/bin/minertim.rs`.

Priorities, in order:

1. **The consistency question.** The startup report now computes
   `verify_effective = verify_shares && native_effective`, where
   `native_effective = native_loop && cfg!(target_arch = "aarch64")`. That
   encodes "verification only applies when the native loop is on". Check it
   against what `worker_loop` actually does —
   `ShareVerifier::new(verify_shares && native_loop)` — **which has no `cfg!`
   term**. If those two expressions can ever disagree, the miner reports one
   thing and does another, which is worse than the original overclaim R12-F1
   named. Determine whether they can disagree, and on which target.
2. **R12-F1's fix.** Does the line now report effective state in all four
   switch combinations, on both aarch64 and non-aarch64? Does the
   "verification DISABLED" warning still fire in exactly the right cases, and
   *not* fire spuriously when the native loop was already off?
3. **R12-F2's fix.** Is the `--help` note now positioned and formatted so it
   reads as documentation of both switches rather than as a third flag?
4. **The comments and AUDIT.** R12-F1 also required dropping the word
   "unconditional". Verify no code comment or AUDIT line still claims it.

Verify by reading and running against `593a410`'s source. The requester reports
125 lib + 8 bin tests passing in release and clippy clean on both targets —
confirm rather than trust.

## Round 13 coverage ledger

| # | Item | Status |
|---|---|---|
| 1 | `verify_effective` vs `worker_loop` consistency | DONE — R13-F1, R13-VC6 |
| 2 | R12-F1 fix — four combinations, both targets | DONE — R13-VC1, R13-VC4, R13-F2 |
| 3 | R12-F2 fix — help layout | DONE — R13-VC2, R13-F3 |
| 4 | "unconditional" removed from comments + AUDIT | DONE — R13-VC3 |
| 5 | Test + clippy claims reproduced | DONE — R13-VC5 |

## Round 13 findings

### R13-F1 — The report and the behaviour disagree on non-aarch64: the line says `share verification: off` while `worker_loop` builds an **enabled** verifier  [MINOR]
**Where:** report `src/bin/minertim.rs:86-87` and `:102` (`verify_effective`),
warning `src/bin/minertim.rs:108`; behaviour `src/miner.rs:549`.

Three expressions, two of which were updated by `593a410` and one of which was
not:

| Site | Expression |
|---|---|
| Startup report (`minertim.rs`) | `verify_shares && native_loop && cfg!(target_arch = "aarch64")` |
| `DISABLED` warning (`minertim.rs`) | `!verify_shares && native_loop && cfg!(target_arch = "aarch64")` |
| **Actual verifier** (`miner.rs:549`) | `ShareVerifier::new(verify_shares && native_loop)` — **no `cfg!` term** |

So on a non-aarch64 build with both switches on, the miner prints
`share verification: off` and then constructs `ShareVerifier::new(true)`,
verifying every share. The fix moved the two *reporting* sites to effective
state and left the *behavioural* site on requested state.

**Direction matters:** this is an **under**claim, not an overclaim. The miner
does more than it says, not less. No share is withheld that would otherwise be
submitted, and no wrong hash is possible: on non-aarch64 the mining VM and
`reference()`'s `set_native_loop(false)` VM are both the interpreter, so their
hashes always agree and `verify_failures` stays 0 (wasted double-hash per share
only, and shares are rare).

**Severity MINOR, deliberately not major.** On aarch64 `cfg!` is `true`, so all
three expressions coincide and the shipping target reports exactly what it does.
The divergence exists only on a target that today is reached solely by
`cargo clippy --target x86_64-apple-darwin` — there is no non-aarch64 test or
run anywhere (open issue #2, ARM64/multi-platform CI, is the same gap).

**If it is fixed:** the honest repair is to make `miner.rs:549` read the same
effective predicate rather than to weaken the report — otherwise the next
platform port re-opens it. There is no fourth consumer: `grep` for
`verify_shares|native_loop` across `src/` and `benches/` returns only
`minertim.rs`, `miner.rs`, `vm.rs`, `jit/compiler.rs`, `tests.rs` and the bench,
and only the two sites above decide whether verification runs.
**Confidence:** HIGH — read from both sources, and confirmed on a real x86_64
build (R13-VC6). `ShareVerifier::new` at `miner.rs:389-391` is a plain
`Self { vm: None, dataset: None, key: Vec::new(), enabled }` — no `cfg` of its
own, so the call site's argument is the whole story.

**The same false premise is asserted in two more durable places, both new in
`593a410`**, which is why this is worth fixing rather than tolerating:
- the comment at `minertim.rs:82-85` justifying `verify_effective` — *"verification
  is skipped when the native loop is off, because the mining path is then already
  the reference path"*;
- the round-12 `AUDIT.md` entry, which repeats that sentence and adds *"It now
  reports effective state."*

Both are true on aarch64 and false on non-aarch64. So the belief was encoded in
three places — comment, AUDIT, report expression — and the one site that decides
behaviour (`miner.rs:549`) was left on the old predicate. `AUDIT.md` is the
project's authoritative append-only record, so a future reader will trust the
claim rather than re-derive it.

## Round 13 verdict

**Blockers: none. Majors: none.** Three findings, all MINOR or TRIVIAL, none of
which can produce a wrong hash, a withheld valid share, or an out-of-bounds
access on the shipping target.

**The four brief priorities, answered:**

1. **Can `verify_effective` and `worker_loop` disagree? Yes — on non-aarch64,
   and only there.** The report computes
   `verify_shares && native_loop && cfg!(aarch64)`; `miner.rs:549` computes
   `verify_shares && native_loop`. With both switches on, an x86_64 build prints
   `share verification: off` and then verifies every share. Confirmed empirically
   against a real `x86_64-apple-darwin` build (R13-F1, R13-VC6). It is an
   **under**claim — the miner does more than it says — and on aarch64 all three
   expressions coincide, so the shipping target reports exactly what it does.
   MINOR, and the honest repair is at `miner.rs:549`, not by weakening the line.
2. **R12-F1's fix: correct in all four combinations on aarch64, and the
   `DISABLED` warning fires in exactly the right cases** — including *not*
   firing when the native loop is already off. Traced and then measured against
   the built binary (R13-VC1, R13-VC4). But the fix models only one of the four
   preconditions in `execute_vm_inner`'s guard: `jit.is_some()` is not pinned, so
   a MAP_JIT allocation failure on aarch64 leaves the line saying
   `Native-loop JIT: on` while the interpreter runs — and the verifier then
   compares the interpreter against itself and reports a clean counter forever
   (R13-F2, MINOR, reachable on the shipping platform).
3. **R12-F2's fix: fully closed.** The note is a titled paragraph below both
   switches, heading at column 0 against the flags' column 2, and it names the
   two switches in its heading — better than asked (R13-VC2). Three trivial
   wording carry-overs remain (R13-F3).
4. **"Unconditional": gone from the code**, replaced by a comment that states
   the opposite and its consequence. `AUDIT.md:2401` still carries the word in
   the **round-11** entry, which is correct: the audit is append-only by project
   rule and the round-12 entry corrects it 42 lines below. No defect (R13-VC3).

**Mergeable: yes**, no caveat. Nothing outstanding across rounds 5-13 can
produce a wrong hash, a withheld valid share, or an out-of-bounds access. All
three round-13 findings are about the accuracy of what the miner *reports*, not
what it *does* — with the caveat that R13-F2's second-order effect (a
verification counter reading clean because both sides are the interpreter) makes
a *reassurance* void rather than making anything wrong.

**Note on the standing lesson:** for the first time since round 5, the fix for
the previous round's finding did not introduce a regression in what the code
*does* — but R13-F1 is a genuine defect **introduced by** `593a410` (the
report/behaviour split did not exist before this commit), so the pattern of "the
fix needs review too" holds. R13-F2 is R12-F1(b) closed only halfway.

## Remaining work if this review is interrupted

- **Round 13 is complete.** All five ledger items done; three findings filed;
  full release suite and both clippy targets reproduced against `593a410`.
- Order I would take the findings: **R13-F1** (align `miner.rs:549` with the
  effective predicate — one line, and it is the only one where the miner says
  one thing and does another), then **R13-F2** (log the discarded
  `mmap MAP_JIT failed` at `vm.rs:1681,1714` instead of `.ok()`-swallowing it,
  and/or qualify the startup line on `jit.is_some()`), then **R13-F3** (three
  wording clauses). None is required for merge.
- Unchanged and still open by choice: R5-F2, R5-F4, R5-F6, issue #1 (R5-F7),
  issue #2 (ARM64 CI), `worker_loop` testability. **Issue #2 is now doubly
  earned:** R13-F1 exists precisely because no non-aarch64 build is ever run,
  only linted.

### R13-VC1 — item 2: the four switch combinations are right on aarch64, and the `DISABLED` warning fires in exactly the right cases.
On aarch64 `cfg!` is `true`, so `native_effective == native_loop` and
`verify_effective == verify_shares && native_loop`. Traced against the source at
`minertim.rs:79-125` and confirmed against a built binary (see R13-VC4):

| NL | VS | line | warnings |
|---|---|---|---|
| on | on | `Native-loop JIT: on \| share verification: on` | none |
| on | off | `on \| off` | verification-DISABLED |
| off | on | `off \| off` | native-loop-DISABLED only |
| off | off | `off \| off` | native-loop-DISABLED only |

Row 3 is the one R12-F1 asked about, and it is now right in both halves: the
line reports `share verification: off`, which **matches** what `worker_loop`
builds (`verify_shares && native_loop` = `true && false` = disabled), and the
verification-DISABLED warning correctly does **not** fire — verification is moot
when the mining path is already the reference path. No spurious warning.

On non-aarch64 the native half is also right (`off (requested on; unavailable on
this target)`), and only the verification half diverges from behaviour — R13-F1.

One asymmetry, noted and **not** filed as a defect: the native-loop-DISABLED
warning is keyed to `!native_loop` (requested), not `!native_effective`, so on a
non-aarch64 build with the switch on nothing warns even though the loop is not
running. That is defensible — the warning's payload is "unset the flag to
restore it", which is not actionable advice on a target that has no native loop
— and the info line already says `unavailable on this target`.

### R13-F2 — R12-F1(b) is closed for the *wrong architecture* but not for the *missing JIT*: on aarch64 the line still reports `Native-loop JIT: on` when the JIT could not be allocated  [MINOR]
**Where:** `src/bin/minertim.rs:86` (`native_effective`); `src/randomx/vm.rs:1681,1714`
(`jit: super::jit::JitCompiler::new().ok()`); guard at `src/randomx/vm.rs:1221-1223`.

`execute_vm_inner`'s native-loop guard has four preconditions:
`use_native_loop && version == V1 && dataset.is_some() && jit.as_mut().is_some()`,
inside `#[cfg(target_arch = "aarch64")]`. `native_effective` models exactly one
of them (the `cfg`). Two of the remaining three are pinned by `worker_loop`
always constructing `RandomXVm::new_full` — V1, dataset present — which I
verified at `miner.rs:578-583`. **`jit.is_some()` is not pinned.** It is
`JitCompiler::new().ok()`: if the `mmap(..., MAP_ANON|MAP_PRIVATE|MAP_JIT)` in
`jit/memory.rs:38-51` fails, the `Err("mmap MAP_JIT failed")` is discarded by
`.ok()` with no log of any kind, `jit` is `None`, and execution falls through to
the interpreter (`vm.rs:1248-1252` makes `jit_fn` `None`; there is no panic).

So on the shipping target — macOS / Apple Silicon, the only place this runs —
the startup line can say `Native-loop JIT: on | share verification: on` while
the miner is running the **interpreter**. Unlike the arch case in R13-F1, this
one is reachable on the platform the MR ships to.

**Compounding effect, worth stating separately:** in that state the mining VM
and `ShareVerifier::reference()`'s `set_native_loop(false)` VM are *both* the
interpreter, so `verify_failures` reads 0 forever and the operator sees a clean
verification counter that is comparing a path against itself. `uses_native_loop`'s
own doc comment at `vm.rs:1735-1743` warns about precisely this shape (it is
structurally round 5's F1, the A/B benchmark measuring one arm against itself).
Nothing mis-computes — the interpreter is the reference — but the reassurance
the counter provides is void.

**Not a wrong-hash or memory-safety risk.** Cost is hashrate (silently, and far
more than the ~7% the native-loop warning quotes, since the fallback is the
interpreter rather than the body JIT) plus two false reassurances.
**Two separable sub-issues, both pre-existing in part:** the silent `.ok()` swallow
at `vm.rs:1681,1714` is older than this MR and out of its scope; the *report*
that now asserts effective state is new in `593a410`, and it is the assertion
that makes the swallow visible as a defect. A fix that only qualifies the
startup line would still leave the JIT failure itself unlogged.
**Confidence:** HIGH on the code path (read end to end); the failure is
untriggered here — I did not force an mmap failure.

### R13-VC2 — item 3: R12-F2 is properly closed. The note now reads as documentation, not as a flag.
Rendered from the built binary (`./target/release/minertim` with no args):
```
  --verify-shares on|off  Re-check every candidate share on the reference
                    ...  Also MINERTIM_VERIFY_SHARES.

Switch values (--native-loop, --verify-shares):
  on/off, true/false, yes/no, 1/0. An empty value is treated as unset: it is
  ignored with a warning rather than overriding an earlier setting, so
  `--native-loop "$VAR"` with $VAR unset will not silently undo one.
```
Both halves of R12-F2 are fixed: it is now **below** both switches it describes
(so "Switch values" has its antecedent), and it is a titled paragraph — blank
line, heading at column 0 where every flag entry starts at column 2, body
indented uniformly — so it cannot be mistaken for an option. Naming the two
switches in the heading is better than the generic "Switch values" I asked for.

### R13-F3 — Three small wording carry-overs in `--help` and the warnings, none behavioural  [TRIVIAL]
**Where:** `src/bin/minertim.rs:20` (usage synopsis) and `:47-51` (the new note).
1. Round 12's "small content gap" is unaddressed: the example is still
   flag-only (`--native-loop "$VAR"`), while the environment variables warn and
   behave identically and `MINERTIM_NATIVE_LOOP="$NL"` is the shape that was
   silent until round 12's commit. Half a clause.
2. On a non-aarch64 build with `--native-loop off`, the native-loop-DISABLED
   warning fires and advises *"Expect roughly 7% lower hashrate. Unset
   --native-loop / MINERTIM_NATIVE_LOOP to restore it"* — non-actionable on a
   target that has no native loop to restore. Same family as the two below;
   folded in here rather than filed separately.
3. The usage synopsis reads
   `<pool:port> <wallet> [threads] [--donate-level N] [--native-loop on|off]` —
   `--verify-shares` is missing from it, though it has a full entry below and is
   the switch that guards share correctness. Pre-existing, not introduced by
   `593a410`; not previously filed in rounds 5-12 (checked the archive).
**Confidence:** HIGH — read from rendered output and source.

### R13-VC3 — item 4: "unconditional" is gone from the code, and the AUDIT is handled correctly.
- **Code:** the only remaining occurrence in `minertim.rs` is at `:90`, where the
  comment now says the opposite of the old claim — *"it is NOT unconditional,
  and an earlier comment wrongly said so: `RUST_LOG=warn` suppresses it"*, plus
  the actionable consequence. That is the R12-F1(a) correction landing exactly
  where the next reader is. The other four tree-wide hits (`vm.rs:1228`,
  `tests.rs:592`, `jit/compiler.rs:775`, `jit/aarch64.rs:537`) are unrelated uses
  of the word. `DESIGN_JIT_NATIVE_LOOP.md` has none.
- **AUDIT:** `AUDIT.md:2401` still reads *"One unconditional line now reports both
  switches at startup"*. That is the **round-11** entry, and leaving it is right:
  `CLAUDE.md` requires the audit to be append-only ("Do not delete prior audit
  history; append chronologically"), and the round-12 entry corrects it in place
  42 lines below at `:2443` — *"It is not unconditional. It is `log::info!`..."*.
  History plus correction, not a stale claim. **No defect.** The brief's "no
  AUDIT line still claims it" cannot mean rewriting the ledger.
- **Wider read of the same item — the premise, not just the word:** the *new*
  comment at `minertim.rs:82-85` and the *new* AUDIT entry both assert
  "verification is skipped when the native loop is off". That claim is false on
  non-aarch64. Recorded under R13-F1, since it is the same defect stated three
  times rather than a separate one.

### R13-VC4 — item 2, measured: the four combinations behave exactly as traced.
Run against the freshly built `593a410` binary at `RUST_LOG=info` (pool refused,
which is after the line, so nothing hides it):
```
NL=on  VS=on   Native-loop JIT: on  | share verification: on    no warnings
NL=on  VS=off  Native-loop JIT: on  | share verification: off   verification-DISABLED warn
NL=off VS=on   Native-loop JIT: off | share verification: off   native-loop-DISABLED warn only
NL=off VS=off  Native-loop JIT: off | share verification: off   native-loop-DISABLED warn only
```
Identical to the table in R13-VC1. In particular no spurious verification
warning in rows 3-4, which was the specific regression risk in R12-F1's fix.

### R13-VC5 — item 5: the requester's claimed state is real, reproduced on `593a410`'s source.
```
git diff 2a6b5fa..HEAD -- src/ benches/ Cargo.toml Makefile   -> only src/bin/minertim.rs
cargo test --release      running 127 tests; 125 passed, 0 failed, 2 ignored (92.67s)
                          8 bin tests passed; 0 doc tests
cargo clippy --all-targets -- -D warnings                          clean (aarch64)
cargo clippy --all-targets --target x86_64-apple-darwin -- -D warnings   clean
```
125 + 8 exactly as claimed, and unchanged from rounds 11-12 — this commit adds no
tests, consistent with it being one log-line rewrite plus a `--help` reflow. The
2 ignored are the two ~2 GiB / 30-120 s dataset tests at `tests.rs:728,841`,
ignored since before this MR (R5-F4's memory concern, still open by choice).

**Not covered by any test, and worth stating plainly:** nothing in the suite
exercises the startup reporting path in `main`, on either target. All four
combinations in R13-VC4 and the x86_64 line in R13-VC6 were verified by running
binaries, not by tests — so a future edit to `native_effective` /
`verify_effective` would be caught only by another manual run. That is what let
R13-F1 through.

### R13-VC6 — R13-F1 confirmed empirically on a real non-aarch64 build.
```
$ cargo build --release --target x86_64-apple-darwin      (exit 0)
$ RUST_LOG=info ./target/x86_64-apple-darwin/release/minertim 127.0.0.1:1 4Awallet 1 \
      --native-loop on --verify-shares on
Native-loop JIT: off (requested on; unavailable on this target) | share verification: off
```
The native half is right and its qualifier reads well. The verification half says
`off` while `miner.rs:549` builds `ShareVerifier::new(true && true)` — enabled.
The report/behaviour split is real, not a reading error.

**The other three x86_64 combinations are not worth running, and a resumed
reviewer should not redo them.** Divergence requires `verify_shares && native_loop`
true while `verify_effective` is false, i.e. both switches on and `cfg!` false —
which is exactly the row above. `NL=off` zeroes both sides; `VS=off` zeroes both
sides. The remaining three rows are non-divergent by construction.
