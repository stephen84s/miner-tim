# Independent review — PR #7 "Protect main; record the six unreviewed commits that prompted it"

Reviewer: independent agent, no prior context on this change.
Branch: `chore/branch-protection` @ `90e19d4`. Base: `main` @ `d621978`.
Diff: `AUDIT.md` (+40), `CLAUDE.md` (+8). No code.
Date of review: 2026-09-05.

## Verdict

**Mergeable once B1 is corrected on this branch.** One blocker, one
scoped-blocker, four minors.

The mechanism is right. Branch protection is configured exactly as the PR table
says, and the five required contexts are string-exact — the highest-stakes
check, and it passes.

The one thing that must not merge as written is **B1**: "CI is green on them" is
false, and it is entering `AUDIT.md`, which is an append-only ledger by this
project's own rule. A false factual claim, made permanent, inside a PR whose
entire subject is that unreviewed documentation commits contained false factual
claims. Fix it on this branch — a follow-up correction commit would itself be a
doc commit of exactly the kind this PR exists to stop shipping unreviewed.

**B2** is blocking only for the one sentence in `CLAUDE.md` step 0; the PR body's
own wording is careful and is not at fault. **B3** and **B4** are minors —
neither defect was introduced by this diff, but both are cheap to fold in while
the branch is open, and B3 in particular is the self-contradiction failure mode
this PR cites as its own motivation.

---

## Findings

### B1 (Blocking, factual) — "CI is green on them" is not true of all six commits

`AUDIT.md` and the PR body both justify leaving the six commits in place partly
on the grounds that "CI is green on them". Checked against the live API:

| commit | lint | audit | test | jit-macos | jit-linux-arm |
|---|---|---|---|---|---|
| `e460643` | ok | ok | ok | ok | ok |
| `bcad873` | ok | ok | ok | ok | ok |
| `966ffda` | ok | ok | ok | ok | ok |
| `7c92e4c` | ok | ok | ok | ok | ok |
| `6414ba1` | ok | ok | ok | **never ran** | **never ran** |
| `d621978` | ok | ok | ok | **in progress** | **in progress** |

- `6414ba1` — the "JIT gate (aarch64)" workflow run (`id=33941786130`) has
  `conclusion: cancelled` and **zero jobs**. This is permanent: that commit has
  no aarch64 verdict and never will. `gh api repos/.../commits/6414ba1/check-runs`
  returns three check-runs, not five.
- `d621978` — the current tip of `main`. Its JIT gate run
  (`created 2026-09-05T03:28:28Z`) was still `in_progress` throughout this
  review. It may well be green by the time this is read; it was not green when
  the claim was written.

The durable half of this finding is `6414ba1`. Four of six are fully green; one
has no aarch64 verdict at all; one was unverified at time of writing.

**Fix:** state it accurately. Something like "CI is green on four of the six;
`6414ba1`'s aarch64 gate was cancelled and never produced a verdict; `d621978`'s
was still running." The argument for not rewriting history survives this
correction intact — it does not depend on all six being green.

### B2 (Blocking — scoped to one sentence in `CLAUDE.md`) — protection does not enforce the thing that lapsed

**Scope first, because the PR body is not at fault here.** The PR body is
careful and accurate: "The real gate is the PR plus the five checks plus the
independent reviewer agent." That correctly separates what protection enforces
from what it does not. The charge is against **`CLAUDE.md` step 0's closing
sentence alone**: "The protection exists so the rule no longer depends on
remembering it." In step 0, "the rule" is the rule the same paragraph just
stated — which includes "have an **independent reviewer agent** examine it
before merge". Protection does not enforce that half. `AUDIT.md`'s "structural,
not a resolution to try harder" leans the same way, though it is less explicit.

What lapsed was **independent review**. What protection enforces is *branch +
PR + five checks*. With `required_approving_review_count: 0` the author can open
a PR and merge it seconds later with no reviewer having looked at it, and every
protection rule is satisfied. The review step — the one the AUDIT entry itself
identifies as "the arrangement that has actually been catching defects" — remains
exactly as dependent on remembering as it was before this PR.

This is not an argument against the 0 (see M1); it is an argument that the claim
is stronger than the mechanism. Two supporting points:

- `gh api repos/stephen84s/miner-tim/rulesets` returns `[]`. Classic branch
  protection is the only layer; there is no second rule enforcing review.
