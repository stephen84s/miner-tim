# Migration plan: MinerTim's agent process → AI-DLC, or Anthropic's practice

*Research and plan only. Written 2026-10-02 from primary sources fetched that
day (cited inline and listed at the end). Nothing in the repo was changed.*

> **Status note (added 2026-10-03, PROC-09 implementation).** This document's
> own proposed task-ID numbering (§6: "PROC-09 through PROC-14") collides with
> the actual first PR, which also landed as **PROC-09** — `tasks/README.md`'s
> highest existing prefix at the time was PROC-08, so the real numbering
> shifts by one from what this document guesses throughout §6. Read any
> "PROC-09" below as this document's own placeholder, not the real task.
>
> The following six items update, settle or correct this document:
> - **§6 step 4.1's open question — does `omitClaudeMd` block path-scoped
>   `.claude/rules/` too?** Answered empirically, independently, twice (a
>   sequenced before/after/match probe in the real worktree, and a separate
>   scratch-repo probe by the planning agent): **no.** `omitClaudeMd: true`
>   suppresses the project and user-global `CLAUDE.md` only; path-scoped
>   rules and subdirectory `CLAUDE.md` files still load lazily when a
>   matching file is read. See the `PROC-09` entry in `AUDIT.md` for the
>   commands and verbatim output.
> - **§6 step 4.1's proposed `InstructionsLoaded` hook observer does not work
>   as described**, per the planning agent's own test on Claude Code 2.1.286:
>   the hook fired for the main session's own startup loads but never fired
>   for a subagent's startup loads, and the lazy-load events it did log
>   carried no `agent_id`/`agent_type`. Use the transcript's `usage` field
>   (or the per-model `modelUsage` breakdown in `--output-format json`)
>   instead, which is what PROC-09's own verification did.
> - **§5.1's "available from v2.1.271" is unverified.** The docs fetched for
>   PROC-09 give no introduction version for `omitClaudeMd`; only "observed
>   working on 2.1.287" (and the planner's run on 2.1.286) is established.
> - **A path-scoped rule matches against the *primary checkout's* copy of
>   `.claude/rules/`, even for an agent reading a file inside a worktree.**
>   The planning agent's own scratch-repo experiment found this; it means
>   step 4's idea of using a path-scoped rule as a lighter alternative to
>   pasting facts directly won't see a branch's own edit to that rule
>   until it reaches `main` — worth knowing before step 4 relies on it.
> - **§1 and step 5's claim that "why" paragraphs survive as free HTML
>   comments, "stripped before injection," is false, and the variable is
>   indentation, not list-nesting.** PROC-09's own n=1-per-shape probe first
>   framed this as "nested in a list" vs. "column-0", but the independent
>   review that followed measured it directly (`cacheCreationInputTokens`
>   against a ~9k-token comment payload in scratch repos) and found the
>   actual threshold is the comment's **indent level**: a comment indented
>   exactly 4 spaces is present in context and costs tokens — whether it
>   sits inside a list or at top level with no list at all — while a
>   comment indented 0-3 spaces, including at column 0 between list items,
>   is stripped. Only 0-3 and 4 spaces were tested; 5-or-more is untested
>   and should not be assumed to behave like 4 (round-2 review correction —
>   an earlier draft of this note said "4 or more", overclaiming beyond the
>   data). The *mechanism* behind the threshold is still not
>   established — it is not a plain CommonMark parse — so treat the
>   threshold itself as "observed here," not a documented product
>   guarantee, and re-verify before step 5 relies on it at scale. See the
>   `PROC-09` entry in `AUDIT.md` for both rounds of probes.
> - **PROC-09 implemented step 4's "subagents stop loading CLAUDE.md" before
>   both step 1 (CI enforcement) and step 0 (the lesson-inventory
>   traceability table), reversing this document's own risk ordering, and
>   the reordering was never recorded at the time.** The commit that added
>   this document (`6d0d420`) describes PROC-09 as "the first,
>   lowest-risk step it identifies" — but §6 labels **step 1**, not step 4,
>   as "lowest risk, highest value", and §1 item 4 says step 4 should run
>   "only after" each agent's CLAUDE.md dependencies have already been
>   moved. The reasoning behind skipping ahead anyway — "this PR deletes
>   nothing" — is the lead's own unrecorded planning rationale, not a
>   quote traceable to anything in this repository (round-2 review,
>   R2-N7); it does not fully cover what step 4 actually does, either: an
>   agent that stops loading `CLAUDE.md` entirely is a larger behavioural
>   change than deleting a passage from it. Worse, step 0 — this
>   document's own named mitigation for "an agent silently dropping one of
>   three requested items" — was skipped too, and that is exactly the risk
>   that then materialized three times during PROC-09's own implementation
>   (see the `PROC-09` `AUDIT.md` entry's grading of its sealed risk).
>   Recorded here after the fact, rather than silently left implicit.
>
> Everything else below — the AI-DLC comparison, the six-step migration
> outline, the risk table — is unchanged and still represents the state of
> the research as written; only the six items above and the numbering were
> corrected after a real PR exercised them.

> **Status note 2 (added 2026-10-03, PROC-10): an independent Opus pass was
> asked to actively try to overturn "don't adopt AI-DLC" and to survey
> anything else, open-source or Anthropic-native, that might standardize
> this repo's process better.** It fetched AI-DLC's current state (stable
> `v2.10.0`, 2026-09-24; ~30 commits landed 2026-10-01–02; a full read of
> the `AI-DLC-Workflows-2.0-Specification.pdf`, which the original pass
> did not read), checked this repo's own `AUDIT.md` review-tier track
> record, and surveyed Spec Kit, OpenSpec, BMAD, AutoGen, CrewAI and
> LangGraph. Full report, sources and "could not verify" list are the
> `PROC-10` entry in `AUDIT.md`; the following is what changes this
> document.
>
> **The rejection holds, reaffirmed on different grounds than this
> document gave.** Three reasons beyond §4's original list, specific to
> this repo: AI-DLC's Bugfix/Express profiles make review advisory-only or
> off entirely, while most of this repo's own work is fixes whose
> reviewers need to be able to block (`Review (` entries in `AUDIT.md`
> that returned NOT MERGEABLE are exactly this in action); its two
> built-in reviewers check architectural soundness, not the specific
> silent-defect shapes this repo's reviewers are tuned for (a
> sign-extended `imm19`, a test filter matching nothing); and its
> rule-learning loop appends every kept correction to a project file
> forever — the same unbounded accumulation this migration exists to
> escape, automated rather than fixed.
>
> **Two AI-DLC ideas are worth taking without installing the engine,
> folded into the still-pending steps below rather than added as a new
> step:** its "three-compartment" model (every rule is either checkable
> by a program or needs human judgement, stated explicitly) sharpens step
> 0 into "tag each row: can a script check this, yes or no" — do this
> when step 0 is actually run, which status note 1 already flagged as
> skipped; and its reviewer-robustness pattern (a turn limit, and a
> missing/partial verdict recorded as NOT-READY rather than silently
> dropped) is directly available on Claude Code today via a subagent's
> `maxTurns` setting — no AI-DLC dependency needed, and a cheap fix for
> the 560k-token cold-reviewer incident this repo already hit once.
>
> **No outside framework fits better, for a shared reason: this repo's
> hard problem is verification, not unclear requirements, and nothing
> surveyed targets that.** Spec Kit, OpenSpec and BMAD are all
> spec-driven — they turn a vague ask into a written plan, which is not
> this repo's bottleneck. AutoGen, CrewAI and LangGraph are agent
> runtimes, not development processes, and adopting one would mean
> leaving Claude Code, the tool actually in use here — ruled out on that
> basis alone, independent of their individual merits.
>
> **The pass's actual new finding: this repo already runs a second,
> undisclosed process layer.** The `superpowers` plugin is enabled in
> `~/.claude/settings.json` (user-global) and injects a session-start
> directive — "if you think there is even a 1% chance a skill might
> apply … you ABSOLUTELY MUST invoke it" — into every main session; found
> present in all 11 of this repo's recorded session transcripts. Neither
> this document nor `CLAUDE.md` had ever mentioned it. At least one of
> its skills conflicts with this repo's own rules (its TDD skill says
> delete code written before its test; this repo's own break-testing rule
> explicitly allows a test added after a fix, provided the fix is proven
> to break it). No `AUDIT.md` incident traces to it, so this was a risk
> that had not fired, not a demonstrated failure — but see `PROC-10` for
> the decision taken on it.
>
> Everything else in this document — including status note 1 — is
> unchanged by this pass.

