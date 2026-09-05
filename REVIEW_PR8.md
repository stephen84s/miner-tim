# Review — PR #8 "Run CI on pull requests only"

Branch `ci/run-on-pr-only` @ `18055cf`, reviewed against `main`.
Diff: `.github/workflows/ci.yml`, `.github/workflows/jit.yml`, `AUDIT.md` (+61/-9).

**Verdict: request changes.** The mechanism is sound — the central safety
argument holds, the workflows parse, the triggers are exactly as described and
`release.yml` is genuinely untouched. The *justification* is not accurate: the
headline saving is overstated by roughly 1.7x, three mutually inconsistent
figures are being written into permanent code comments and the audit ledger, and
the `cancel-in-progress` change is unsafe by the PR's own reasoning. Nothing
here is a correctness risk to the build; everything here is a durable committed
artifact that will be cited later.

---

## 1. The safety argument — it holds (crux passes)

Verified `strict: true` on `main`:

```
gh api repos/stephen84s/miner-tim/branches/main/protection
  required_status_checks.strict = true
  contexts = [lint …, audit …, test …, jit-macos …, jit-linux-arm …]  (all 5 job names)
  enforce_admins.enabled     = true
  allow_force_pushes.enabled = false
  allow_deletions.enabled    = false
```

The claim "a PR's head already contains the latest `main`, so its run validates
exactly the tree that lands" survives scrutiny:

- **Merge commit** (`allow_merge_commit: true`) — PR checks run on
  `refs/pull/N/merge`, a test merge of head into base. With head up to date that
  merge is fast-forward-equivalent, so the tested tree equals the landed tree.
- **Squash** (`allow_squash_merge: true`) — the squash commit's tree is the same
  merge-result tree. Equal.
- **The two-green-PRs race is genuinely closed.** If PR A is green and up to
  date with `main@X` and PR B merges first, A is now behind; `strict` blocks it,
  the branch must be updated, and the update produces a *new SHA* with no
  reported contexts — so merge stays blocked until all five checks re-run. The
  semantic-conflict window this design would otherwise open does not exist.
