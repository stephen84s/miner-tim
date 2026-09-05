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
| A | Re-measure durations from the API; reconcile every figure on the branch | **done — R2-1, R2-2, R2-8** |
| B | Billing claim (`billable` really zero; not over-corrected) | **done — R2-5** |
| C | `cancel-in-progress` reverted in BOTH workflows; comment accurate | **done — R2-6 (pass)** |
| D | Did the corrections introduce anything new that is wrong (esp. `ci.yml` `timeout-minutes` comment) | **done — R2-3, R2-4** |
| E | Safety argument still sound under `strict: true` | **done — R2-7 (pass)** |
| F | Overclaims / omissions; cache-scoping finding honestly scoped | **done — R2-9, R2-10** |

## R2-1 The PR body was never corrected — both Round-1 blockers survive verbatim — **MAJOR**

`gh pr view 8` on the live PR still reads:

| Job | PR body still says | Reality |
|---|---|---|
| `test` | ~19 min | 4.0 min |
| `jit-linux-arm` | ~12 min | 11.7 min |
| `jit-macos` | ~11 min | ~13.8 min |
| `audit` | ~6 min | 0.3 min |
| `lint` | ~2 min | 0.3 min |
| **Total** | **~50 runner-minutes**, "~50 minutes saved per merged PR" | ~30 min |

and, under "Concurrency simplified as a consequence":

> "With no push trigger that case cannot arise, so it is now unconditionally `true`."

That sentence is now **flatly contradicted by the code on the branch**, where
`cancel-in-progress: ${{ github.event_name == 'pull_request' }}` was restored in
both files. A reader of the PR is told the opposite of what merges.

So the fix commit corrected the two blockers in the tree and left them standing
in the artifact a maintainer actually reads. "Fixed in the tree only" is not
fixed — and it means a *fourth* set of figures is now in circulation (PR body
50 / old comments 45 / corrected comments 29.5 / measured ~30), which is the
same defect Round 1 blocked on, one artifact to the left.

**Scoped honestly:** this does not reach `main`'s permanent history.
`gh api repos/stephen84s/miner-tim` gives `squash_merge_commit_message:
COMMIT_MESSAGES` and `merge_commit_message: PR_TITLE`, so neither merge style
copies the PR body into the commit. The damage is to the review record, not to
git history. That is why this is major and not blocking.

## R2-2 `13.4`, `0.2`, `0.2` are not what they are labelled — **MAJOR**

Both workflow comments and `AUDIT.md` state the figures as "mean of 3 completed
runs each". I re-derived every one from the API, over **all** completed
`event=push` runs (the runs this PR deletes), from `started_at`/`completed_at`
on `actions/runs/<id>/jobs`:

| Job | Run IDs | Durations (s) | Mean | Claimed |
|---|---|---|---|---|
| `jit-macos` | 33919683141, 33936008111, 33941437894, 33921568817, 33941945603 | 691, 802, 804, 986, 854 | **827.4 s = 13.79 min** | 13.4 |
| `jit-linux-arm` | same five | 698, 706, 699, 695, 701 | **699.8 s = 11.66 min** | 11.7 ✓ |
| `test` | 33919479186, 33919683137, 33921569019, 33936008172, 33941437757, 33941786081, 33941945621 | 257, 231, 235, 244, 248, 230, 235 | **240.0 s = 4.00 min** | 4.0 ✓ |
| `audit` | same seven | 199\*, 17, 17, 17, 17, 14, 18 | warm mean **16.7 s = 0.28 min** | 0.2 |
| `lint` | same seven | 32, 20, 16, 21, 15, 15, 14 | **19.0 s = 0.32 min** | 0.2 |

\* `33919479186`'s `audit` at 199 s is the cold-cache run; excluded from the
warm mean, and its exclusion is itself unstated — see R2-5.

Honest total: **~30.1 min**, against the stated 29.5. The 2% gap is not the
finding. The provenance is:

- **`jit-macos` 13.4 cannot be a mean of any three of these runs.** The eight
  candidate 3-subsets give 12.8, 13.0, 13.0, 13.7, 14.4 min — none is 13.4.
  13.4 min = 804 s is the **median of Round 1's four-run set**, i.e. verbatim
  Round 1's *"Typical"* column. The number was lifted from the reviewer's table
  and re-labelled with a provenance it does not have.
