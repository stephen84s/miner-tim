# REVIEW_PR23 — round 1, independent review of PR #23 (`security/bound-recv-buffer`)

Cold `pr-reviewer`, 2026-09-17. Base `origin/main` = `f2abc1e`; head `06e240f`
(branch contains `origin/main`, linear). Diff: `AUDIT.md`, `CLAUDE.md`,
`src/pool_connection.rs`, +308 / -9.

**Verdict: MERGEABLE.** No blockers, no majors, five minors and three nits.
Something is **ACTIONABLE** — F1 (a line with no coverage), F2 (a test that
spins instead of failing), F3/F4/F6 (one sentence each in AUDIT) and F7.

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
`macos-14` runner an OOM risk.

Stated precisely: this is what **my** instantiation of that mutation —
`const MAX_LINE_BYTES: usize = (1 << 20) * 1000;`, i.e. the 1000× raise as
literally described — does. I did not reproduce the author's exact mutation and
cannot say theirs did not go cleanly red (a smaller multiple would). The durable
half is the test shape, not the arithmetic: `a_newline_free_stream_is_refused…`
has an uncapped `loop` whose only exit is the production check firing, so any
future weakening of that check turns the test into a spin rather than a failure.
Cap the iterations (e.g. `for _ in 0..(MAX_LINE_BYTES / chunk.len() + 2)` then
fail with a message).

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
configured maximum". It can, by up to one read. Two conditions hold that ceiling and both are worth
naming: `chunk` is 4096 bytes, **and** `take_complete_lines` is called after
every single read — the second is what actually bounds it, since a read that did
not re-check would let the remainder run on. Harmless today, but it is a number
in the record and it tracks `chunk`'s size if anyone grows that buffer. One
sentence in AUDIT settles it.

**F5 (minor) — the two paths share the constant but not the semantics.** The
doc comment says the value lives in one place "so the two cannot drift apart".
The *number* cannot; the *behaviour* already differs: `read_line` rejects a line
of `MAX+1` bytes outright, while `receiver_loop` will deliver a complete line of
up to `MAX + 4096` to `handle_pool_message` — which is exactly what
`a_large_but_terminated_message_is_accepted` asserts. Worth saying, since the
comment currently reads as if the two paths enforce the same rule.

**F6 (nit, and it *strengthens* the entry rather than contradicting it) — the
"always" claim checks out, and the history says something sharper.** I checked
it because it is load-bearing: `git log -S "1 << 20" -- src/pool_connection.rs`
returns exactly one commit, `bd96af3` (NET-01, 2026-07-25), and
`git show bd96af3^:src/pool_connection.rs | grep "fn read_line"` is empty — the
free `read_line` helper is *new* in that commit, the file having previously used
std's `BufRead::read_line` at a different call site. So the helper has carried
the 1 MiB cap for every commit of its existence: "always" is true of it. What
the history adds is that the capped helper and today's `receiver_loop` were
written **in the same commit**, so this was never drift — the asymmetry was
present from birth. One sentence to that effect would make the AUDIT entry
stronger. No correction needed.

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
  exercised over TLS. This is *not* a gap in the fix: `PoolStream::read` caps at
  `buf.len()` in both arms, so the bound on the miner's own `pending` is
  transport-independent. What is unaudited is memory held **inside** rustls —
  a different allocation, and not what #21 is about. Recorded so a later round
  does not re-open it as a hole in this change.
- I did not run `make verify-jit`: the diff touches no JIT code and both
  `jit-*` CI jobs are green on the PR head.

---

# Round 2 — review of the fixes (`298696b`)

Cold reviewer, 2026-09-19. Base for this round: `3cf9394` (end of round 1);
head `298696b`. Diff: `AUDIT.md` (+55), `CLAUDE.md` (1 row), `src/pool_connection.rs`
(+127/−12). Nothing in `src/randomx/jit/`, the emitter, `vm.rs`'s native-loop
path, `benches/`, `.github/workflows/`, `Makefile`, `scripts/` or
`.cargo/config.toml` — **nothing to hand off to `jit-reviewer` or `ci-reviewer`.**