- **Rebase merge** (`allow_rebase_merge: true`) replays N commits, of which only
  the tip's tree was tested. This is **not a regression**: `push: branches:
  [main]` fired once per push and tested only the tip, so intermediate commits
  were untested before this PR too. Noted for precision, not as a finding.

**Non-blocking note.** The argument's soundness now rests entirely on repository
configuration that is not in the diff and not version-controlled. Today that
config is robust (`enforce_admins: true`, no force pushes, no deletions). But if
protection is ever relaxed, nothing fails loudly — coverage silently drops to
zero and no signal is emitted. The PR acknowledges this in prose; there is no
detection mechanism, and adding one is out of scope for this diff.

---

## 2. `cancel-in-progress: true` — BLOCKING (by the PR's own logic)

The stated rationale is false as written:

> "With no push trigger that case cannot arise, so it is now unconditionally `true`."

The concurrency group is `ci-${{ github.ref }}` / `jit-${{ github.ref }}`. For
`workflow_dispatch`, `github.ref` is `refs/heads/<branch>` — so **two manual
dispatches of `main` share the group `ci-refs/heads/main`**, and with
`cancel-in-progress: true` the second now silently kills the first. The case
does still arise; only its trigger changed.

This is empirically demonstrable in this repo's own history. Under the *old*
expression, at 21:05:55 a push run on `main@445466b` was in progress; a
`workflow_dispatch` on the same SHA at 21:06:20 queued behind it rather than
cancelling it (the expression evaluates `false` from the incoming run's
context); it was then displaced as *pending* by the next push at 21:08:29 —
runs `33919479186`, `33919511056`, `33919683137`. That sequence proves the
group is shared between `push` and `workflow_dispatch` on `main`.

**State the delta narrowly:** for PR runs the behaviour is unchanged, and a
dispatch on a feature branch (`refs/heads/foo`) does not collide with that
branch's PR run (`refs/pull/N/merge`). The single new cancellation this PR
enables is *dispatch-on-main #2 killing dispatch-on-main #1*.

That is precisely the mechanism the PR nominates as its own mitigation for the
lost `main` coverage — "`workflow_dispatch` stays, so `main` can still be
checked on demand". The change removes the guard from the one path it now
depends on. The old expression cost nothing; it should be restored, or the
group made event-aware.

---

## 3. Triggers and parsing — correct as described

All three files parse (`YAML.load_file`) and the trigger sets are exactly as
claimed. `git ls-tree` confirms the workflow set is unchanged at exactly three
files — no fourth workflow reintroduces a push-to-main trigger.

| File | `on:` | concurrency |
|---|---|---|
| `ci.yml` | `pull_request`, `workflow_dispatch` | `ci-${{ github.ref }}`, cancel `true` |
| `jit.yml` | `pull_request`, `workflow_dispatch` | `jit-${{ github.ref }}`, cancel `true` |
| `release.yml` | `push: tags: ['v[0-9]*']` | none |

The two group prefixes differ (`ci-` / `jit-`), so the workflows do not collide
with each other. Live confirmation: both workflows fired on `pull_request` for
this very branch (runs `33942289290`, `33942289266`).

Required-status-check contexts still match the five job names exactly, so
branch protection remains satisfiable.

---

## 4. `release.yml` — genuinely unaffected

Byte-identical (absent from the diff). It triggers only on `push: tags:
v[0-9]*`, which is orthogonal to the branch-push triggers removed here. It has
no `needs:` on any CI job and never gated on CI passing — before or after this
change — so tag-triggered releases behave identically. Nothing to flag.

---

## 5. The ~50 runner-minute figure — BLOCKING (it is wrong)

Measured from **precisely the runs this PR deletes** — warm-cache
`push`-on-`main` runs, so the comparison is apples-to-apples. Four runs each of
CI (`33941437757`, `33936008172`, `33921569019`, `33919683137`) and JIT
(`33941437894`, `33936008111`, `33921568817`, `33919683141`):

| Job | PR claim | Measured (4 runs) | Typical |
|---|---|---|---|
| `test` | **19 min** | 231 / 235 / 244 / 248 s | **~4.0 min** |
| `jit-linux-arm` | 12 min | 695 / 698 / 699 / 706 s | ~11.6 min |
| `jit-macos` | 11 min | 691 / 802 / 804 / 986 s | ~13.4 min (understated) |
| `audit` | **6 min** | 17 / 17 / 17 / 17 s | **~0.3 min** |
| `lint` | 2 min | 15 / 16 / 20 / 21 s | **~0.3 min** |
| **Total** | **~50 min** | | **~30 min** |

The saving is roughly **30 runner-minutes, not 50** — overstated by ~1.7x. Only
`jit-linux-arm` is accurate; `test`, `audit` and `lint` are out by 5x, 20x and
7x. "Most of it the two JIT gates" is directionally right and actually
*understates* the concentration: the JIT gates are ~25 of the ~30 minutes.

The figures appear to be copied prose rather than measurement — `jit.yml`'s
pre-existing header comment already says "the interpreter suite takes ~19
minutes", which is where the `test` number comes from; it describes a local run,
not this CI job.

**Compounding this: the numbers are internally inconsistent.** The PR body and
`AUDIT.md` say **50 minutes**; the comments committed into both workflow files
say **45 minutes**. Three different numbers, none measured, two of them landing
permanently in the repository.

**What the saving actually buys is unstated.** The repo is public
(`visibility: public`), and the API confirms as fact rather than inference that
every job bills zero:

```
gh api repos/stephen84s/miner-tim/actions/runs/33941437894/timing
  billable.UBUNTU.total_ms = 0
  billable.MACOS.total_ms  = 0     # incl. macos-14 and ubuntu-24.04-arm
