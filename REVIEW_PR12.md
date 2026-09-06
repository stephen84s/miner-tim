# Review — PR #12, "Record three CI-hygiene rules in the agent protocol"

Round 1. Independent reviewer, spawned cold. Branch `docs/ci-hygiene-rules`
(`1f68477`), base `main` (`a0473c0`). Documentation only: three bullets in
`CLAUDE.md` step 0, a `PROC-05` entry in `AUDIT.md`, one task-board row.

## Coverage ledger

| # | Item | Result |
|---|---|---|
| 1 | Can the gate go red? | **N/A, stated rather than skipped** — the PR adds no gate, no test, no workflow key, no `if:`/`paths:`. Nothing to break. The adjacent empirical check the PR *does* rely on (cancel-in-progress) was verified from real run data. |
| 2 | Commands/targets exist and mean what is claimed | Done — live workflow trigger keys read directly |
| 3 | Required-check names exact | Done — 5 contexts, string-exact against real check-run names |
| 4 | Path filters / conditional execution | Done — no `paths:`, no `branches:`, no `continue-on-error`; `cancel-in-progress` is conditional and correctly so |
| 5 | Live configuration vs. description | Done — `gh api .../branches/main/protection`, repo visibility, merge methods |
| 6 | Platform assumptions | N/A — no build config touched |
| 7 | Resource limits | N/A — no change to parallelism, datasets or `RUST_TEST_THREADS` |
| 8 | Coverage given up | None — no trigger removed, no matrix narrowed |
| + | Numbers reproduced | Done, at a **larger sample than claimed** (below) |
| + | Rebase resolution lossless | Done — 2 hunks per file, nothing else moved |
| + | Internal consistency with step 6 (PR #10) | Done |
| + | Issue-numbering convention | Done, incl. GitHub markdown render |

## What I verified

**Triggers (rule 1).** `ci.yml` and `jit.yml` both carry exactly
`pull_request:` + `workflow_dispatch:` — no `push`, no `branches:`, no `paths:`.
`release.yml` is `push: tags: v[0-9]*`. The bullet's claim is accurate.

**Branch protection (rule 2).** Live API: `strict: true`,
`required_linear_history: **false**`, `enforce_admins: true`,
`required_approving_review_count: 0`, five contexts matching the job `name:`
strings exactly. `required_linear_history: false` is the direct confirmation of
the PR's load-bearing claim that `strict: true` **is satisfied by a merge
commit**. Independently corroborated: PR #8's commit list contains two
two-parent commits (`3b9f829e`, `b695e4ca`) — it was in fact brought up to date
by merge, as the entry says.

**Cancellation (rule 3).** `concurrency: {ci,jit}-${{ github.ref }}` with
`cancel-in-progress: ${{ github.event_name == 'pull_request' }}`. Confirmed from
real runs, not from the config: JIT run `33997841524` (started 23:08:16) was
cancelled at 23:20:45 by run `33998408078` (started 23:20:26) on the same
branch. Its CI sibling `33997841544` survived, because CI finishes in ~4 min.
"A push cancels the run still in flight" is true, and true specifically of the
JIT gate.

**Numbers.** Re-derived from the Actions API over all 58 successful runs
(the entry's sample was n=12–14):

| job | n | mean (min) | median |
|---|---|---|---|
| jit-macos | 27 | 13.95 | 14.12 |
| jit-linux-arm | 27 | 11.67 | 11.65 |
| test | 31 | 4.05 | 4.07 |
| audit | 31 | 0.36 | 0.27 |
| lint | 31 | 0.30 | 0.30 |
| **sum of means** | | **30.33** | |

JIT-workflow wall-clock: mean 14.82, median 14.60 (n=27). So "~30
runner-minutes", "~15 minutes of wall-clock" and "`jit-macos` the long pole" all
reproduce, at a larger sample. `billable.total_ms` is **0 on 58/58** runs — the
public-repo claim holds.

**Rebase resolution.** `git diff origin/main...HEAD` is 2 hunks in `AUDIT.md`
and 2 in `CLAUDE.md`; nothing else in either file moved, so DOC-02's entry, its
task-board row and PR #10's step-6 rewrite are all intact and unduplicated.
Branch is linear, three commits, no merge commit — the Sequencing note's account
is correct, including "the rebase had to take the later wording of the row".

**Rule (3)'s history claim.** Accurate: `6292e98` wrote "Consolidate commits
before pushing"; `112162b` replaced it with "batch the push, not the commits"
and quotes the user. In-place editing of an unmerged entry is what the
audit-correction rule permits.

**Convention + render.** The new row carries no bare `#N`; the AUDIT entry's
`#8`/`#10` are explicitly prefixed "PR", and `AUDIT.md` is outside the
convention's stated scope. Rendered lines 108–152 through GitHub's `/markdown`
API: `<blockquote>` closes before `<table>` and the PROC-05 row is inside the
table — PR #10's round-3 defect has not recurred.

**PR #12's own compliance with the rules it adds.** Base SHA is `a0473c0` =
current `origin/main`; `rebaseable: true`, `mergeable_state: blocked` only on
pending checks. `lint`, `audit`, `test` green; `jit-macos` and `jit-linux-arm`
still running at the time of writing (run `34007190238`). Rule (2) requires
green on the rebased head before merge — that condition is not yet met and must
be re-checked at merge time.

## Findings

### F1 (minor) — both rationales assume the branch's commits survive into `main`; in practice they do not

`CLAUDE.md` rule (3): "Keep making separate, logical commits ... so the history
stays reviewable and **a single mistake can be reverted on its own**". Rule (2):
"so the branch **keeps a linear history** and the tested tree is exactly the
tree that lands." `AUDIT.md` repeats the first.

All three merge methods are enabled (`allow_merge_commit`, `allow_squash_merge`,
`allow_rebase_merge` all `true`) — nothing is *enforced*. But **uniform practice
discards those commits**: PRs #7/#8/#9/#10 carried 12/13/14/21 commits and each
landed on `main` as a single commit (`c74787c`, `9102bf3`, `10d5b2e`, `a0473c0`).
After merge there is no per-commit history to review and no individual commit to
revert. The prescribed *action* is right and matches the user's clarification;
only the justification overreaches. One scoping clause closes it — the benefit
is real **within the PR's review window**, not in `main`'s history.

### F2 (minor) — the same mechanism is now described in two places in one file

The new bullet ("a push to a branch with **no** open PR runs nothing at all")
and step 6's second paragraph ("a push to a branch with no open PR is checked by
nothing at all", added by PR #10) say the same thing about the same workflows.
They agree today, and both match the live triggers, so this is not a
contradiction. It is the shape DOC-02 was written to clean up: CI-03 left *three*
places asserting "every push", and the next trigger change now has two places to
update instead of one. A cross-reference from one to the other would remove the
drift surface.

### F3 (nit) — "CI runs only where a PR exists" is broader than the workflows

The bullet's heading generalises over all of CI; `release.yml` triggers on
`push:` of `v[0-9]*` tags with no PR involved. The bullet's body correctly scopes
itself to `ci.yml` and `jit.yml`, and the AUDIT entry names `release.yml`
explicitly, so only the heading overreaches.

### F4 (nit) — "against that PR's head" is imprecise

No `actions/checkout` step overrides `ref:`, so a `pull_request` run checks out
`github.sha` = `refs/pull/N/merge` — head merged into base, not head. Under
`strict: true` at merge time head ⊇ base, so merge-tree == head-tree ==
landed-tree and the doc's "the tested tree is exactly the tree that lands"
**does** hold. Phrasing only.

### F5 (nit) — a stray blank line inside PR #10's merged entry

`AUDIT.md` hunk 1 adds a third consecutive blank line before the `DOC-02`
heading (lines 4212–4214) — a rebase artifact, not mentioned anywhere. It is
cosmetic, but it is an unannounced edit inside an entry already on `main`, which
the audit-correction rule says is corrected by appending. Also a trailing blank
line at EOF that other entries do not have.

## Not verified

- I did not run any build, test or `make` target: the PR changes no code, no
  workflow and no script, so there is nothing whose behaviour a local run could
  falsify.
- `jit-macos` / `jit-linux-arm` on this head were still in progress when this
  ledger was written; I confirmed they were queued and running, not green.
- I did not re-derive PR #8 round 3's or PR #10 round 2's own figures from their
  ledgers; I re-measured from the API instead, which is the stronger check.

## Verdict

**MERGEABLE.** No blockers, no majors. One minor worth fixing before merge (F1 —
a normative rule whose stated reason the repository's own merge practice
falsifies), one minor worth a cross-reference (F2), three nits. Merge only once
`jit-macos` and `jit-linux-arm` report green on `1f68477`, which is rule (2)
applied to this PR itself.
