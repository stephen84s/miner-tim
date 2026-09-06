# Releasing MinerTim

MinerTim targets **macOS on Apple Silicon only**. The binary is **built locally
on a Mac** and attached to a GitHub Release; CI opens the Release, you fill it.

**Who does what.** Pushing a `vx.y.z` tag fires
`.github/workflows/release.yml`, which creates the Release **as a draft**. You
then attach the artifacts and publish it. The split matters: before it, both CI
and this document ran `gh release create` for the same tag, so following these
steps produced a collision (GitHub #11).

A draft is visible only to people with push access, so the window between the
tag landing and the binary being attached is not a public one — which is the
other reason for it. A published release with no assets is worse than no
release.

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
   git log -1 --oneline          # must be the version bump
   ```

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

5. **Wait for CI to create the draft.** The tag push fires the Release workflow;
   it takes well under a minute. Do not skip this — the next step fails if the
   draft does not exist yet.

   ```bash
   gh run list --workflow=release.yml --limit 1   # find the run
   gh run watch <run-id>                          # optional: follow it
   gh release view vx.y.z                         # succeeds once the draft exists
   ```

   `gh release view` resolves draft releases by tag, so it is the reliable
   check. How long this takes has not been measured — the job is a checkout and
   one API call, so expect seconds, but poll rather than assume.

   Note the **tag is public immediately** even though the release is not. The
   draft hides the empty release, not the tag.

   If the workflow failed or was never triggered, create the draft yourself and
   carry on:

   ```bash
   gh release create vx.y.z --draft --verify-tag \
     --title "MinerTim vx.y.z" --notes 'Draft — artifacts pending.'
   ```

   `--notes` is not optional here. Without it `gh` drops into its interactive
   flow, where `--draft` only sets the *default* of the "Submit?" prompt — one
   wrong keystroke publishes an empty release, which is the outcome this whole
   design exists to prevent. `--verify-tag` makes `gh` fail rather than invent a
   tag if you mistype the version.

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