`mutants.out/` is untracked in this worktree and is not mine; left alone.

## Coverage ledger (round 2)

| # | Item | State |
|---|---|---|
| 1 | Does the new test discriminate? flaky? | done — discriminates, deterministic; R2-F9 |
| 2 | Is the author's correction of round 1's prediction right? | done — **substantively yes**; R2-F3 is only about its absolute phrasing |
| 3 | The capped flood test — clean failure on a raised limit? | done — **no**; **R2-F2** |
| 4 | Round-1 minors F3/F4/F5/F7 closed? | done — F3/F4 yes, F5 half, F7 **no**; R2-F4, R2-F5 |
| 5 | `read_line`'s uncovered limit — scope call | done — still uncovered; R2-F7 |
| 6 | Doc / AUDIT / PR-body accuracy | done — **R2-F1**, R2-F5, R2-F6 |
| 7 | `cargo test --release`, clippy | done — 158 lib + 18 bin + 0 doc, 2 ignored, 0 failed; clippy exit 0 |

**Verdict: MERGEABLE** on content, subject to the standing merge conditions
below. No blockers, no majors. **Five minors and four nits.** Something is
**ACTIONABLE**: R2-F1, R2-F2, R2-F4, R2-F5, R2-F6.

Merge conditions, checked: branch **contains `origin/main`** (`git log HEAD..origin/main`
empty), and **all five checks pass on the current head** — `lint` 23 s, `audit`
14 s, `test` 4 m 1 s, `jit-macos` 14 m 21 s, `jit-linux-arm` 11 m 42 s. No
rebase needed. `scripts/verify-jit.sh`'s six `JIT_FILTERS` are all
`randomx::`-prefixed, so none matches `pool_connection::` and `EXPECTED_PASSES=92`
is untouched by the ninth test — the two `jit-*` jobs above confirm it.

## What I ran

All mutations taken against a scratchpad copy of `src/pool_connection.rs` and
restored from it; `git diff --quiet -- src/pool_connection.rs` clean at the end.

| Mutation / run | Result |
|---|---|
| Delete `pending.clear()` from the overflow arm (`:567`), keep `reconnect()` | `the_first_job_after_a_flood_…` **FAILS**, `left: None / right: Some("after-flood")`. **4/4 runs**, 10.1 s each. Full lib suite under the mutation: **157 passed, 1 failed** — that test and nothing else. |
| Unmutated, same test, 5 consecutive runs | 5/5 pass, **8.07 s every time** (5 s `RECONNECT_DELAY` + the server's 3 s sleep + ~0.07 s of actual work) |
| `MAX_LINE_BYTES` × 4 and × 16 | see R2-F2 |
| `read_line`'s `>` → `>=` (`:833`) | **survives** — 158 passed |
| `read_line`'s `>` → `<` (`:833`) | killed, by the *new* test (login's `read_line` errors, so the job never arrives) |
| `cargo test --release` / `cargo clippy --all-targets --release -- -D warnings` | 176 passed, 2 ignored / exit 0 |

`mutants.out/` is untracked in this worktree and is not mine; left alone.

## Findings

**R2-F1 (minor) — the new test carries two stacked doc-comment blocks, and the
first states the claim the commit exists to retract.** `pool_connection.rs:1064-1088`.
Block one: *"the very next read re-enters the overflow arm and reconnects again —
forever … This asserts the miner **settles**: after dropping the flood it
reconnects exactly once and stays connected while normal traffic flows."* Block
two, immediately below, says that is *"not what happens"*. Both bind to the same
`fn`, so this is not an orphaned comment — it is a doc comment that argues with
itself, and its first half **claims an assertion the test does not make** (there
is no settle/third-accept assertion anywhere in the body). Exactly the repo's
named "a stale claim contradicts a new one" pattern. Delete block one.

