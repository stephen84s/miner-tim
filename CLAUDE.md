# AI Agent Protocol / Project Manager

> **Note to AI:** This section defines your operational logic. You are the **Project Manager** and **Lead Engineer**. Follow these rules strictly.

## Identity & Mandate
- **Role:** Project Manager & Lead Engineer.
- **Mandate:** Execute tasks, verify code health, and maintain the `AUDIT.md` ledger.
- **Constraint:** **No implementation is complete until it is committed to `AUDIT.md`.**

## Operational Protocol
0.  **Process rules, before the task itself.**

    - **Reviewer agents.** Independent review uses the repo's own agents in
      `.claude/agents/`, not ad-hoc briefs: **`jit-reviewer`** for
      `src/randomx/jit/`, the emitter, `vm.rs`'s native-loop path or `benches/`;
      **`ci-reviewer`** for `.github/workflows/`, `Makefile`, `scripts/` or
      `.cargo/config.toml`; **`pr-reviewer`** for everything else. They share
      `.claude/agents/_shared-context.md`, which carries this repo's failure
      history and the verification rules. **Spawn cold, one per round** — never
      resume a reviewer across rounds; a long-lived one reached 560k tokens of
      context and could no longer start.

    - **Worktrees for concurrent branches.** When more than one branch is in
      flight, give each its own worktree under `.claude/worktrees/` rather than
      switching branches in the shared checkout:
      `git worktree add .claude/worktrees/<branch-with-dashes> <branch>`. The
      primary checkout stays on `main`. **That directory must stay in
      `.gitignore`** — a tracked worktree is committed as a gitlink, which is how
      `.claude/worktrees/platform-neutral` got into `3b2cc9d` and had to be
      stripped during the SHA-256 conversion. This rule exists because switching
      branches while two reviewers were running made one of them commit its
      ledger to the wrong branch.

    - **Correcting `AUDIT.md`.** An entry already merged to `main` is corrected
      by **appending**. An entry added on an unmerged branch may still be edited
      in place — it is not yet part of the record. Never claim to append while
      editing in place.

    - **CI runs only where a PR exists** (the gating workflows, that is —
      `release.yml` is separate and fires on a `v*` tag). `ci.yml` and `jit.yml`
      trigger on `pull_request` and `workflow_dispatch` only — never on `push`.
      A push to a branch that has an open PR runs the five checks against that
      PR's `refs/pull/N/merge`, the merge of head into base rather than the head
      commit itself. A push to a branch with **no** open PR runs nothing at all.
      This is deliberate (CI-03): `main` requires branches to be up to date, so
      that merge ref and the tree that lands are the same tree, and a post-merge
      pass would only re-test an identical one. Two consequences to hold in
      mind: open the PR early if you want the checks running, and never read
      "I pushed and nothing went red" as evidence about a bare branch — run
      `make verify-jit` locally there.

    - **Rebase on `main`, then merge on green.** A PR merges only when it is
      **rebased on the current `main`** and all five checks are green on that
      rebased head. Branch protection enforces up-to-date-ness (`strict: true`)
      but accepts a merge commit as satisfying it; prefer
      `git rebase origin/main` so the branch keeps a linear history for review
      and the tested tree is exactly the tree that lands. (`main` itself stays
      linear either way, since PRs are squashed — what a rebase buys is a
      reviewable branch and no merge-commit noise in the diff.) Rebasing rewrites the branch,
      so do it *before* asking for review, not after — and never while a
      reviewer agent is running against that worktree.

    - **Batch the push, not the commits.** Keep making separate, logical
      commits — one per coherent change, so the history stays reviewable **while
      the PR is open** and a single mistake can be reverted on its own *during
      review*. Be honest about the horizon: every PR in this repo has landed as
      one squashed commit, so these commits are a reviewing and bisecting aid
      inside the PR's window, not history that survives into `main`. What to hold back is the
      **push**: every push to a PR's head branch starts a full pass (five jobs,
      ~30 runner-minutes, ~15 minutes of wall-clock, `jit-macos` the long pole)
      and cancels any run still in flight, so a series of small pushes burns
      runs and restarts the reviewer's clock. Finish the work, then push once.
      Do not squash logical commits together merely to reduce the push count —
      that trades away history for nothing, since one push carries any number
      of commits. The repository is public, so GitHub bills **zero** minutes;
      what is saved is queue time, runner capacity and a reviewer's attention.
      Worth saving — but never by skipping a verification step to avoid a run.

    - **Branch and PR, always.** `main` is protected: direct pushes are
      rejected, a pull request is required, and all five CI checks must pass —
      including for admins. Work on a branch, open a PR, and have an
      **independent reviewer agent** examine it before merge. Followed for
      MRs !1–!4, then quietly dropped at the GitHub migration: six commits
      reached `main` unreviewed, two of them changing CI workflows, and several
      correcting earlier mistakes — the worst place to skip review. Protection
      enforces the **branch, the PR and the checks**; it does not and cannot
      enforce that a reviewer looked. At `required_approving_review_count: 0` an
      author can still merge their own PR unreviewed, and the same account can
      disable protection. **Spawning the reviewer is still on you.**

