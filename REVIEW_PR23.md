# REVIEW_PR23 — round 1, independent review of PR #23 (`security/bound-recv-buffer`)

Reviewer: cold `pr-reviewer`, 2026-09-17. Base `origin/main` = `f2abc1e`; head `06e240f`.
Diff: `AUDIT.md`, `CLAUDE.md`, `src/pool_connection.rs` (+308 / -9).

## Scope — nothing to hand off

No `src/randomx/jit/`, no emitter, no `vm.rs` native-loop path, no `benches/`, no
speed or hashrate claim → **not** `jit-reviewer`. No `.github/workflows/`, no
`Makefile`, no `scripts/`, no `.cargo/config.toml` → **not** `ci-reviewer`.
The whole diff is mine.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Correctness of the bound itself | done |
| 2 | Do the tests discriminate (mutation battery) | in progress |
| 3 | Is the socket test honest / can it flake | done |
| 4 | Behaviour under the fix (reconnect, in-flight state) | done |
| 5 | Resource use | done |
| 6 | Doc / AUDIT accuracy | done |
| 7 | Concurrency | done |

## Findings

(filled in below as they are found)