- **`audit` 0.2 is not obtainable at all.** The three fastest warm runs are
  14/17/17 s → 16 s → 0.27 min → 0.3. Round 1's own table said 0.3. 0.2 is a
  *fresh* understatement invented in the fix commit, not a copy.
- **`lint` 0.2** requires cherry-picking the three fastest (14/15/15 s = 0.24);
  the mean over all seven is 0.32. Round 1 said 0.3.
- **`jit-macos` is quoted to one decimal over a 691–986 s spread** — a 43%
  range on a virtualised macOS runner. "13.4" is false precision; a range
  (11.5–16.4 min) is the only honest form for that job.

This is the shared-context failure mode reproduced *inside the fix for the
finding that named it*: a figure quoted without a reproduction, and a stated
method ("mean of 3 completed runs") that does not produce the stated number.

## R2-3 The corrected number and the corrected benefit are in different currencies — **MAJOR (new, introduced by the fix)**

Both workflow comments now read, in consecutive sentences:

> "...costs a second full pass — measured at **29.5 minutes** of runner time
> across the five jobs ... The repo is public, so those are free minutes: what
> this saves is wall-clock and queue time, not money."

29.5 is a **sum across five jobs that all run concurrently**. `grep -n 'needs:'`
returns no match in either workflow, so nothing serialises them, and the two
workflows fire on the same event simultaneously. The API agrees:
`actions/runs/33941945603/timing` → `run_duration_ms: 1014000` (16.9 min, the
JIT workflow) and `.../33941945621/timing` → `263000` (4.4 min, CI). The
wall-clock the deleted post-merge pass actually occupies is **~17 minutes**,
set by `jit-macos` on the critical path — not 29.5.

Round 1's blocker was that 50 measured nothing. The fix measured something, then
changed the claim's currency to wall-clock while keeping a number that is only
meaningful as consumed runner-minutes. A sum-across-parallel-jobs is the right
unit for the billed minutes the same sentence disavows. Either quote ~17 min of
wall-clock, or quote 29.5 as consumed runner time and drop the wall-clock
framing — not both.

(Secondary: GitHub bills whole minutes per job, so under a runner-minute
convention `audit` and `lint` are 1 min each and the total is ~32. 29.5 is only
right under a raw-elapsed convention. Moot for cost, since billing is zero, but
it is another way "runner time" is the wrong label for the arithmetic done.)

## R2-4 The two workflows now contradict each other about the "~19 minutes" figure — **MINOR (new, introduced by the fix)**

Same commit, `b039673`:

- `ci.yml:183` — "**Took ~19 minutes on GitLab's x86_64 runners**; measures 4.0
  minutes here" — asserts the GitLab attribution as established fact.
- `jit.yml:26-28` — "The \"~19 minutes\" this comment used to quote for the
  interpreter suite **was never measured** — it is 4.0."

One figure, two mutually exclusive claims about it, in the same commit. I traced
the provenance and the surviving assertion is the false half:

- `main:.github/workflows/jit.yml:25` says only "the interpreter suite takes ~19
  minutes" — **no mention of GitLab**. So `AUDIT.md:3818`'s account ("copied
  from a `jit.yml` comment describing GitLab's runner") misdescribes its own
  source.
- `.gitlab-ci.yml.archived` contains no duration for the test job, only
  `timeout: 1h`.
- `CLAUDE.md` CI-02 records that at the time those workflows were written
  "nothing had run on GitHub — runner specs and RAM headroom were **asserted
  from the issue, not tested**."

There is no measurement of 19 minutes anywhere in this repo, on GitLab or
otherwise. The correction **hardened** an unmeasured figure into a factual
assertion in `ci.yml` while correctly calling it unmeasured in `jit.yml`.

## R2-5 The stated billing verification does not establish the billing claim — **MINOR**

The claim is true. The cited method does not show it.

`AUDIT.md`'s new Verification section says "Billing checked via
`actions/workflows/<id>/timing`". I ran that on all three workflow IDs
(350515410 `ci.yml`, 350515411 `jit.yml`, 350528605 `release.yml`) and every one
returns `{"billable":{}}` — an **empty object**, which is evidence of nothing,
not evidence of zero. The entry's phrasing "`billable.total_ms` is zero across
all workflows" names a key that exists at neither level.

