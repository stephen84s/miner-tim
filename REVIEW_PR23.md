# REVIEW_PR23 — round 1, independent review of PR #23 (`security/bound-recv-buffer`)

Cold `pr-reviewer`, 2026-09-17. Base `origin/main` = `f2abc1e`; head `06e240f`
(branch contains `origin/main`, linear). Diff: `AUDIT.md`, `CLAUDE.md`,
`src/pool_connection.rs`, +308 / -9.

**Verdict: MERGEABLE.** No blockers, no majors, seven minors and two nits.
Something is **ACTIONABLE** — items F1, F2, F6 and F7 are cheap and worth doing
before merge.

## Scope — nothing to hand off

No `src/randomx/jit/`, no emitter, no `vm.rs` native-loop path, no `benches/`,
no speed or hashrate claim → not `jit-reviewer`. No `.github/workflows/`,
`Makefile`, `scripts/` or `.cargo/config.toml` → not `ci-reviewer`. Whole diff
reviewed here.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Correctness of the bound | done — holds; see "the bound" below |
| 2 | Do the tests discriminate (mutation battery) | done — 7 mutations, table below |
| 3 | Socket test honesty / flake | done — honest; one nit |
| 4 | Behaviour under the fix (reconnect, in-flight state) | done |
| 5 | Resource use | done — nothing new allocated |
| 6 | Doc / AUDIT accuracy | done — F5, F6, F7 |
| 7 | Concurrency | done — no lock held across `reconnect()`'s sleeps |

## Ran myself

- `cargo test --release`: **157 lib + 18 bin**, 2 ignored, 0 failed. Matches the
  claim exactly (149 from SEC-02 + 8 new).
- `cargo clippy --all-targets --release -- -D warnings`: exit 0.
- `gh pr checks 23`: all five green (`lint`, `audit`, `test`, `jit-macos`,
  `jit-linux-arm`).
- `scripts/verify-jit.sh`'s filters do not match `pool_connection::`, so
  `EXPECTED_PASSES=92` is untouched by the 8 new tests. Confirmed against the
  filter list, and the two `jit-*` jobs are green.

## The bound

`chunk` is a fixed `[0u8; 4096]` (`pool_connection.rs:494`), so one read can
never deliver more than 4096 bytes — the "single read larger than the limit"
case cannot arise, over TCP or TLS (`PoolStream::read` is capped by `buf.len()`
in both arms). `drain(..=pos)` with `pos` from `position()` on the *current*
buffer removes exactly through the newline; indices are recomputed each
iteration, so no byte is dropped or duplicated. Drain-then-check is the right
order: a complete line arriving with its newline in the same read is consumed
before the remainder is measured, which is why `MAX` content + `\n` works.
Behaviour is otherwise identical to the old inline loop (same trim, same
skip-empty, same order); the only change is that lines are handled after the
drain loop rather than interleaved, and `handle_pool_message` touches neither
`pending` nor the connection, so that is not observable.

Steady-state ceiling: `MAX_LINE_BYTES + 4096` — see F4.

## Mutation battery (verified by running, not reading)

Backup taken to scratchpad, restored from it after each run; final
`git diff --quiet -- src/pool_connection.rs` clean.

| # | Mutation | Result |
|---|---|---|
| M1 | remove the `pending.len() > MAX` check | 4 fail, incl. the socket test |
| M2 | raise the limit 1000× | socket test fails; helper flood test **spins** — see F2 |
| M3 | `>` → `>=` | 2 fail (`the_limit_is_exact_and_not_off_by_one`, flood test) |
| M4 | drain `while` → `if` | 1 fail (`several_lines_in_one_read_all_come_out_in_order`) |
| **M5** | **revert the call site to the original inline unbounded loop, leaving `take_complete_lines` and all 7 helper tests intact** | **`the_receiver_loop_really_drops_a_newline_free_stream` FAILED**, with its intended diagnostic. The PR's central claim holds. |
| M6 | drop the `Err` arm only (`if let Ok(lines) = …`) | socket test fails — the `Err` arm is load-bearing |
| M7 | drop `pending.clear()` from the `Err` arm, keep `reconnect()` | **GREEN** — see F1 |

## Findings

**F1 (minor) — `pending.clear()` in the overflow arm has zero coverage.**
M7 ships green. The line is correct as written, but if it were lost the fix
inverts into a reconnect storm: the >1 MiB newline-free remainder survives the
reconnect, so the very next read re-enters `Err`, logs, reconnects again —
forever, one cycle per `RECONNECT_DELAY`. Nothing in the suite would notice,
because the socket test only needs *a* second accept and `reconnect()` still
supplies it. Cheapest fix: have the socket test observe a **third** accept and
assert the gap, or assert the buffer is empty after an overflow via the helper
contract.

**F2 (minor) — the "raise the limit 1000×" mutation does not produce a clean
red.** `a_newline_free_stream_is_refused_instead_of_buffered` has an uncapped
`loop` and rescans the whole buffer each iteration, so with a 1 GiB limit it is
O(n²): I measured **>19 min CPU and >3.2 GB RSS** still climbing before killing
it. Only the socket test fails (at its 30 s timeout) and the run never reports
a verdict — in CI that is a job timeout, not a failing test, and on the 7 GB
`macos-14` runner an OOM risk. The PR and AUDIT both say the four mutations are
"each caught"; true only in the sense that the suite eventually goes red. Cap
the loop (e.g. `for _ in 0..(MAX_LINE_BYTES / chunk.len() + 2)` then fail).

