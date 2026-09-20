# MIGRATE-01 — Migrated to GitHub; aarch64 JIT under CI for the first time.

**Status:** Completed

GitLab could not run it — x86_64 runners, `no_matching_runner` for arm64, then `ci_quota_exceeded` — and no GitLab tier offers macOS at any price. Repo converted SHA-256→SHA-1 (GitHub supports only SHA-1, established from its own protocol advertisement and from REST/GraphQL having no object-format parameter); 181 commits and 3 tags preserved, trees identical bar an accidental gitlink, 118 commit references remapped from a verified 188-entry mapping, `SHA256_TO_SHA1_MAP.txt` committed. Five jobs green: `lint`/`audit`/`test` on `ubuntu-24.04`, **`jit-macos`** (`macos-14`) and **`jit-linux-arm`** (`ubuntu-24.04-arm`), each running the 92-test gate in debug **and** release. The first `macos-14` run caught a latent build failure invisible for the project's life: `-C target-cpu=native` resolves to a feature-poor model on a virtualised runner and trips a `ring` compile-time assertion; fixed with `target-cpu=apple-m1`, matching `make dist`. GitLab archived read-only with a pointer; working copy moved to `code/github/miner-tim`. Its "Closes issues #2 and #4" was wrong twice: the two issues tracking this gap are **#2** and **#6**, and neither was closed when that was written. #2 closed 2026-09-06; #6 closes with the PR that corrected this line.

---

*Full record: the `MIGRATE-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
