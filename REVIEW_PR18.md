# REVIEW_PR18 — round 1, independent (`chore/drop-review-ledgers`)

Reviewer: `pr-reviewer`, cold. Base `origin/main` (`ef40bea`), head `380617a`.

**Verdict: MERGEABLE with two majors that are ACTIONABLE before merge.**
No blockers. Nothing is lost; two accuracy defects and one unstated precondition.

## Scope

Diff touches `src/randomx/jit/memory.rs`. Verified **comment-only** (below), so
no `jit-reviewer` handoff. No `benches/`, no speed claim. No
`.github/workflows/`, `Makefile`, `scripts/` or `.cargo/config.toml`, so no
`ci-reviewer` handoff.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Is anything lost? (all 13 + `445466b`) | done — nothing lost |
| 2 | Do all references resolve? | done — **three missed classes (M1)** |
| 3 | The rule change itself | done — sound, but **M2** + m3 |
| 4 | `memory.rs` comment-only, build/clippy | done — holds |
| 5 | LEDGER-01 / `CLAUDE.md` row vs the diff | done — m1, m2 |
| 6 | Overstated / omitted | done — M1, M2 |
| 7 | Concurrency | n/a — no runtime code changed |

## Verified good (reproduced, not trusted)

- **All thirteen ledgers remain retrievable.** For each deleted file,
  `git rev-list -1 origin/main -- <file>` gives a commit that is an ancestor of
  `origin/main`, and `git show <sha>:<file>` returns content. Checked all 13,
  not one.
- **`445466b` holds `REVIEW_PLAT01.md`** and it is an ancestor of `origin/main`
  (`git merge-base --is-ancestor` → yes). The file contains three `F11`
  mentions, so the citation resolves to real content.
- **Every measured figure reproduces exactly**: 13 ledgers = **530,655** bytes
  and **9,193** lines; `src/` + `benches/` `*.rs` **on `origin/main`'s tree** = **503,986** (summed
  via `git cat-file -s` over `git ls-tree -r origin/main`, not the working
  filesystem);
  `AUDIT.md` = **288,719**; `CLAUDE.md` = **43,122**; sum 862,496 ≈ "862 KB";
  `REVIEW_MR1_ARCHIVE.md` = 175,768 (34.9% of source).
- **`memory.rs` is comment-only.** No changed line outside a `//` comment. It is
  a plain `//` block, not a doc comment, above
  `#[cfg(target_os = "linux")] mod platform` — nothing orphaned.
  `cargo clippy --all-targets -- -D warnings` clean on the branch.
- **Crash-recovery rule genuinely unchanged.** `_shared-context.md` rules 1-3
  are byte-identical; the diff only appends paragraphs under rule 3.
- **Instruction addressed to the right party**: `CLAUDE.md` step 0 says the
  *lead* `git rm`s it.
- **Markdown splices render.** `CLAUDE.md`'s new paragraph uses the 6-space
  continuation indent of its bullet; `_shared-context.md`'s two paragraphs use
  the 3-space indent of the ordered list. Both are valid list continuations.
- No reference in `README.md`, `.github/workflows/`, `Makefile`, `scripts/`,
  `.cargo/config.toml` or `mining.conf.example`.

## M1 (major) — the PR missed two reference classes, and says so in `AUDIT.md`

`AUDIT.md`'s LEDGER-01 Verification line asserts:

> no remaining reference to a `REVIEW_*.md` file outside `AUDIT.md`'s historical
> citations and this entry.

False on the branch's own tree:

1. **`CLAUDE.md`'s task board carries seven dangling citations** — rows VIS-01
   (`REVIEW_ISSUE4.md`), PROC-01 (`REVIEW_PR7.md`), CI-03 (`REVIEW_PR8.md`),
   DOC-02 (`REVIEW_PR10.md`), BENCH-02 (`REVIEW_PR13.md`), REL-01
   (`REVIEW_PR14.md`), PERF-02 (`REVIEW_PR15.md`). `CLAUDE.md` is not
   append-only — this PR edits it, and merged rows have been rewritten in place
   before — so the append-only argument that protects the `AUDIT.md` citations
   does not cover these.
2. **`.claude/agents/_shared-context.md:68`**, a file this PR edits, still says
   *"Never read `AUDIT.md` (~210 KB) or `REVIEW_MR1_ARCHIVE.md` (~175 KB) in
   full"* — pointing a future reviewer's context-budget rule at a file the same
   PR deletes. Its `~210 KB` for `AUDIT.md` is also stale: the PR measured
   288,719 B (282 KB) and did not propagate it.

3. **`DESIGN_JIT_NATIVE_LOOP.md`** (tracked on `main`, untouched by this PR)
   refers to the MR !1 ledgers *without naming them* — line 99, "each verified
   against `vm.rs`, line refs in **the review on MR !1**", and line 254, "**The
   review** stated this as exactly zero margin; it conflated ...". Both point at
   `REVIEW_MR1.md` / `REVIEW_MR1_ARCHIVE.md`, both deleted here. Line 99 is
   load-bearing: it is the provenance for ordering hazards a reader is warned
   not to "simplify".