The claim is nonetheless correct, established by the *per-run* endpoint:
```
actions/runs/33941945603/timing -> billable.MACOS.total_ms  = 0
                                   billable.UBUNTU.total_ms = 0
actions/runs/33941945621/timing -> billable.UBUNTU.total_ms = 0
```
with `visibility: public`, `private: false`. Round 1 used the per-run endpoint;
the fix wrote down the workflow-level one.

**Not over-corrected in the other direction.** The entry states the condition
("The repository is public, so Actions minutes are free"), says plainly that the
change "frees no billed minutes at all", and names the real benefit without
inflating it. That part is honest.

## R2-6 `cancel-in-progress` genuinely reverted in both files — **PASS**

`git diff main...HEAD` shows the `cancel-in-progress:` line as *context* (no
`+`/`-`) in both `ci.yml` and `jit.yml`; both read
`${{ github.event_name == 'pull_request' }}`, byte-identical to `main`.

`ci.yml:39-42`'s replacement comment is accurate: `workflow_dispatch` on `main`
does set `github.ref` to `refs/heads/main`, the group `ci-${{ github.ref }}` is
therefore shared between two such dispatches, and the dispatch is the
mitigation this PR nominates. No cross-cancellation exists between a PR run
(`refs/pull/N/merge`) and a dispatch on the same branch (`refs/heads/foo`), so
"cancel superseded PR runs only" describes the behaviour correctly.

*Nit:* `jit.yml:53`'s "killing a manual verification of main is the one thing
that must not happen" is rhetorical overstatement in a file whose whole point is
that a wrong-hash JIT defect is the thing that must not happen.

## R2-7 Safety argument unchanged and still sound — **PASS**

The fix commit touched no `name:` field and no `release.yml`. Protection re-read
fresh: `strict: true`, `enforce_admins: true`, `allow_force_pushes: false`, and
the five required contexts still match the five job `name:` fields exactly
(`lint (clippy, x86_64 linux)`, `audit (cargo-audit / RustSec)`, `test (cargo
test --release, x86_64 linux)`, `jit-macos (aarch64 darwin, make verify-jit)`,
`jit-linux-arm (aarch64 linux, scripts/verify-jit.sh)`). Round 1's analysis of
merge/squash/rebase carries over unchanged. Nothing to re-litigate.

## R2-8 In-tree figures are now internally consistent — **PASS**

`grep -rn` across `.github/`, `AUDIT.md`, `CLAUDE.md`, `README.md`: both
workflow comment blocks and the `AUDIT.md` entry carry the identical set
(29.5 / 13.4 / 11.7 / 4.0 / 0.2 / 0.2). Round 1's "45 vs 50 in the same branch"
is resolved *in the tree*. I confirmed the 45 it describes was real
(`git show 18055cf:.github/workflows/ci.yml`). The remaining inconsistency is
the PR body — R2-1.

## R2-9 Cache-scoping finding is honestly scoped — **PASS**, with one coupling the entry misses

Re-listed `actions/caches` myself: still `total_count = 8`, **all eight** at
`ref: refs/heads/main`, 523,216,212 bytes (499.0 MiB). The AUDIT's "~496 MB" is
Round 1's figure carried forward and is within rounding. This PR's own runs
wrote no `refs/pull/8/merge` entry — consistent with exact-key restores, where
`actions/cache` skips the save.

The entry says "Not measured, not fixed here" and "deliberately left
unquantified rather than guessed at". That is the correct treatment and it is
the honest form. **Pass.**

One coupling neither the entry nor R2-2's numbers acknowledge: the 29.5 figure
is measured **on warm, `main`-scoped caches that this very change stops
repopulating**. The one cold-cache data point in the set — `audit` at 199 s
against a warm 17 s, a 12x factor — is the magnitude of what going cold costs,
and it was silently dropped from the mean. The saving and the unquantified cost
are measured in the same units against opposite signs; quoting one to a decimal
place while leaving the other unquantified overstates the net.

*Process nit:* this repo's convention (`CLAUDE.md`, JIT-01) is to file deferred
findings as numbered issues. `gh issue list --state all` shows six, none for
cache scoping. Recorded in `AUDIT.md` only.

