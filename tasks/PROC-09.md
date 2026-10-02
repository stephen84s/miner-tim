# PROC-09 — Subagents stop loading CLAUDE.md

**Status:** Active

First PR of the Claude-Code-native migration described in
`AIDLC_MIGRATION_PLAN_RESEARCH.md`. Sets `omitClaudeMd: true` on all six
`.claude/agents/*.md` definitions and pastes each agent's real CLAUDE.md
dependencies directly into its own body (or `_shared-context.md`) in the same
PR, so no agent silently loses a fact it needs. No CLAUDE.md rewrite, no new
rules/skills/hooks infrastructure — those are later, separate PRs.

**Sealed expectations, written before implementation and before any
reviewer spawns** (per CLAUDE.md's own grading rule — predictions must be
written down first or grading is post-hoc):

- Each modified agent's first-request context should drop by roughly
  9-11k tokens once `omitClaudeMd: true` is set (measured baseline below),
  offset by a small increase (~0.3-1.5k) from the pasted compensation text.
- `omitClaudeMd` is expected to suppress project and user-global CLAUDE.md
  only, not `.claude/rules/*.md` path-scoped rules or subdirectory
  `CLAUDE.md` files — confirmed empirically before this file was written
  (see below), independently, twice.
- Expected review outcome: **mergeable with at most minors** (wording,
  drift risk between the pasted copies and their CLAUDE.md source). No
  blocker expected, since nothing is deleted from CLAUDE.md and the change
  is additive compensation plus a frontmatter flag.
- Risk being watched for specifically: a fact an agent needs that was not
  identified and not pasted anywhere — this is the one way this change
  fails silently, per CLAUDE.md's own §0 risk ("`omitClaudeMd` hides a fact
  an agent silently needed").

**Empirical finding established before implementation**, by two
independent methods (this session's own headless `claude -p` probe pair,
and the planning agent's separate probe in a scratch repo): `omitClaudeMd:
true` suppresses the project `CLAUDE.md` and the user-global
`~/.claude/CLAUDE.md` (and its `@RTK.md` import), but does **not** suppress
`.claude/rules/*.md` path-scoped rules or subdirectory `CLAUDE.md` files —
those still load lazily when the agent reads a matching file. This
resolves the research doc's open question. This session's probe artefacts
(agent definitions, the canary rule, exact `claude -p` commands, Claude
Code 2.1.287, and verbatim STEP1/STEP2/STEP3 output from both the control
and flagged runs) and the token-delta measurement (control 15,547 vs.
flagged 3,515 subagent tokens for an otherwise-identical trivial Haiku
probe, i.e. roughly 12.0k tokens of fixed CLAUDE.md-family baseline, n=1
each — not a claim about real-agent spawns, which the V4 verification
below measures directly) are saved at
`/private/tmp/claude-501/-Users-stephen-code-github-miner-tim/e4ebdc35-b2fd-4f81-bf74-766924c82168/scratchpad/omit-probe/`.

---

*Full record: the `PROC-09` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative
once written. This file is the summary.*