4. **`CLAUDE.md:172`** (PERF-02) says "Reviewed over several cold rounds (**see
   the ledger for the count**)" — after the merge there is no ledger to see.
   Recoverable only because the same row names `REVIEW_PR15.md` further on.

Classes 3 and 4 matter for *why* this recurs: the enumeration was produced by a
filename grep, which structurally cannot find a reference that does not name its
target. A semantic sweep (`grep -niE '\bledger\b|the review'`) finds them.

**Related**: LEDGER-01 calls itself "the pointer that makes them resolvable",
but the recipe it offers — `git show <sha>:REVIEW_X.md` — needs a sha at which
the file still existed, and a reader holding only "Ledger: `REVIEW_PR13.md`" has
no sha. `memory.rs` gives a concrete one (`445466b`); the 24 `AUDIT.md`
citations and the 7 `CLAUDE.md` ones do not. One generic line in LEDGER-01
(e.g. `git log --all --diff-filter=D -- REVIEW_PR13.md` to find the sha, then
`git show <sha>^:REVIEW_PR13.md`) would make the claim true.

So the reference classes are five, not two. The `CLAUDE.md` Project Structure
line does give a generic retrieval command, which softens (1) and (4) but does
not make the Verification sentence true, and does not reach (3) at all. This is the repo's recurring defect — a claim in
the authoritative record that the diff does not support — and once merged the
correction can only be appended. LEDGER-01 is on an unmerged branch, so it may
still be edited in place.

## M2 (major) — "readable forever" rests on an unstated precondition

`CLAUDE.md`: *"It remains readable forever via `git show <sha>:REVIEW_X.md` on
the branch."* `_shared-context.md`: *"stays recoverable from the branch's own
history forever."* Both unconditional. In fact:

- This repo **squash-merges**. `origin/main` is linear since the migration, and
  the heads of six merged branches (`ci/run-on-pr-only`,
  `docs/ci-hygiene-rules`, `docs/retire-manual-gate`, `fix/bench-barrier`,
  `fix/release-flow`, `chore/pr-reviewer-agents`) are **not ancestors of
  `origin/main`**.
- Under the new rule the ledger commit therefore never enters `main`'s ancestry.
  It survives only while the feature branch ref exists.
- It holds today only because `delete_branch_on_merge` is `false` on the repo
  (checked via the API) and no merged branch has been deleted. One click of
  "Delete branch" leaves the objects reachable only from GitHub's
  `refs/pull/N/head`, which no ordinary `git clone` fetches and which local `gc`
  will drop.

That is a data-loss shape, latent rather than present. It does **not** affect
the thirteen ledgers deleted here — all thirteen are in `main`'s ancestry and
are safe permanently. Fix is one sentence: state that the branch must be
retained, and/or have the lead record the ledger's sha in the `AUDIT.md` entry
(as `memory.rs` already does for `445466b`).

## Minors

- **m1 — "25 citations in `AUDIT.md`" does not reproduce.**
  `git show origin/main:AUDIT.md | grep -o 'REVIEW_[A-Za-z0-9_]*\.md' | wc -l`
  gives **24**, on **22** distinct lines; counting the three bare `REVIEW_*`
  glob mentions too gives **27**. No method yields 25. Small, but this repo has
  been bitten repeatedly by unreproducible figures, and the count appears twice
  (PR body and `AUDIT.md`).
- **m2 — record and PR body disagree.** `AUDIT.md` says the rule is recorded in
  "all four agent files"; the PR body says "all three agent definitions".
  `_shared-context.md`'s own header says it is *"Not an agent"*. Harmless but
  they should agree.
- **m3 — the new rule describes an intention; nothing enforces it.** The lead
  must remember to `git rm`. The recurrence this PR fixes came from a rule that
  was followed exactly as written, so relying on a second unenforced rule is a
  weak guard. A three-line job (fail the PR if any `REVIEW_*.md` is present in
  the head tree) would make it a gate rather than a note — that would need to
  land under `ci-reviewer`, not here.
- **nit** — the three agent files use passive voice ("it *is* deleted from the
  branch before merge") where `CLAUDE.md` names the lead. Working rule 4 makes
  misreading unlikely, but "the lead deletes it" would be unambiguous.
- **nit** — the appended paragraphs turn `_shared-context.md`'s ordered list
  into a loose list (extra vertical spacing on rules 1-6). Cosmetic.

## On the rule itself, as its reflexive test case

The rule is right. A ledger is working state; its findings are reproduced in the
PR description and the `AUDIT.md` entry, which are the durable record. Splitting
"commit for crash recovery" from "retain on `main`" is the correct cut, and
rules 1-3 survive intact. My only substantive objection is M2: the guarantee
that makes deletion safe is asserted rather than secured.

## Not verified

- `cargo build --release` was not run separately; `cargo clippy --all-targets`
  type-checks the same code and was clean.
- The five CI checks were not run by me.
- `REVIEW_PR16.md` on `docs/live-test` was not reviewed; the PR's statement that
  it is still in flight is correct (PR #16 is open).

## Actionable

**Yes.** M1 (the false Verification sentence + the two dangling references) and
M2 (state the branch-retention precondition) should be fixed on this branch
before merge, while LEDGER-01 is still editable in place. m1 and m2 are cheap
corrections to the same paragraphs.
