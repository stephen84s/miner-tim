# REVIEW_PR43_R4 — round 4 (post-rebase onto bf4868b / NET-06), Opus, pr-reviewer

Scope: verify the rebase of PR #43 past PR #45 (NET-06). Head reviewed: 162e428.
Pre-rebase head: 0c2439f (base b4501f0). Post-rebase base: bf4868b (= origin/main).
No JIT / CI / bench files touched — no hand-off needed.

## Coverage ledger
1. write_and_register body vs main's inline version — DONE, faithful
2. submit_share call site / log line — DONE, identical format string
3. rotation_wait_since work unaltered — DONE, interdiff clean
4. spot tests + NET-06 reproducers — DONE
5. full suite + baseline — DONE 188/4/20; main 181 listed - 4 ignored = 177; +11 #[test], 0 removed
6. up to date + CI — DONE, merge-base == origin/main == bf4868b; 6/6 checks green on 162e428
7. AUDIT wording — DONE, findings below
8. mutants — RUNNING

## Method
Interdiff: `git diff -U0 b4501f0 0c2439f` vs `git diff -U0 bf4868b 162e428`, +/- lines only, per file.
pool_connection.rs: the ONLY differences are the timing captures (removed-from-main lines now
include NET-06's Instant lines; write_and_register gains them; call site becomes a match).
miner.rs, bin/minertim.rs, tasks/NET-05.md: zero interdiff. CLAUDE.md / tasks/README.md: only the
context of main's own task-board lines moved (NET-04 -> NET-06), expected.

## Findings

R4-F1 (minor, docs): AUDIT.md NET-05 ledger shas are all unreachable after this rebase.
  Line ~7372 records 995a1ee / 65b8c4d / 3ac05c2. `git for-each-ref --contains` is empty for all
  three; origin's branch and refs/pull/43/head both = 162e428. Same paragraph asserts "this branch
  was never rebased again after round 2" — now false. Rebased equivalents, content verified
  byte-identical with cmp: 995a1ee -> 828d76b (REVIEW_PR43.md), 65b8c4d -> e5a20a7
  (REVIEW_PR43_R2.md), 3ac05c2 -> 8fb2b65 (REVIEW_PR43_R3.md). R2-F1's lesson, third time.

R4-F2 (minor, docs): "Post-review rebase note" says main advanced "twice more (PR #39's LIVE-03
  entry, then PR #45)" after round 3's fix. False: 06ec147 (#39) is an ancestor of 0c2439f
  (`git merge-base --is-ancestor` true); #39 merged 09:43, #42 13:21, round-3 fix 16:17 on Oct 2.
  Only #45 landed after round 3.

R4-F3 (minor, docs): verification line ~7315 still says "2 ignored (unchanged)" and "after ... the
  post-#37 rebase"; the rebase note says 4 ignored. The entry now argues with itself. Measured: 4.

R4-N1 (nit): write_and_register's `///` doc comment narrates merge history ("carried through from
  main's version ... merged here when rebasing"). That is AUDIT material; the doc comment should say
  what the tuple is. Also the substantive design comment above it is `//`, so rustdoc shows only the
  timing note (pre-existing structure, not introduced here).

## Verified-correct (no finding)
- Capture positions: lock_requested immediately before stream.lock(); lock_acquired immediately
  after the lock() `?`; write_done immediately after write_request `?`. Same as main.
- Tuple: ((lock_acquired - lock_requested), (write_done - lock_acquired)) * 1000.0 ms — same
  expressions, same order; destructured as (lock_wait_ms, write_ms) at the call site, matching.
- Lock scope: stream_guard held across write AND pending insert, released at fn return (before the
  log), as in both parents. Error paths return before timing exactly as main's `?`s did.
- Log line byte-identical to main's: "Share submitted: rpc_id={} job_id={} nonce={} lock_wait_ms={:.3} write_ms={:.3}".
- Tests: submit_share_registers_the_same_id_it_writes_to_the_wire ok; a_stale_entry_does_not_survive_a_two_value_ring_round_trip ok;
  release --ignored: quiet reproducer FAILED at 2.796s (as on main), chatty control ok at 5.2ms.
- Full debug suite: 188 passed / 4 ignored lib, 20 bin. main --list: 181 total, 4 ignored.
- clippy --all-targets clean. gh pr view 45: MERGED, mergeCommit bf4868b.

R4-F4 (major, record): PR #43's description is stale from round 1 and becomes the squash commit
  message. `gh pr view 43 --json body` says "7 new tests + 2 extended", "178 lib (+7)", mutants
  "17 tested, 13 caught, 1 unviable, 3 missed" with the round-1 scope; nothing about rounds 2-3's
  fixes (reconnect() clear, note_donation_target), nor the NET-06 merge resolution. Actual: 11 new
  tests, 188 lib / 4 ignored. NET-06's own review graded a stale PR body as a major (AUDIT ~7254).

Checked, no finding: the rebase's "now confirmed directly" upgrade for NET-06 is supported by
  NET-06's merged text (AUDIT ~7217: lock_wait_ms within 2-3ms of found_to_submit_ms at every
  percentile; ~7230: both rejections attributed to the starvation). PR #45 is MERGED (bf4868b).