## R2-10 `CLAUDE.md`'s task board was not updated — **MINOR (process, still open)**

`git diff main...HEAD --stat` shows four files; `CLAUDE.md` is not among them.
`grep -n 'CI-03' CLAUDE.md` → no match; the board still ends
`| **Pending** | - | **Awaiting User Task** |`.

Operational Protocol step 4 requires the Current Task table to reflect the new
state. Round 1 raised the sibling omission (the missing `AUDIT.md` Verification
section); the fix added that one and not this one — the same protocol step, half
applied.

---

## Round 2 summary

| # | Item | Severity |
|---|---|---|
| R2-1 | PR body never corrected; both Round-1 blockers verbatim, one contradicting the code | **Major** |
| R2-2 | `13.4` / `0.2` / `0.2` mislabelled "mean of 3 runs"; 13.4 lifted from Round 1's median, audit 0.2 unobtainable; false precision over a 691–986 s spread | **Major** |
| R2-3 | 29.5 is a parallel-job sum sold as wall-clock; real wall-clock is ~17 min (new, from the fix) | **Major** |
| R2-4 | `ci.yml` asserts the "~19 min GitLab" figure as fact while `jit.yml` calls it unmeasured; no such measurement exists (new, from the fix) | **Minor** |
| R2-5 | `actions/workflows/<id>/timing` returns `{}`; the cited verification does not establish the (true) billing claim | **Minor** |
| R2-6 | `cancel-in-progress` reverted in both files, comments accurate | **Pass** |
| R2-7 | Safety argument / `strict: true` / contexts unchanged | **Pass** |
| R2-8 | In-tree figures now mutually consistent (45-vs-50 resolved) | **Pass** |
| R2-9 | Cache finding honestly scoped; misses that 29.5 is measured on caches the change stops warming | **Pass** |
| R2-10 | `CLAUDE.md` task board not updated (Protocol step 4) | **Minor** |

**Verdict: not mergeable as it stands.**

Round 1's Blocking 2 (`cancel-in-progress`) is properly fixed — R2-6 verifies it
against `main` byte-for-byte, and the replacement comment explains it correctly.

Round 1's Blocking 1 (the invented numbers) is **not** fixed. It is fixed in two
of the three places it appeared and re-committed with new invention in one of
those two: `jit-macos` 13.4 is Round 1's median wearing a "mean of 3 runs"
label, `audit` 0.2 matches no subset of any measurement, and the PR body still
carries the original 50-minute table unchanged. Then the correction added a
defect of its own (R2-3): it fixed the *currency* of the claim to wall-clock
while keeping a number that is a sum across parallel jobs, so the headline
figure now overstates the thing it purports to measure by ~1.7x — the same
factor, in the same direction, as the error Round 1 caught.

The mechanism remains sound and I would still support the trigger change on its
merits; branch protection carries the safety argument and nothing here can put
bad code on `main`. What blocks is unchanged in kind from Round 1: durable
committed artifacts asserting measurements that were not made.

To clear: correct the PR body (numbers **and** the `cancel-in-progress`
paragraph); quote `jit-macos` as a range (11.5–16.4 min) or a mean over a named,
complete run set; fix `audit`/`lint` to 0.3; either quote ~17 min wall-clock or
drop the wall-clock framing from the 29.5 sum; reconcile `ci.yml`'s
"Took ~19 minutes on GitLab" with `jit.yml`'s "never measured"; cite the
per-run `timing` endpoint for billing; and add CI-03 to the `CLAUDE.md` board.

**Not verified:** I did not run any workflow, dispatch anything, or exercise the
concurrency collision live — R2-6 rests on reading the restored expression and
on Round 1's empirical demonstration of the shared group, not on a fresh
reproduction. I did not measure the cache-scoping cost either; R2-9 accepts the
entry's own refusal to guess and adds only the one cold-cache data point already
present in the run history.

---

## Round 2 — corrections to this ledger

Appended, not amended, per this project's own rule that a correction must be
visible. Two defects in the text above, found on re-reading before sign-off.
Neither changes the verdict.