**R2-F2 (minor, and the most important item in the round; deliberately *not*
major — unlike SEC-02's half-closed major, the mutation is still caught, by the
socket test, so the suite is not blind and nothing is shippable-green) — round 1's F2 is
recorded as closed and is not. The cap scales with the constant it defends
against, so the raise-the-limit mutation now passes instead of spinning.**

`let max_iterations = (MAX_LINE_BYTES * 2) / chunk.len();` — derived from the
quantity the mutation moves, so the feed always outruns whatever the limit is
and the terminal `panic!` is unreachable under any pure change to
`MAX_LINE_BYTES`. Measured on this Mac, single test, release:

| `MAX_LINE_BYTES` | `a_newline_free_stream_is_refused…` |
|---|---|
| `1 << 20` (shipping) | **pass**, 0.05 s |
| × 4 | **pass**, 0.82 s |
| × 16 | **pass**, 18.29 s |

0.05 → 0.82 → 18.29 is ~16× and ~22× per 4× of limit, i.e. still O(n²).
Extrapolated to round 1's stated 1000× mutation that is ~20 h and a 2 GiB feed —
round 1's OOM, merely given a termination proof nobody will wait for.

`AUDIT.md`: *"Now bounded at twice the limit, so that mutation fails cleanly."*
**False on both clauses** — it does not fail, and at 16× it is already 350×
slower than baseline. A number in the authoritative record that does not
reproduce.

What the cap *did* fix: with the bound check deleted the test now fails in
**0.04 s** instead of spinning. That is worth keeping. Not blind overall,
either — at 16× `the_receiver_loop_really_drops_a_newline_free_stream` **fails**
(30 s). So the mutation is caught, just not by this test. Fix is one of: an
absolute iteration cap (`min(derived, 1024)`), or correct the AUDIT sentence to
say the socket test is what catches a raised limit.

**R2-F3 (nit — downgraded from minor after re-deriving it; see the correction
note at the end) — "No second overflow, no loop" is false as an absolute, but
the author's conclusion is right.** Demonstrated with a throwaway helper test
(run, then removed), simulating a surviving >MAX stale buffer:

- next read **contains** a newline → `Ok`, buffer self-clears, one bogus
  `MAX+`-byte line, the real message lost. The author's account — confirmed.
- next read is **newline-free** (4096 bytes of `y`) → `Err(> MAX)`, buffer still
  oversized → `reconnect()` → repeat.

So a second overflow *can* occur and the phrasing overstates. What it does **not**
establish is round 1's F1 framing that losing the clear "inverts the fix into a
reconnect storm". Against a peer that keeps flooding, the storm is there either
way: **with** the clear the cycle is 1 MiB + `RECONNECT_DELAY`, **without** it
4 KiB + `RECONNECT_DELAY`, and on any link where a megabyte arrives in
milliseconds both are dominated by the same 5 s sleep. The clear changes the
per-cycle byte cost, not whether the loop exists or how fast an operator sees
it. The author's substantive conclusion — that what the clear actually buys is
the first real message after a flood — stands, and the test measures exactly
that. One word in the commit message and the `AUDIT.md` append ("no loop" →
"no loop against a pool that resumes normal traffic").

**R2-F4 (minor) — round 1's F5 was answered in `AUDIT.md` but the inaccurate
sentence is still shipping in the code.** `pool_connection.rs:213-215` still
reads *"`read_line` has always had this limit … The value lives here so the two
cannot drift apart."* Round 1's point was that the *number* cannot drift but the
*behaviour* already differs (`read_line` rejects `MAX+1`; `receiver_loop`
delivers a complete line of up to `MAX+4096`). The correction is in the ledger
and not in the file a reader of `MAX_LINE_BYTES` will actually see.

