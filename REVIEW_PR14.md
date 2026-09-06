# REVIEW_PR14 — round 1, independent

PR #14, "Make CI own the Release entry as a draft; fix the documented collision"
(`fix/release-flow`, `5c5886d`), reviewed against `origin/main`. Closes #11.

**Verdict: MERGEABLE.** No blockers, no majors. Thirteen minors/nits. The two I
would fix before merge are **F1** (the rewritten steps can tag a commit that is
not on `main`) and **F2** (a workflow comment left asserting the very defect this
PR removes).

## Coverage ledger

Standing infrastructure checklist, then the parent's priorities.

| # | Standing item | Result |
|---|---|---|
| 1 | Can the gate still go red? | Yes — simulated. `gh release create` failing gives `rc=1`. Note this workflow is not a *gate*: it is not a required check and nothing in CI covers the release procedure. |
| 2 | Do the commands exist and mean what is claimed? | Yes — `release upload`, `release edit --draft=false --notes-file`, draft-by-tag resolution all confirmed against `cli/cli` source and `gh --help`. |
| 3 | Required-check names | **N/A** — `release.yml` is tag-triggered and is not among `main`'s five required contexts (verified against the API). |
| 4 | Path filters / conditional execution | **N/A** — no `if:`, no `paths:`, no `continue-on-error:` anywhere in the diff. |
| 5 | Live config vs description | Checked: `gh release list` empty, three tags, none containing `bcad873`; `main` protection real (`enforce_admins: true`, 5 contexts) so step 1's new sentence is accurate. |
| 6 | Platform assumptions | None introduced — the job is `ubuntu-24.04` and runs `gh` only. |
| 7 | Resource limits | **N/A** — no change to parallelism, dataset count or `RUST_TEST_THREADS`. |
| 8 | What coverage is given up? | **Real, and unnamed anywhere in the PR** — see "The cost side of the draft decision" below. |

| # | Parent's priority | Result |
|---|---|---|
| 1 | Walk RELEASING.md step by step | Steps 1-8 work; **F1** is a sequencing hole, plus F5-F8, F12 |
| 2 | The workflow's shell | Correct; simulated all three branches including red |
| 3 | Is "draft" right, and is the claim true? | Claim **true**, confirmed against GitHub's REST docs |
| 4 | The idempotence claim | Right trade; fails in the safe direction — see below |
| 5 | AUDIT / CLAUDE.md accuracy, numbering convention | Convention obeyed; **F3**, **F4** |
| 6 | Honesty about not having executed the flow | Honest — nothing claimed as tested that was not |

## What I verified positively (not just read)

- **Draft visibility (the load-bearing claim).** GitHub REST documentation, list
  releases: *"Information about published releases are available to everyone.
  Only users with push access will receive listings for draft releases."* The
  design's premise holds.
- **`gh` resolves drafts by tag.** `cli/cli` `pkg/cmd/release/shared/fetch.go`,
  `FetchRelease`, runs a published lookup **and** a `fetchDraftRelease` GraphQL
  lookup "by pending tag name". Present in v2.40.0, v2.55.0 and v2.83.2, so it
  predates any `gh` on `ubuntu-24.04`. `release view`, `release upload`
  (`upload.go:85`) and `release edit` (`edit.go:103`) all go through it — so
  step 5's wait condition, step 6's upload and step 7's publish all work against
  a draft, and so does the workflow's own existence check.
- **`gh release edit --draft=false --notes-file` is real** and is gh's own
  documented example (`gh release edit v1.0 --draft=false`).
- **The workflow shell.** I extracted the `run:` block and ran it against a stub
  `gh` on `PATH`:
  - release exists → prints "already exists", `rc=0` (step green, no create);
  - release absent, create succeeds → create invoked, `rc=0`;
  - release absent, **create fails** → `rc=1`. **The step can still go red.**
  A non-zero `gh release view` inside `if` does not trip `set -e`, and `exit 0`
  ends the step successfully. `permissions: contents: write` + `GH_TOKEN` is
  sufficient for creating a draft; `actions/checkout` is load-bearing (gh
  resolves the repo from the git remote). YAML parses; trigger is
  `push: tags: [v[0-9]*]`.
