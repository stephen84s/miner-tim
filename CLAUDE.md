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

      **Delete the reviewer's `REVIEW_*.md` from the branch before merging.**
      Reviewers commit their ledger as they go because they can be killed
      mid-review — that is crash recovery and must not change. But the ledger is
      working state, not a repo artifact: fold its findings into the `AUDIT.md`
      entry and the PR description, then `git rm` it in the final commit.

      **Record the ledger's commit sha in the `AUDIT.md` entry when you do.**
      This repo squash-merges, so a ledger commit never enters `main`'s
      ancestry: it survives only while the branch ref does. That is safe today
      because `delete_branch_on_merge` is `false` and no merged branch has been
      deleted — but one "Delete branch" click leaves the objects reachable only
      from `refs/pull/N/head`, which an ordinary clone does not fetch. A
      recorded sha plus a retained branch is what makes
      `git show <sha>:REVIEW_X.md` work; "readable forever" on its own was an
      assertion, not a guarantee.
      This rule exists because nobody ever decided ledgers should live in the
      tree — it fell out of the crash-recovery mechanism, and thirteen of them
      accumulated to 530 KB, larger than the entire Rust source (LEDGER-01).

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

    - **Delegate the mechanical work to Haiku; keep the judgement.** The
      default division of labour is that a **Haiku subagent** does the verbose,
      mechanical work and **you supervise and verify it**. Delegate: long test
      and benchmark runs, `debug_assert!`/symbol inventories, `AUDIT.md`
      write-ups, repetitive edits across files, and anything whose output is
      long but whose decisions are few.

      **Why, in this repo specifically.** Two costs compound. Haiku tokens are
      weighted far below Opus against the usage window, and a cold subagent
      carries a far smaller context than the lead does — this file alone is
      **~60 KB** and rides along on every request the lead makes.

      **Be precise about that second reason, because an earlier draft of this
      rule overstated it.** `CLAUDE.md` is re-sent every turn but it is
      **cached**: sessions run with a prompt-cache TTL, so after the first
      request the repeated prefix is billed at cache-read rates rather than
      fresh-input rates — roughly an order of magnitude cheaper. The file is
      not free per turn, but it is nothing like full price, and "the lead's
      cost per tool call is high" was the wrong way to put it. **The dominant
      term is model weighting, not context size.** Counted from this session's own task notifications:
      Haiku implementations ran **101,608** and **111,957** subagent tokens and
      cold reviewers **94,350-104,304**, none of which enters the lead's
      context — only the brief and the returned summary do, which is order
      1-3k, estimated rather than measured. Note the limit that actually bites
      is a **session** limit spanning every model, so delegating stretches the
      window rather than sidestepping it.

      **Never delegate to Haiku:** the independent review itself (the repo's
      whole quality mechanism rests on a strong cold reviewer — see the
      reviewer-agents rule above), design decisions, merge decisions, or the
      final verification of someone else's claim.

      **Rework is the only thing that makes this lose, and it is not
      hypothetical.** In one session Haiku agents skipped the break-test that
      *was* the task and wrote "ensured by design" instead; appended an
      `AUDIT.md` entry as a task-board **table row** in the wrong format; left a
      stray `src/randomx/jit/aarch64.rs.bak` inside the source tree; and
      silently dropped one of three requested items. Each cost Opus tokens to
      catch and redo, which is the expensive direction.

      So the brief carries the difference. **Hand over every finding you have
      already verified, with the exact commands**, so the agent cannot
      re-derive them wrong; name the house-style formats it must follow; tell
      it what "not established" means here; and warn it of local traps — the
      `rtk` hook mangles trailing `cargo test` filter arguments, so runs must go
      through **`rtk proxy cargo ...`** or a filter silently matches nothing and
      libtest still prints `ok`.

      **Use a specialised agent, never a general-purpose one.** Delegation
      goes to a tuned definition in `.claude/agents/`, whose file already
      carries the scope, the house formats and the traps — so the brief does
      not have to re-teach them and cannot forget one. Reviewing has
      **`jit-reviewer`**, **`ci-reviewer`** and **`pr-reviewer`**; implementing
      has **`rust-implementer`** (writes the code from a plan you have already
      settled), **`audit-writer`** (writes `AUDIT.md` entries and task-board
      rows from findings already established — a scribe, explicitly not an
      investigator) and **`break-tester`** (reintroduces a defect and proves
      the test catches it, then runs `scripts/mutants.sh`). All of them read
      `_shared-context.md`.

      **You plan; the implementer writes.** The division is deliberate: the
      expensive mistakes here have been mistakes of *judgement* — a test
      mutated in a way it catches for the wrong reason, a claim recorded
      without being checked — and those are the lead's to make and to catch.
      So hand over a plan that names the files, the functions, the behaviour
      and the tests, and say what "done" looks like. An implementer that has to
      infer the design will infer it wrong, and the rework costs more than
      writing the plan did.

      Tell it explicitly that **a plan it cannot follow is to be reported, not
      worked around**, and that a dropped step must be named — an agent here
      once silently omitted one of three requested items.

      **Raise the model with the cost of being wrong: Haiku → Sonnet → Opus.**
      The agent definitions default to Haiku; the `model` argument overrides
      that per call, so escalation is a deliberate act each time.

      **The trigger is not difficulty — it is whether a defect would be
      silent.** Work where failure announces itself (a test goes red, a build
      breaks, a connection errors) is cheap to get wrong and safe to delegate
      cheaply. Work where a defect *passes* — wrong hashes the pool quietly
      rejects, a verifier that accepts anything, a check that reports success
      having verified nothing — is expensive to get wrong no matter how small
      the diff, and that is what buys a stronger model.

      | Tier | Use it for |
      | :--- | :--- |
      | **Haiku** | The default. Write-ups, inventories, long test runs, docs and CLI wording, mechanical edits across files. |
      | **Sonnet** | Real logic with a visible blast radius — the Stratum client, the miner loop, argument parsing, test harnesses. A defect here tends to show up as a failure, not as a wrong answer. **Also the default for reviewing such a diff**, which is why `pr-reviewer` carries `model: sonnet`. |
      | **Opus** | Anything whose failure is silent: `src/randomx/jit/`, the emitter, `vm.rs`'s native-loop path, hashing and crypto correctness, TLS and certificate verification, concurrency and shared state — **and the review of any diff touching those**, since review is the mechanism that has caught every class of defect listed in this file. |

      `rust-implementer` is barred by default from the JIT paths for exactly
      this reason. If that code must change, raise the model deliberately, keep
      the JIT gate in the loop, and review with `jit-reviewer`.

      **If no agent fits, write one before delegating** — a few paragraphs in
      `.claude/agents/`, committed, rather than a one-off brief that dies with
      the session. That is the difference between a lesson that compounds and
      one relearned each time: every failure mode listed above was first paid
      for in an ad-hoc brief that did not mention it. A generic agent starts
      from nothing and repeats them.

      **Verify, do not accept.** An agent reporting a check as passed is not
      that check passing. Re-run what the merge decision rests on yourself.

      **Log every review's tier and how it performed, in that PR's `AUDIT.md`
      entry.** One trial is an anecdote; the ladder above is only worth obeying
      if it keeps being right, and the way to know is a record that accumulates
      instead of a memory that does not. Each entry's review paragraph should
      name **the tier used**, what the review **found**, what it **missed**
      that was later discovered, and **how many false positives** it raised —
      `grep -n 'Review (' AUDIT.md` then reads as the running series.

      Grade honestly: write down what you expect a review to find **before**
      spawning it, or the grading is post-hoc and worthless. A false positive
      counts against a tier as much as a miss does, because disproving an
      invented finding costs a round of rework.

      **An earlier draft of this rule put *every* review on Opus. The evidence
      on this very branch contradicted it**, so it was narrowed: Sonnet reviewed
      PR #30 and PR #31, found the major in each — including a blocker — with
      zero false positives, and on #31 cost *more* raw tokens than any Opus
      review here. Review tier now follows the same silent-failure test as
      everything else rather than being a blanket exception to it. The tiers are
      structural, not remembered: `jit-reviewer` and `ci-reviewer` carry
      `model: opus`, `pr-reviewer` carries `model: sonnet`, and the `model`
      argument raises any of them per call.

      The series so far — kept in `AUDIT.md`, which is where the grep runs; this
      table is a summary and `grep -n 'Review (' AUDIT.md` is the authority:

      | PR | Tier | Result |
      | :--- | :--- | :--- |
      | #28 (CI gating) | Opus | Mergeable. 7 minors, 5 of them false statements in the record. **Missed** an `AUDIT.md` heading two levels off-format in the file it was auditing — the series' first recorded miss, and a reminder that reviews are poor at mechanical checks a one-line `grep` settles. |
      | #30 (wording + docs) | Sonnet | Mergeable. Found the major (untested `cfg!` wiring) by mutation, reproduced all five seeded claims by running them, 3 correct unpredicted findings, **0 false positives**. Graded against a list sealed beforehand. |
      | #31 (evidence claims) | Sonnet | **Not mergeable** — blocker (stale base) and major (failure counts did not reproduce); both right, and the major turned out worse than reported. Correctly dismissed a scary-looking diff artefact instead of reporting it. 0 false positives. |
      | #29 (this change) | Opus | **Not mergeable** — 4 majors, including that this very table disagreed with the rule above it, and that the verification paragraph measured the wrong tree. |

      What that series shows so far: one miss, by **Opus**, on a mechanical
      format check; no miss yet by Sonnet, across three reviews that returned a
      blocker and two majors between them. That is not enough to rank the tiers
      and it is not claimed to be — but it is enough to have killed the
      blanket-Opus rule. Note
      also that Sonnet cost **91,085** subagent tokens against Opus reviews at
      **76,575-104,304** — on raw volume there is no saving at all, and one
      Opus review was cheaper. The saving is weighting, and that is the whole
      of it.

      **When you catch an agent deviating, fix its file in the same PR.** A
      correction you make by hand fixes one instance; a correction written into
      `.claude/agents/<name>.md` fixes every future one, and the agent files
      exist precisely so a lesson survives the session that learnt it. Write
      the specific case, not a platitude — "do not skip steps" teaches nothing,
      while "an agent probed with a hand-built 69-test filter instead of the
      gate and reported the guard unreached; the filter had excluded the two
      suites that reach it" is followable. Every rule in those files was paid
      for once already; the point is not to pay twice.

      Deviations caught so far, each now in the relevant agent file: a
      break-test skipped as "ensured by design"; a negative verdict drawn from
      a filter narrower than the gate; an `AUDIT.md` entry written as a
      task-board table row, and another with an invented heading style; a
      stray `.bak` left inside `src/`; one of three requested items silently
      dropped.

    - **Long runs need `caffeinate`, and the miner is always a long run.** Any
      live pool session, benchmark sweep or multi-hour gate must be launched
      under **`caffeinate -dimsu`** — `-d` keeps the display awake, `-i` blocks
      idle sleep, `-m` keeps disks spinning, `-s` holds the system awake while
      on mains power, `-u` asserts user activity. Without it macOS sleeps the
      host part-way and the run is lost: an eight-hour session that dies at
      hour three is not a shorter result, it is **no** result, because the
      claims being tested are about sustained behaviour — reconnects, donation
      rotations, seed changes and share acceptance over time.

      Launch shape in an **interactive** shell:

      ```bash
      caffeinate -dimsu ./target/release/minertim <pool> <wallet> <threads> \
          2>&1 | tee LIVE8H_RUN.log
      ```

      **For a background run, prefer attaching by PID** — start the process,
      then point `caffeinate` at it, so the inhibitor lives exactly as long as
      what it protects and its own pid is recorded for the check below:

      ```bash
      nohup ./target/release/minertim <pool> <wallet> <threads> >> run.log 2>&1 &
      echo $! > /tmp/miner.pid
      nohup caffeinate -dimsu -w "$(cat /tmp/miner.pid)" >/dev/null 2>&1 &
      echo $! > /tmp/caffeinate.pid
      ```

      *Why prefer it rather than require it:* **the wrapper form is not broken.**
      It was once reported here as dying under `nohup`, leaving the miner
      orphaned — that report was wrong, and the way it was wrong is worth
      keeping. `caffeinate <utility>` **forks a child to hold the assertions and
      `exec`s the utility in the original process**, so `ps` shows the utility
      at the pid you launched, with `caffeinate` as its *child*:

      ```
      99097     1  ./target/release/minertim      <- the exec'd process, ppid 1 under nohup
      99100 99097  caffeinate                     <- holds the assertions
      ```

      A `ppid` of 1 is therefore **normal** for `nohup ... &`, not evidence of
      orphaning, and a process listing that misses the child reads as "the
      wrapper died" when nothing died. Review could not reproduce the failure
      across three constructions, which was the correct answer. The `-w` form is
      preferred only because it makes the inhibitor's pid explicit and
      recordable, which the check below needs — not because the wrapper fails.

      **Then verify, and note the check itself has a trap.** The assertions must
      be held by **your** `caffeinate`:

      ```bash
      test -s /tmp/caffeinate.pid || { echo "no caffeinate pid recorded"; exit 1; }
      pmset -g assertions | grep "pid $(cat /tmp/caffeinate.pid)("
      ```

      The guard and the trailing `(` are both load-bearing. With the pid file
      missing the command collapses to `grep "pid "`, which matches **every**
      assertion on the machine and reports success — review reproduced exactly
      that, matching an unrelated `sharingd` entry. Finding some *other* process
      holding `PreventUserIdleSystemSleep` is not evidence about your run; that
      is what masked the original failure.

    - **Break-testing binds you, not just the reviewer.** If you write a test to
      cover a specific defect, **reintroduce that defect and watch the test
      fail** before claiming it is covered. This rule already existed — in
      `.claude/agents/_shared-context.md`, a file headed "for MinerTim
      reviewers" — so reviewers break-tested and the author did not. They have
      caught **eight** instances of a test that passed against the bug it was
      written for, several of them described in `AUDIT.md` as "break-tested" at
      the time.

      The failure is specific and worth naming, because "test harder" does not
      prevent it: **break-testing by hand means choosing a mutation, and a
      mutation the test catches for an unrelated reason proves nothing.** Real
      examples from this repo — a pinned all-zeros fingerprint that rejects
      everything, so the test passed while the default verifier was wide open;
      a flooding socket the test server dropped, so the miner reconnected on
      EOF and the "second accept" had nothing to do with the buffer bound; a
      test asserting no third reconnect, when the stale buffer self-clears on
      the next newline.

      So prefer the tool over your own judgement: **`./scripts/mutants.sh
      <function-regex> <test-filter>`** tries every mutation rather than the one
      you thought of. Pass both arguments — scope is the difference between ~31-33
      seconds and 28 minutes, measured. Treat a MISSED mutant as a question:
      some are *equivalent* and no test can kill them, which the script's header
      explains with the worked example that found one.

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
4.  **Status Update:** Add or update the task's own file in `tasks/`, add its
    line to `tasks/README.md`, and point **Current task** at the top of this
    file at it. Keep the task file a *summary* — the `AUDIT.md` entry is the
    full record, and duplicating it here is what grew the old table to 53% of
    `CLAUDE.md`. Do not leave a task "Active" once it is complete.
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