**C1 — R2-2's subset enumeration was miscounted.** It reads "The eight candidate
3-subsets give 12.8, 13.0, 13.0, 13.7, 14.4 min". There are **ten** subsets
(C(5,3)), and five values were listed for eight claimed subsets. A finding whose
whole charge is fabricated provenance cannot itself ship a miscount. All ten,
over {691, 802, 804, 854, 986} s, mean in minutes:

| Subset (s) | Mean |
|---|---|
| 691, 802, 804 | 12.76 |
| 691, 802, 854 | 13.04 |
| 691, 804, 854 | 13.05 |
| 802, 804, 854 | 13.67 |
| 691, 802, 986 | 13.77 |
| 691, 804, 986 | 13.78 |
| 691, 854, 986 | 14.06 |
| 802, 804, 986 | 14.40 |
| 802, 854, 986 | 14.68 |
| 804, 854, 986 | 14.69 |

Target 13.4 min = 804.0 s. **The conclusion is unchanged and now complete: no
3-subset yields 13.4**; the nearest are 13.05 below and 13.67 above. R2-2 stands
as written apart from the count.

**C2 — R2-3 was over-graded; downgrading MAJOR → MINOR.** The charge was that
the headline "overstates by ~1.7x". Re-reading the comment's actual words —
"measured at **29.5 minutes** of runner time" — 29.5 *is* the right number for
runner time consumed across five jobs. It is correctly labelled. The defect is
narrower than stated: the **next sentence** names wall-clock and queue time as
the benefit, and 29.5 does not measure that (~17 min does). That is a
non-sequitur between two adjacent sentences, not a mismeasurement, and it is a
smaller thing than Round 1's 50-vs-30.

The verdict paragraph's line — "the same factor, in the same direction, as the
error Round 1 caught" — asserted an equivalence the evidence does not support
and is withdrawn. The correct statement is: the fix changed the claim's currency
to wall-clock without changing the number, so a reader who takes the two
sentences together infers a ~17-minute benefit is ~29.5. Worth fixing; not
blocking.

**Effect on the verdict: none.** "Not mergeable" rests on R2-1 and R2-2 alone —
an uncorrected PR body asserting the opposite of the code it ships, and two
committed artifacts stating a provenance ("mean of 3 completed runs each") that
produces neither 13.4 nor 0.2. R2-3 at minor, R2-4, R2-5 and R2-10 are the
remaining cleanup.

---

# Round 3 — verification of the round-1/2 fixes, and a fresh pass

Branch `ci/run-on-pr-only` @ `19d2f78`, against `origin/main` @ `10d5b2e`.
Diff: `ci.yml`, `jit.yml`, `AUDIT.md`, `CLAUDE.md`, `REVIEW_PR8.md`.

**Verdict: mergeable — no blockers, one major, three minors.** The gating
mechanism is sound and now verified empirically rather than by argument, and
every quoted figure reproduces exactly from the API. The one major is a
self-contradiction inside `AUDIT.md`, not a build risk.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Can the gate still go red? | done — R3-P1, R3-P2, scope stated below |
| 2 | Commands/targets mean what the change says | done — R3-P3 |
| 3 | Required-check names match exactly | done — R3-P1 |
| 4 | `paths:` / `if:` / `continue-on-error:` traps | done — R3-P3 (none exist) |
| 5 | Live config vs description (`gh api`) | done — R3-P1 |
| 6 | Platform assumptions (`target-cpu`) | done — untouched by this diff |
| 7 | Resource limits (`RUST_TEST_THREADS=3`) | done — untouched by this diff |
| 8 | Coverage given up | done — R3-P4, R3-3 |
| — | Reproduce the numbers | done — R3-P5 |

## R3-1 `AUDIT.md`'s Verification section contradicts its own entry — **MAJOR**

`AUDIT.md:4059-4062`:

> "Durations measured via `actions/runs/<id>/jobs`, **three completed runs per
> job**. Billing checked via **`actions/workflows/<id>/timing`**."

Both clauses are the exact charges of R2-2 and R2-5, and both are disavowed
**earlier in the same entry**: the table at `AUDIT.md:4005-4012` states n=12-14
(and I reproduced it at that n), and `AUDIT.md:4030-4033` says of the workflow
endpoint that it "returns `{"billable":{}}` and is evidence of nothing — an
earlier revision of this entry cited that endpoint." It still cites it, thirty
lines later. The line-wrap break after "measured via" is the fingerprint: the
endpoint name was edited, the sample-size clause and the billing sentence were
not.