- **`make release` really fires it.** `Makefile:136-137` tags `v$(VERSION)` and
  pushes it; `v0.1.3` matches `v[0-9]*`. `dist` filenames in step 6 match
  `DIST_NAME := minertim-$(VERSION)-macos-arm64` and `dist/SHA256SUMS`.
- **The latency claim.** `gh release list` empty; tags `v0.1.0`-`v0.1.2` exist
  and `git tag --contains bcad873` is empty. The PR's "nobody hit it" reasoning
  is accurate.

## Findings

**F1 (minor, most consequential) — step 1 now requires a PR, and nothing tells
the operator to get back onto `main` before step 4.** Step 1 became *"Commit the
bump **through a pull request** — `main` is protected and rejects direct
pushes."* Steps 2-3 are local. Step 4 is `make release`, which checks only that
`git status --porcelain` is empty and then tags **whatever HEAD is**
(`Makefile:135-137`). Followed literally from the branch the operator just
opened the PR from, it tags the branch head — which, under a squash or rebase
merge, is a commit that never appears on `main`. The release is then built,
tagged and published from an orphaned commit. The old wording ("Commit the
bump") implied a local commit on `main` and did not have this gap; the fix
introduced it. Step 1 needs a closing clause: merge the PR, `git checkout main
&& git pull`, and only then continue. (`make release`'s clean-tree check does
not catch this — a branch head is perfectly clean.)

**F2 (minor) — `ci.yml`'s header, and the `Makefile`, still describe the old
flow.** `.github/workflows/ci.yml:16-26` reads, in the present tense:
*"`release.yml` creates the Release entry … and `RELEASING.md` still carries
GitLab-era text that contradicts `release.yml` — it claims 'the CI job, if it
runs at all, only creates an empty entry' and proposes a self-hosted
`macos-arm64` runner. … Those defects … are tracked in GitHub #11, deliberately
not fixed in the PR that found them."* PR #14 closes #11 and deletes both quoted
sentences, so the comment is false the moment it merges and points a future
reader at a closed issue. Same class, two more places: `Makefile:34`
(`make release  Tag v$(VERSION) and push (triggers the CI release)`) and
`Makefile:132-133` (*"the CI 'release' job then creates the GitHub Release"*) —
CI now creates a **draft** the human must publish. `CLAUDE.md:127` puts "the
`Makefile` … and the workflow comments" explicitly inside the convention's
Scope, so all three are in scope for this PR.

**F3 (minor) — the AUDIT entry names a trigger the workflow does not have.**
*"a manual `workflow_dispatch` or a re-run is exactly what an operator reaches
for"*. `release.yml`'s `on:` is `push: tags` only — there is no
`workflow_dispatch`. Only a job re-run (or deleting and re-pushing the tag) can
retrigger it. This is the repo's recurring shape: the entry claims slightly more
than the diff supports. Either add the trigger or drop the words.

**F4 (minor) — the AUDIT entry's "Files changed" list omits `CLAUDE.md`.** It
names `RELEASING.md`, `.github/workflows/release.yml` and `AUDIT.md`; the diff
is those three **plus `CLAUDE.md` (+1)**, the REL-01 task-board row. Same defect
class as F3, in the one section of the entry whose entire job is matching the
diff.

**F5 (minor) — step 5's fallback `create` is interactive, and can publish.**
`gh release create vx.y.z --draft --title "MinerTim vx.y.z"` passes no
`--notes`/`--notes-file`/`--generate-notes`, so on a TTY `create.go:322`
(`if !opts.BodyProvided && opts.IO.CanPrompt()`) enters the interactive flow:
Title prompt, a "Release notes" selector that may open `$EDITOR`, "Is this a
prerelease?", then "Submit?". `--draft` only sets that last prompt's *default*
(`if opts.Draft { defaultSubmit = saveAsDraft }`) and the selection then
**overwrites** `opts.Draft`. An operator who takes the first option publishes an
empty release — the exact outcome the draft design exists to prevent. Add
`--notes '...'` (or `-F`) to keep it non-interactive.

**F6 (minor) — the same fallback wants `--verify-tag`.** It is reached precisely
when the workflow "was never triggered", and one cause of that is the tag never
reaching the remote. `gh release create` then *creates* the tag — for a draft,
at publish time, from the default branch. Step 7 would then publish a release
tagged at whatever `main` points to rather than the intended commit.
`--verify-tag` aborts instead. (Interacts with F1: both end in a release tagged
at the wrong commit.)

**F7 (minor) — step 6 has no `--clobber`, so a retry fails.** `upload.go:107`
errors with "asset under the same name already exists" unless `--clobber` is
given. A partial or repeated upload is exactly the situation this rewrite is
meant to make survivable; worth a note beside the command.

**F8 (nit) — `gh run watch` is the weaker half of step 5.** With no run-id it
needs a TTY and lists only *in-progress* runs, erroring "found no in progress
runs to watch" (`watch.go:119`) — likely here, since the job is seconds long and
the operator is switching windows. Without `--exit-status` it also exits 0 on a
failed run. The `gh release view` poll next to it is the reliable one; lead with
it.

**F9 (nit) — "it takes well under a minute" is an unverified duration.**
`release.yml` has never run. Runner queue time is not bounded on the free tier,
and this repo has a history of quoted figures that did not reproduce. The
sentence adds nothing the `gh release view` poll does not already give.

**F10 (nit) — "not a public window" is broader than what is true.** The Release
*entry* is hidden, confirmed. But step 4 pushes the **tag**, which is public
immediately, listed on the Tags page with auto-generated source archives. Three
places (workflow header, `RELEASING.md`, `AUDIT.md`) state the window itself is
not public; the accurate claim is narrower.

**F11 (nit) — `RELEASE_NOTES.md` does not exist** and no step creates it.
Carried over from the old document, so not a regression, but step 7's first form
fails as written.

**F12 (nit) — step 8 confirms assets only.** It is the flow's last check and
does not look at the title or notes, which is where F5's fallback and the
idempotence branch can leave something wrong.

**F13 (nit) — the existence check would no-op on an empty tag.**
`gh release view ""` views the *latest* release and exits 0, so the workflow
would report success having done nothing. `GITHUB_REF_NAME` is always set for a
tag push, so this is not reachable in practice; noted only because the check's
success is what silences the step.

## On the idempotence trade (priority 4)

Right call, and it degrades safely. If `gh release view` fails for any reason
other than "not found" — API error, rate limit, auth — it returns non-zero, the
script falls through to `create`, and a genuine failure there still reddens the
step (simulated above). What it *can* mask is a pre-existing release with wrong
content: realistically a draft left by F5's fallback with a different title, or
a stale draft from an abandoned attempt. The workflow's only product is an empty
draft, so the blast radius is a title and a placeholder body, and the operator
overwrites the body in step 7 anyway.

## The cost side of the draft decision (standing item 8)

Named nowhere in the PR, the workflow header, `RELEASING.md` or the AUDIT entry,
and it should be. Under the old design a forgotten follow-through left a
**visible** empty release — ugly, but self-reporting: someone would find it and
say so. Under the new one, an abandoned release after step 4 is **silent**. The
tag is public, the Release entry is not, and nothing — not CI, not the flow, not
a required check — detects a draft that was never published. The argument that
published-empty is worse is defensible; the trade it makes is a
loud-and-embarrassing failure for a quiet one, and that half is unstated.

## Honesty of the verification claims (priority 6)

Stated correctly. The PR body and the AUDIT entry both say the flow was **not**
executed end to end, name what executing it would require (pushing a `v*` tag to
a public repo), and label the work "verified by reading, not by observation".
The only positive claims made — the YAML parses, the shell is `set -euo
pipefail` with an explicit existence check — are true and I reproduced both.
Nothing is claimed as tested that was not. Retiring #11's "inferred, not tested"
item as *moot* rather than resolved is the correct framing: the fixed flow never
issues a second `create`.

## What I did not verify

- The flow end to end. I did not create, modify or delete any tag or release; no
  draft has ever existed on this repo, so every statement about `gh`'s behaviour
  against a real draft comes from `cli/cli`'s source and GitHub's REST
  documentation, not from observation. That residual is the same one the PR
  declares.
- Which `gh` version the `ubuntu-24.04` image ships. I established the draft
  lookup exists in v2.40.0 onwards, far older than any current image.
- Nothing in CI exercises the release procedure, so the five green checks on this
  PR say nothing about it.
