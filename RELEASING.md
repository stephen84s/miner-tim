# Releasing MinerTim

MinerTim targets **macOS on Apple Silicon only**. GitLab's shared CI runners are
x86_64 Linux and cannot build (or cross-compile) the macOS arm64 binary, so the
binary is built **locally on a Mac** and attached to a GitLab Release. CI creates
the Release entry and runs lint/audit/tests on the tag.

## One-time

- A GitLab **personal access token** with `api` scope (for uploading the binary),
  if you attach assets from the command line rather than the web UI.

## Steps

1. **Bump the version** in `Cargo.toml` (`version = "x.y.z"`). The Stratum agent
   string tracks it automatically via `CARGO_PKG_VERSION`; no other edit needed.

2. **Verify** on your Mac:

   ```bash
   make test          # RandomX vectors (JIT path)
   make audit         # dependency advisories
   cargo clippy --all-targets -- -D warnings
   ```

3. **Build the portable artifact.** This uses `target-cpu=apple-m1` (not the
   local `native`), so it runs on every Apple Silicon Mac:

   ```bash
   make dist
   # → dist/minertim-x.y.z-macos-arm64.tar.gz
   # → dist/SHA256SUMS
   ```

4. **Tag and push.** This triggers the CI `release` job, which creates the GitLab
   Release for the tag:

   ```bash
   make release       # tags vx.y.z and pushes it
   ```

5. **Attach the binary** to the release. Either:
   - **Web UI:** project → Deploy → Releases → edit `vx.y.z` → attach
     `dist/minertim-x.y.z-macos-arm64.tar.gz` and `dist/SHA256SUMS`; or
   - **API** (replace `TOKEN`, `PROJECT_ID`, `x.y.z`):

     ```bash
     # Upload to the project's package registry
     curl --header "PRIVATE-TOKEN: TOKEN" \
       --upload-file dist/minertim-x.y.z-macos-arm64.tar.gz \
       "https://gitlab.com/api/v4/projects/PROJECT_ID/packages/generic/minertim/x.y.z/minertim-x.y.z-macos-arm64.tar.gz"

     # Link it as a release asset
     curl --request POST --header "PRIVATE-TOKEN: TOKEN" \
       --data name="minertim-x.y.z-macos-arm64.tar.gz" \
       --data url="https://gitlab.com/api/v4/projects/PROJECT_ID/packages/generic/minertim/x.y.z/minertim-x.y.z-macos-arm64.tar.gz" \
       "https://gitlab.com/api/v4/projects/PROJECT_ID/releases/vx.y.z/assets/links"
     ```

## Verifying a download

```bash
shasum -a 256 -c SHA256SUMS
codesign -s - minertim   # optional ad-hoc signing to avoid a Gatekeeper prompt
```

## Fully automated releases (optional, later)

To have CI build **and** attach the macOS binary automatically, register a
**self-hosted macOS runner** (see the discussion in the project history) tagged
`macos-arm64`, then add a tagged CI job that runs `make dist` and uploads the
artifact. Until then, step 3/5 are done manually on a Mac.