**R2-F5 (minor) — round 1's F7 is still open, and the same entry now carries two
further stale numbers.** `AUDIT.md:5825` *"Files changed: `src/pool_connection.rs`
(… eight tests), `AUDIT.md` (this entry)"* — still omits `CLAUDE.md`, which both
commits edit, and "eight tests" is now nine. The entry's Verification block
still says "157 lib + 18 bin"; the real figure is **158 + 18** (measured). The
`CLAUDE.md` row updated in `298696b` says "Nine tests" but kept "157+18". The
round-2 append corrects none of the three. The entry is unmerged, so it may be
edited in place.

**R2-F6 (minor) — the PR body was not updated.** It still says "Testing it took
**two** attempts", "**Seven** tests", "157 lib + 18 bin", and does not mention
`pending.clear()`, the third attempt, the cap, or the three claim corrections.
The **fourth** round 2 in this repo to find an un-updated PR body.

**R2-F7 (nit) — `read_line`'s limit still has no test, and this PR edits that
line.** The diff changes `1 << 20` → `MAX_LINE_BYTES` at `:833` and rewrites its
error string; `>` → `>=` there survives the whole suite (verified: 158 passed).
Pre-existing on `main`, and the PR's framing ("`read_line` has *always* rejected
past 1 MiB") is a claim about code nothing guards. I judge it **out of scope to
fix here** — but not for the usual reason: `PoolStream::Plain(TcpStream)` is
trivially constructible against a local listener, so coverage is ~15 lines of the
pattern already in this file. The honest minimum is that `AUDIT.md` names the
gap, since Files changed lists "`read_line` using it" while Verification claims
the mutations are caught. File a follow-up issue.

**R2-F8 (nit) — round 1's F8 (leaked reconnect thread) is now doubled.** Both
socket tests end with the server dropping its listener while the miner sits in
`reconnect()`'s infinite retry loop, so two threads now retry released ephemeral
ports every 5 s for the life of the test binary. Two leakers and two
`127.0.0.1:0` binds is a (low-probability) cross-test port-reuse vector that did
not exist before.

**R2-F9 (nit) — the new test's real deadline is 3 s, not the 5 s it writes
down.** The server sleeps `Duration::from_secs(3)` then returns, dropping both
sockets *and* the listener; the miner then sees EOF, `reconnect()` fires and
**clears `current_job`**, so the remaining 2 s of polling can only return `None`.
Measured slack is ~0.07 s, i.e. ~40× margin — not flaky on any plausible runner,
but the number in the code overstates the margin by 67%.

## Checked and clean

- **The test cannot pass spuriously.** The assertion is on the exact `job_id`
  `"after-flood"`, which has one source; `get_work()` returns `None` until that
  message parses. Confirmed by the mutated runs: `left: None`, never a wrong id.
- **No deadlock between the flooding server and the miner.** `reconnect()`
  (`:603-605`) sets `*self.stream.lock() = None` **before** the
  `RECONNECT_DELAY` sleep, so the server's blocked `write_all` errors, `break`s,
  and reaches the second `accept()`. This is why the three-attempt history's
  hang does not recur.
- **The login response cannot be over-read into `pending`.** `send_request`
  consumes it through `read_line`, which reads **one byte at a time** and stops
  at the newline, so the job line is genuinely the first thing `receiver_loop`
  sees after the reconnect. That is what makes R2's break test deterministic
  rather than segmentation-dependent.
- **Scope**: no `src/randomx/jit/`, emitter, `vm.rs` native-loop path,
  `benches/`, `.github/workflows/`, `Makefile`, `scripts/` or
  `.cargo/config.toml`. Nothing to hand to `jit-reviewer` or `ci-reviewer`.
- **Append-only**: the round-2 `AUDIT.md` text is appended at end of file; the
  `CLAUDE.md` row is the task board, which is maintained in place by design.

## Could not verify