- `enforce_admins: true` prevents an admin *pushing past* the rule. It does not
  protect the rule itself — the same account can disable branch protection via
  the API and push. That makes this a real gate against an absent-minded bypass
  and a speed bump against a deliberate one. Worth saying, since the PR's framing
  is that the rule is now out of the agent's hands.

**Fix:** narrow the claim to what is enforced. "Branch, PR and the five checks no
longer depend on remembering; the reviewer step still does."

### B3 (Minor, contradiction) — step 0 contradicts line 127's "blocks a push"

Item 5 of the review brief. `CLAUDE.md` now says three inconsistent things about
what enforces the JIT gate:

- **step 0** (new): "`main` is protected: direct pushes are rejected, a pull
  request is required, and all five CI checks must pass"
- **step 6** (existing, line 26): "CI enforces this on **every push**
  (`jit-macos` ..., `jit-linux-arm` ...)"
- **Platform coverage table** (lines 94–95): `jit-macos` / `jit-linux-arm`,
  "**every push**"; and line 127: "the gate that used to depend on a human now
  **blocks a push**."

Both workflows trigger on `push: branches: [main]` and a bare `pull_request:`.
The bare `pull_request` trigger includes `synchronize`, so once a PR is open,
every push to its head branch does run all five checks — which is what PR #7's
own rollup shows. **"Every push" is therefore loose, not false**, for the case
that matters, and steps 6 and the Platform coverage rows are defensible as-is.

The line that is now actually wrong is **line 127**: "the gate that used to
depend on a human now **blocks a push**." After step 0 a direct push to `main`
is rejected by protection *before* CI is ever consulted, so CI cannot block it —
no such push can happen. The gate now blocks a **merge**, via the required
contexts on the PR.

Step 0 does not create the error, but it makes it load-bearing and leaves it
uncorrected in the same file, in the same commit. This is precisely the failure
`7c92e4c` was written to fix — "sections whose premise had changed, producing
files that contradicted themselves" — and this PR cites that commit as evidence
for why review matters. Changing the premise and not rewriting the dependent
sections is the same defect one paragraph later.

**Fix:** line 127 — "now blocks a push" becomes "now blocks a merge". Optionally
tighten "every push" in step 6 and the two Platform coverage rows to "every pull
request, and on `main` after merge", which is more precise but not currently
wrong.

### B4 (Minor, incomplete account) — the bootstrap carve-out is stated but not enumerated

Item 4. The list of six (`e460643`, `bcad873`, `966ffda`, `7c92e4c`, `6414ba1`,
`d621978`) is correct as far as it goes: none has an associated PR
(`/commits/{sha}/pulls` returns `[]` for every one), and all six are on `main`.
No extras, and nothing after `d621978`.

But three further commits sit between the GitLab merge `f02950b` and `e460643`,
and they too reached `main` with no PR:

| commit | authored (UTC) | check-runs |
|---|---|---|
| `f6f351e` ci: GitHub Actions workflows, incl. the two arm64 JIT gates | 2026-09-04T20:02Z | none |
| `4cfdf85` ci: bring GitHub Actions workflows onto main for the migration | 2026-09-04T21:03Z | none |
| `445466b` docs: remap 118 commit references after the SHA-256→SHA-1 conversion | 2026-09-04T21:05Z | 5 — **jit-macos: failure** |

The repository was created at **2026-09-04T20:26Z**. The evidence says all three
went up in the single bootstrap push: a `push` event runs workflows only for the
head SHA, which exactly explains why `f6f351e` and `4cfdf85` have no runs and
`445466b` has all five. So `445466b` was mechanically the head of the import
push the PR carves out as "unavoidable when bootstrapping".

The PR body **does** state the carve-out — "the initial push to `main` was
unavoidable when bootstrapping the repo". What it never does is say *which
commits it covers*, so a reader cannot tell whether the count is six or nine, and
`AUDIT.md` — the durable record — omits the carve-out sentence entirely. And two
of the three are not imported history:
`4cfdf85` and `445466b` were authored **37 and 39 minutes after the repository
already existed** — new work, pushed straight to `main`.

The sharp detail: **`445466b`'s `jit-macos` concluded `failure`.** The bootstrap
push left `main` red on the aarch64 gate, and `e460643` — the first of the
acknowledged six — was the fix for it. That undercuts the PR's implicit framing
that the migration-era direct pushes were the harmless part.