Same shape as R2-1 (corrected in one artifact, left standing in another), third
round running. It is self-contradicting rather than wrong — the correct n and
the correct endpoint are both present in the same entry — which is what keeps it
off blocker. Fix is a two-line edit.

## R3-2 Per-job and wall-clock figures come from different sample epochs — **MINOR**

The per-job table (n=12/14) is the cut through run `33966976457` (2026-09-05
~13:00); the wall-clock figures (n=17/20) are the cut at branch-head time
(20:12). Both are labelled "every completed run to date", so the per-job one was
already five runs stale when `19d2f78` landed. Recomputed at the later cut the
sum is **30.28** against the stated 30.44 — conclusion untouched, which is why
this is minor and not a repeat of R2-2.

## R3-3 The correction cites a review round that does not exist in the ledger — **MINOR**

`AUDIT.md:4023` says "*Corrected after round 3 flagged it*", and `CLAUDE.md:77`
says "Two review rounds". Before this section, `REVIEW_PR8.md` had no Round 3 —
whatever prompted `19d2f78` was never recorded. This round confirms the
correction it refers to does reproduce (R3-P5); `CLAUDE.md`'s CI-03 row now
needs "three".

## R3-4 No `merge_group:` trigger — **MINOR (latent)**

`gh api repos/:owner/:repo/rulesets` → `[]`, classic protection has no merge
queue, `mergeStateStatus` is `CLEAN` not `QUEUED`: no merge queue is live, so
this is not a defect today. But with `push` gone, `pull_request` is the sole
trigger; enabling a merge queue later would mean the five required contexts
never report on `merge_group` and every PR would hang. Worth a line in the
workflow comments next to the `workflow_dispatch` note.

## R3-P1 The gate still gates, on every path — **PASS (empirical)**

Required contexts and the check-run names produced by a **`pull_request`** run
on `19d2f78` are a 1:1 byte match, both directions empty:

```
required not reported: []      reported not required: []
lint (clippy, x86_64 linux) | audit (cargo-audit / RustSec)
test (cargo test --release, x86_64 linux)
jit-macos (aarch64 darwin, make verify-jit)
jit-linux-arm (aarch64 linux, scripts/verify-jit.sh)   — all SUCCESS
```

Paths a commit can reach `main`:

- **Merge / squash / rebase from a PR** — `strict: true`, so head contains the
  base tip and the tested `refs/pull/N/merge` tree is the tree that lands under
  all three methods (`allow_merge_commit`, `allow_squash_merge`,
  `allow_rebase_merge` all true). Checks reported by the PR run.
- **Direct push to `main`** — rejected: required status checks plus
  `enforce_admins: true`, `allow_force_pushes: false`, `allow_deletions: false`.
  The removed trigger cannot produce an unverified commit on `main`, because the
  commit cannot get there.
- **`merge_group`** — not enabled; see R3-4.
- **Fork PR** — `pull_request` runs in the base repo and reports the same five
  contexts; unchanged by this diff (the deleted trigger never covered forks).
- **Tags** — `release.yml` is byte-identical (`git diff` empty) and still
  `push: tags: [v[0-9]*]`.

## R3-P2 `cancel-in-progress` cannot manufacture a green — **PASS (empirical)**

Run `33968902195` was **cancelled** while its `jit-macos` job concluded
`success` at 16.67 min. The run conclusion, and therefore the required check, is
`cancelled` — not success — so branch protection still blocks. The fail-safe
direction is correct.

## R3-P3 No weakening of the gates — **PASS**

Ruby `YAML.load_file` on all three workflows: `continue-on-error` nil for every
job, no job-level `if:`, **zero** steps with a step-level `if:`, no `paths:`
filter, no `|| true` / `set +e` in `.github/`, `scripts/` or the `Makefile`.
`on:` parses to exactly `{pull_request, workflow_dispatch}` for `ci.yml` and
`jit.yml`. `EXPECTED_PASSES=92` is intact, and this PR's own pull-request run
(`33989440550`) logged `92 passed` in **debug and release on both platforms** —
`verify-jit: GATE PASSED on Darwin arm64` and `on Linux aarch64`. The gate fired
on precisely the trigger path this PR makes the only one.