## Current task

**DOC-04 — Split the task board out of CLAUDE.md into tasks/.**
Completed. See [`tasks/DOC-04.md`](tasks/DOC-04.md).

Every task has its own file in [`tasks/`](tasks/), newest last in
[`tasks/README.md`](tasks/README.md). The matching `AUDIT.md` entry is the
authoritative record; a task file is its summary.

> **Why this is not a table here any more.** It was, and it reached **38 rows
> and 42,921 bytes — 53% of this file**, which every session reads before
> reaching anything operational. Worse, each row duplicated an `AUDIT.md` entry
> that already said the same thing at greater length, so the longest rows had
> grown past 4,400 characters: audit entries wearing a table cell. It also broke
> **four times in one week**, because a single blank line terminates a GFM table
> and every task was required to append to it. The accretion was not a lapse —
> Operational Protocol step 4 mandated it, the same shape as LEDGER-01, where
> thirteen review ledgers piled up because every reviewer obeyed correctly.

> **Issue-numbering convention.** A bare `#N` means the **GitHub** issue. The
> migration renumbered everything — GitLab 1→1, 2→2, 5→3, 6→4, 8→5, 9→6 — and
> GitLab #3, #4 and #7 were closed before it and never imported, so those take
> the explicit form `GitLab #N`. Where a historical reference needs its modern
> number too, write `GitLab #6 (now GitHub #4)`. Applies to `tasks/`,
> `README.md`, the `Makefile`, `scripts/` and the workflow comments; older
> `AUDIT.md` entries and `src/` comments predate the rule and are fixed only
> where they actively mislead.


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
`jit.yml`, `release.yml`) and `AUDIT.md` (the append-only change log).
Independent-review ledgers live on their branches only and are removed before
merge; retrieve one with `git show <sha>:REVIEW_X.md`, using the sha recorded
in the corresponding `AUDIT.md` entry. LEDGER-01 carries the table for the
thirteen removed in bulk.

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
`--native-loop` / `MINERTIM_NATIVE_LOOP` and `--verify-shares` /
`MINERTIM_VERIFY_SHARES`. Both default on. The bare names `NATIVE_LOOP` and
`VERIFY_SHARES` are `mining.conf` keys that the `Makefile`'s `run` target
converts to CLI flags; they are not environment variables the binary reads
directly. Malformed input fails **safe**, which differs per switch: an
unparseable `--native-loop` falls back to *off* (slower but cannot mine wrong
hashes), while an unparseable `--verify-shares` falls back to *on* (keeps the
safety net). An empty value warns and leaves any earlier explicit setting
intact. The startup line reports the *request*; each worker logs its own
*effective* state once its VM exists.

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