**Fix:** enumerate the boundary in `AUDIT.md`. One sentence: "`f6f351e`, `4cfdf85`
and `445466b` went up in the bootstrap push and are excluded on that basis,
though the last two were authored after the repo existed and `445466b` left
`main` red on `jit-macos`."

---

### M1 (Minor) — 0 approvals is defensible, and the PR understates its own gate

Item 2. The reasoning holds. GitHub does not permit a user to approve their own
pull request, so on a solo-maintainer repository with `enforce_admins: true`, a
`required_approving_review_count` of 1 would block every merge outright. That is
correct as stated.

What 0 leaves is not nothing, and the PR's table omits the reason:

- **`required_conversation_resolution: true`** is set in the live config and is a
  top-level setting, independent of the review block. An unresolved review thread
  blocks the merge button. This is a genuine reviewer lever that works at 0
  approvals, and the PR does not mention it — the PR **understates** its own gate.
- `dismiss_stale_reviews: true` and `require_last_push_approval: false` are both
  configured but are no-ops at count 0. Harmless; worth not citing as gates.

**Fix (optional):** add `required_conversation_resolution: true` to the PR table
and to the AUDIT entry's settings list, and cite it as the mechanism that makes 0
non-vacuous.

### M2 (Minor) — the push-refusal test is presented as stronger evidence than it is

Both the PR body and `AUDIT.md` say the settings were "verified by attempting a
direct push and being refused ... **rather than** by reading the configuration
back", framing the push test as the superior method.

A rejected push proves two things: a PR is required, and `enforce_admins` binds
the pusher. It proves nothing about the five required contexts, `strict`,
`allow_force_pushes` or `allow_deletions` — which are four of the six rows in the
PR's own table, and which are only observable by reading the configuration back.
The two methods are complementary, not ranked.

(This review could not reproduce the push refusal — pushing is outside its
constraints. The configuration read is in O1 below and every row checks out.)

### M3 (Minor) — `CLAUDE.md`'s Current Task Board is not updated

The file's own Operational Protocol step 4 requires the `Current Task` table to
be updated to reflect new state after each implementation batch. The diff adds an
`AUDIT.md` entry keyed `PROC-01` and adds step 0, but adds no `PROC-01` row to the
task board, which still ends at `MIGRATE-01` / `Pending — Awaiting User Task`.
A PR about restoring lapsed process should observe the file's own process.

---

## Observations (not defects in this diff)

### O1 — every row of the PR's protection table verified against the live API

`gh api repos/stephen84s/miner-tim/branches/main/protection`:

| PR claim | live value | |
|---|---|---|
| Pull request required | `required_pull_request_reviews` present | ok |
| Required checks: all 5 | 5 contexts, listed below | ok |
| Branch must be up to date | `strict: true` | ok |
| Applies to admins | `enforce_admins.enabled: true` | ok |
| Force push blocked | `allow_force_pushes.enabled: false` | ok |
| Deletion blocked | `allow_deletions.enabled: false` | ok |
| Approvals required | `required_approving_review_count: 0` | ok |

No discrepancies. `rulesets` is `[]`, so classic protection is the only layer and
there is no bypass list. `restrictions` is absent (no push allowlist),
`lock_branch: false`, `block_creations: false`, `required_signatures: false`,
`required_linear_history: false`.

### O2 — the five required contexts are exact; no typo

This was the brief's "worst outcome" check. Configured contexts vs. the actual
check-run names on the PR head `90e19d4`:

```
lint (clippy, x86_64 linux)                        == lint (clippy, x86_64 linux)
audit (cargo-audit / RustSec)                      == audit (cargo-audit / RustSec)
test (cargo test --release, x86_64 linux)          == test (cargo test --release, x86_64 linux)
jit-macos (aarch64 darwin, make verify-jit)        == jit-macos (aarch64 darwin, make verify-jit)
jit-linux-arm (aarch64 linux, scripts/verify-jit.sh) == jit-linux-arm (aarch64 linux, scripts/verify-jit.sh)
```

All five string-match, and each is pinned to `app_id: 15368` (GitHub Actions), so
the context cannot be satisfied by a status posted by a different app. They match
the `name:` fields in `.github/workflows/ci.yml` and `jit.yml`. The PR table's
shorthand (`lint` for `lint (clippy, x86_64 linux)`) is cosmetic, not a mismatch.
PR #7's own rollup shows all five present and running. Nothing here blocks
forever.