```

So the change frees **no billed minutes and no quota**. The real benefit is
wall-clock queue time and concurrency headroom — a legitimate goal, but a
different one from the resource saving the PR argues for.

**Process finding (blocking, and the reason the wrong numbers survived).** The
`AUDIT.md` CI-03 entry has no *verification performed* section. `CLAUDE.md`
requires every entry to record "verification performed (build/tests/runtime
checks)", and the protocol states no implementation is complete until committed
to `AUDIT.md`. Had the durations been checked against the API — a single call —
the 50-minute claim would not have been written down.

---

## 6. Coverage actually lost

The PR names one loss (`main` gets no runs of its own). That is the main one,
and it is correctly stated. Two corrections to the surrounding analysis:

**`cargo audit` drift is a near-non-issue — the PR is right, if by accident.**
Every merge to `main` is necessarily preceded by a PR run that includes `audit`,
so removing the post-merge run barely changes how often advisories are checked.
The genuine gap is that **no workflow has a `schedule:` trigger at all**
(`grep -rn 'schedule\|cron' .github/workflows/` → no matches), so a repo that
goes quiet gets no advisory scanning under either configuration. That gap
predates this PR and is untouched by it. Not a finding against #8; worth its own
issue.

**No status badge exists.** `README.md` contains no
`actions/workflows/*/badge.svg`, so the fact that `main` will never again have a
run of its own breaks no published signal. Closing this out as a non-finding.

**Unmeasured risk the PR does not consider: Actions cache scoping.**
All five jobs use `actions/cache@v4` for `~/.cargo` and `target/`. GitHub scopes
a cache to the ref that *wrote* it; a run may restore from its own ref, its PR
base, and the default branch. PR runs write to `refs/pull/N/merge`, which is
invisible to every other PR and discarded with the PR. Today every cache in the
repo was written by the push-on-`main` runs this PR deletes:

```
gh api repos/stephen84s/miner-tim/actions/caches
  total_count = 8;  all 8 ref = refs/heads/main   (~496 MB:
  target-jit-macos 149MB, target-jit-linux-arm 142MB, audit 83MB,
  target-test 61MB, target-lint 37MB, 3x cargo 8MB)
  last_accessed_at = 2026-09-05T03:35-03:36Z  — i.e. being restored right now
  by this PR's own runs
```

After this change, nothing writes into `refs/heads/main`'s scope again except a
manual dispatch. The existing caches stay warm only while their keys match; once
`Cargo.lock` changes, the exact-key hit is gone for good and every PR falls back
to an increasingly stale `restore-keys` prefix match, rebuilding more each time.
`CACHE_EPOCH: v1` becomes effectively un-bumpable for the same reason — bumping
it would invalidate everything with nothing left to repopulate it.

**The magnitude of this is unmeasured and I am not going to guess at it** — that
would repeat the error found in item 5. The mechanism is real and the cache
listing above proves the current state; whether it materially offsets the ~30
minutes saved per merge is an open question the PR should answer, not assume.
The cheap mitigation, if it does bite, is a narrow scheduled or post-merge
cache-warming run on `main`. **Non-blocking; needs measurement.**

---

## Summary

| # | Item | Severity |
|---|---|---|
| 1 | Safety argument (`strict: true`) — holds for merge-commit and squash; no regression for rebase | **Pass** |
| 2 | `cancel-in-progress: true` — dispatch-on-`main` can now cancel dispatch-on-`main`; rationale false as written | **Blocking** |
| 3 | Triggers and parsing — exactly as described, 3 workflows, contexts still satisfiable | **Pass** |
| 4 | `release.yml` — byte-identical, tag path orthogonal and never CI-gated | **Pass** |
| 5 | ~50 min claim — measures ~30 min; 50 vs 45 inconsistency committed to repo; zero billed minutes; `AUDIT.md` missing required verification section | **Blocking** |
| 6 | Coverage lost — `main` self-coverage (stated); `audit` drift ~nil (non-finding); no badge (non-finding); default-branch cache scope goes cold | **Non-blocking, needs measurement** |

**Is PR #8 mergeable? Not as it stands — request changes.** No defect here can
break a build or let bad code onto `main`; branch protection plus `strict: true`
genuinely carries the safety argument, and I'd support the trigger change on its
merits. What blocks is that the diff commits incorrect and self-inconsistent
figures into two workflow comments and the permanent audit ledger, and relaxes
concurrency on the exact mechanism it nominates as its own safety net. Both are
small edits: correct the durations to the measured ~30 minutes (and reconcile
45 vs 50), restore the `cancel-in-progress` guard, and add the verification
section `CLAUDE.md` requires. Then it should go in.

---

# Round 2 — re-review of the corrections (`b039673`)

Branch `ci/run-on-pr-only` @ `b039673`, reviewed against `main`. The fix commit
under review is `b039673 "ci: correct PR #8 per independent review — wrong
numbers, unsafe cancellation"`. Round 1's two blockers are the subject.

**Coverage ledger** (updated as I go):

| # | Check | State |
|---|---|---|
| A | Re-measure durations from the API; reconcile every figure on the branch | in progress |
| B | Billing claim (`billable` really zero; not over-corrected) | pending |
| C | `cancel-in-progress` reverted in BOTH workflows; comment accurate | pending |
| D | Did the corrections introduce anything new that is wrong (esp. `ci.yml` `timeout-minutes` comment) | pending |
| E | Safety argument still sound under `strict: true` | pending |
| F | Overclaims / omissions; cache-scoping finding honestly scoped | pending |