**F3 (minor) — complete lines drained alongside an overflow are silently
discarded, and only a test doc comment says so.** `take_complete_lines` returns
`Err` and throws away the `lines` it already built. That is defensible —
`reconnect()` clears `current_job` anyway, so nothing is left half-applied — but
the AUDIT entry states only "`handle_pool_message` only ever receives whole
lines", which does not tell a future reader that whole lines are *also* dropped.
The `error!` message does not mention them either, so an operator cannot tell a
job was discarded.

**F4 (minor) — the ceiling is `MAX_LINE_BYTES + 4096`, not `MAX_LINE_BYTES`.**
Issue #21's acceptance criterion is "no peer can make `pending` grow beyond the
configured maximum". It can, by up to one read. Harmless, but it is a number in
the record and it silently tracks `chunk`'s size if anyone grows that buffer.
One sentence in AUDIT settles it.

**F5 (minor) — the two paths share the constant but not the semantics.** The
doc comment says the value lives in one place "so the two cannot drift apart".
The *number* cannot; the *behaviour* already differs: `read_line` rejects a line
of `MAX+1` bytes outright, while `receiver_loop` will deliver a complete line of
up to `MAX + 4096` to `handle_pool_message` — which is exactly what
`a_large_but_terminated_message_is_accepted` asserts. Worth saying, since the
comment currently reads as if the two paths enforce the same rule.

**F6 (minor) — "`read_line` has **always** rejected input past 1 MiB" is not
what the history says.** `git log -S "1 << 20" -- src/pool_connection.rs`
returns exactly one commit, `bd96af3` (NET-01, 2026-07-25). Before it the file
used std's `BufRead::read_line`, which is unbounded (`bd96af3^` has no such
helper). So the bounded `read_line` and today's `receiver_loop` were introduced
**by the same commit**, and only one of them got a cap. The asymmetry claim
survives — it is arguably sharper this way — but "always" is wrong and this
repo has been bitten before by a rhetorical adverb hardening into a fact in
`AUDIT.md`.

**F7 (minor) — the AUDIT entry's "Files changed" omits `CLAUDE.md`,** which the
same commit edits (task-board row). Earlier entries list it explicitly, e.g.
"`CLAUDE.md` (task board)" at `AUDIT.md:5259`.

**F8 (nit) — the socket test leaks a reconnect thread.** The server returns at
`n == 1`, dropping the listener; the miner is then inside `reconnect()`'s
infinite loop, retrying a released ephemeral port every 5 s and logging a
warning, for the remaining lifetime of the test binary (~45 s in the release
`--lib` run). No other test binds `127.0.0.1:0` (grepped), so there is no
interference today. It is noise and a latent vector.

**F9 (nit) — clear/reconnect ordering is inconsistent** with the two
pre-existing arms (`Ok(0)` and `Err(e)` clear *after* `reconnect()`; the new arm
clears before). No behavioural difference, since `reconnect() == false` returns.

## Checked and clean

- **Socket test honesty (priority 3).** Only four sites create a connection:
  `receiver_loop`'s three `reconnect()` calls and `relogin_as`. Within the test's
  ≤40 s window: donation rotation first fires at **5700 s**
  (`beneficiary_at` with `DEFAULT_DONATE_LEVEL`, cycle 6000 s), keepalive at
  60 s and does not reconnect on failure anyway, the 50 ms read timeout lands in
  the no-op `WouldBlock | TimedOut` arm, and EOF cannot occur because the server
  holds the socket in `held`. M5 and M6 confirm this empirically: with the bound
  removed at the call site, no second accept arrives. The test measures what it
  claims.
- **Not a `TLS_PORTS` hazard**, and for a checkable reason rather than by
  assertion: every entry is ≤ 14433 while the ephemeral range starts at 49152 on
  macOS (`net.inet.ip.portrange.first`) and 32768 on Linux.
- **Timing margin**: overflow is reached after ~256 reads of a 1.5 MB flood
  (milliseconds), then one `RECONNECT_DELAY` (5 s) before the second accept,
  against a 30 s assertion. Whole filtered run measured at 5.08 s.
- **Priority 4, in-flight state.** `reconnect()` clears `current_job` first, so
  workers idle via `get_work() -> None` rather than mining a job from a dead
  session; the stream is replaced, so no stale bytes survive. A share submitted
  but unanswered is lost with no resubmission — **pre-existing**, identical on
  the EOF and read-error arms, not introduced here (and related to #17, the
  unidentified response logging).
- **Priority 7, concurrency.** The stream lock is scoped to the single `read`,
  so `reconnect()` — which sleeps in a loop — is called with no lock held. No
  new shared state.
- **Failure visibility.** The overflow is logged at `error!` with the byte count
  and the limit; `reconnect() == false` logs at `error!` too. Nothing is
  swallowed.
- **Append-only:** the AUDIT entry is appended at end of file; the `CLAUDE.md`
  row is inserted above `Pending`. No in-place edit of merged history.
- **No orphaned doc comments:** `take_complete_lines` is appended after
  `boost_current_thread_priority`'s `#[cfg(not(…))]` stub, not spliced under an
  existing comment; `MAX_LINE_BYTES` carries its own.

## Could not verify

- No end-to-end run against a real pool (the PR says so itself).
- The wiring test is **plain TCP by construction**, so the bound has never been
  exercised over TLS. The logic is transport-independent because `read` is
  capped by the 4096 chunk in both `PoolStream` arms, but rustls's own internal
  plaintext buffering was not audited and is outside this bound.
- I did not run `make verify-jit`: the diff touches no JIT code and both
  `jit-*` CI jobs are green on the PR head.
