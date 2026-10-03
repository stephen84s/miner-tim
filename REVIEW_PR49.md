# REVIEW_PR49 — Fix #44: fair stream lock (parking_lot::FairMutex)

Reviewer: pr-reviewer (Opus tier), cold start. Branch `fix/issue-44-stream-lock-fairness`, head 8abb44d.
Base check: `git merge-base HEAD origin/main` == origin/main tip 646080f (after `git fetch`). Rebased: yes.

Scope: touches src/pool_connection.rs, Cargo.toml/lock, AUDIT.md, CLAUDE.md, tasks/. No jit/, benches/, workflows, Makefile, scripts/. Nothing to hand off.

## Coverage ledger
1. Correctness of change — done. Every critical section is the same as before; only the mutex type changed. No new nesting. Lock order stream -> pending_shares holds.
2. Silent failure — done. Dropping poisoning removes a silent path: on main, `receiver_loop` did `Err(_) => return` with no log on a poisoned stream lock, so job updates would stop and nobody would be told. No panic="abort" in Cargo.toml, so the poisoning change is live in release builds. Pre-existing `if let Ok` on pending_shares in write_and_register is unchanged and out of scope.
3. Safety switches — n/a. No --native-loop/--verify-shares code is touched.
4. Tests / break-test — done (see below). Flake check: pool_connection:: module 10/10 green, full suite 3/3 green, T1 latencies 4-128ms against a 500ms bound. x86_64 CI `test` passed on head 8abb44d (T1/T2 ok).
5. Resource use — done. Spinners are joined before any assert. The helper clears `address`, and I confirmed `reconnect()` then returns false (L1003), so the leaked receiver exits. Nothing new allocated.
6. Docs/audit accuracy — done (F1-F6).
7. Concurrency — done. FairMutex hands off on every unlock, so a parked receiver cannot be starved by submits either, and FIFO among workers. send_request still holds the lock up to 30s on login; that is disclosed in the docs and unchanged.

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

### F5 (nit) — `stream` field doc on parking_lot::Mutex reads backwards
"Otherwise it barges. With 50ms hold times that timer has nearly always run out." If the FairTimeout has run out, the unlock is fair, so this sentence argues that plain `parking_lot::Mutex` *would* hand off here. That matches M2, where it passed everything. The actual reason to keep FairMutex is that it guarantees the handoff rather than making it probabilistic. Say that, so a future reader does not read it as "plain Mutex was seen to barge".

### F6 (nit) — T2 doc "It cannot turn a barging lock green"
That is too strong. If the test thread is preempted between `drop(guard)` and `lock()`, the woken submitter can win the lock on a barging lock, and that iteration goes green. 10 iterations make an all-green run improbable (observed 8/8 red at iteration 0), so the gate itself is sound. Suggest "is very unlikely to".

### Not verified
- Live pool behaviour and the 1h `lock_wait_ms` rerun. The PR itself lists this as required before merge, and it has not run.
- Break-test on x86_64 Linux (CI's `test` platform). I ran it on macOS only. The author's Linux evidence is from arm64 Docker. std's futex mutex is the same code on both arches, so I expect the same result, but did not run it.
- jit-macos / jit-linux-arm were still pending at review time. No JIT code is touched.

## Verdict
The code review passes. No blocker, no major. F1-F3 are audit-accuracy fixes to the unmerged NET-07 entry and the status lines. F5 and F6 are doc nits. **Not mergeable yet**: the PR's own "required before merge" live acceptance run is outstanding, and the jit gates had not finished. False-positive risk: F5 and F6 are wording only.
