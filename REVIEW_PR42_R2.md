# REVIEW_PR42_R2 — round 2, commit 2c9aac2 only

Reviewer: pr-reviewer, Opus tier. Head 2c9aac2, base 06ec147 (= origin/main, up to date).
Scope: src/pool_connection.rs (login() reorder + new test), AUDIT.md NET-04 entry. No JIT/bench/CI paths.

## Coverage ledger
1. Correctness of reorder / new gap (Q1) — pending
2. Dead-code removal (Q2) — pending
3. F2 break-test (Q3) — pending
4. F3 re-verification (Q4) — pending
5. mutants.sh (Q5) — pending
6. Full suite + clippy (Q6) — pending
7. AUDIT accuracy — pending

## Findings

### R2-F1 (minor, test gap) — the error null-filter's actual purpose is untested; removing it leaves the whole suite green
Primary sources: monero-stratum `proto.go:11-16` `JSONRpcResp{Result interface{} json:"result"; Error interface{} json:"error"}` (no omitempty) and `stratum.go:313` success reply `Error: nil` -> wire `"error":null`. Snipa22/nodejs-pool `lib/pool.js:936-940` sendReply: `error: error ? {...} : null`. So **every successful login from both reference pools carries `"error":null`**. No test login reply in the file carries `"error":null` (grep: only the two rejection replies at 2333 and 2453 mention `error`).
Mutation: `response.get("error").filter(|v| !v.is_null())` -> `response.get("error")`. Under it every real-pool login returns `Err("Login error: null")`. Result: `rtk proxy cargo test --release` -> 176 lib passed / 20 bin passed, **all green**. Restored, cmp identical.
The `!`-deletion mutant on that filter is killed only incidentally (F2 test: non-null error filtered out -> "Unexpected login response"), so "the null-filtering logic F1 added is itself covered" (AUDIT) overstates it. The repo already learned this exact lesson for the submit path (`a_success_reply_carrying_error_null_is_accepted_not_rejected`, line 1961). Failure would be loud (every login fails), hence minor not major. Fix: a success-login test with `{"id":1,"jsonrpc":"2.0","error":null,"result":{"id":"...","job":...,"status":"OK"}}`.

### Process incident (no tree damage left) — shared scratchpad collision
The scratchpad dir is shared with at least one other concurrent agent (files `pool_connection.rs.{full,good,orig}` there are not mine). My backup `$S/pc.orig` was overwritten at 12:44 by another agent's copy of `pool_connection.rs` (contains #32's `ROTATION_SETTLE_LIMIT`), and my Q3 restore copied it into this worktree. Detected via `git diff`; restored with `git checkout HEAD -- src/pool_connection.rs`; confirmed `git diff --quiet 2c9aac2 -- src/`. The other agent's file was the last writer, so its own restore (if any) gets its own content. Lead: reviewers/implementers running concurrently need unique backup names; the shared-context advice `cp file /tmp/…` invites this. Subsequent backups use a private subdir + `git checkout` restores.

### Q3 (F2 break-test) — reproduced
Revert F1 only (result-first, no filters, trailing `else if error` restored, byte-matching 2c9aac2~1's login()): `a_login_rejection_with_a_null_result_reports_the_real_reason` FAILS at :2468 with `got: Login response carried no session id: null`; 38 others pass. Matches the claim.

### Q4 (F3 correction) — reproduced; corrected bullet is accurate
- Revert A (relogin_as clear -> `connect()?; login()?`): test 2 FAILS at the stream-is-none assert ("a failed relogin must clear the stream"). Also fail: test 1 (stream assert), test 4 (15s third-accept timeout), and the new F2 test (its stream assert).
- Revert B (login let-else -> main's `if let Some(id)`): test 2 FAILS at `.is_err()` ("must fail, not succeed silently"); test 4 fails (timeout); test 1 passes.
- `session_id == "old"` cannot discriminate: under A the fixed login() errs before writing sid; under B main's code skips the write when no id. Bullet's claim correct.
All restored via `git checkout HEAD -- src/pool_connection.rs`, `git diff --quiet 2c9aac2 -- src/` OK.

### Q5 (mutants) — reproduced exactly
`./scripts/mutants.sh 'relogin_as|login' 'pool_connection::'`: 6 tested, 4 caught, 2 unviable, 0 missed. Caught: login->Ok(()) :445, delete ! :473 (error filter), delete ! :475 (result filter), relogin_as->Ok(()) :890. Unviable: && -> || :451, :491. Killers: :473 by the new F2 test only; :475 by `a_successful_relogin_restores_the_poll_interval` and `the_first_job_after_a_flood_is_not_swallowed_by_the_stale_buffer`. AUDIT's "F2 test and the existing suite respectively" is correct — but see R2-F1: the :473 kill is via the non-null branch; the null branch is unpinned.

### R2-F2 (minor, AUDIT accuracy) — stale "Not established" xmrig bullet contradicts the new review paragraph
AUDIT.md:7168 still says "no verification of the xmrig source was performed in this session"; :7171 says the reviewer verified it against Client.cpp. Round 1's F4 asked exactly for this bullet to be resolved; it was neither fixed nor listed as deferred. The entry now argues with itself — the failure mode the review brief names ("No stale claim contradicts a new one"). Same class, smaller: "Files changed" (:7149ish) still says "four new tests" (now five), and the first verification bullet's 175 is superseded only by a later paragraph.

### R2-F3 (minor, AUDIT accuracy) — the review paragraph misreports round 1's findings
Header says "4 minors, 3 nits"; body lists "Two real findings" (F1, F2), F3 inline, and two nits. F4 (xmrig bullet) and N3 (no sealed prediction) are silently dropped — "Two real findings" reads as if the other minors were not real. Also the F1 bullet says the pre-fix code "silently read that as a successful login ... swallowing the pool's real rejection reason ... behind a generic error": those are two different behaviours (main: `Ok(())`, silent; pre-review branch: `Err("...no session id: null")`, loud but wrong reason). Round 1's ledger kept them apart.
And the commit deleted the bullet that recorded the tier decision "before the review runs", which is the only pre-registration this entry had (N3 already noted there was no prediction); now there is not even the decision record. Unmerged entry, so editing in place is allowed — but the grading record lost content.

### R2-N1 (nit, format) — **Review (** paragraph is a lazy continuation of the last bullet
AUDIT.md:7170 (last "Not established" bullet) is followed at :7171 by `**Review (Opus, ...` with no blank line, so GFM renders it as part of the "No live evidence" list item. `grep '^\*\*Review ('` still finds it; rendering does not. Every neighbouring entry (7065, 7113, 7132) has the blank line.
