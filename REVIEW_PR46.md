# REVIEW_PR46 — PROC-09 (omitClaudeMd on all six agents)

Reviewer: pr-reviewer, raised to Opus. Cold spawn, round 1.
Reviewed sha: 09802cc (local worktree HEAD; one commit AHEAD of PR head eddcdc8,
not yet pushed — CI has not run on 09802cc). Base: origin/main 0be2000 (merge-base matches).

## Scope check
Diff touches only `.claude/agents/*.md`, CLAUDE.md, AUDIT.md, tasks/, research doc.
No src/, jit/, benches/, workflows, Makefile, scripts/. Nothing to hand off.

## Coverage ledger
- [x] 1 Completeness of compensation
- [x] 2 Empirical claims (spot-check, HTML comments, memory confound)
- [ ] 3 Token-delta math / sealed prediction
- [ ] 4 rust-implementer out-of-scope use
- [ ] 5 CI / base
- [ ] 6 Grading honesty

## Findings

### Checked and NOT a finding (recorded so they are not re-raised)
- jit-reviewer DOES carry `native_loop_applies` / the four preconditions — natively, item 3 of its
  checklist (jit-reviewer.md:66). Not a dropped fact.
- ci-reviewer DOES carry the apple-m1/ring override natively (item 6, ci-reviewer.md:68-71).
- Memory confound eliminated: a built-in `general-purpose` subagent (no omitClaudeMd) run from the
  worktree reports FOUND for all three markers incl. "exits 141" (rev46/gp_worktree.json). So the
  worktree does load auto-memory and the pr-reviewer NOT-FOUND is the flag, not the path.
- Spot-check re-run (pr-reviewer, primary vs worktree, same prompt as v345.sh): primary FOUND x3,
  worktree NOT-FOUND x3, both answer the empty-value question correctly. Haiku usage 20,167 -> 9,184
  (delta 10,983; entry recorded 11,277 at an earlier sha). Pattern reproduces.
- Token arithmetic for all six rows reproduces from the recorded v345/*.json modelUsage.
- 78a5f42 touches only .md files; content choices came from the plan (PLAN_PROC09.md), implementer
  judgement limited to the three disclosed ambiguities.

### F1 (minor) — HTML-comment mechanism is mis-attributed: it is indentation, not list nesting
Mechanical token test (scratch repos, `claude -p "Reply with exactly: ack" --output-format json`,
~9k-token comment payload, cacheCreationInputTokens):
- none 20,868 | 4-space comment nested in list 29,879 (+9,011, SURVIVES)
- column-0 above heading 20,871 (+3, stripped) | column-0 after paragraph 20,882 (stripped)
- column-0 *between list items* 20,872 (stripped) | 1-space top-level 20,826 (stripped)
- **4-space indent at top level, no list at all: 29,834 (SURVIVES)**
So the variable is >=4-space indentation (CommonMark indented code block), not "nested inside a
list item". The entry's conclusion about *which of the seven* survive is right; its stated rule is
wrong, and the research-doc status note propagates the wrong rule ("nested inside a list item").
Consequence: the three surviving guards could simply be dedented to column 0 and cost nothing; step 5
can keep "why" comments free if they start at column 0-3. Confirms the 50-80-token cost estimate is
plausible but it remains unmeasured on the real file.

### F2 (minor) — AUDIT entry describes the exits-141 fact backwards
Entry: "an unguarded `grep -q` in a shell pipeline silently returns success when no match is found".
Reality (`bash -c 'set -o pipefail; yes | grep -q y; echo $?'` -> 141; no-match -> 1): a MATCH
returns 141, i.e. reads as failure; no-match is a correct 1. The pasted _shared-context.md row is
correct; only the AUDIT prose is inverted. Load-bearing because AUDIT is the trusted record.

### F3 (minor) — stale/untraceable numbers in the entry
- "12 files changed, 1008 insertions(+)" was true at 336e243 only; eddcdc8 (PR head) is 1024,
  09802cc is 1040.
- "the 8-file/158-insertion figure above" — no such figure appears above in the entry.
- "this AUDIT.md entry itself (155 lines)" — it is 166 now.
- "Fix applied to audit-writer.md in this same PR" — that paragraph lands only in 09802cc, which is
  not pushed; at PR head eddcdc8 the AUDIT claim is false. Files Changed for audit-writer.md also
  omits that paragraph.

### F4 (minor) — grading: auto-memory suppression and pre-review gaps not graded as prediction outcomes
Seal predicted suppression of "project and user-global CLAUDE.md only" and named one watched risk
(a missed fact). Actual: auto-memory was also suppressed (unpredicted, narrated not graded), and the
watched risk materialised three times before review (exits-141, tier/sealing logging, stale-base).
The entry says "Every tested marker/fact behaved as expected" while also saying the exits-141
NOT-FOUND "caught the gap" — those two sentences pull against each other. Token miss is graded
honestly (disclosure suffices; not counted as a defect).

### F5 (minor) — compensation gaps for pr-reviewer's own domain
pr-reviewer's scope is "miner logic, pool/Stratum code" yet none of CLAUDE.md's Threading Model,
Mining Flow, Stratum Protocol or Dataset & Cache facts were pasted (incl. the "agent string is
CARGO_PKG_VERSION, NOT literal MinerTim/1.0" correction). Research doc step-0 table routes these to
`.claude/rules/pool.md`/`miner.md`, which this PR skipped. Derivable from source, hence minor.
Also: pasted verifier text "With no JIT the verifier disarms itself" is narrower than the code —
miner.rs:657-658 disarms whenever `native_loop_effective()` is false (switch off, light mode,
non-rx/0, no JIT).

### F6 (minor) — plan-ordering deviation not recorded as a deviation
Research doc §1 item 4 / §6 orders this as step 4, after step 0's inventory and 4.1's rules/skill.
The planner skipped 0-3 deliberately (PLAN_PROC09.md §1: "this PR deletes nothing"), but that
reasoning does not hold for agents, which no longer see CLAUDE.md regardless. Commit 6d0d420 calls it
"the first, lowest-risk step it identifies" — the doc calls step 1 (CI) lowest-risk. Neither the
AUDIT entry nor the research-doc status note records that the ordering was changed.

### Nits
- N1 research-doc status note: "Two things ... are now settled" + "only the three items above" over
  five bullets.
- N2 rust-implementer.md "Finishing" still says "task-board row" (fixed in audit-writer only).
- N3 audit-writer lacks CLAUDE.md step 4's "do not leave a task Active once complete" (this PR's own
  Status error is the instance).
- N4 PR description files the token-prediction miss under "corrections to the research doc's own
  premises" — it is the task file's prediction.
- N5 three v345 runs logged exit=1 while their JSON reports success, unmentioned.
