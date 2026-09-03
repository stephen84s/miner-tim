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

## Status after round 12 (`309cfda..74c8186`)

No blockers, no majors. Two minors filed (R12-F1, R12-F2), both since fixed in
`6765b17` — **unverified; that is round 13's job.** All four round-11 minors
verified fixed. **Mergeable as of `74c8186`.**

Rounds 5–12 found nothing that can produce a wrong hash, a withheld valid share,
or an out-of-bounds access.

## Open items — carried, by choice

| ID | Item | Why still open |
|---|---|---|
| R5-F2 | `make test` runs debug; AUDIT verification was release, so `debug_assert` nets never ran in the verified profile | Deferred |
| R5-F4 | Two 2 GiB `LazyLock` test datasets, ~4.5 GiB peak | May block contributors on 16 GB machines |
| R5-F6 | 11-thread bench phase has no inter-thread barrier | Dilutes rather than inflates the result |
| R5-F7 | 8 redundant FMOVs per iteration in the f-load path | GitLab issue #1; opportunity, not a defect |
| — | ARM64 / multi-platform CI | GitLab issue #2. **Reviewer: the one item not to leave open indefinitely.** |
| — | `worker_loop` verifier glue (3 lines) | Reviewer to re-check whether the `ShareVerdict`/`classify_share` extraction closed this |

## Closed findings — index only

Reasoning for each is in `REVIEW_MR1_ARCHIVE.md`; grep the ID.

**Round 5 (initial):** F1 (MAJOR — A/B benchmark measured the native loop against
itself; +9.01% retracted), F2, F3, F4, F5, F6, F7.
**Round 6 (`d49535a..`):** R6-F1 (CBZ range assert 2x too loose — imm19 is
signed), R6-F2 (t-table buckets anti-conservative), R6-F3 (empty `--native-loop`
silently ignored), R6-F4 (MAJOR — published CI narrower than run-to-run
reproducibility), R6-Q1 (fail-safe direction).
**Round 7 (`bbecd15..`):** R7-F1 (MAJOR — 256 MiB Argon2d cache per full-mode VM,
never read; ~2.75 GiB at 11 threads), R7-F2 (fail-safe **inverted** for
`--verify-shares`), R7-F3, R7-F4, R7-F5, R7-F6, R7-Q1.
**Round 8:** R8-F1, R8-F2 (`set_var` racing `getenv`).
**Round 9 (`3c281dc..5fe7eb3`):** R9-F1 … R9-F7.
**Round 10 (`5fe7eb3..6f2b95b`):** R10-F1, R10-F2 (MAJOR — an empty value
*erased* an explicit `off`, silently re-enabling the native loop).
**Round 11 (`6f2b95b..309cfda`):** R11-F1 … R11-F4.
**Round 12 (`309cfda..74c8186`):** R12-F1 (startup line reports *requested*, not
*effective*, state; invisible under `RUST_LOG=warn`), R12-F2 (`--help` note
formatted as a flag entry).

## Standing lesson

Every round from 5 onward found a real defect **in the fix written for the
previous round's finding** — including a benchmark measuring a path against
itself, a signed/unsigned range assert 2x too loose, and an inverted fail-safe on
a safety switch. Treat each round's fixes as unreviewed code, not as corrections.

---

# Round 13 — `74c8186..6765b17`

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

Verify by reading and running against `6765b17`'s source. The requester reports
125 lib + 8 bin tests passing in release and clippy clean on both targets —
confirm rather than trust.

## Round 13 coverage ledger

| # | Item | Status |
|---|---|---|
| 1 | `verify_effective` vs `worker_loop` consistency | DONE — R13-F1 |
| 2 | R12-F1 fix — four combinations, both targets | TODO |
| 3 | R12-F2 fix — help layout | TODO |
| 4 | "unconditional" removed from comments + AUDIT | TODO |
| 5 | Test + clippy claims reproduced | TODO |

## Round 13 findings

### R13-F1 — The report and the behaviour disagree on non-aarch64: the line says `share verification: off` while `worker_loop` builds an **enabled** verifier  [MINOR]
**Where:** report `src/bin/minertim.rs:87-88` (`verify_effective`), warning
`src/bin/minertim.rs:~110`; behaviour `src/miner.rs:549`.

Three expressions, two of which were updated by `6765b17` and one of which was
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
**Confidence:** HIGH — read from both sources; `cfg!` is compile-time and
unambiguous.

## Remaining work if this review is interrupted

Round 13 not started.