1.  **Task Analysis:** Break user requests into atomic steps.
2.  **Execution:** Implement changes in the repository.
3.  **Audit:** **Immediately** after implementation, append a detailed entry to `AUDIT.md`.
4.  **Status Update:** Update the `Current Task` table at the top of this file (`CLAUDE.md`) to reflect the new state. Do not leave tasks "Active" if they are completed.
5.  **Review:** Before replying "Done", verify `make check` and `make test` passed.
6.  **JIT gate:** Any change touching `src/randomx/jit/`, or `vm.rs`'s
    native-loop path, must **pass the JIT gate** — but running it by hand is no
    longer your duty, and pasting its output into a PR is no longer evidence
    anyone needs. CI enforces it on every **pull request** (`jit-macos` on
    `macos-14`, `jit-linux-arm` on `ubuntu-24.04-arm`), both required checks on
    a protected `main`, so a failure blocks the merge rather than merely
    reporting. CI evidence has replaced the self-reported kind, which is what
    issue #6 asked for; the demotion is real and `make verify-jit` is a
    convenience now, not a checklist item.

    Note what CI does *not* cover — see step 0's **CI runs only where a PR
    exists**, which is the single authority on the triggers; the short version
    is that **a branch with no open PR is checked by nothing at all.** That is
    the one window worth running `make verify-jit` in yourself, and it is
    cheaper than a red PR, since the `jit-macos` job takes **~14 minutes** end
    to end (13.95 min mean over all 27 successful runs; earlier derivations at
    n=8 and n=12 gave 14.08 and 13.94). An earlier version of this sentence said
    "the macOS debug profile takes ~8 minutes", which does not reproduce.
    `make verify-jit-linux`
    runs the same gate under native linux/arm64 and is worth running directly
    when `jit/memory.rs` or other platform-conditional code changes. Never cite
    the x86_64 jobs as evidence about the JIT. See **Platform coverage** below.

## Current Task Board

> **Issue-numbering convention.** A bare `#N` means the **GitHub** issue. The
> migration renumbered everything — GitLab 1→1, 2→2, 5→3, 6→4, 8→5, 9→6 — and
> GitLab #3, #4 and #7 were closed before it and were never imported, so those
> take the explicit form `GitLab #N`. Where a historical reference needs its
> modern number too, write `GitLab #6 (now GitHub #4)`.
>
> **Scope:** this table, `README.md`, the `Makefile`, `scripts/` and the workflow
> comments. Older `AUDIT.md` entries and `src/` comments predate the rule and
> still carry pre-migration numbers; fix those where they actively mislead
> rather than sweeping them. The rule exists because stale cross-references have
> been found in three consecutive review rounds — twice inside the commit that
> was fixing them.

