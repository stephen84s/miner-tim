# PROC-10 — Second AI-DLC/alternatives research pass; disable Superpowers for this repo

**Status:** Completed

Follow-up to PROC-09 (#46). The maintainer asked for an independent Opus
re-check of "don't adopt AI-DLC", explicitly tasked with trying to overturn
that conclusion rather than confirm it, plus a wider survey of other
Anthropic-native or open-source frameworks for standardizing this repo's
development process.

**Outcome:** the rejection holds, on sharper grounds than the original
research gave (AI-DLC's Bugfix/Express profiles make review advisory-only,
which doesn't fit a repo whose reviewers need to block; its rule-learning
loop recreates the CLAUDE.md-bloat problem this migration exists to fix).
No surveyed outside framework (Spec Kit, OpenSpec, BMAD, AutoGen, CrewAI,
LangGraph) fits better — this repo's hard problem is verification, not
unclear requirements, and the agent-runtime options would mean leaving
Claude Code entirely.

The pass's actual actionable finding: the `superpowers` plugin was enabled
in user-global settings and silently injecting a second, undisclosed
process layer into every session (confirmed in all 11 recorded session
transcripts), with at least one rule conflicting with this repo's own
(TDD ordering). **Decision: disabled for this repo**, via a project-level
`enabledPlugins` override in `.claude/settings.json`, empirically confirmed
to actually suppress the injection rather than trusted on the research's
own "untested" claim.

*Full record: the `PROC-10` entry in [`AUDIT.md`](../AUDIT.md). This file is
the summary. See also `AIDLC_MIGRATION_PLAN_RESEARCH.md`'s "Status note 2".*
