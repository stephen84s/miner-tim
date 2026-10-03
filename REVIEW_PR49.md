# REVIEW_PR49 — Fix #44: fair stream lock (parking_lot::FairMutex)

Reviewer: pr-reviewer (Opus tier), cold start. Branch `fix/issue-44-stream-lock-fairness`, head 8abb44d.
Base check: `git merge-base HEAD origin/main` == origin/main tip 646080f (after `git fetch`). Rebased: yes.

Scope: touches src/pool_connection.rs, Cargo.toml/lock, AUDIT.md, CLAUDE.md, tasks/. No jit/, benches/, workflows, Makefile, scripts/. Nothing to hand off.

## Coverage ledger
1. Correctness of change — in progress
2. Silent failure — pending
3. Safety switches — pending (none touched expected)
4. Tests / break-test — pending
5. Resource use — pending
6. Docs/audit accuracy — in progress
7. Concurrency — pending

## Findings

### Verification run by reviewer (macOS arm64, M-series, 12 cores)
- Full suite `rtk proxy cargo test --release --locked`: lib 191 passed / 0 failed / 3 ignored, bin 20 passed. Matches claim.
- clippy `--release --all-targets -D warnings`: clean. `cargo audit`: clean, 99 crates.
- Ignored attrs: main had 4 (pool reproducer, pool chatty control, 2 randomx); HEAD 3. Net -1.

### Break-test (done myself; copy at scratchpad/pool_connection.rs.orig, restored, `cmp` identical, `git status` clean)
- M1: `StreamLock` -> tuple-struct shim over `std::sync::Mutex` (lock via into_inner, try_lock via ok). T2 run alone with `--exact`: **8/8 FAILED at iteration 0**, intended message. T1 pair, 3 runs: **6/6 FAILED** (2.79s, 10.46s, 2.79s, 2.80s, 3.44s, 62.85s).
- M2: `StreamLock` -> `parking_lot::Mutex`. `pool_connection::tls_tests::a_` (32 tests) 5/5 runs all green. Disclosed gap reproduces exactly.
- Restored: T2 3/3 green (1 passed, not 0 passed).
- Verdict on break-test evidence: real. T2 is a genuine gate against a barging lock on this host.

### F1 (minor, audit accuracy) — ignored-count arithmetic and T2 history wrong in NET-07
NET-07 says ignored 4->3 is "the two #40/#44 reproducers un-ignored (-2)" and "T2 was added as not-ignored from the start (net 0)". That sums to -2, not -1. On main only ONE reproducer was ignored (`a_submit_is_not_held_hostage_by_a_quiet_receiver`); the full-load variant is new. And commit 9b0e559 adds T2 *with* `#[ignore]`, so "not-ignored from the start" contradicts the branch history. Correct accounting: -1 ignored (one reproducer un-ignored), +2 new non-ignored tests (full-load, T2); 188+1+2 = 191.

### F2 (minor, audit accuracy) — line count
NET-07 "src/pool_connection.rs (+287/-70 across 3 commits)"; `git diff --numstat origin/main...HEAD` gives 232/70.

### F3 (minor, stale status) — "3 commits" / "PR not yet opened"
AUDIT NET-07 ("Commits (3 ... not yet pushed/merged)", "PR is not yet open"), CLAUDE.md Current task ("(3 commits), PR not yet opened") and tasks/NET-07.md ("PR not yet opened") are stale: PR #49 is open, branch has 4 commits (the 4th, 8abb44d, is the AUDIT commit itself). Unmerged-branch entry (not on origin/main), so in-place edit is fine.

### F4 (note) — no sealed prediction before this review
No prediction in tasks/NET-07.md or the NET-07 entry. Third consecutive PR (after PROC-09-CLEANUP and PROC-10) without one; AUDIT already names this pattern. Not a blocker.
