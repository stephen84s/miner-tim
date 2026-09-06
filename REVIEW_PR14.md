# REVIEW_PR14 — round 1, independent

PR #14, "Make CI own the Release entry as a draft; fix the documented collision"
(`fix/release-flow`, `5c5886d`), reviewed against `origin/main`. Closes #11.

**Verdict: MERGEABLE.** No blockers, no majors. Ten minors/nits below; the one I
would fix before merge is F1, because it leaves a workflow comment asserting the
very defect this PR removes.

## Coverage ledger

| # | Item | Done | Result |
|---|---|---|---|
| 1 | Walk RELEASING.md step by step against `release.yml` | yes | steps 1-8 are sound; four rough edges (F3-F6, F9) |
| 2 | Workflow shell under `set -euo pipefail`; `exit 0`; token/permissions | yes | correct, and simulated all three branches incl. red |
| 3 | Is the draft-visibility claim true? | yes | **true**, confirmed against GitHub's REST docs |
| 4 | Idempotence — can it mask a real failure? | yes | right trade; fails in the safe direction |
| 5 | AUDIT + CLAUDE.md accuracy, issue-numbering convention | yes | convention obeyed; one unsupported claim (F2) |
| 6 | Is the "not executed end to end" limit stated honestly? | yes | yes — nothing claimed as tested that was not |

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
- **Step 1's new sentence.** `main` protection is real: five required contexts,
  `enforce_admins: true`, so a direct push is rejected.

## Findings

**F1 (minor) — `ci.yml`'s header still asserts the defect this PR removes.**
`.github/workflows/ci.yml:16-26` reads, in the present tense: *"`release.yml`
creates the Release entry … and `RELEASING.md` still carries GitLab-era text
that contradicts `release.yml` — it claims 'the CI job, if it runs at all, only
creates an empty entry' and proposes a self-hosted `macos-arm64` runner. … Those
defects … are tracked in GitHub #11, deliberately not fixed in the PR that found
them."* PR #14 closes #11 and deletes both quoted sentences from `RELEASING.md`,
so this comment is false the moment it merges — and it points a future reader at
a closed issue. `CLAUDE.md:127` puts "the workflow comments" explicitly inside
the numbering/accuracy convention's Scope. Two clauses need updating; while
there, `release.yml` now creates the entry **as a draft**.

**F2 (minor) — the AUDIT entry names a trigger the workflow does not have.**
`AUDIT.md`: *"a manual `workflow_dispatch` or a re-run is exactly what an
operator reaches for"*. `release.yml`'s `on:` is `push: tags` only — there is no
`workflow_dispatch`. Only a job re-run (or deleting and re-pushing the tag) can
retrigger it. This is the repo's recurring shape: the entry claims slightly more
than the diff supports. Either add the trigger or drop the words.

**F3 (minor) — step 5's fallback `create` is interactive, and can publish.**
`gh release create vx.y.z --draft --title "MinerTim vx.y.z"` passes no
`--notes`/`--notes-file`/`--generate-notes`, so on a TTY `create.go:322`
(`if !opts.BodyProvided && opts.IO.CanPrompt()`) enters the interactive flow:
Title prompt, a "Release notes" selector that may open `$EDITOR`, "Is this a
prerelease?", then "Submit?". `--draft` only sets that last prompt's *default*
(`if opts.Draft { defaultSubmit = saveAsDraft }`) and the selection then
**overwrites** `opts.Draft`. An operator who takes the first option publishes an
empty release — the exact outcome the draft design exists to prevent. Add
`--notes '...'` (or `-F`) to keep it non-interactive.

**F4 (minor) — the same fallback wants `--verify-tag`.** It is reached precisely
when the workflow "was never triggered", and one cause of that is the tag never
reaching the remote. `gh release create` then *creates* the tag — for a draft,
at publish time, from the default branch. Step 7 would then publish a release
tagged at whatever `main` points to rather than the intended commit.
`--verify-tag` aborts instead.

**F5 (minor) — step 6 has no `--clobber`, so a retry fails.** `upload.go:107`
errors with "asset under the same name already exists" unless `--clobber` is
given. A partial or repeated upload is exactly the situation this rewrite is
meant to make survivable; worth a note beside the command.

**F6 (nit) — `gh run watch` is the weaker half of step 5.** With no run-id it
needs a TTY and lists only *in-progress* runs, erroring "found no in progress
runs to watch" (`watch.go:119`) — likely here, since the job is seconds long and
the operator is switching windows. Without `--exit-status` it also exits 0 on a
failed run. The `gh release view` poll next to it is the reliable one; lead with
it.

**F7 (nit) — "it takes well under a minute" is an unverified duration.**
`release.yml` has never run. Runner queue time is not bounded on the free tier,
and this repo has a history of quoted figures that did not reproduce. The
sentence adds nothing the `gh release view` poll does not already give.

**F8 (nit) — "not a public window" is broader than what is true.** The Release
*entry* is hidden, confirmed. But step 4 pushes the **tag**, which is public
immediately, listed on the Tags page with auto-generated source archives. Three
places (workflow header, `RELEASING.md`, `AUDIT.md`) state the window itself is
not public; the accurate claim is narrower.

**F9 (nit) — `RELEASE_NOTES.md` does not exist** and no step creates it. Carried
over from the old document, so not a regression, but step 7's first form fails
as written.

**F10 (nit) — the existence check would no-op on an empty tag.**
`gh release view ""` views the *latest* release and exits 0, so the workflow
would report success having done nothing. `GITHUB_REF_NAME` is always set for a
tag push, so this is not reachable in practice; noted only because the check's
success is what silences the step.

## On the idempotence trade (checklist item 4)

Right call, and it degrades safely. If `gh release view` fails for any reason
other than "not found" — API error, rate limit, auth — it returns non-zero, the
script falls through to `create`, and a genuine failure there still reddens the
step (simulated above). What it *can* mask is a pre-existing release with wrong
content: realistically a draft left by F3's fallback with a different title, or
a stale draft from an abandoned attempt. The workflow's only product is an empty
draft, so the blast radius is a title and a placeholder body, and the operator
overwrites the body in step 7 anyway. Note that step 8 confirms **assets** only,
not the title — a one-word addition there would close it.

## Honesty of the verification claims (checklist item 6)

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
  lookup exists in v2.40.0 onwards, which is far older than any current image.
- Nothing in CI exercises the release procedure, so the five green checks on this
  PR say nothing about it.