### O3 — a `main` CI verdict was silently cancelled, which both workflows assert cannot happen

The mechanism behind B1. `6414ba1`'s JIT gate run was cancelled at
`2026-09-05T03:28:29Z`, **one second after** `d621978`'s run was created at
`03:28:28Z`, with zero jobs having run. The `CI` run from the same push was not
cancelled — different concurrency group.

Both workflow files carry a comment asserting exactly this is prevented:

```yaml
# Cancel superseded PR runs, but never a run for a commit already on main:
# a rapid second push must not silently cancel the verdict for the first.
cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```

Empirically the guard did not hold. **This review did not determine the
mechanism** — queued-runner supersession and expression evaluation are both
plausible and neither was confirmed; do not act on a guess. It is out of scope
for this diff, but it is the reason `6414ba1` has no aarch64 verdict, so it is
load-bearing for B1 and worth an issue.

Forward-looking context, not a defect in PR #7: the sibling branch
`ci/run-on-pr-only` (`18055cf`, not merged) removes `push: branches: [main]` from
both workflows. If merged, `main` would have no post-merge verdict at all — which
interacts with B3's wording and with O3's cancellation, and should be considered
together with them rather than separately.

### O4 — what this PR does fix, and it is real

Worth recording plainly, since the findings above are all about the prose. Before
this change, nothing prevented a direct push to `main`; now `enforce_admins: true`
plus a required PR plus five required contexts plus `strict: true` means the exact
sequence that produced the six commits cannot recur without a deliberate act of
disabling the rule. Force-push and deletion are blocked. The decision not to
rewrite the six commits is correct and well argued. The PR is itself the first
exercise of the flow it establishes.

---

## Summary

| # | Severity | Finding |
|---|---|---|
| B1 | Blocking | "CI is green on them" false — `6414ba1` has no aarch64 verdict (run cancelled, 0 jobs); `d621978`'s was still running |
| B2 | Blocking (scoped) | `CLAUDE.md` step 0's "no longer depends on remembering it" covers the reviewer agent, which protection does not enforce. The PR body's own wording is correct and not at fault |
| B3 | Minor | Line 127's "blocks a push" is now impossible for `main` — protection rejects the push before CI runs. ("Every push" elsewhere is loose but defensible: the bare `pull_request` trigger covers head-branch pushes) |
| B4 | Minor | The bootstrap carve-out is stated but never enumerated, and `AUDIT.md` omits it; `4cfdf85`/`445466b` were authored after repo creation and `445466b` left `main` red on `jit-macos` |
| M1 | Minor | 0 approvals is defensible, but the table omits `required_conversation_resolution: true`, the setting that makes it non-vacuous |
| M2 | Minor | The push-refusal test is presented as superior to reading the config; it proves only 2 of the 6 table rows |
| M3 | Minor | No `PROC-01` row added to the Current Task Board, contrary to the file's own step 4 |

**B1 must be fixed before merge.** B2 is a one-sentence edit to `CLAUDE.md` step
0. B3, B4, M1–M3 are corrections worth folding in while the branch is open, none
of them introduced by this diff. Every one is a text edit inside the two files
already in the diff.

---

# Round 2 — re-review of the corrections

Reviewer: second independent agent, spawned cold, no prior context and no part
in writing either the change or the round-1 ledger.
Branch: `chore/branch-protection` @ `6ef8921`. Base: `main` @ `d621978`.
Correction commit under review: `6ef8921` "docs: correct PR #7 per independent
review — the 'CI is green' claim was false" (`AUDIT.md` +35/-3, `CLAUDE.md`
+11/-3). Date: 2026-09-05.

Brief: verify each of round 1's seven findings is *actually* fixed and fixed
correctly, and — the higher-value half — check what the corrections introduced.

## Verdict (round 2)

**Not mergeable as it stands.** One blocking finding (**F1**) and one major
(**F2**); four minors and a nit.

Five of the seven round-1 findings are genuinely and correctly fixed in the two
files. But:

- **F1** — the sentence the correction edited contains a *second* false claim
  that was left standing. "The six commits ... they are all documentation" is
  false: `e460643` and `bcad873` changed CI workflows and the `Makefile`, and
  one of them added a whole release workflow. The fix reached into that clause,
  removed "CI is green on them", and left the adjacent falsehood — into an
  append-only ledger, in the entry whose subject is unreviewed commits carrying
  false claims. Round 1 called that exact shape blocking; so does this round.