---

## 1. Recommendation (read this if nothing else)

**Do not adopt AWS AI-DLC. Do not throw out the bespoke process either. Keep
the process, and move it onto Anthropic's documented Claude Code primitives.**

What MinerTim built by trial and error is already, in substance, what
Anthropic's guidance recommends:

- verification the agent can run;
- explore and plan before coding, with a human approving the plan;
- a fresh-context reviewer separate from the writer;
- specialised subagents with a `model:` per agent;
- worktrees for parallel sessions.

What it got **wrong by Anthropic's own measure** is *where the rules live*.
Almost all of it lives in prose in one always-loaded file. The CLAUDE.md in
this worktree is **664 lines and 40.9 KB**. Anthropic's guidance is a target of
**under 200 lines**, and about 64% of the file (lines 1-425) is process
protocol rather than project facts. Anthropic's docs are explicit that
CLAUDE.md is advisory. Rules that must hold every time belong in hooks or
permissions. Procedures belong in skills, which load on demand. Guidance that
applies to only part of the code belongs in path-scoped rules.

So the migration is a **re-homing, not a methodology change**. Every lesson is
kept, and each moves to the cheapest mechanism that enforces it. The order is:

1. **Deterministic enforcement first** (CI checks and hooks), because a rule
   enforced by a machine no longer needs a paragraph to plead for it.
2. **Procedures into skills.** `/review-pr`, `/delegate`, `/live-run` and
   `/merge-pr` take the step-by-step protocol out of the always-loaded context.
3. **JIT-only guidance into a path-scoped rule** under `.claude/rules/`.
4. **Subagents stop loading CLAUDE.md** (`omitClaudeMd: true`), but only after
   each agent's real dependencies on CLAUDE.md have been moved into the agent
   or a preloaded skill. This is the single biggest cost finding in this
   document (§5.1).
5. **CLAUDE.md shrinks** to roughly 150-200 lines of facts. The *why* behind
   each rule survives as HTML comments, which Claude Code strips before
   injection: humans keep it, and it costs no context.

`AUDIT.md`, `tasks/`, the model-tier ladder, the review-tier series,
break-testing, `scripts/mutants.sh`, the JIT gate and the ledger-deletion rule
all stay as they are. They are the parts that work.

**Confidence.** High that AI-DLC is the wrong fit; the reasons are in §4 and
come from its own documentation. Medium-high that the re-homing pays off. The
size and adherence guidance is Anthropic's, but whether a 200-line CLAUDE.md
actually yields fewer deviations *in this repo* is not established. §7 builds
in a way to measure it rather than assume it.