- No end-to-end run against a real pool (unchanged from round 1).
- The 20 h / 2 GiB extrapolation for the 1000× mutation in R2-F2 is quadratic
  extrapolation from three measured points (1×, 4×, 16×), **not** a measurement
  at 1000×; I stopped at 16× deliberately.
- I did not run `make verify-jit`; the diff touches no JIT code.

## Correction to this round's own first pass

R2-F3 was first written as a minor claiming the author's refutation was scoped
to a cooperative pool and that round 1's reconnect loop is what a hostile peer
produces. That over-reached on my own evidence: **with** `pending.clear()` a
peer that keeps flooding also loops, one cycle per `RECONNECT_DELAY`, so the
clear never averted the storm and round 1's F1 framing does not survive either.
Only the absolute phrasing is wrong. Downgraded to a nit and rewritten above.
The first ledger commit (`78fc74d`) still carries the over-claim in its message;
this commit is the correction, not an amend (shared-context rule 3).

---

# Round 3

Fresh reviewer, cold context. Scope as briefed: the tip commit `e3b339f` as new
work, plus the round-2 items it claims to close. Head reviewed:
`e3b339fc904cbeb83d5bc129819e31cf4382cf7a`.

**Verdict: NOT MERGEABLE — no blockers, two majors, four minors, one nit. All
ACTIONABLE and all small.** The code change is right and the new cap genuinely
works; what fails is the record and what the commit swept into the tree.

## Coverage ledger

| # | Item | Status |
|---|---|---|
| 1 | Correctness of the change itself | done — cap verified by mutation at 2×/16×/1024× |
| 2 | Silent failure / fallbacks | done — no new fallback; overflow arm logs at `error!` and reconnects |
| 3 | Safety switches, fail-safe direction | n/a — no switch touched |
| 4 | Tests — do they fail when the subject breaks? | done — break-tested, see R3-F3/F4 |
| 5 | Resource use | done — R3-F1 (5.5 MB of artifacts committed) |
| 6 | Documentation and audit accuracy | done — R3-F1, F2, F3, F5, F6, F7 |
| 7 | Concurrency | done — socket tests are deadline-bounded; no production concurrency change |

Ran myself: `cargo test --release` → **159 lib + 18 bin, 2 ignored, 0 failed**;
`cargo clippy --all-targets --release -- -D warnings` → clean.

## Priority 1: the new absolute cap works

`FEED_CEILING_BYTES = 4 * 1024 * 1024` is independent of `MAX_LINE_BYTES`.
Mutating `src/pool_connection.rs:216` and running the single test in release:

| `MAX_LINE_BYTES` | `a_newline_free_stream_is_refused…` |
|---|---|
| `1 << 20` (shipping) | pass, 0.04 s |
| `1 << 21` (2×) | **pass**, 0.20 s |
| `1 << 24` (16×) | **fail**, 0.67 s — `fed 4194304 bytes without being refused` |
| `1 << 30` (1024×) | **fail**, 0.69 s — same message |

Round 1's F2 is now genuinely closed: no spin, no OOM, a clean named failure,
and the time is flat in the size of the mutation because the feed is absolute.
The cap cannot be defeated by a change to `MAX_LINE_BYTES` — it shares no term
with it. Source restored byte-identical after every mutation (`diff` clean).

## R3-F1 (major) — the fix commit committed 5.5 MB of `mutants.out/` build artifacts

`e3b339f` adds **25 files / ~5 000 lines** under `mutants.out/`: cargo build
logs, `outcomes.json`, `mutants.json`, `debug.log`, `lock.json`.

- Both earlier rounds recorded it as *untracked and not theirs* (this ledger,
  lines 214 and 253). The tip commit swept it in — the shape of a `git add -A`.
