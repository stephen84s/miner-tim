# Releasing MinerTim

MinerTim targets **macOS on Apple Silicon only**. The `macos-14` CI runner is
Apple Silicon and could in principle build the artifact, but release builds are
deliberately not automated yet (see the note at the end). For now the shipping
x86_64 Linux and cannot build (or cross-compile) the macOS arm64 binary, so the
binary is **built locally on a Mac** and published with the GitHub CLI (`gh`).

> The CI `release` job does **not** create releases in practice (no macOS runner,
> and it can't attach a binary it can't build). The `gh` flow below is the real
> path; the CI job, if it runs at all, only creates an empty entry.

## One-time

- Install and authenticate the GitHub CLI:

  ```bash
  brew install gh
  gh auth login              # authenticate to github.com
  gh auth status             # should show "Logged in to github.com as <you>"
  ```

## Steps

1. **Bump the version** in `Cargo.toml` (`version = "x.y.z"`). The Stratum agent
   string tracks it automatically via `CARGO_PKG_VERSION`; no other edit needed.
   Commit the bump.

2. **Verify** on your Mac:

   ```bash
   make test          # RandomX vectors (JIT path)
   make audit         # dependency advisories
   cargo clippy --all-targets -- -D warnings
   ```

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

5. **Create the release and attach the artifacts** — one command with `gh`:

   ```bash
   gh release create vx.y.z \
     dist/minertim-x.y.z-macos-arm64.tar.gz \
     dist/SHA256SUMS \
     --name "MinerTim vx.y.z" \
     --notes-file RELEASE_NOTES.md      # or --notes "..."
   ```

   `gh release create` targets the existing tag, creates the GitHub Release, and
   uploads the files as downloadable assets in one step. Verify with:

   ```bash
   gh release view vx.y.z               # confirm the assets list shows both files
   ```

## Verifying a download

```bash
shasum -a 256 -c SHA256SUMS
codesign -s - minertim   # optional ad-hoc signing to avoid a Gatekeeper prompt
```

## Fully automated releases (optional, later)

To have CI build **and** attach the macOS binary automatically, register a
**self-hosted macOS runner** tagged `macos-arm64`, then add a tagged CI job that
runs `make dist` and `gh release create` on a `macos-14` runner. Until then, steps 3–5
are done locally on a Mac.
