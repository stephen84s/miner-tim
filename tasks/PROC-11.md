# PROC-11 — Refine the implementation-tier rule

**Status:** Completed

During #41/NET-08 planning, the lead invoked `rust-implementer` at Opus tier for
a concurrency/shared-state change, even though the brief was detailed enough
(exact line numbers, verbatim before/after code, exact test specs) that a
cheaper model could execute it as transcription. The user raised the point; the
lead agreed the default to Opus based on "file path touches shared state" was
weaker than the rule's actual design: sufficient plan detail is what lets work
move down the tier ladder.

**Decision: clarified in `CLAUDE.md`'s delegation section.** Implementation may
stay at Sonnet on a concurrency/shared-state diff if the plan has already retired
the judgment calls — exact line numbers, verbatim before/after, exact test/break-test
specs. Review stays Opus unconditionally on such diffs (catching a mechanical slip
is a different question from whether the implementer needed to exercise judgment).
The criterion is "does anything remain for the implementer to decide," not
"is this file path on the Opus list."

**Addendum:** writing this very entry's `audit-writer` run repeated an earlier,
already-documented incident — it landed the AUDIT/tasks edits in the primary
checkout (on `main`) instead of the named worktree. Fixed in the same change:
`_shared-context.md` and `audit-writer.md` now spell out that a passing `pwd`
check in Bash says nothing about whether the `file_path` typed into
`Edit`/`Write` actually contains the worktree segment.

*Full record: the `PROC-11` entry in [`AUDIT.md`](../AUDIT.md).*