- **F2** — **nothing was corrected in the PR body.** Three of round 1's findings
  named it explicitly. It still says "They are all documentation, CI is green on
  them" — the precise sentence B1 flagged as blocking — still frames the push
  test as superior to reading the config (M2), and its table still omits
  `required_conversation_resolution` (M1). The PR description is what a human
  reads at merge time, and it is the artifact GitHub keeps.

---

## F1 (Blocking, factual) — "they are all documentation" is false, in the sentence the correction edited

`AUDIT.md`, PROC-01, after the fix:

> The six commits are left in place: they are all documentation, and rewriting
> published history to make the process look observed would be worse than the
> lapse it concealed.

Verified with `git show --stat` on each of the six:

| commit | subject prefix | files changed |
|---|---|---|
| `e460643` | **`ci:`** | `.github/workflows/jit.yml` (+13) |
| `bcad873` | `docs:` | **`.github/workflows/release.yml` (new, +39)**, `.gitlab-ci.yml` → `.gitlab-ci.yml.archived`, **`Makefile`**, AUDIT/CLAUDE/README/RELEASING |
| `966ffda` | `docs:` | AUDIT.md, NEON_FP_PORT_NOTES.md |
| `7c92e4c` | `docs:` | AUDIT.md, CLAUDE.md, README.md |
| `6414ba1` | `docs:` | CLAUDE.md, README.md |
| `d621978` | `docs:` | AUDIT.md |

Two of the six are executable automation, not prose:

- **`e460643`** sets `RUSTFLAGS: -C target-cpu=apple-m1` on the `jit-macos` job.
  That is a change to the JIT gate's own build configuration — the verification
  apparatus this project treats as its highest-stakes artifact — pushed to
  `main` with no branch, no PR and no review.
- **`bcad873`** *adds* `.github/workflows/release.yml`, 39 lines that publish a
  GitHub Release on any `v*` tag, and edits the `Makefile`. A release pipeline
  reaching `main` unreviewed is the substance of the risk, not a pedantic
  exception to a rounding-off phrase.

The claim is not decorative: its function in the paragraph is to justify leaving
the six unreviewed and unrewritten. "All documentation" is the load-bearing half
of "harmless". And the ledger contradicts itself two paragraphs apart — the same
entry lists `e460643`, whose own subject line begins `ci:`.

This is the failure shape the shared context names: a correction that edits one
clause and leaves the defect beside it. The correction deleted five words from
this sentence and read past the four in front of them.

**Fix:** say what is true. "Four are documentation; `e460643` and `bcad873` also
changed CI workflows and the `Makefile` — including a new release workflow —
which makes the lapse worse, not better." The argument against rewriting
published history survives the correction unchanged.

## F2 (Major) — the PR body was never corrected; it still carries the blocking claim

`gh api repos/stephen84s/miner-tim/pulls/7 --jq .body`, read today at head
`6ef8921`:

- line 40: *"They are all documentation, CI is green on them, and rewriting
  published history ..."* — **both** F1's falsehood and round 1's B1 falsehood,
  verbatim, unchanged.
- line 24: *"Verified by attempting a direct push and being refused, **rather
  than** by reading the configuration back"* — the framing M2 flagged. `AUDIT.md`
  was corrected; the body was not.
- the settings table still lists six rows and omits
  `required_conversation_resolution: true` — M1's subject, which the finding
  asked to be added "to the PR table and to the AUDIT entry's settings list".

Round 1's B1 opened with "`AUDIT.md` **and the PR body** both justify leaving the
six commits in place partly on the grounds that 'CI is green on them'". M2 opened
with "**Both the PR body** and `AUDIT.md` say...". M1 asked for a table row.
Three findings, three explicit mentions of the PR body, zero corrections there.

Why it was missed is legible and worth stating: the corrections were made as a
commit, and a commit cannot edit a PR description. The remedy is `gh pr edit`,
not another commit — which also makes it cheap.

Why it matters beyond tidiness: the PR body is the artifact a reader sees at
merge time and the one GitHub retains against the merge commit. Merging as-is
publishes, permanently and in the project's own PR record, the claim a review
round classified as blocking — in the PR whose entire subject is that unreviewed
documentation carried false claims.

**Fix:** edit the PR body: correct "all documentation, CI is green on them" to
match the corrected AUDIT text, drop the "rather than" ranking of the push test,
and add the `required_conversation_resolution` row.
