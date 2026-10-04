# REVIEW_PR56 — Fix #53: compare current_job by Arc identity (NET-10)

Reviewer: pr-reviewer (Opus 5.5), independent. Branch `fix/issue-53-job-blob-staleness`, head 1058a1d.
Base check: `git fetch origin`; merge-base == origin/main == e7f17c7. Up to date.

Scope: src/miner.rs, src/pool_connection.rs (doc only), AUDIT.md, tasks/. No jit/, benches/,
workflows, Makefile, scripts/ touched -> nothing handed off to jit-reviewer / ci-reviewer.

## Coverage ledger
| # | Item | Status |
|---|------|--------|
| 1 | Correctness | done, no defect |
| 2 | Silent failure | done, no finding |
| 3 | Safety switches | done, untouched by diff |
| 4 | Tests / break-tests | done (F1) |
| 5 | Resource use | done, no finding |
| 6 | Docs / audit | done (F2, F3, N1, N2) |
| 7 | Concurrency | done, no finding |

## Findings
### Verified (no finding)
- **Install invariant.** `current_job` is touched at exactly 4 sites in pool_connection.rs:
  `login()` (~644, `*current = Some(Arc::new(job))`), `handle_pool_message()` (~1194, same),
  `reconnect()` (~1057, `= None`), `relogin_as()` (~1125, `= None`). Plus `get_work()` (~655) which
  only `.clone()`s the Arc out. No `get_mut`/`make_mut`/re-wrap anywhere; no code outside
  pool_connection.rs touches the field (only `miner.rs` calls `get_work`). `Job: Clone` is used
  only by the new test. Claim holds.
