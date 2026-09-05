# Shared context for MinerTim reviewers

*Not an agent. Reference material the reviewer agents in this directory quote.
Keep the lessons here; keep each agent's file about its own scope.*

## What this project is

A Monero CPU miner in pure Rust for Apple Silicon. It writes ARM64 machine code
at runtime and jumps into it.

**A defect in that machine code does not crash. It silently produces wrong
hashes, the pool rejects the shares, and the user loses money.** Weight
wrong-hash risk and memory-safety risk above everything else, always.

## The rule that matters most

**Verify by reading and running. Never by trusting a commit message, a PR
description, or the implementer's summary.**

This is not generic caution. On MR !1, *every* review round from 5 onward found
a real defect **in the fix written for the previous round's finding**. The
corrections are the most dangerous code in any change.

## Failure modes this repo has actually produced

Look for these specifically. Each one passed a green test suite.

| What happened | Shape to watch for |
|---|---|
| A benchmark measured the new path against **itself** — the "+9.01%" claim was retracted | An arm whose identity comes from a default another commit can move |
| `assert!(s.contains("requested"))` **could not fail** — the literal was unconditional in the format string | Assertions on text the code always emits |
| A range assert was **2× too loose** — CBZ `imm19` is signed, so the bound was `1<<18`, not `1<<19` | Signed/unsigned confusion in encoding bounds |
| A fail-safe was **inverted** — malformed `--verify-shares` silently disabled the safety net | Fail-safe direction differs per switch; check each |
| An empty value **erased** an explicit `off`, silently re-enabling the fast path | Option composition and last-write-wins ordering |
| 256 MiB Argon2d cache allocated per VM and **never read** — 2.75 GiB at 11 workers | Allocation whose only consumer is behind a `cfg` or a branch that cannot be taken |
| Doc comments **orphaned** by splicing a new function under an existing one | New code inserted directly beneath a doc comment |
| Reported RSS figures **did not reproduce** — a 2.7× overstatement | Any measurement quoted without a reproduction |
| A test's `#[ignore]`/filter matched nothing, so libtest reported **success** | Test filters; renamed modules |

## Break-testing is required, not optional

If a change adds or relies on a test, **mutate the production code that test
guards and confirm the test fails.** A test that still passes with its subject
broken is testing nothing. State the mutation and the observed failure.

## What CI does and does not prove

- `lint`, `audit`, `test` run on **x86_64 Linux**, where `randomx::jit` is
  `cfg`'d out. **A green pipeline says nothing about emitted ARM64.**
- `jit-macos` (`macos-14`) and `jit-linux-arm` (`ubuntu-24.04-arm`) run
  `scripts/verify-jit.sh`: 92 tests, in **both** debug and release. It asserts an
  **exact pass count**, because libtest reports a filter matching nothing as
  success.
- Reproduce claimed results yourself. Wrap long runs in `caffeinate -i` so the
  Mac does not sleep. `make verify-jit` takes ~6 minutes locally.

## Context budget — this has killed reviewers before

An earlier reviewer was kept alive across rounds until its context reached 560k
tokens and it could no longer start. You are spawned cold on purpose.

- **Never read `AUDIT.md` (~180 KB) or `REVIEW_MR1_ARCHIVE.md` (~175 KB) in
  full.** `grep` them for a finding ID; `tail` for recent entries.
- Prefer `sed -n 'A,Bp'` and `grep -n` over `cat` on large sources
  (`vm.rs` ~2200 lines, `compiler.rs` ~1700, `miner.rs` ~1100).
- Start from `git diff main...<branch>`.

## Working rules

1. Write findings to your ledger file **as you go** — after each finding, not at
   the end. Assume you can be killed at any moment.
2. Keep a coverage ledger of your checklist, updated **before** and after each
   item, so an interruption leaves an accurate picture.
3. `git add <your ledger> && git commit` periodically. **That file only.** Never
   amend, never push, never merge.
4. **Do not fix anything.** Review only. Do not touch the working tree apart from
   your ledger. `.claude/settings.local.json` is often dirty and is never yours.
5. Severity: **blocker** (wrong hash, memory unsafety, data loss), **major**
   (silently wrong behaviour, a safety net that cannot fire), **minor**
   (accuracy, clarity, test quality), **nit**.
6. Finish by stating plainly whether the PR is mergeable, and say what you could
   **not** verify rather than implying you checked it.
7. End commit messages with:
   ```
   Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
   ```
