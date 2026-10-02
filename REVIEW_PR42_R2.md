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