- **is_new_job truth table.** `!cached.is_some_and(|c| ptr_eq(c,f))`: None -> !false = true;
  same Arc -> !true = false; distinct Arc (any content) -> !false = true. All four cases correct.
  ABA: worker holds `Option<Arc<Job>>`, keeping the cached allocation alive (also across a
  reconnect's clear), so a freed address can't be reused under it. Correct.
- **worker_loop read in full (589-895).** Only other comparison is `job.seed_hash != current_key`,
  a content comparison of the dataset key -- correct as content (the dataset is a function of the
  seed bytes), not a job-identity check. All `job.job_id`/`job.target`/`job.generation`/`job.blob`
  reads use the per-iteration fetched Arc. After the fix the cached blob always derives from that
  same Arc, so blob/target/job_id/generation are now mutually consistent within an iteration --
  which pre-fix they were not under id reuse. `nonce` never reset on job change (confirmed);
  `pipeline_ready=false` on a new job discards the old job's in-flight prepare. No missed site.
- **Empty job_id edge case.** Old: first job with id "" never copied its blob, so the loop sat in the
  short-blob branch forever, logging a *false* "blob too short (N bytes)" with N = real blob length
  (it prints `job.blob.len()` while testing the empty cache). New: mines it and submits with
  job_id "" -- worst case a pool reject. Neither path can produce a wrong hash. Strictly not worse;
  NET-10 understates how broken the old behaviour was (nit).
- **Break-test 1 (revert to job_id-only), redone:** 3 passed / 2 failed (`a_reused_job_id...`,
  `an_identical_reinstall...`). Matches NET-10. Restored, `cmp` + `git diff --quiet` clean.
- **Break-test 2 (job_id||generation alternative), redone:** 4 passed / 1 failed
  (`an_identical_reinstall...`). Matches NET-10; design choice is genuinely exercised. Restored clean.

### F1 (minor) -- the call site in worker_loop is guarded by no test; NET-10 overstates its evidence
All of NET-10's hand break-tests mutate the body of `is_new_job`; none touches `worker_loop`, which
no test calls (sole caller miner.rs:249). Yet NET-10 calls those three "the integration evidence"
for the call site. My call-site mutations:
- A: `current_job = Some(Arc::clone(&job));` -> `current_job = Some(Arc::new((*job).clone()));`
  (re-wrap; every iteration then looks like a new job -> one extra `prepare_scratchpad` per hash
  (not measured); hashes stay correct). **Full suite 200/0/3 + 20 green, clippy -D warnings
  clean.** Not caught by anything.
- A-variant: deleting the assignment outright is caught, but only incidentally by `unused_mut`
  under clippy -D warnings.
- B: call site reverted to `current_job.as_ref().is_none_or(|c| c.job_id != job.job_id)` (#53
  restored at the call site, helper intact). Caught only incidentally: clippy's `dead_code` on
  `is_new_job` in the non-test lib build. (Inferred, not run: a variant that keeps the helper
  referenced would pass. Mutation A already shows the gap empirically.)
Severity minor: no wrong-hash path (the realistic surviving mutation is a perf regression), and the
change is small enough to review by eye. But the AUDIT sentence "the three hand break-tests above are
the integration evidence for it" is inaccurate. **Correcting that sentence is a merge condition**
(same edit as F2). Extracting the job-change block (cache update + blob copy) into a testable
function is a non-blocking follow-up.
Checked and withdrawn: I first wrote that cargo-mutants could reach the call site. `cargo mutants
--list -f src/miner.rs` lists 34 `worker_loop` mutants, none at lines 638-639 (operators/returns
only). NET-10's "not mutation-testable the same way" is **accurate**. Count that as a false
positive that I raised and then retracted myself.

### Reproduced
- Doc placement checked: main's lines above `#[allow]` are `classify_share`'s closing brace, with no
  `///` block, so `is_new_job` orphans nothing. The `stream` field's doc ends at `stream:`, and the new
  `current_job` doc attaches to its own field. Commit messages scanned: no undisclosed errors
  beyond the two NET-10 already owns.
- Working-tree hygiene: one mutation-B command aborted on a shell error before its restore step,
  which left `src/miner.rs` dirty briefly. I restored it from the scratchpad copy in the next command and
  verified it with `cmp` and `git diff --quiet`. Every other mutation was restored and verified the same way.
- `rtk proxy cargo test --release --locked`: 200 passed / 0 failed / 3 ignored (lib) + 20 (bin). Matches.
- `cargo clippy --all-targets -- -D warnings` and `--release`: both exit 0. Matches.
- `./scripts/mutants.sh 'is_new_job' 'miner::'`: exit 0, 3 mutants, 3 caught (7s baseline, 12s). Matches.
- Item 3: diff touches no `--native-loop`/`--verify-shares` code. Item 5: each worker now keeps one
  `Arc<Job>` (tens of bytes of blob) alive past a reconnect clear -- negligible, and it is what makes
  the identity check ABA-safe. Item 7: no new lock; `get_work()` lock per hash is pre-existing.

### F2 (minor) -- NET-10 / tasks/NET-10.md are stale now that PR #56 exists
NET-10 is not on origin/main (`grep -cF NET-10` = 0), so in-place correction is fine. Stale text:
"Review: Not yet reviewed -- no PR has been opened"; "Commits (4, ...; no PR opened yet)" while the
list has 5 entries (4 hashes + "this commit", 1058a1d); "Not Established: No PR opened ... no CI
run"; tasks/NET-10.md "No PR opened yet, no review". The PR #56 body says "(status: Active -- no review yet)" and lists independent review as outstanding.
Update it after this review. Its test counts and break-test descriptions match what I reproduced.
Needs a `**Review (Opus, round 1): ...**`
paragraph recording this review's findings and false positives.

### F3 (note) -- no expected review findings recorded before this review
Neither NET-10 nor tasks/NET-10.md wrote down predicted findings before the review ran. Not a blocker.

### N1 (nit) -- empty-job_id disclosure understates the old behaviour
See verified section: old code never mined such a job and logged a false "blob too short" warning.
NET-10 says only that it "would never copy its blob". The change is an improvement, not a neutral one.

### N2 (nit) -- the #3785 quotation is not verbatim
NET-10 quotes "Our worker nonce is monotonic across job changes; we never reset it". The source
(AUDIT.md:833) reads "...across job changes (`miner.rs:341`); we never reset it". The meaning is
unchanged, but the text inside the quotation marks has been edited without saying so.

### CI
At review time: lint, audit, test, mutation(advisory) pass; jit-macos and jit-linux-arm pending.
The diff touches no JIT code, so those jobs cannot be affected by its substance, but they are
required checks.

## Verdict
Mergeable once three things are done: (a) F2's stale audit/PR text is fixed; (b) F1's "integration evidence"
sentence is corrected; (c) the required jit checks go green. The code change is
correct: the install invariant holds at all 4 sites, the predicate is right for every case, it is
ABA-safe, and no other identity comparison in worker_loop was missed. Not verified: behaviour against a live pool that actually reuses job_id (none known, as NET-10
states); the JIT CI jobs, which were still pending.