---

## 2. What "AIDLC" actually is

There are two related things, and they should be kept apart.

### 2.1 AI-DLC, the methodology (AWS, July 2025)

Raja SP, a Principal Solutions Architect at AWS, introduced it in *"AI-Driven
Development Life Cycle: Reimagining Software Engineering"* on the AWS DevOps
blog (31 July 2025). Its core ideas:

- **Two principles**: "AI Powered Execution with Human Oversight" (the AI plans
  and asks, humans decide) and "Dynamic Team Collaboration".
- **Three phases**: **Inception**, where business intent becomes requirements,
  stories and units of work through **"Mob Elaboration"**, the whole team
  validating the AI's questions. **Construction**, where the AI proposes
  architecture, domain model, code and tests through **"Mob Construction"**.
  **Operations**, where the AI handles infrastructure and deployment under
  oversight.
- **New vocabulary**: **"bolts"** replace sprints and run hours or days rather
  than weeks; **"units of work"** replace epics.
- **Context persistence**: the AI stores its artifacts in the repository so
  that later phases can reuse them.
- The blog names Amazon Q Developer rules and Kiro custom workflows as the
  original implementation vehicles.

It is a **team process methodology**: mobs, a product-to-operations frame and
sprint replacement. It was designed around organisations, not a single
maintainer.

### 2.2 `awslabs/aidlc-workflows`, the tooling (current v2.10.0, MIT-0)

This is the open-source implementation. Its README (fetched raw) describes:

- **One "harness-neutral core"** that runs in Claude Code, Kiro, Codex CLI,
  Cursor, opencode and GitHub Copilot.
- **"5 phases and 33 stages"**, **"14 agents: 11 domain experts, 2 reviewers,
  and an adaptive composer"**, **"11 workflow profiles"**, **"Human approval
  gates and source-bound review evidence"**, and a **"108-event audit trail
  plus persistent state, team knowledge, and learned rules"**, all driven by
  "the same deterministic engine across every supported harness".
- **Installation for Claude Code**: a native `aidlc` binary installed with
  `curl … | sh`, then `aidlc config --harness claude`. The second step "writes
  the selected harness runtime, creates the `aidlc/` workspace, merges managed
  project integrations", and the hooks it installs must be approved through
  `/hooks`. Each piece of work becomes an "intent" under
  `aidlc/spaces/<space>/intents/<YYMMDD>-<label>/`, holding "workflow state,
  audit shards, questions, decisions, and stage artifacts".
- **Optional MCP defaults** that are mostly AWS: `aws-mcp`, `aws-pricing`,
  `aws-iac` and `aws-serverless`, plus `context7`.
- **"The current recommended model is Claude Opus 4.8."**
- **Workflow profiles** range from Express (10 of 33 stages; it "disables stage
  reviewer dispatch" and turns sensors and learnings off) to Enterprise (all
  33 at Comprehensive depth). Classic is the default. In Classic, "Reviews are
  advisory (one pass per stage, findings at the approval gate)", and its Guard
  Policy "defaults to off" for undirected work.

It is also repackaged as Claude Code plugins: by a third party
(`vokako/AIDLC-skills`) and in an AWS sample repo
(`aws-samples/sample-oh-my-aidlcops`). Neither is first-party Anthropic
material.

**What could not be established.** I found no authoritative definition of
"AIDLC" other than AWS's. The AI-DLC "Method Definition Paper" and the "2.0
Specification" PDF are linked from the README but were not read, so claims
here about AI-DLC's internals rest on the README, the workflow-profile guide
and the getting-started guide only.

---

## 3. What Anthropic promotes

**Anthropic does not publish a named lifecycle methodology comparable to
AI-DLC.** There is no Anthropic "xDLC". What it publishes is a set of
**practices** plus a set of **extension primitives**, together with guidance on
which primitive fits which job. That is the honest comparison target.

### 3.1 The practices (Claude Code docs, "Best practices for Claude Code")

`anthropic.com/engineering/claude-code-best-practices` now 308-redirects to
`code.claude.com/docs/en/best-practices`.