- Not in `.gitignore`; `main` has no `mutants.out` and no `scripts/mutants.sh`
  (that is #24, unlanded), so this is not sanctioned evidence like
  `PERF1_RUNS.log`.
- **Not in the AUDIT Files-changed list** — the very list this commit edits to
  add the `CLAUDE.md` it had omitted. It now omits 25 files instead of one.
- Contents are machine-local and the repo is **public**: `lock.json` carries
  `"hostname": "Stephens-MacBook-Pro-2.local"`, `"username": "stephen"`, and the
  logs carry absolute `/Users/stephen/...` paths.
- Stale: generated 2026-09-16, before the rebase.

For scale: LEDGER-01 removed 530 KB of review ledgers as too large to keep on
`main`. This is 5.5 MB. Squash-merge lands all of it permanently.

**Fix:** `git rm -r --cached mutants.out`, add it to `.gitignore`.

## R3-F2 (major) — the task board is broken, and SEC-03 appears twice

`CLAUDE.md` lines 184-191 after the rebase:

```
184  | ... SEC-02 ... |
185  (blank)
186  | ... FIX-01 ... |        <- from main (#26)
187  (blank)
188  | ... SEC-03 ... |        <- this branch, PRE-round-1 text
189  (blank)
190  | ... SEC-03 ... |        <- this branch, current text
191  | **Pending** | - | ... |
```

Two defects in one place.

**(a) The blank lines terminate the GFM table.** Confirmed against GitHub's own
renderer (`POST /markdown`, `mode: gfm`) on the live file: exactly one `<table>`
is emitted and it **ends at SEC-02**. FIX-01, *both* SEC-03 rows and the
`Pending` row render as paragraph text with literal `|` characters — four rows,
including this PR's own entry and the board's only pending marker, fall out of
the Current Task Board. `main`'s board is contiguous (no blank line between
rows) and renders whole, so the branch causes this. This is DOC-02 round 3
recurring exactly: *"the numbering fix had broken the very table its convention
note documented — GitHub swallowed the whole task board."*

**(b) The SEC-03 row is duplicated, and the stale copy is the retracted one.**
`git blame`: 188 is from `6f15fe40`, 190 from `e3b339f` — the rebase resolved
"additively" and added a second row instead of replacing the first. Line 188
still claims *"Eight tests; four helper mutations and the wiring mutation all
caught. 157+18 tests"* — the exact claims this branch's own AUDIT withdraws.

**(c) The surviving row still carries the stale count.** Line 190 ends
*"157+18 tests, clippy clean"* while the same commit corrects AUDIT to 159+18.
Measured: 159 lib + 18 bin. The commit that fixed the count in one file left it
wrong in the other.

## R3-F3 (minor) — "fails in 42 s" does not reproduce, and it inverts the comparison

`AUDIT.md` and the commit message: *"Verified by raising the limit 16x: the test
fails in 42 s … where the old cap passed in 18 s having proved nothing."*

Round 2's 18.29 s was explicitly *"single test, release"*. The comparable
measurement for the new cap is **0.67 s** (table above). 42 s is not the test:
`cargo test --release --lib` at 16× takes 43.78 s (3 tests fail);
`--lib pool_connection` at 2× takes 42.31 s. Either way it is a suite time
presented as a test time, and compared against a single-test time.

As written the record says the new cap is 2.3× **slower** than the cap it
replaces. It is ~27× **faster**. A figure in the authoritative record that does
not reproduce — in the paragraph whose job is to withdraw a figure that did not
reproduce.

## R3-F4 (minor) — the detection threshold is 4×, not "a raised limit"

The source comment claims *"4 MiB is four times the real limit and independent
of it, so a raised limit runs out of iterations and fails cleanly here."* A 2×
raise **passes** this test in 0.20 s; only ≥4× reaches the ceiling. Not blind
overall — I confirmed that at 2× the two socket tests
(`the_receiver_loop_really_drops_a_newline_free_stream`,
`the_first_job_after_a_flood_is_not_swallowed_by_the_stale_buffer`) fail in 30 s.
So the suite catches a 2× raise; this test does not, and the comment should say
which. Same distinction round 2 drew for the old cap.

## R3-F5 (minor) — the PR body was never updated

