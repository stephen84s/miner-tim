# REVIEW_PR42 — GitHub #37 (relogin stream-clear, login session-id check)

Reviewer: pr-reviewer, Opus tier (raised by lead). Branch `fix/issue-37-stale-relogin-session`, head c5f67e3, base 06ec147 (= origin/main, up to date).

Scope: diff touches `src/pool_connection.rs`, `AUDIT.md`, `CLAUDE.md`, `tasks/`. No JIT, benches, CI paths — nothing handed off.

## Coverage ledger
1. Correctness — done (F1, F2)
2. Silent failure — done (F1: the PR closes a broader silent-success than it claims)
3. Safety switches — n/a (no switch touched)
4. Tests / break-tests — done; all three reproduced (below); F2 coverage gap
5. Resource use — done; nothing new allocated
6. Docs / AUDIT accuracy — done (F3, F4, N3)
7. Concurrency — done; no finding (reasoning below)

## Verification performed
- Baseline: `rtk proxy cargo test --release --lib relogin` -> 4 passed, 173 filtered.
- Break 1 (failure-arm clear reverted to `connect()?; login()?`): tests 1, 2, 4 FAIL — 1 and 2 on the `stream.is_none()` asserts, 4 on the 15s `recv_timeout` expect. Test 3 passes. Restored, `cmp` identical.
- Break 2 (login let-else reverted to old `if let Some(id)`): tests 2 and 4 FAIL — test 2 on the **`result.is_err()`** assert. Test 1 passes. Restored, `cmp` identical.
- Break 3 (`set_read_timeout(RECV_POLL_INTERVAL)` in relogin_as deleted): test 3 FAILS, "got Some(30s)". Restored, `cmp` identical.
- `./scripts/mutants.sh 'relogin_as|login' 'pool_connection::'`: 4 mutants, 2 caught (login->Ok(()), relogin_as->Ok(())), 2 unviable (&&->|| at 451, 481). No mutant on the `*s = None` assignment or the let-else return — cargo-mutants does not mutate plain assignments or let-else. AUDIT caveat is accurate.
- xmrig claim checked against xmrig master `src/base/net/stratum/Client.cpp`: `parseLogin` does `setRpcId(Json::getString(result,"id")); if (rpcId().isNull()) { *code = 1; return false; }` — TRUE. Nuance: `parseResponse` handles `error.IsObject()` **before** `result`, and requires `result.IsObject()`; `parseLogin` also fails on an invalid job.

- Full suite `rtk proxy cargo test --release`: 175 lib passed / 2 ignored, 20 bin passed — matches AUDIT.
- Not re-run: clippy, `make check`. CI `jit-macos`/`jit-linux-arm` had no conclusion when checked; lint/test/audit green. MoneroOcean nodejs-pool fork's error shape not checked.
- Side effect: `mutants.sh` rotated the gitignored `mutants.out` -> `mutants.out.old`.

## Findings

### F1 (minor) — login() inspects `result` before `error`; real pool login rejections now surface as "carried no session id: null"
sammy007/monero-stratum `stratum.go:324-327` `sendError` builds `JSONRpcResp{Id: id, Version: "2.0", Error: reply}` (login branch, line 275, routes `handleLoginRPC` errors through it with drop=true) with `Result interface{}` (no omitempty) -> wire shape `{"id":1,"jsonrpc":"2.0","result":null,"error":{...}}`. serde_json `get("result")` returns `Some(Null)`, so:
- **Before this PR**: `Null.get("id")` is None, fell through, returned **Ok(())** — a rejected login (e.g. invalid wallet) reported as success, no session id, no job. A broader silent-success than the PR describes.
- **After**: Err("Login response carried no session id: null") — no longer silent (good; at startup the binary exits), but the pool's actual message is dropped. User sees "Login failed: Login response carried no session id: null" instead of the pool's reason.
Fix: check a non-null `error` first (as xmrig does), or treat `result: null` as absent. (Snipa22/nodejs-pool omits `result` on errors — `JSON.stringify` drops undefined — so it already hits the error branch.)

### F2 (minor) — the tested shape is not the realistic one
The AUDIT and test motivate the login check with a submit-ack `{"id":99,"result":{"status":"OK"}}` read as the login reply. A submit carrying the *previous* session id on a freshly opened connection is not acknowledged OK by either pool checked: Snipa22/nodejs-pool replies `Unauthenticated` (error), monero-stratum replies error + `result:null`. The realistic silent-success shape is F1's, and no test covers it. Code handles it; coverage and narrative don't. Add a test with `{"id":1,"jsonrpc":"2.0","result":null,"error":{"code":-1,"message":"..."}}`.

### F3 (minor) — AUDIT "Not established" bullet 3 is false and contradicts the entry's own break-test 2
It says test 2 does not isolate the login() change and that its "true load-bearing assertion is the logged warning". There is no log assertion in the test. Break 2 (reproduced) shows test 2 fails on `result.is_err()` with only the login fix reverted; break 1 shows it fails on `stream.is_none()` with only the relogin fix reverted. It discriminates both. Only the `session_id == "old"` assertion is non-discriminating (old code also left the id untouched). Since unmerged, edit in place.

### F4 (minor) — "Not established" xmrig bullet can now be resolved, with nuance
Verified true (see above). The comment in login() and the AUDIT should cite it, and note xmrig checks `error` first (relevant to F1).

### N1 (nit) — `connected` stays true after the failure-arm clear
`connect()` stored true; the failure arm clears the stream but not the flag. Only `start_receiver` reads it and `reconnect()` resets it immediately, so harmless.

### N2 (nit) — test 4 leaks a receiver_loop thread
After the server returns, the loop sees EOF and `reconnect()` retries a dropped listener every 5s for the rest of the test binary. No assertion impact.

### N3 (nit, process) — no sealed prediction
The entry records the tier decision before review but not what the review is expected to find, which CLAUDE.md's grading rule asks for.

## Concurrency (no finding)
- Failure arm takes `stream` alone; `send_request`'s guard is dropped by then; no nested lock, order stream -> pending_shares unaffected.
- Only the receiver thread calls connect/relogin_as/reconnect after `Miner::initialize`; nothing can reinstall a stream between the clear and the next read. Clears are idempotent.
- A submit that wrote onto the new stream between connect() and the failure is still registered; it is drained as lost by the `reconnect()` the next read triggers. Not orphaned.
- Strictly better for lock latency: previously the leftover stream had a 30s read timeout, so the receiver's read could hold the stream lock ~30s, blocking submits.

## #32 interaction (no conflict)
No timing assumption added. The only coupling: retrying with the donation wallet after a failure relies on `self.wallet` being set before/inside the attempt (receiver loop sets it before `relogin_as`; `login()` also sets it, but only if connect succeeded). If #32 moves the wallet write to after a successful relogin, a connect failure would reconnect as the previous wallet — and test 4 would catch that. A guard, not a hazard.

## Verdict
Mergeable; F3 (a false claim in `AUDIT.md`) must be edited in place before merge. F1/F2 are recommended in this PR but not blocking: the behaviour is strictly safer than main. 0 blockers, 0 majors, 4 minors, 3 nits.
