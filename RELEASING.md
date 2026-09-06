# Releasing MinerTim

MinerTim targets **macOS on Apple Silicon only**. The binary is **built locally
on a Mac** and attached to a GitHub Release; CI opens the Release, you fill it.

**Who does what.** Pushing a `vx.y.z` tag fires
`.github/workflows/release.yml`, which creates the Release **as a draft**. You
then attach the artifacts and publish it. The split matters: before it, both CI
and this document ran `gh release create` for the same tag, so following these
steps produced a collision (GitHub #11).

A draft is visible only to people with push access, which is the other reason
for it: a published release with no assets is worse than no release, because
someone can find it and download nothing. Be precise about what that hides,
though — **the tag is public the moment you push it.** The draft keeps the
*empty release* out of view, not the tag.

**Why the build is local.** CI runners can build it — `macos-14` is Apple
Silicon — but automating a release build needs a decision about reproducibility
and about whether an unsigned CI-built binary should carry the project's name.
Until that is made, `make dist` runs on your Mac. (On GitLab this was forced
rather than chosen: those runners were x86_64 Linux and could not produce an
aarch64-apple-darwin binary at all.)

## One-time

```bash
brew install gh
gh auth login              # authenticate to github.com
gh auth status             # should show "Logged in to github.com as <you>"
```

## Steps

1. **Bump the version** in `Cargo.toml` (`version = "x.y.z"`). The Stratum agent
   string tracks it automatically via `CARGO_PKG_VERSION`; no other edit needed.
   Commit the bump **through a pull request** — `main` is protected and rejects
   direct pushes.

   **Then merge it and come back to `main`.** `make release` tags whatever
   `HEAD` is, and this repo squash-merges, so tagging from the PR branch would
   tag a commit that never lands on `main`:

   ```bash
   git checkout main && git pull --ff-only
   grep '^version' Cargo.toml    # must be the version you are releasing
   ```

   Check `Cargo.toml`, not the commit subject: the repo squash-merges, so the
   subject is the PR title rather than "bump version", and any later merge
   displaces it anyway. `make release` derives the tag from this exact line
   (`Makefile`'s `VERSION :=`), so it is the invariant that matters — and note
   `make release` tags whatever `HEAD` is, with nothing but that comment
   stopping you tagging the wrong commit.

2. **Verify** on your Mac:

   ```bash
   make verify-jit    # the aarch64 JIT gate: 92 tests, debug AND release
   make test          # the rest of the suite
   make audit         # dependency advisories
   cargo clippy --all-targets -- -D warnings
   ```

   `make verify-jit` is the one that matters for a release: it is the only check
   that exercises emitted ARM64, and a JIT defect does not crash — it silently
   produces wrong hashes that the pool rejects.

3. **Build the portable artifact.** This uses `target-cpu=apple-m1` (not the
   local `native`), so it runs on every Apple Silicon Mac (M1 and newer):

   ```bash
   make dist
   # → dist/minertim-x.y.z-macos-arm64.tar.gz
   # → dist/SHA256SUMS
   ```

4. **Tag and push:**

   ```bash
   make release       # tags vx.y.z and pushes it
   # (or: git tag -a vx.y.z -m "MinerTim vx.y.z" && git push origin vx.y.z)
   ```

5. **Wait for CI to create the draft.** The tag push fires the Release
   workflow. Do not skip this — the next step fails if the draft does not exist
   yet.

   ```bash
   gh run list --workflow=release.yml --limit 1   # find the run
   gh run watch <run-id>                          # optional: follow it
   gh release view vx.y.z                         # succeeds once the draft exists
   ```

   `gh release view` resolves draft releases by tag, so it is the reliable
   check. How long it takes has never been measured — the job is a checkout and
   one API call, so seconds is the expectation, but poll rather than assume.

   If the workflow failed or was never triggered, create the draft yourself and
   carry on:

   ```bash
   gh release create vx.y.z --draft --verify-tag \
     --title "MinerTim vx.y.z" --notes 'Draft — artifacts pending.'
   ```

   `--notes` is not optional here. Without it `gh` drops into its interactive
   flow, where `--draft` only sets the *default* of the "Submit?" prompt — one
   wrong keystroke publishes an empty release, which is the outcome this whole
   design exists to prevent. `--verify-tag` makes `gh` fail rather than create a
   tag of its own if the one you name is not on the remote — a mistyped version
   is one way to hit that, but the likelier one is step 4 not having pushed. If
   it fails, check `git ls-remote --tags origin` and push the tag before
   retrying, rather than letting `gh` invent it.

6. **Attach the artifacts** to the draft — `upload`, not `create`:

   ```bash
   gh release upload vx.y.z --clobber \
     dist/minertim-x.y.z-macos-arm64.tar.gz \
     dist/SHA256SUMS
   ```

   `--clobber` so a retry after a partial or interrupted upload replaces the
   asset instead of failing on a duplicate name.

7. **Publish**, with your release notes:

   ```bash
   gh release edit vx.y.z --draft=false --notes "Release notes here."
   # or from a file you have written:
   #   gh release edit vx.y.z --draft=false --notes-file /path/to/notes.md
   ```

   There is no `RELEASE_NOTES.md` in this repository; an earlier version of this
   document pointed at one. Write the notes inline or keep them wherever you
   like.

8. **Confirm** the assets are actually there:

   ```bash
   gh release view vx.y.z               # both files must appear under Assets
   ```

## If you abandon a release part-way

The workflow is idempotent: re-running it on an existing tag exits successfully
and leaves the release alone. That is deliberate — a re-run is what you reach
for when something went wrong — but it means **an abandoned draft is never
cleaned up and never complained about.** A draft from a failed attempt sits
there invisibly and a later re-run will not replace it. Delete it yourself:

```bash
gh release list --limit 20            # drafts appear here (you have push access)
gh release delete vx.y.z --yes        # remove the draft
git push --delete origin vx.y.z       # and the tag, if you are abandoning it
```

## Verifying a download

```bash
shasum -a 256 -c SHA256SUMS
codesign -s - minertim   # optional ad-hoc signing to avoid a Gatekeeper prompt
```

## Fully automated releases (optional, later)

To have CI build **and** attach the macOS binary, add a `macos-14` job to
`release.yml` that runs `make dist` and `gh release upload`, then publishes. No
self-hosted runner is needed — `macos-14` is free for public repositories, and
an earlier version of this section asking for a self-hosted `macos-arm64` runner
was left over from GitLab, where no macOS runner existed at any price.

What is actually blocking it is the decision named at the top: reproducibility,
and whether an unsigned CI-built binary should carry the project's name. That is
a judgement call, not a missing runner.