## R3-P4 Coverage given up — accurately stated — **PASS**

`main` gets no runs of its own; with protection as configured nothing can land
there unchecked, and if protection were relaxed nothing would check it — the PR
body says exactly this. `audit` per-merge frequency is unchanged (every merge is
preceded by a PR run); the time-based gap — a new advisory landing between
merges — is real and predates this change, since push runs also only fired at
merges. There is no `schedule:` trigger anywhere; the entry says so. I confirmed
the entry's "no README badge to break": `grep -niE 'badge|shields\.io'` on
`README.md` returns nothing. The cache-scoping cost remains open and
unquantified, which the entry states plainly.

## R3-P5 Every number reproduces — **PASS**

Recomputed from `actions/runs/<id>/jobs` over successful runs through
`33966976457` (per-job) and `33969996500` (wall-clock):

| job | n | mean | median | range | claimed |
|---|---|---|---|---|---|
| `jit-macos` | 12 | 13.94 | 14.29 | 11.20–16.43 | identical |
| `jit-linux-arm` | 12 | 11.65 | 11.65 | 11.58–11.77 | identical |
| `test` | 14 | 4.07 | 4.03 | 3.28–5.43 | identical |
| `audit` | 14 | 0.49 | 0.28 | 0.22–3.32 | identical |
| `lint` | 14 | 0.29 | 0.26 | 0.23–0.53 | identical |
| **sum** | | **30.44** | | | identical |

Wall-clock from `run_started_at`→`updated_at`: JIT **15.01** min (n=17, median
14.52, range 11.72–21.50); CI **4.34** min (n=20, median 4.21, range
3.58–5.78) — both identical to the entry. Billing: `visibility: public`,
per-run `timing` gives `billable.MACOS.total_ms = 0` and
`billable.UBUNTU.total_ms = 0`. After two rounds of unreproducible figures,
this is the headline result: the third set holds exactly, at the stated n, with
mean, median and range each checkable.

## Not verified

- I did **not** re-run the deliberate-drift test on `scripts/verify-jit.sh`.
  `git diff origin/main...HEAD --stat` touches no file under `scripts/` or the
  `Makefile`, so the gate's redness mechanism is outside this diff's blast
  radius; the evidence offered instead is R3-P3 (the assertion is present and
  fired at 92 on this PR's own run). A synthetic-log simulation would have
  proved nothing about this change.
- I did not exercise a push to `main`, a fork PR or a `workflow_dispatch`
  collision live. R3-P1's push and fork limbs rest on the protection config as
  returned by the API, not on a reproduction.
- The Actions cache-scoping cost is still unmeasured, by me and by the entry.

## Round 3 summary

| # | Item | Severity |
|---|---|---|
| R3-1 | `AUDIT.md` Verification section still says "three completed runs per job" and cites `actions/workflows/<id>/timing`, both disavowed earlier in the same entry | **Major** |
| R3-2 | Per-job table (n=12/14) and wall-clock (n=17/20) are different sample cuts, both labelled "every completed run"; 30.44 → 30.28 at the later cut | Minor |
| R3-3 | Entry cites a "round 3" absent from the ledger; `CLAUDE.md` says "two rounds" | Minor |
| R3-4 | No `merge_group:` trigger — latent, no merge queue enabled today | Minor |
| R3-P1 | Gate holds on every path to `main`; contexts byte-match a `pull_request` run | Pass |
| R3-P2 | A cancelled run reports cancelled even with a successful job | Pass |
| R3-P3 | No `continue-on-error`, `|| true`, `paths:` or any `if:`; 92/92 debug+release both platforms | Pass |
| R3-P4 | Coverage loss accurately stated; no README badge; no `schedule:` (pre-existing) | Pass |
| R3-P5 | All figures reproduce exactly, per-job and wall-clock | Pass |

**Mergeable.** R3-1 is the same category that blocked rounds 1 and 2 — a durable
committed artifact asserting a method that was not used — but it is a
self-contradiction with the correct values present in the same entry, and it is
a two-line edit. The call on whether to land first and correct, or correct
first, belongs to the maintainer; nothing here can put an unverified commit on
`main`.