- **Context is the binding constraint**: "performance degrades as it fills."
- **"Give Claude a way to verify its work"**: tests, builds, scripts. A check
  can gate a stop at four strengths: in-prompt, `/goal`, a **Stop hook** ("a
  deterministic gate"), or a **verification subagent**. "Have Claude show
  evidence rather than asserting success."
- **Explore → Plan → Implement → Commit**, using plan mode, and skipped for
  changes "you could describe … in one sentence".
- **The Writer/Reviewer pattern**: "A fresh context improves code review since
  Claude won't be biased toward code it just wrote." There is also an explicit
  "adversarial review step", with the warning that reviewers asked to find
  gaps will invent some, so they should be told to "flag only gaps that affect
  correctness".
- **CLAUDE.md**: "keep it short"; "For each line, ask: *Would removing this
  cause Claude to make mistakes?*"; "Bloated CLAUDE.md files cause Claude to
  ignore your actual instructions!"; "If Claude already does something
  correctly without the instruction, delete it or convert it to a hook."
- **Hooks**: "Use hooks for actions that must happen every time with zero
  exceptions … Unlike CLAUDE.md instructions which are advisory, hooks are
  deterministic."
- **Worktrees** for parallel sessions.

### 3.2 The primitives, and when to use each ("Extend Claude Code", "How Claude remembers your project")

| Mechanism | Loads | Use for (Anthropic's words, condensed) |
|---|---|---|
| CLAUDE.md | Every session, in full | "Always do X" rules, build commands. **"Keep CLAUDE.md under 200 lines."** `@imports` "don't reduce its context cost". |
| `.claude/rules/*.md` with `paths:` | Only when matching files are read | Directory- or language-specific guidance. |
| Skills (`.claude/skills/*/SKILL.md`) | Description each session, body on use | Procedures, checklists, `/name` workflows. Use `disable-model-invocation: true` for side effects. |
| Subagents (`.claude/agents/`) | Fresh context per spawn | Isolation and specialised workers. Frontmatter includes `model`, `skills`, `hooks`, `memory`, `isolation: worktree`, `effort`, **`omitClaudeMd`**. |
| Hooks | Zero context unless they print | Must-happen-every-time automation. **"Put guardrails in hooks … If a rule must hold every time, make it a hook rather than a prompt instruction."** |
| Permissions / settings | Enforced by the client | Hard blocks. The hook `if` matcher is "best-effort". |

The docs also suggest an adoption order driven by triggers rather than designed
up front: "Claude gets a convention … wrong twice → CLAUDE.md … paste the same
playbook for the third time → skill … want something to happen every time →
hook."

Anthropic's "Steering Claude Code" blog post (summarised by fetch; the author
and the June 2026 date come from that summary and were not checked against
the raw page) puts it as: "The model choosing to run a formatter is different
from the formatter running automatically." Under pressure or in long sessions
"the model can fail to follow a prompted rule".

### 3.3 Anthropic's engineering-blog principles that bear on this repo

- *Building effective agents* (19 Dec 2024): add complexity "*only* when it
  demonstrably improves outcomes". Its **evaluator-optimizer** pattern is the
  writer/reviewer loop.
- *Harness design for long-running application development* (24 Mar 2026):
  separate generator from evaluator, because "tuning a standalone evaluator to
  be skeptical turns out to be far more tractable than making a generator
  critical of its own work". And: "**every component in a harness encodes an
  assumption about what the model can't do on its own** … those assumptions …
  can quickly go stale as models improve."

The second principle cuts both ways for MinerTim. It endorses the
reviewer-separation the repo already has. It also says the repo's harness
should be **audited for stale components**, which is what §6's traceability
table does.

---

## 4. Why not AI-DLC, judged against this repo's specific needs

| MinerTim's need | AI-DLC's answer | Verdict |
|---|---|---|
| **Single maintainer.** | Mob Elaboration and Mob Construction; team approval rituals. | The core rituals assume a team. A solo maintainer approving their own mob gains nothing. |
| **The dominant risk is silent wrong hashes** (JIT, crypto), so review must *not* be light. | Express "disables stage reviewer dispatch". In Classic, the default, reviews are "advisory" and the Guard Policy "defaults to off". Only Feature and Enterprise run full ceremony. | The cheap profiles cut exactly the control this repo depends on, and the heavy ones add Inception and Operations stages (market research, observability, incident response) that a CPU miner does not need. |
| **Reviewers tuned to this repo's failure history** (signed `imm19` bounds, inverted fail-safes, filters matching nothing). | 2 generic reviewers and 11 generic domain experts. | The repo's three reviewers carry its failure table. Generic reviewers would not have caught what `_shared-context.md` encodes. |
| **An append-only audit log**, already 433 KB and feeding the review-tier series. | Its own "108-event audit trail" in `aidlc/spaces/.../intents/`. | A second, parallel ledger. Either AUDIT.md loses its role or the two diverge. |
| **Model tiering by silent-failure risk**: Haiku, then Sonnet, then Opus. | Recommends one model (Opus 4.8); "balanced reviewer agents may cap reasoning effort". | No equivalent of the tier ladder, which is one of the repo's best-evidenced policies. |
| **A small, auditable process surface.** | A native binary installed via `curl \| sh`, 82 engine tools, its own hooks and workspace, and an ownership baseline over project files. | Large, third-party and AWS-oriented. It adds a dependency the maintainer must trust and keep current, on a repo whose CI deliberately pins everything. |
| **Plans approved by a human before implementation.** | Approval gates; "the agent never executes a plan nobody approved". | **This is a genuine match.** The repo already does it ("You plan; the implementer writes"), and Anthropic's plan mode gives the same thing natively. |

**What is worth borrowing from AI-DLC** (cheap, and needs no installation):

- **"Intent" records that keep their questions and decisions.** `tasks/<ID>.md`
  is already close. Adding a two-line "Decisions / open questions" section to
  the task template would capture what AI-DLC's intent folder holds.
- **Workflow profiles: matching ceremony to risk.** The repo already does this
  through its tiers. Making it an explicit choice at task start (a trivial
  doc fix gets no reviewer; JIT work gets Opus review plus a break-test) is a
  good idea, and fits naturally as a section in the `/plan-task` skill (§5.3).

---

## 5. Findings that drive the migration

### 5.1 Every subagent pays for the 41 KB CLAUDE.md, and the repo's delegation rule doesn't know it

Anthropic's sub-agents doc says a custom subagent's fresh context contains
"CLAUDE.md and git status" by default. Only the built-in Explore and Plan
agents omit them, along with any agent whose frontmatter sets
`omitClaudeMd: true`, available from v2.1.271; this machine runs 2.1.286.

**No agent in `.claude/agents/` sets it.** So each Haiku implementer and each
cold reviewer starts with the whole 664-line protocol, including about 165
lines on *delegation policy* that no delegate needs.

CLAUDE.md's own case for delegation leans on "a cold subagent carries a far
smaller context than the lead does". It is cheaper than the lead's context,
but by less than the rule assumes.

**It cannot simply be switched off.** Some agents rely on content that lives
only in CLAUDE.md:

- `caffeinate` usage. `_shared-context.md` points at "CLAUDE.md's rule".
- Platform coverage: which CI jobs prove what. `_shared-context.md` partly
  duplicates it.
- The architecture notes: register allocation, the CBRANCH `i16` cast rule,
  `native_loop_applies`. `jit-reviewer` needs these.
- The `rtk proxy cargo` filter trap is already duplicated into
  `rust-implementer.md` and `break-tester.md`, but **not** into the three
  reviewers, which also run filtered tests.

So step 4 (§6) moves these dependencies first, then flips the flag.

### 5.2 Several "always" rules are enforceable mechanically, and some already failed as prose

The repo's own record shows prose rules failing:

- the Opus review that missed an `AUDIT.md` heading "two levels off-format —
  … reviews are poor at mechanical checks a one-line `grep` settles";
- a stray `.bak` file in `src/`;
- six unreviewed commits after the migration, before branch protection made
  review mandatory.

The ledger rule is already the model to follow (PROC-07): a CI step in
`ci.yml` fails if `REVIEW_*.md` reaches the tree. Apply the same treatment
wherever a check is mechanical.

### 5.3 Most of CLAUDE.md lines 1-425 is *procedure*, which is what skills are for

The reviewer-spawn procedure, the merge checklist, the delegation brief, the
`caffeinate` launch and the break-test procedure are all multi-step workflows
needed only at specific moments. Anthropic's docs say: "If an entry is a
multi-step procedure or only matters for one part of the codebase, move it to
a skill or a path-scoped rule."

### 5.4 Stale figures inside the repo (fix in passing)

- CLAUDE.md says it is "**~60 KB**". It is **≈41 KB / 664 lines** on both
  this worktree (40,942 B) and `main` (40,930 B).
- `_shared-context.md` says `AUDIT.md` is "~290 KB". It is **433 KB**.
- `_shared-context.md` gives the commit trailer as `Claude Opus 5`. The
  session-supplied trailer is now different, so the file should say "use the
  trailer the session gives you" rather than hard-code a model.

---

## 6. Migration steps

Each step is **one PR**, ordered from lowest to highest risk, following the
repo's existing rules: branch, PR, rebase, five green checks, cold reviewer,
AUDIT.md entry, task file.

**Before each step, seal the expected outcome** in the task file, as the
review-tier rule requires, so the step can be graded rather than declared a
success.

**Proposed task IDs.** `PROC-09` through `PROC-14`; they are not yet in
`tasks/README.md`.

### Step 0: Lesson inventory (PROC-09), the traceability table

*Do this before moving a single line.* The repo's recorded failure mode is an
agent silently dropping one of three requested items, and pruning 400 lines is
where that would happen.

1. Have an agent enumerate every **rule** and every **"this rule exists
   because…"** paragraph in CLAUDE.md lines 1-425 and in
   `_shared-context.md`, one row each, with source line numbers. Use
   **Sonnet, not Haiku**. By the repo's own trigger this is silent-failure
   work: a row skipped here is never caught later, because the step 5 gate
   only checks rows that exist. Cross-check the count mechanically. Count the
   protocol bullets (`grep -c '^    - \*\*'` over lines 1-425) and the
   rationale paragraphs ("exists because", "This rule exists"). The table
   must have at least that many rows, or say why not.
2. The lead assigns each row a destination from the table below. The
   assignment is judgement, so it stays with the lead.
3. Commit the table as `tasks/PROC-09.md`. **Every later step must tick its
   rows off.** A row without a destination blocks step 5.

The starting point for that table:

| Lesson / rule (current home) | New home | Mechanism |
|---|---|---|
| Ledgers stripped before merge (step 0) | **Already CI** (`ci.yml` "no review ledgers") + `/merge-pr` skill | Keep the CI step. The *procedure* moves to the skill. |
| Record the ledger sha in AUDIT.md | `/merge-pr` skill checklist + optional CI grep | Skill. A CI check that the PR's AUDIT.md diff contains `REVIEW_` with a 7-40 char hex sha is feasible. |
| Reviewer choice by path (jit / ci / pr) | `/review-pr` skill (computes `git diff --name-only` and picks the agent) | Skill. The choice becomes computed, not remembered. |
| Spawn reviewers cold, one per round (560k-token incident) | `/review-pr` skill + each reviewer's description (already there) | Skill + agent file |
| Worktrees under `.claude/worktrees/`, gitignored | **CI**: no tracked gitlink (mode `160000`) under `.claude/worktrees/` + 3 lines in CLAUDE.md | CI + CLAUDE.md |
| Correcting AUDIT.md: append only once merged | **CI**: on PRs, fail if the diff of `AUDIT.md` *modifies or deletes* lines that exist on `origin/main` (pure additions only) | CI. This is exactly a mechanical check. |
| AUDIT.md heading format (missed by the Opus review on #28) | **CI**: check that every `## ` heading *added* by the PR matches `YYYY-MM-DD — title` (see the snippet traps in step 1) | CI |
| CI runs only where a PR exists; the merge ref | CLAUDE.md (3 lines: "no PR, no CI; run `make verify-jit` locally") | CLAUDE.md fact |
| Rebase on `main`, then merge on green | Branch protection (already `strict`) + `/merge-pr` skill | Already enforced; procedure → skill |
| Batch the push, not the commits | `/merge-pr` or `/ship` skill + 1 line in CLAUDE.md | Skill |
| Delegate to Haiku; tier by silent-failure risk (the full rationale, ~165 lines) | **`/delegate` skill** (the tier table, brief template, "report don't work around", "verify, don't accept") + `model:` frontmatter (already there) + **a 6-line tier table kept in CLAUDE.md** | Skill + frontmatter + short CLAUDE.md table. The *policy* stays always-loaded; the *procedure* does not. |
| Review-tier series and its grading rules | `/review-pr` skill (seal expectations first; log tier, found, missed and false positives in the AUDIT entry) | Skill. The series table itself → an HTML comment or `tasks/PROC-08.md`, since it is history, not instruction. |
| Fix an agent's file when it deviates; the "Deviations caught so far" list | 2 lines in CLAUDE.md; the list itself moves into the relevant agent files (most are already there) | CLAUDE.md + agent files |
| `caffeinate -dimsu` for long runs; the `-w` form; the `pmset` check with the `pid N(` trap | **`/live-run` skill** (`disable-model-invocation: true`) + **PreToolUse hook** that warns or blocks `Bash(*target/release/minertim*)` without `caffeinate` in the command or a live `-w` inhibitor | Skill + hook. The hook `if` is best-effort, so treat it as a tripwire, not a guarantee. |
| Break-testing binds the author; `mutants.sh` with both args | `/break-test` skill (or keep the `break-tester` agent) + 3 lines in CLAUDE.md | Skill/agent + CLAUDE.md. Mutation choice is judgement and stays the lead's. |
| `rtk` hook mangles `cargo test` filters → use `rtk proxy cargo` | **PreToolUse hook**, active **only when `command -v rtk` succeeds**, that blocks a filtered `cargo test` without `rtk proxy`, and prints the reason. Better in user or `settings.local.json` scope than the shared `settings.json`, because rtk is the maintainer's global tool, not a repo dependency. Also stated in `_shared-context.md` so reviewers get it. | Hook. It currently exists only as prose, and libtest prints `ok` when a filter matches nothing, the textbook silent failure. |
| `.bak` and scratch files inside `src/` | **CI**: fail if any tracked path under `src/` ends in `.bak` or `.orig` | CI |
| Branch and PR, always; spawning the reviewer is on you | Branch protection (already) + `/merge-pr` refuses without a review entry in AUDIT.md | Protection + skill |
| JIT gate rules, the x86 jobs are not JIT evidence, the 92-test exact count | **`.claude/rules/jit.md`** with `paths: ["src/randomx/jit/**", "src/randomx/vm.rs", "benches/**", "scripts/verify-jit.sh"]` | Path-scoped rule. It loads only when JIT files are read. |
| Register allocation, CBRANCH `i16` cast, `native_loop_applies` predicate | `.claude/rules/jit.md` (same rule) | Path-scoped rule |
| Stratum protocol details, runtime-switch fail-safe directions | `.claude/rules/pool.md` (`src/pool_connection.rs`, `src/bin/**`) and `.claude/rules/miner.md` | Path-scoped rules |
| Project structure tree, versions table | Cut, or move to an HTML comment. `/doctor` flags these as derivable. | Delete (the docs' "Exclude: file-by-file descriptions") |
| The *why* paragraphs (the 560k reviewer, the 13 ledgers, the six unreviewed commits, the `caffeinate` misdiagnosis) | **HTML comments** next to the rule's pointer in CLAUDE.md, or in the skill body | `<!-- -->`, which is stripped before injection: kept for humans, free in context |

### Step 1: Mechanical enforcement in CI (PROC-10), lowest risk, highest value

Add to the `lint` job in `ci.yml`, next to the existing ledger check:

- AUDIT.md is append-only against `origin/main`. The diff contains no `-`
  lines other than the header.
- New AUDIT.md headings match the house format.
- No `*.bak` or `*.orig` under `src/`.
- No gitlinks under `.claude/worktrees/`.

**Snippet traps.** These have bitten this repo or would bite these exact
checks, so write each check to avoid them:

- **`\d` is not ERE.** Under `grep -E` it does not mean a digit, so a heading
  check written with it rejects every valid heading. Use `[0-9]`.
- **A no-match `grep` exits 1.** Under `set -euo pipefail`, a pipeline like
  `git ls-files src | grep …` goes red on a *clean* tree.
- **`producer | grep -q` under `pipefail` exits 141** when `grep` matches
  early and the producer takes SIGPIPE. The user's auto-memory records this
  from `run12h.sh`. It makes the guard pass while the defect is present.
- **Use a safe shape.** Capture first, then test for non-empty output:
  `bad=$(git ls-files src | grep -E '\.(bak|orig)$' || true); if [ -n "$bad" ]; then echo "$bad"; exit 1; fi`.

Each check must be **break-tested in both directions**. Make the PR violate
each check once and see it go red, then confirm a clean tree goes green. The
auto-memory note "Test guards both directions" exists because `run12h.sh`'s
guards refused correct input.

*Reviewer:* `ci-reviewer` (Opus). *Sealed expectation:* zero false reds on
the last five merged PRs replayed locally.

### Step 2: Hooks (PROC-11)

Create `.claude/settings.json` `hooks` with:

- **PreToolUse / Bash**: the `rtk proxy` filter guard (block, with the reason
  on stderr, exit 2). The script must first check `command -v rtk` and exit 0
  when it is absent. Otherwise, on any host without rtk it refuses correct
  input: a contributor's clone, or a CI runner if Claude Code ever runs
  there. Its sealed expectation includes **"passes `cargo test foo` on a host
  without rtk"**. Prefer user-scope or `settings.local.json` for this hook.
- **PreToolUse / Bash**: the `minertim`-without-`caffeinate` tripwire (deny,
  with a pointer to the `live-run` skill).
- **Optional: SubagentStop** for reviewer agents, failing if the agent's
  ledger is not committed (`git log -1 --name-only` contains `REVIEW_`). This
  is crash recovery made mechanical.

Caveats to record in the AUDIT entry:

- Project hooks need trust approval via `/hooks`, and a fresh clone runs none
  until that happens.
- The `if` matcher "is best-effort"; for hard blocks the docs point to
  permissions.
- **`.claude/settings.json` is not in `ci-reviewer`'s stated scope.** Extend
  that description to include `.claude/settings.json` and `.claude/hooks/`
  in the same PR. Hooks are code that runs on the maintainer's machine.

*Sealed expectation:* each hook demonstrably blocks its bad case and passes
its good case. Prove both in the PR, both directions.

### Step 3: Skills (PROC-12)

**Design choice: all five skills are model-invocable.** In this repo the
*lead agent* runs reviews and merges, and a skill with
`disable-model-invocation: true` is "invisible to Claude until you invoke
it". User-invoked skills would leave CLAUDE.md pointing the lead at
procedures it cannot load. The cost of model invocation is that a skill may
fail to fire. Contain that three ways:

- Give each skill a tight, non-overlapping description.
- Have CLAUDE.md name each skill at the moment it applies ("before
  spawning a reviewer, load `review-pr`").
- Keep the merge-critical floor in CI and branch protection.

Steps with outside side effects (push, `gh pr merge`, starting a live run)
are written in the skill as **"confirm with the user first"**, which keeps
the human gate without hiding the procedure.

Create `.claude/skills/` with:

| Skill | Contents moved from CLAUDE.md |
|---|---|
| `review-pr` | Path→reviewer selection, cold spawn, sealed expectations, grading (tier/found/missed/FP), ledger-sha recording |
| `merge-pr` | Rebase on main, batch push, wait for 5 checks, ledger strip, AUDIT entry, task file, README line, Current-task pointer |
| `delegate` | Tier table and the silent-failure trigger, brief template ("hand over verified findings with exact commands", "a plan you cannot follow is reported"), never-delegate list |
| `live-run` | `caffeinate` launch forms, the `-w` pid pattern, the guarded `pmset` check |
| `plan-task` | Explore → plan in plan mode, choose a ceremony level (the borrowed AI-DLC "profile" idea), name files, functions, tests and "done", seal review expectations |

Agents preload what they need through the `skills:` frontmatter, e.g.
`rust-implementer` and `break-tester` preload nothing from the lead's
procedures. Keep each skill's description precise: Claude chooses skills by
matching descriptions, so overlapping descriptions will misfire.

*Sealed expectation:* in the next two real PRs, the lead invokes
`/review-pr` and `/merge-pr` and no protocol step is missed (graded against
the step 0 table).

### Step 4: Path-scoped rules, and subagents stop loading CLAUDE.md (PROC-13)

1. Create `.claude/rules/jit.md`, `pool.md` and `miner.md` with `paths:`
   frontmatter, holding the architecture content from CLAUDE.md lines
   549-646.

   **Verify first, unestablished:** nothing in the docs I fetched says
   whether `omitClaudeMd: true` also suppresses project `.claude/rules/`.
   Before relying on it, spawn a test `jit-reviewer` with an
   `InstructionsLoaded` hook logging which files load. If the rule does not
   load, or to avoid the question altogether, put the JIT notes in a
   `jit-architecture` skill and preload it via `jit-reviewer`'s `skills:`
   field. That path is deterministic: preloaded skills are "fully preloaded
   into its context at launch".
2. **Per agent, list what it actually uses from CLAUDE.md** (use step 0's
   table) and re-home it in `_shared-context.md`, the agent file, or a
   preloaded skill. Known items: `caffeinate` (stop pointing at "CLAUDE.md's
   rule"), platform coverage, the `rtk proxy` trap for the reviewers, and the
   JIT architecture notes for `jit-reviewer`. The path rule only loads if the
   agent reads JIT files, which it will; but list it explicitly in the agent
   body as well.
3. Add `omitClaudeMd: true` to all six agents. Two notes:
   - When an agent runs as the main session via `--agent`, the flag is
     ignored, which is harmless. It simply has no effect there.
   - Managed policy files still load. The user's global `~/.claude/CLAUDE.md`
     (the RTK instructions) is *also* skipped, which is another reason the
     `rtk` trap must live in `_shared-context.md` or a hook.
4. *Sealed expectation:* subagent token counts in task notifications drop.
   Record before/after for one Haiku implementation and one cold review of
   comparable size. The repo already logs these (101,608 / 111,957 for
   implementations; 94,350-104,304 for reviews). Predict the delta before
   measuring. A naive bound is up to ~10k tokens per spawn, from 40.9 KB of
   CLAUDE.md, but cache reads make the cost effect smaller than the token
   count suggests. Count a failure to drop as a finding, not noise.
5. *Reviewer:* `pr-reviewer`, raised to **Opus** for this PR. A silently
   missing rule in a reviewer's context is a silent-failure risk, by the
   repo's own trigger.

### Step 5: Shrink CLAUDE.md (PROC-14), last, because it depends on everything above

Target **≤ 200 lines**:

- Build and run commands.
- Platform coverage (CI proves X, not Y) in 6-8 lines.
- The silent-failure statement.
- The tier table (6 lines).
- One line per skill, naming the moment to load it (`plan-task`,
  `review-pr`, `merge-pr`, `delegate`, `live-run`).
- Branch/PR/CI facts in 5 lines.
- Current task.
- The issue-numbering convention.
- One line per path rule, saying it exists.

Move every "why" paragraph into an HTML comment beside its pointer.

**Gate:** every row of the step 0 table is ticked with its new location, and a
reviewer verifies each claimed destination exists by **grepping for it**, not
by reading the summary. Then run `/doctor prompt-audit`. It is advisory
input, not a verdict.

### Explicitly *not* changing

- **`AUDIT.md`**: same structure, same append-only rule, now CI-enforced. It
  is never loaded wholesale (the agents `grep` and `tail` it), so its size
  costs nothing per turn, and it is the dataset behind the review-tier series.
- **`tasks/`**: same layout. Optionally add a "Decisions / open questions"
  section (§4).
- **The model-tier ladder and the `model:` frontmatter.**
- **`_shared-context.md`'s failure table**: it becomes *more* important once
  CLAUDE.md is no longer loaded into agents.
- **No subagent `memory:` in place of agent-file lessons.** Agent memory is
  written by the agent and is not reviewed in a PR. This repo's lesson
  discipline ("fix its file in the same PR") depends on reviewed edits.

---

## 7. Tradeoffs and risks

| Risk | Mitigation |
|---|---|
| **A lesson is lost in the move.** This is the main risk, and it matches a failure this repo has already had. | The step 0 traceability table plus a grep-verified gate at step 5. |
| **Skills don't fire** when model-invoked, so the procedure is silently skipped. | Tight descriptions, and CLAUDE.md naming each skill at its trigger moment. The merge-critical bits are backed by CI, branch protection and hooks. A skill is guidance; CI is the floor. Track "skill not loaded when it should have been" as a deviation category in AUDIT.md. |
| **Hooks are machine-local trust.** A fresh clone, or a session that never approved `/hooks`, runs none. | Hooks are a fast local tripwire. CI remains the authority for anything that gates a merge. |
| **`omitClaudeMd` hides a fact an agent silently needed.** | Step 4.2's per-agent dependency list, plus an Opus review of that PR. |
| **Smaller CLAUDE.md does not actually improve adherence here.** Anthropic's guidance is general, not measured on this repo. | Keep counting "deviations caught" per PR in AUDIT.md, as now. Compare the five PRs before and after step 5. If it does not help, the cost saving (§5.1) still stands. |
| **More files to keep consistent** (skills, rules, hooks, agents). | `/doctor prompt-audit` periodically. The PR reviewer's scope already includes "documentation and audit accuracy". |
| **The "re-home" is itself harness complexity.** | Per Anthropic's "every component encodes an assumption", each new hook or skill states in a comment which failure it prevents. If that failure stops occurring with newer models, delete it. |

---

## 8. Sources

**AWS AI-DLC**

- AWS DevOps Blog, *AI-Driven Development Life Cycle: Reimagining Software
  Engineering* (Raja SP, 31 Jul 2025):
  https://aws.amazon.com/blogs/devops/ai-driven-development-life-cycle/
- `awslabs/aidlc-workflows` README, raw, v2.10.0, MIT-0:
  https://github.com/awslabs/aidlc-workflows/blob/main/README.md
- Workflow profiles guide:
  https://github.com/awslabs/aidlc-workflows/blob/main/docs/guide/workflow-profiles.md
- Getting started guide:
  https://github.com/awslabs/aidlc-workflows/blob/main/docs/guide/01-getting-started.md
- Third-party packaging, not authoritative: https://github.com/vokako/AIDLC-skills
  and https://github.com/aws-samples/sample-oh-my-aidlcops

**Anthropic**

- Best practices for Claude Code: https://code.claude.com/docs/en/best-practices
  (the target of the redirect from
  https://www.anthropic.com/engineering/claude-code-best-practices)
- Extend Claude Code (features overview):
  https://code.claude.com/docs/en/features-overview
- How Claude remembers your project (CLAUDE.md, rules, imports, HTML
  comments, auto memory): https://code.claude.com/docs/en/memory
- Subagents (frontmatter, `omitClaudeMd`, `memory`, what loads at startup):
  https://code.claude.com/docs/en/sub-agents
- Hooks reference: https://code.claude.com/docs/en/hooks
- *Steering Claude Code: when to use CLAUDE.md, skills, hooks, and subagents*
  (Claude blog; author and date as reported by a fetch summary):
  https://claude.com/blog/steering-claude-code-skills-hooks-rules-subagents-and-more
- *Building effective agents* (19 Dec 2024):
  https://www.anthropic.com/engineering/building-effective-agents
- *Harness design for long-running application development* (24 Mar 2026):
  https://www.anthropic.com/engineering/harness-design-long-running-apps

**Repo baseline.** These were read from
`/Users/stephen/code/github/miner-tim/.claude/worktrees/share-ids`:

- `CLAUDE.md` (664 lines, 40,942 B);
- `.claude/agents/*` (6 agents plus `_shared-context.md`);
- `.claude/settings.json` (permissions only, no hooks);
- `tasks/README.md`;
- the `AUDIT.md` heading index and its `Review (` series;
- the `.github/workflows/ci.yml` ledger check.

Claude Code version on this host: 2.1.286.