| Status | Task ID | Description |
| :--- | :--- | :--- |
| **Completed** | **SYS-01** | **Agent Initialization.** Establishing management protocol. |
| **Completed** | **NET-01** | **Pool robustness.** Fixed TLS double-session receiver bug; added keepalive, auto-reconnect/relogin, 8-byte target support. Clippy backlog cleared (63→0). |
| **Completed** | **PERF-01** | **Bench harness + P-core threads.** Added criterion benchmark (regression guard); default threads now = performance-core count (macOS `hw.perflevel0.logicalcpu`) instead of 2. Confirmed via xmrig docs that affinity/huge-pages are unavailable on ARM macOS. |
| **Completed** | **SEC-01** | **Dependency vuln scanning.** Added `make audit` + `rust:audit` CI job (cargo-audit / RustSec). Fixed 3 rustls-webpki advisories via 0.103.10→0.103.13. Rest of `.gitlab-ci.yml` noted stale (Android paths). |
| **Completed** | **CI-01** | **CI rewrite.** Replaced stale Android/Gradle pipeline with lint (clippy -D warnings), audit, and test jobs for the root CLI crate. |
| **Completed** | **RUST-01** | **Toolchain upgrade 1.94→1.97.1.** Fixed 2 new clippy lints; dropped the (incorrectly-added, never-clean) fmt gate; 87 vectors pass on new compiler. |
| **Completed** | **EDITION-01** | **Edition 2021→2024.** Migrated in a worktree; A/B benchmark showed no perf impact (the "8%" was thermal noise); 87 vectors pass. Merged. |
| **Completed** | **DONATE-01** | **Donate-level.** XMRig-style donation: default 5% (min 1%, sub-1% needs recompile), split 50/50 author/XMRig via rolling login rotation. Disclosed at startup + README. Version → 0.1.0. |
| **Completed** | **RELEASE-01** | **Release flow.** `make dist` (portable apple-m1 tarball + SHA256SUMS), `make release` (tag+push), CI release job on `v*` tags, RELEASING.md. Agent string tracks CARGO_PKG_VERSION. |
| **Completed** | **PORT-01** | **xmrig upstream survey (v6.24–v6.26).** Most items N/A (RISC-V, Zen4/5, VAES, Windows ARM64). RandomX v2 still blocked — Monero mainnet is on HF 16, no v17 entry in `mainnet_hard_forks`; `PLAN_RANDOMX_V2.md` refreshed. JIT `emit_mem_addr` opt (#3708) implemented, measured at 0.35% fewer emitted instructions, **reverted** — codebase is latency-bound, not instruction-count-bound. |
| **Completed** | **BENCH-01** | **xmrig 6.26.0 vs MinerTim ABAB benchmark.** MinerTim won both positions (+4.4%, +13.1%); no rx/0 hashrate gap exists. NEON FP closed for v1 (re-gate under v2 only). Next: RandomX v2 gated port. |
| **Completed** | **RESEARCH-01** | **Research delivered**: RANDOMX_V2_SEMANTICS.md (v2 = 5 changes; plan's Stratum mapping was backwards — corrected in place) and NEON_FP_PORT_NOTES.md (~300-400 LOC port, ABI risk eliminated). V2 implementation unblocked on semantics. |
| **Completed** | **RX2-01** | **RandomX v2 gated port (offline half).** RxVersion plumbing, 384-instr programs, conditional CFROUND (interpreter + JIT), AES F/E mix (NEON/AES-NI/soft), mp-aliasing prefetch, commitment fn. All reference vectors pass on interpreter AND JIT paths; 87 v1 vectors bit-identical. Nothing selects V2 at runtime yet. Fork-day remainder (dispatch + Stratum) documented in AUDIT.md 2026-08-16. |
| **Completed** | **JIT-01** | **JIT native iteration loop.** Stages A-D done on `feat/jit-native-loop` (MR !1): the 2048-iteration loop is emitted in ARM64 and is now the **default** for rx/0 + full mode + aarch64. Measured **+6.8%-+7.4%** at 11 threads across two independent runs (96/96 paired rounds positive; per-run CIs are tighter than the between-run spread and do not describe reproducibility), via the new paired A/B harness, which also verified ~147k hashes bit-identical against the body JIT. Thirteen rounds of independent review (round 5 caught the harness measuring the native loop against itself; the earlier +9.01% claim is retracted). Round 13: **mergeable, no blockers, no majors**; its three minors plus three older deferred findings are filed as GitLab issues #3–#8. Also repaired CI, red on `main` since the edition-2024 migration. **Merged as MR !1 (`365d288`).** |
| **Completed** | **VIS-01** | **Silent MAP_JIT fallback made visible (GitLab #4 + #3 — closed pre-migration, never imported).** `JitCompiler::new()` failure is now logged at `error!` instead of `.ok()`-swallowed; `RandomXVm::native_loop_effective()` evaluates all four native-loop preconditions from the VM's own fields and is the single authority for both the per-worker startup report and for arming the share verifier. The startup line in `main` now reports the *request* through a testable `startup_state_line()`. Closes GitLab #3 as a side effect: the verifier's enablement no longer carries its own `cfg!` term, so x86_64 verification goes from armed-but-vacuous to off. Independent review returned **mergeable, four minors**; all four closed on the branch (two orphaned doc comments, the missing positive assertion on `native_loop_effective()`, a vacuous test assertion, and an over-stated `new_jit()` error), plus the false non-aarch64 warning text. Review record: `REVIEW_ISSUE4.md`. **Merged as MR !2 (`1790a9f`); GitLab #3 and #4 closed.** |
| **Completed** | **PLAT-01** | **JIT ported to Linux aarch64 (issue #2, phase 1a).** `JitMemory` split into cfg'd platform arms: Darwin keeps `MAP_JIT` + `pthread_jit_write_protect_np` + `sys_icache_invalidate` byte-for-byte; Linux uses `mmap(RW)` -> `mprotect(RX)` + `__clear_cache`, with checked `mprotect` and constants read from the platform headers. `compiler.rs` unchanged, as the API shape was preserved. The "only `memory.rs` is platform-specific" assumption **held** and was confirmed. Verified natively on arm64 Linux (colima, no emulation): full suite **131 lib + 10 bin, 2 ignored, 0 failed** — parity with macOS — including the native-loop differential tests against the interpreter and `full_mode_v1_vm_reports_the_native_loop_effective`, the one test that hard-requires a live JIT allocation. Phase 1b (the arm64 CI job) **not** done: the pipeline has no arm64 runner (`no_matching_runner`). |
| **Completed** | **PLAT-02** | **JIT gate made explicit (GitHub #2 interim mitigations).** `scripts/verify-jit.sh` + `make verify-jit` (macOS host) and `make verify-jit-linux` (native linux/arm64 via colima; read-only repo mount, container-local `CARGO_TARGET_DIR`). 92 tests — JIT unit + native-loop differential + known-answer vectors — in **both** debug and release, so the native loop's `debug_assert!` guards finally execute (GitLab #6 — now GitHub #4). Hard gates: non-zero exit on any failure *and* on an unexpected test count, so a renamed module cannot empty a filter and leave the gate green (verified by a deliberate drift run). Platform-coverage wording landed in README + this file (CI validates the interpreter on x86_64 Linux only; GitLab #9 — now GitHub #6 — is the GitHub-Actions plan); F11's Linux `mprotect`-per-compile cost recorded in `jit/memory.rs`; the gate documented as mandatory before any MR touching `src/randomx/jit/`, with its result pasted into the MR description. |
| **Completed** | **MEM-01** | **Test-suite peak RSS (GitLab #7).** The binary held two never-freed 2 GiB `LazyLock` datasets; now one. Measured, not inferred: release `--lib` peak **8.16 GB → 6.23 GB** at this host's 12-thread default, **~4.07 GB** at `--test-threads=3` (the macos-14 runner's core count, ~2.9 GB headroom under 7 GB); debug `verify-jit` filter **6.27 GB → 5.43 GB**; wall clock 94s→50s and 316s→193s. The issue's "~4.5 GiB" estimate was wrong — the real 12-thread baseline was 8.16 GB. (Review corrections: the debug pair was first recorded as 6.77→4.50 GB and does not reproduce; and "already over GitLab #9's (GitHub #6's) budget" holds only at 12 threads — at the runner's 3 cores `main` measured 6.00 GB, marginal rather than over.) Differential coverage is unchanged: the diff tests' programs, entropy, ma/mx and `dataset_offset` all derive from the seed, not the key, and both paths read the same dataset. The verifier rotation test genuinely needs a second distinct dataset (R9-F2), so it got a synthetic zeroed one. `make verify-jit` 92/92 debug+release; 131 lib + 10 bin green. Unblocks GitLab #9 (GitHub #6). |
| **Completed** | **CI-02** | **GitHub Actions workflows (GitLab #9 — now GitHub #6 — workflows only).** `.github/workflows/ci.yml` ports `.gitlab-ci.yml`'s three x86_64 jobs to pinned `ubuntu-24.04` (`lint` = clippy `-D warnings`, no fmt gate and the reason preserved; `audit` = cargo-audit, keeping the "binary already exists in destination" install guard; `test` = `cargo test --release --locked`). `.github/workflows/jit.yml` adds the two jobs that are the point of the move: **`jit-macos`** (`macos-14`, `make verify-jit` — the Darwin `MAP_JIT`/W^X path no GitLab tier can run) and **`jit-linux-arm`** (`ubuntu-24.04-arm`, `scripts/verify-jit.sh` **directly** — the colima/Docker wrapper dropped because the runner *is* native aarch64; its toolchain pin and host-facts print kept). All five are hard gates — no `continue-on-error`, no `|| true`, no step-level `if:`. The 7 GB `macos-14` box is handled with a job-level `RUST_TEST_THREADS=3` (MEM-01, GitLab #7: 4.07 GB at 3 threads vs 6.23 GB at 12), set explicitly rather than left to the runner's core count; libtest's reading of that env var was verified empirically, not assumed. Two files so the ~19-min interpreter suite and the JIT verdict do not gate each other. *(State at the time of writing: nothing had run on GitHub — runner specs and RAM headroom were asserted from the issue, not tested. Superseded by MIGRATE-01, which ran them.)* |
| **Completed** | **MIGRATE-01** | **Migrated to GitHub; aarch64 JIT under CI for the first time.** GitLab could not run it — x86_64 runners, `no_matching_runner` for arm64, then `ci_quota_exceeded` — and no GitLab tier offers macOS at any price. Repo converted SHA-256→SHA-1 (GitHub supports only SHA-1, established from its own protocol advertisement and from REST/GraphQL having no object-format parameter); 181 commits and 3 tags preserved, trees identical bar an accidental gitlink, 118 commit references remapped from a verified 188-entry mapping, `SHA256_TO_SHA1_MAP.txt` committed. Five jobs green: `lint`/`audit`/`test` on `ubuntu-24.04`, **`jit-macos`** (`macos-14`) and **`jit-linux-arm`** (`ubuntu-24.04-arm`), each running the 92-test gate in debug **and** release. The first `macos-14` run caught a latent build failure invisible for the project's life: `-C target-cpu=native` resolves to a feature-poor model on a virtualised runner and trips a `ring` compile-time assertion; fixed with `target-cpu=apple-m1`, matching `make dist`. GitLab archived read-only with a pointer; working copy moved to `code/github/miner-tim`. Its "Closes issues #2 and #4" was wrong twice: the two issues tracking this gap are **#2** and **#6**, and neither was closed when that was written. #2 closed 2026-09-06; #6 closes with the PR that corrected this line. |
| **Completed** | **PROC-01** | **`main` protected after six unreviewed commits reached it.** PR required, all five checks required, strict up-to-date, `enforce_admins: true`, force-push and deletion blocked, 0 approvals (a solo maintainer cannot approve their own PR). Verified by a refused direct push (`GH006`) *and* by reading the live API — the push test alone covers only 2 of the 7 settings. Enforces branch/PR/checks, **not** that a reviewer looked. Three review rounds: round 1 found the entry's "CI is green on them" claim false (`6414ba1`'s JIT gate cancelled with zero jobs); round 2 found the same sentence still claiming the six commits are "all documentation" when two changed CI workflows, and that the PR body had never been updated; round 3 found no blockers and confirmed the mechanism independently (live protection matches the table, the five contexts are string-exact against the real check-run names, and pushing to the PR flipped it `clean`→`blocked`), but found the entry had been corrected by accretion into three self-contradictions — rewritten in place rather than appended to again. Ledger: `REVIEW_PR7.md`. |
| **Completed** | **CI-03** | **CI runs on pull requests only.** Dropped `push: branches: [main]` from `ci.yml`/`jit.yml`; `release.yml` untouched. Safe because `main` requires branches up to date before merging, so a PR's head is the tree that lands and the post-merge pass re-tested an identical tree. Measured cost of that duplicate pass: **30.4 runner-minutes** (mean per job over 12–14 completed runs) or **~15 min wall-clock** (n=17, median 14.52), the jobs being concurrent. Public repo, so `billable.total_ms` is 0 — this buys back wait time and runner capacity, not money. Three review rounds: round 1 caught the figures as invented and `cancel-in-progress: true` as unsafe (`workflow_dispatch` shares `refs/heads/main`, so a second manual run kills the first); round 2 caught the *corrections* — figures mislabelled as means, an un-updated PR body, and an unsupported "~19 min on GitLab" claim; round 3 re-derived every number from the run data (all reproduced exactly), confirmed the five required check contexts still match the job names a `pull_request` run produces, and found the entry's own Verification summary still citing the two methods the entry had already disavowed. Ledger: `REVIEW_PR8.md`. |
| **Completed** | **DOC-02** | **Manual JIT gate retired in prose; a false safety claim corrected.** CI-03 left three places saying the gate runs "every push" when the workflows now trigger on `pull_request` + `workflow_dispatch` only — including Operational Protocol step 6, which told a future session CI had taken the duty over. It had not: **a push to a branch with no open PR is checked by nothing.** Step 6 now states that window and inverts the advice (run it locally *more*, not less). Issue #6's retirement items done: "mandatory" dropped from the `Makefile` help text, the paste-into-MR requirement removed from the `Makefile` comment and from `verify-jit.sh`'s final `echo`, and the stale "Issue #9 tracks replacing this" deleted rather than renumbered. Both make targets kept, demoted to useful. `README.md` deliberately unchanged — "on every change ... blocks the change" got *more* accurate under CI-03. Closes #6 — but only after review found box 4 unmet (step 6 still read "must pass `make verify-jit`" and had been *strengthened*, not demoted) and five more live GitLab `#9` references, including one in `verify-jit.sh`'s own header. #2 closed separately, and one carve-out is recorded rather than hidden: `RELEASING.md` still contradicts `release.yml`, filed as #11. Three review rounds, each reviewing the previous round's *fixes*: round 2 found the same stale-numbering defect surviving inside the file the fix had rewritten, plus a "~8 minutes" figure that does not reproduce (`jit-macos` is 14.08 min, n=8); round 3 found the numbering fix had broken the very table its convention note documented — GitHub swallowed the whole task board into the blockquote, confirmed against the markdown API. Ledger: `REVIEW_PR10.md`. |
| **Completed** | **PROC-05** | **Three CI-hygiene rules recorded in the agent protocol.** User instruction, now bullets in Operational Protocol step 0: (1) Actions run only on branches that have a PR — already the implemented behaviour since CI-03, recorded for its *consequence*, that a push to a branch with no open PR is checked by nothing; (2) rebase on `main` before merging and merge only on green — **stricter than the enforced `strict: true`**, which a merge commit also satisfies, so the rule asks for a rebase to keep the tested tree identical to the landed one; (3) batch the **push**, not the commits — keep separate logical commits, but push once the work is done, since each push to a PR head costs a full ~30 runner-minute / ~15 wall-clock-minute pass and cancels any run in flight (30.33 and 14.82 over all 58 successful runs; three independent derivations at three sample sizes agree). (First drafted as "squash and push once"; corrected after the user clarified they want the logical commits kept.) Recorded with the unit corrected: the repo is public, `billable.total_ms` is 0, so what is saved is queue time and reviewer attention, not money. No workflow or code change. |
| **Completed** | **BENCH-02** | **Barriered the multi-thread A/B phase (#5).** Threads ran unsynchronised A-B-B-A schedules while the aggregation assumed round i was concurrent across threads — assuming the thing being measured. A barrier makes it true; because a barrier risks trading phase drift for **tail-idle bias**, the harness now *measures* that (levels and a paired CI, not just a difference). Review returned nine minors and **four were errors of reasoning**: a claimed "+0.13 pp point-estimate rise" was between-run drift and is withdrawn (a fourth run gave +7.05%, below both unbarriered runs); "no divergence assert fired" was never evidence about the barrier, since checksums are invariant to it; the barrier had introduced a **deadlock** on a real divergence (panic between two `wait()`s strands every sibling), now fixed and break-tested at the last thread and last pair; and the spread level was never reported, which is what makes the conclusion safe (5-8%, not 30-40%). What survives: the concurrency claim is now true, and the aggregate CI narrows across all four barriered runs (±0.19-0.41 vs ±0.43/±0.64). Per-thread paired diffs are now labelled authoritative. Bench-only; no `src/` change. Two rounds; round 2 found the corrections had repeated the pattern — a withdrawn over-claim replaced by a new one, a half-corrected baseline range, and a stale hardcoded list in the harness's own output. Ledger: `REVIEW_PR13.md`. |
| **Completed** | **REL-01** | **Release flow no longer contradicts itself (#11).** `RELEASING.md` and `release.yml` both ran `gh release create` for the same tag, so following the documented steps collided; the doc also asserted the workflow "only creates an empty entry, if it runs at all", false on every clause. Latent only because no `v*` tag has been pushed since the migration and all three existing tags predate the workflow. Fixed by deciding **who owns the entry**: CI creates it **as a draft** (a published-but-empty release is worse than none, and a draft is invisible to the public), the operator does `gh release upload` then `--draft=false`. Workflow made idempotent so a re-run reports success. Also fixed: an unparseable half-migrated sentence, and an automation note asking for a self-hosted `macos-arm64` runner that `macos-14` made obsolete. `make verify-jit` added to the release checklist — the only check that exercises emitted ARM64. **Not executed end to end**: that needs a `v*` tag pushed to the public repo, which was not done without asking. Verified by reading, not observation. Round 1 review confirmed the load-bearing draft-visibility claim from GitHub's REST docs and that `gh` resolves drafts by tag from `gh`'s own source, and ran the workflow's shell against a stub `gh` (it can still go red). It caught two defects this change introduced — the bump step never said to merge the PR and return to `main`, though `make release` tags `HEAD`; and `ci.yml`/`Makefile` comments still described the pre-draft behaviour while pointing at the issue this closes. Ledger: `REVIEW_PR14.md`. |
| **Completed** | **PERF-02** | **#1 measured and reverted — null result.** Giving `emit_cvt_packed_int` explicit destinations lets the f-load convert straight into `f_regs(i)`, dropping 2 FMOVs per lane. **Instruction saving exact and confirmed**: `iter_pre` 111->103, **131,072 fewer per hash**, matching the issue's arithmetic. **Time saving not measurable.** The criterion was committed *before* the first run — which earned its keep, because the write-up then got the verdict wrong twice and review caught both. First it claimed a *regression* at 11 threads; `main`'s own three runs span 0.29 pp on unmodified code against a 0.31 pp separation, so that was noise. Then it reported "all three criteria fail" while endorsing a gate that discards the runs producing two of those failures — on the admissible three runs criteria 1 and 2 are **unevaluable** rather than failed — criterion 2 would even read as passing at n=2 vs n=1 — while criterion 3, which carries no sample-size clause, still fails. The revert stands regardless: the rule is keep-only-if-all-hold, and a change that cannot be shown to clear the bar is not kept. `compiler.rs` byte-identical to `main`; the change was *correct* (`verify-jit` 92/92 both profiles) and bought nothing. Raw data committed as `PERF1_RUNS.log`. Same outcome as `emit_mem_addr`. Four review rounds, each finding a defect in the previous round's correction — including that `main` was the hotter arm in **all three** rounds and always ran second, so arm identity was confounded with warm-up **by construction**. Ledger: `REVIEW_PR15.md`. |
| **Pending** | - | **Awaiting User Task** |

---

# CLAUDE.md - MinerTim
Monero (XMR) CPU miner for macOS (Apple Silicon). Pure Rust — no C/FFI dependencies. aarch64 JIT compiler, pipelined hashing, full RandomX dataset mode.

## Build & Run

```bash
make build        # Release binary (target-cpu=native via .cargo/config.toml)
make run          # Build + run (reads mining.conf)
make test         # Rust unit tests, debug, whole suite (NOT the JIT gate)
make verify-jit   # aarch64 JIT gate on this Mac (CI runs this too)
make verify-jit-linux  # the same gate under native linux/arm64 (colima)
make bench        # criterion benchmarks
make check        # Quick type-check
make audit        # cargo-audit against the RustSec advisory DB
make dist         # Portable apple-m1 tarball + SHA256SUMS
make release      # Tag and push a release
make clean        # cargo clean
```

**CLI configuration:** Copy `mining.conf.example` to `mining.conf` and set `POOL`, `WALLET`, `THREADS`.

```bash
make run POOL=pool.supportxmr.com:443 WALLET=<addr> THREADS=12
./target/release/minertim pool.supportxmr.com:443 <wallet> 12
```

**Prerequisites:** Rust 1.97+ via rustup. `make verify-jit-linux` additionally
needs colima running on an aarch64 VM (`colima start --arch aarch64 --cpu 4 --memory 8`).

## Platform coverage — what CI proves, and what it cannot

| Platform | Hashing path | Verified by |
|---|---|---|
| macOS aarch64 (shipping target) | aarch64 JIT + native iteration loop | **CI** — `jit-macos` (`macos-14`), every pull request |
| Linux aarch64 | same JIT; tests only, no release artifact | **CI** — `jit-linux-arm` (`ubuntu-24.04-arm`), every pull request |
| x86_64 (Linux, CI) | interpreter only; `randomx::jit` is `cfg`'d out | **CI** — `lint`, `test`, `audit` (`ubuntu-24.04`), every pull request |

**The x86_64 jobs validate the interpreter path and nothing else.** `mod.rs`
gates the JIT on `#[cfg(target_arch = "aarch64")]`, so those runners never
compile, let alone execute, one emitted ARM64 instruction. Do not cite `lint`,
`test` or `audit` as evidence about the JIT; they are evidence about the
interpreter, the Stratum client, the miner loop and the dependency audit. The
two `jit-*` jobs are what cover the JIT. A JIT defect does not
crash — it silently returns wrong hashes and the pool rejects the shares.

The gate that does cover it is `scripts/verify-jit.sh` — run by the `jit-macos`
and `jit-linux-arm` CI jobs, and available locally through the two
`make verify-jit*` targets: 92 tests — the JIT unit tests, the native-loop
differential tests against the interpreter, and the known-answer vectors — in
**both** the debug and release profiles. Debug matters because the native
loop's `debug_assert!` guards (imm12/imm7 ranges, the CBRANCH forward-target
rule, the CBZ patch range) are compiled out of release, which is the profile
every recorded measurement used (issue #4). The script fails on any failing test
*and* on an unexpected test count, so a renamed module cannot empty a filter and
leave the gate green.

`full_mode_v1_vm_reports_the_native_loop_effective` is load-bearing inside that
set: it is the only test that hard-requires a *successful* JIT allocation. The
known-answer vectors alone pass even with an inert JIT, because the interpreter
fallback produces the same hash (issue GitLab #4).

Why this was manual until September 2026: GitLab SaaS gave this free-tier project
no arm64 runner (probed — `no_matching_runner`), no GitLab tier offers macOS at
any price, and a self-hosted runner on a public repo would let fork MRs run code
on the host. The GitHub migration closed it — `macos-14` and `ubuntu-24.04-arm`
are free for public repositories, so the gate that used to depend on a human now
blocks a merge — protection rejects a direct push before CI is even
consulted. Both issues that tracked the gap are closed.

Linux aarch64 is a *test* platform, not a shipping one: `make dist` builds an
apple-m1 tarball only, and the Linux JIT backend pays two `mprotect` syscalls
per compile (~16 per hash) where Darwin flips a userspace bit — see the note in
`src/randomx/jit/memory.rs`. "The JIT works on Linux" is not "the JIT is fast on
Linux"; no Linux throughput has ever been measured.

## Versions

| Component | Version |
|---|---|
| MinerTim | 0.1.2 (`Cargo.toml`; drives the Stratum agent string) |
| Rust toolchain | 1.97.1 (pinned in CI) |
| Rust edition | 2024 |
| serde_json | 1.0 |
| rustls | 0.23 |
| env_logger | 0.11 |

## Project Structure

```
src/
├── lib.rs                  # Crate root — pub mod declarations
├── bin/minertim.rs         # CLI entry point (args, env_logger, Ctrl+C, stats loop)
├── hex.rs                  # Shared hex_encode / hex_decode utilities
├── miner.rs                # Miner struct, worker thread pool, hashrate tracking
├── pool_connection.rs      # Stratum TCP/TLS, JSON-RPC 2.0, keepalive
├── donate.rs               # Donation addresses + rolling login rotation
└── randomx/
    ├── mod.rs              # Module exports; jit gated on target_arch = "aarch64"
    ├── vm.rs               # RandomXVm: program execution, JIT dispatch, pipelining
    ├── blake2b.rs          # Blake2b (256 and 512 bit)
    ├── blake2gen.rs        # Blake2 generator for key/program derivation
    ├── soft_aes.rs         # Software AES (4-round, no intrinsics)
    ├── aes_hash.rs         # fillAes1Rx4, hashAes1Rx4, hash_and_fill_aes_1rx4
    ├── argon2d.rs          # Argon2d cache init (256 MiB, 3 passes, 1 lane)
    ├── superscalar.rs      # SuperscalarHash program generation
    ├── dataset.rs          # Dataset item computation; SharedDatasetCache (Arc<Mutex>)
    ├── tests.rs            # Known-answer vectors + native-loop differential tests
    └── jit/                # aarch64 JIT (macOS + Linux aarch64; cfg'd out on x86_64)
        ├── mod.rs          # Re-exports JitCompiler
        ├── memory.rs       # JitMemory: MAP_JIT + W^X (macOS), mmap/mprotect (Linux)
        ├── aarch64.rs      # ARM64 instruction emitter (Emitter + reg constants)
        └── compiler.rs     # BytecodeInstruction → ARM64 (256 instrs for rx/0,
                            #   384 for rx/2); body JIT + native-loop JIT
```

Also at the repo root: `benches/` (criterion + the paired A/B harness),
`scripts/verify-jit.sh` (the JIT gate), `.github/workflows/` (CI: `ci.yml`,
`jit.yml`, `release.yml`), `AUDIT.md` (append-only change log) and the
`REVIEW_*.md` independent-review ledgers.

## Architecture

### Threading Model
- **Main thread:** CLI args, env_logger init, Ctrl+C handler, stats print loop (10s)
- **Rust std::thread:** 1 pool connection worker + N mining worker threads

### Mining Flow
1. `Miner::initialize(pool, wallet, threads)` — creates `PoolConnection`, TCP/TLS connects, sends Stratum `login`
2. Pool sends `job` (blob + target + job_id)
3. `Miner::start()` — spawns N workers; `dataset_cache = Arc::new(Mutex::new(None))`
4. Thread 0 calls `get_or_generate_dataset()` — generates 2 GiB dataset (~46s M2 Max); other threads wait on the same mutex
5. Each worker: `RandomXVm::new_full(seed, dataset)` → `prepare_scratchpad(blob)` → loop `calculate_hash_pipelined(next_blob)`
6. On hash ≤ target: if the verifier is armed, recompute the hash on the
   reference path (`ShareVerifier::reference`, a second VM with
   `set_native_loop(false)`) and compare. `classify_share` maps the outcome to a
   `ShareVerdict`; a mismatch **withholds** the share rather than submitting it.
   Otherwise `pool.submit_share(job_id, nonce_hex, hash_hex)`
7. Nonces interleaved: `nonce += thread_count`
8. New job from pool: worker picks it up via `pool.get_work()` → reinitialises VM if seed changed

### Pipelined Hashing (`vm.rs`)
`calculate_hash_pipelined(next_input)` overlaps work:
1. Runs 8 program chains on current scratchpad (JIT or interpreter)
2. Simultaneously calls `hash_and_fill_aes_1rx4` — hashes current scratchpad and fills new scratchpad for `next_input`
3. Returns the current hash; new scratchpad is ready for the next call

`prepare_scratchpad(input)` must be called once before entering the pipeline loop.

### JIT Compiler (`jit/compiler.rs`)
Active on aarch64. Two modes, and which one runs is decided by
`native_loop_applies(use_native_loop, version, has_dataset, has_jit)`
(`vm.rs:1167`) — **one predicate, called both by `execute_vm_inner`'s guard and
by `RandomXVm::native_loop_effective()`**, so what the miner reports and what it
runs cannot drift apart. All four conditions must hold: the switch is on, the
version is rx/0, the VM is in full (dataset) mode, and a `JitCompiler` was
successfully allocated.

- **Native-loop JIT (default).** `compile_native_loop` emits the whole
  2048-iteration loop as ARM64, so the register file is not reloaded and
  re-stored per iteration. Measured **+6.8%–7.4%** at 11 threads across two
  independent paired A/B runs (`benches/nativeloop_ab.rs`). Entered as
  `f(nreg, scratchpad, dataset, iterations, out)`.
- **Body JIT (fallback).** One program body per call, the loop staying in Rust.
  Reached when any precondition fails, or when the operator sets
  `--native-loop off`. This is also the reference path the share verifier
  compares against.

`JitCompiler::compile(bytecode)` (the body JIT):
1. Emits ARM64 prologue: saves callee-saved regs, loads nreg/scratchpad/config pointers
2. Translates each `BytecodeInstruction` to ARM64 via `emit_*` functions
3. Emits epilogue: restores regs, returns
4. Writes to `JitMemory` (MAP_JIT region), toggles W^X via `pthread_jit_write_protect_np`
5. `get_fn()` returns the function pointer; called as `f(nreg, scratchpad, config)`

`get_fn()` and `get_loop_fn()` each reject code compiled in the other mode, so a
body-JIT blob cannot be entered with the native loop's ABI or vice versa.

A failed `mmap(MAP_JIT)` is logged at `error!` and leaves `jit: None` — the VM
still mines correct hashes via the interpreter, but `native_loop_effective()`
then returns false and the share verifier disarms itself, because both paths
would otherwise be the interpreter and the comparison would be vacuous
(issue GitLab #4).

**Register allocation:**
- `r[0..7]` → `x8..x15`; scratchpad → `x16`; e_mask → `x19/x20`; nreg ptr → `x21`
- FP: `f[0..3]` → `d0–d7`; `e[0..3]` → `d8–d15`; `a[0..3]` → `d16–d23`; FSCAL mask → `d24`

**CBRANCH:** `ibc.target` is `i16`. Cast to `i32` before `+1` to avoid overflow. Out-of-bounds target → fall through (no branch emitted).

### Stratum Protocol (`pool_connection.rs`)
- Newline-delimited JSON-RPC 2.0 over TCP; TLS via rustls + webpki-roots
- Login: `{"method":"login","params":{"login":"<wallet>","pass":"x","agent":"MinerTim/<version>","algo":"rx/0"}}` — the agent string is `concat!("MinerTim/", env!("CARGO_PKG_VERSION"))` (`pool_connection.rs:249`), so it tracks `Cargo.toml`; it is **not** the literal `MinerTim/1.0`
- Job: `{"blob":"<168hex>","target":"<8hex>","job_id":"..."}`
- Submit: `{"method":"submit","params":{"job_id":"...","nonce":"<8hex>","result":"<64hex>"}}`
- Keepalive: `{"method":"keepalived"}` every 60s

### Dataset & Cache (`dataset.rs`)
`SharedDatasetCache = Arc<Mutex<Option<DatasetCache>>>`. `DatasetCache` holds `seed_hash` + `Arc<RandomXDataset>`. Thread 0 generates; others call `get_or_generate_dataset()` which waits on the mutex, then clones the `Arc`.

### Runtime switches (`bin/minertim.rs`)
`--native-loop` / `MINERTIM_NATIVE_LOOP` / `NATIVE_LOOP` and `--verify-shares` /
`MINERTIM_VERIFY_SHARES` / `VERIFY_SHARES`. Both default on. Malformed input
fails **safe**, which differs per switch: an unparseable `--native-loop` falls
back to *off* (slower but cannot mine wrong hashes), while an unparseable
`--verify-shares` falls back to *on* (keeps the safety net). An empty value
warns and leaves any earlier explicit setting intact. The startup line reports
the *request*; each worker logs its own *effective* state once its VM exists.

### Optimisation Flags
`.cargo/config.toml` sets `rustflags = ["-C", "target-cpu=native"]` for
`aarch64-apple-darwin`. **CI overrides this** with `target-cpu=apple-m1` on
`macos-14`: on a virtualised runner `native` resolves to a model whose static
feature set omits aes/sha2/neon, which trips a `ring` compile-time assertion.
`make dist` uses `apple-m1` for the same portability reason. `Cargo.toml` release profile: `lto=true`, `opt-level=3`, `codegen-units=1`, `strip=true`.

## Conventions
- **Rust:** `snake_case` functions/variables, `PascalCase` types, `UPPER_SNAKE_CASE` consts
- **Logging:** `env_logger` with `RUST_LOG=info` (default); structured with module path
- **Error handling:** `Result<T, String>` at pool boundaries; panics only for programmer errors

## AI Session Audit Requirement

For any AI-assisted implementation session:

- Maintain an audit log in repository root: `AUDIT.md`.
- Append an entry for each implementation batch that changes repo-tracked files.
- Each entry should include:
  - request/goal summary,
  - files changed,
  - behaviour/API changes,
  - verification performed (build/tests/runtime checks),
  - notable assumptions or constraints.
- Do not delete prior audit history; append chronologically.
