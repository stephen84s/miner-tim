# SEC-01 — Dependency vuln scanning.

**Status:** Completed

Added `make audit` + `rust:audit` CI job (cargo-audit / RustSec). Fixed 3 rustls-webpki advisories via 0.103.10→0.103.13. Rest of `.gitlab-ci.yml` noted stale (Android paths).

---

*Full record: the `SEC-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