Round 2 raised this; it is unchanged. `gh pr view 23` still says **"Seven
tests"** (nine), **"Four mutations are each caught"** (six, plus the wiring and
`pending.clear()` ones), **"157 lib + 18 bin"** (159+18), and still asserts the
constant means the two paths **"cannot drift apart"** — the claim this branch's
AUDIT explicitly corrects. It mentions neither the `pending.clear()` gap, nor
the cap that did not work, nor #27. Third PR in this repo to reach review with
a stale body.

## R3-F6 (minor) — the source comment still says "cannot drift apart"

`src/pool_connection.rs:214-215` is unchanged by the branch's correction:

```rust
/// `read_line` has always had this limit; the long-lived `receiver_loop` did
/// not, which is the asymmetry GitHub #21 records. The value lives here so the
/// two cannot drift apart.
```

`AUDIT.md:6008` records the correction — *"'Cannot drift apart' was true of the
number, not the behaviour"* — so the ledger is not false. But the commit message
lists this round-2 item among its fixes with no verb of action, and the diff does
not touch the line. The source still asserts the retracted version, one file away
from the entry retracting it. Add the `MAX+4096` clause here.

## R3-F7 (nit) — "filed rather than widened" names no issue

AUDIT says the `read_line` gap was *"filed"*; it does not say **#27**. Neither
does the PR body. A reader of the authoritative record cannot reach the issue.

## Priority 4: is the `read_line` disclosure honest and sufficient?

**Yes.** I verified the claim rather than taking it: mutating
`src/pool_connection.rs:833` `>` → `>=` leaves the full lib suite green
(159 passed, 0 failed). The gap is real, the AUDIT paragraph states it without
hedging and explicitly flags that the Verification sentence above it does *not*
cover it, and #27 is specific (three surviving mutants named, four concrete test
cases, a note that `PoolStream::Plain` is trivially constructible). The PR's
edit to that line is a literal-to-constant substitution with no behaviour change.
Deferring is defensible and the disclosure is sufficient — apart from R3-F7, the
missing number.

## Two checks the brief names, closed explicitly

- **`AUDIT.md` append-only.** Cleared. `e3b339f` edits the SEC-03 entry in
  place, which `CLAUDE.md` step 0 permits — *"an entry added on an unmerged
  branch may still be edited in place"*. SEC-03 is unmerged. The entry does not
  claim to append while editing.
- **The tip commit contains no production-code change.** Both
  `src/pool_connection.rs` hunks in `e3b339f` are inside `mod tls_tests` — the
  deleted doc block and the cap. Its entire risk surface is tests, docs and the
  committed artifacts; nothing in it can affect a hash. That is also why the two
  majors are the change's risk rather than a sideshow.

## What I could not verify

- No end-to-end run against a real pool (unchanged from rounds 1-2).
- **I did not re-run the four helper mutations AUDIT names** (removing the
  check, raising the limit 1000×, draining one line instead of all, off-by-one
  `>=`). The 1000× one I covered incidentally at `1 << 30`; the other three I
  took from the record. Note the committed `mutants.out/caught.txt` is a
  *different* set — cargo-mutants' own eight — generated 2026-09-16 and stale.
- I did not run `make verify-jit` — the diff touches no JIT code, no emitter,
  no `vm.rs`. Nothing here is in `jit-reviewer`'s scope.
- CI: I ran `cargo test --release` and clippy locally on this head, not the
  five GitHub checks.
- I did not re-derive round 2's 18.29 s for the old cap; I took it from the
  ledger and measured only the new cap.

## Handoff

Nothing in this diff belongs to `jit-reviewer` (no `src/randomx/jit/`, no
emitter, no `vm.rs` native-loop path, no `benches/`, no hashrate claim) or to
`ci-reviewer` (no `.github/workflows/`, no `Makefile`, no `scripts/`, no
`.cargo/config.toml`). Reviewed in full here.
