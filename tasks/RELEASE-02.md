# RELEASE-02 — Cut and publish v0.1.3, the first-ever GitHub Release

**Status:** Completed

Two months of unreleased fixes (#44/NET-07 fair-lock starvation, #41/NET-08 stale-session-id accounting, #53/NET-10 job-identity edge case) were sitting on `main` since `v0.1.2` (tagged 2026-08-09, never published). Researched signing practice among comparable open-source CLI projects first — xmrig and several small Rust tools ship unsigned macOS binaries as standard practice; decided not to sign (no change to `RELEASING.md`'s existing ad-hoc-signing guidance). Bumped to `0.1.3` (patch, no breaking changes) via PR #60, ran the full local pre-release verification (`make verify-jit` 92/92, full test suite, clippy, audit — all clean), then executed `RELEASING.md`'s documented flow end to end for the first time ever: `make dist`, `make release` (tag `v0.1.3` at `990a9b1`), `release.yml` fired successfully and created the draft, assets uploaded, release published with notes.

---

*Full record: the `RELEASE-02` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
