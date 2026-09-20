# Review: PR #33 — Split the task board out of CLAUDE.md into tasks/

Reviewer tier: **Sonnet** (this session is Sonnet 5, not Opus — recorded
accurately for the AUDIT.md tier log; the escalation ladder in PROC-08
prescribes Opus for this class of review, but the actual tier used is what
belongs in the record).

Branch reviewed: `docs/split-task-board`, commits `3f190bf`, `ec2937c`, rebased
onto current `origin/main` (`04bea60`) — confirmed
`git merge-base --is-ancestor origin/main HEAD` is true.

**Handoff check.** Diff touches `CLAUDE.md`, `.claude/agents/audit-writer.md`,
`AUDIT.md`, and 38 new files under `tasks/`. Nothing in `src/randomx/jit/`,
the emitter, `vm.rs`'s native-loop path, `benches/`, `.github/workflows/`,
`Makefile`, `scripts/` or `.cargo/config.toml`. Nothing to hand off to
`jit-reviewer` or `ci-reviewer`; this is `pr-reviewer` scope in full.

## Coverage ledger

| # | Item | Status |
|---|---|---|
| 1 | Content-loss verification (independent method) | Done — 0 mismatches |
| 2 | Stale references to the old table | Done — none found |
| 3 | `caffeinate` correction, tested | Done — one real defect, one unsupported claim |
| 4 | Restructure justified? | Done — yes, with the accretion risk correctly left open |
| 5 | Record accuracy (AUDIT.md heading, task files, rendering) | Done |
| 6 | Tests / clippy | Done — both green, counts match |

## Priority 1 — content loss: verified independently, 0 losses

Built my own extractor (not the lead's script): regex over
`git show origin/main:CLAUDE.md` capturing `(status, task ID, description)`
per row — 37 completed rows + 1 `Pending` row (which has no task ID and is
correctly not migrated). For each of the 37, opened `tasks/<ID>.md`, stripped
the `# ID — <title>\n\n**Status:** Completed\n\n` header and the trailing
`---\n\n*Full record...*` footer, concatenated title + body, normalised both
that and the original description cell to alphanumerics-only, and checked
substring. **37/37 passed.** This deliberately does *not* require the original
text to be contiguous (the first verifier's bug, correctly diagnosed in the PR
body) — it explicitly separates the header/footer scaffolding from the
migrated content before comparing, which is a different and independent method
from "verified against the whole file" checks.

- **ID coverage:** every one of the 37 `**ID**` values in `origin/main`'s table
  (fixed my first regex to also match IDs with digits in the letter segment,
  e.g. `RX2-01`, which `[A-Z]+-\d+` silently drops — worth flagging as a class
  of extraction bug to watch for in future audits of this table) has a
  matching `tasks/<ID>.md`. The only file in `tasks/` with no counterpart row
  is `DOC-04.md` itself — expected, it's this PR's own new entry.
- **No task on `main` is missing a file.** Confirmed by set difference, not by
  spot-checking.
- **Escaped pipes:** exactly 2 rows contain `\|` in the source table (SEC-02,
  PROC-06 — both quoting `fingerprint == nullptr || match` and `` `\|` vs `^` ``
  respectively). Both are correctly **un-escaped** in the split files (verified
  by grep) — correct, since the escape was only needed to survive a table
  cell, and the new home is prose, not a table.
- **Markdown validity:** no cell contained anything that would be invalid
  outside a table (no code fences, no cross-cell markup); nothing to fix here.
- **Titles:** spot-checked several (`SYS-01`, `TEST-01`, `DOC-04`) — H1 title
  matches the bolded lead sentence of the original description, status line
  matches, body matches.

**Not a finding, but worth naming for the ledger:** the PR's own reported
figure of "41,521 characters" does not correspond to an alphanumeric-only
normalisation of the description cells (that normalisation actually totals
**31,582** characters) — it is much closer to the **raw** (unnormalised)
character count of the 37 description cells, which I measured independently
at **41,524** (3 off, well within noise from cell-boundary handling). Read
charitably, "41,521 characters ... compared against origin/main normalised to
alphanumerics" describes the size of the *source material*, with normalisation
being only the comparison method — not a contradiction, just worth recording
since it's the kind of number a future reader could otherwise misread as "the
normalised total."

## Priority 2 — stale references: none found

Grepped `CLAUDE.md`, `README.md`, all six files in `.claude/agents/`,
`Makefile`, `scripts/*.sh`, `.github/workflows/*.yml` for "task board", "task
table", "add a row", "Current Task Board". The only hits outside `AUDIT.md`
and the new `tasks/` content are://
- `CLAUDE.md`'s own **Current task** section, which correctly says "this is
  not a table here any more" and explains why.
- `.claude/agents/audit-writer.md`, rewritten as claimed — replaced the
  table-editing instructions with `tasks/<ID>.md` + `tasks/README.md`
  instructions.
- The Issue-numbering convention's scope note was correctly changed from
  "this table" (main) to "`tasks/`" (branch) — I checked this specifically
  since it sits right above where the table used to start; no dangling
  self-reference.

`AUDIT.md` hits are all historical entries describing past states — correctly
untouched, since `AUDIT.md` is append-only and corrected by appending, not by
editing merged history. The other five agent files
(`_shared-context.md`, `break-tester.md`, `ci-reviewer.md`, `jit-reviewer.md`,
`rust-implementer.md`) have zero references to the table — nothing was missed
there because there was nothing to fix.

## Priority 3 — the `caffeinate` correction: one real defect (major), one unsupported claim (minor)

### Major — the prescribed verification step cannot work as written

The new rule's final step tells the operator to run:

```bash
pmset -g assertions | grep "pid $(cat /tmp/caffeinate.pid)"
```

**Nothing in this document, or anywhere else in the repo, ever writes
`/tmp/caffeinate.pid`.** Confirmed with `grep -n "caffeinate.pid" CLAUDE.md` —
the only hit is this one line. I reproduced the consequence directly: with the
file absent, `cat` fails silently, `$(...)` evaluates to the empty string, and
`grep "pid "` matches **every** assertion line in the system —confirmed output
included `sharingd`'s unrelated `Handoff` assertion and a stray, unrelated
`caffeinate -i -t 300` process (pid 89159) that has nothing to do with the run
being verified. This is **exactly** the masking failure the very next sentence
in the doc warns against ("Finding some *other* process holding
`PreventUserIdleSystemSleep` is not evidence about your run"). The check
degrades to "some process somewhere is holding the assertion," which is true
almost always on this machine and proves nothing about the run being
protected.

It is also incomplete even for a operator who notices the gap and tries to
fix it by hand: the launch block gives two `nohup ... &` lines (miner, then
caffeinate); by the time you'd write `$!` to a pidfile, you need to do it
**immediately after the second line**, not the first — `$!` after the first
line is the miner's PID, not caffeinate's. The doc supplies no such line at
all, for either PID.

This is squarely the case Priority 3 asks about: "getting this wrong means a
multi-hour run silently loses its protection" — and here the failure is worse
than silent inaction, it's **false reassurance**: the operator runs the
prescribed command, sees matching lines, and concludes protection is held,
when it may not be for their process specifically.

**Fix is a one-line addition** (not something I should make — review only):
add `echo $! > /tmp/caffeinate.pid` immediately after the caffeinate `nohup`
line.

### Minor — "does not survive being backgrounded" did not reproduce under test

Tested this directly and specifically, per the brief's instruction, using
three independent constructions, all via this session's actual Bash tool
(the same mechanism "an agent" would use):

1. `nohup caffeinate -dimsu sleep 120 &` (bare form, `disown`ed) — survived,
   `pmset` showed the assertion held for the correct backing PID after moving
   to a fresh shell.
2. `nohup caffeinate -dimsu sleep 120 2>&1 | tee test2.log &` (pipeline form
   matching the doc, `disown`ed) — survived identically.
3. The discriminating case: doc's **literal** shape, **no** `disown`, with a
   continuously-writing payload (`zsh -c 'while :; do date; sleep 1; done'`)
   so that a dead `tee` would surface via `SIGPIPE` on the very next write
   rather than being masked by an idle payload. Confirmed alive, log still
   growing, and `pmset -g assertions` showing the correct caffeinate pid
   ("asserting on behalf of 'zsh' (pid ...)") **49 seconds** after launch,
   checked from a separate shell invocation.

None of the three reproduced "`caffeinate` exited immediately." I cannot rule
out that the one-off failure described really happened once, under some other
invocation shape or terminal session this reviewer doesn't have access to —
but as a **general, unqualified rule** ("does not survive being backgrounded,
which is how an agent will usually start it" / "observed, not theorised"), it
does not hold up against direct, repeated reproduction in the actual
deployment context. This doesn't change the recommendation (the replacement
`-w $!` form does work, verified independently below), but it's an
unsupported certainty claim in a safety-relevant document, which is the class
of thing this repo's own history says to flag (a "~8 minute" figure that
"does not reproduce" was caught the same way in DOC-02).

**The replacement form itself works.** Verified independently: `nohup
./target/release/minertim ... & ; nohup caffeinate -dimsu -w $! & ` — checked
against the live process actually running on this machine (pid 99333,
`caffeinate -dimsu -w 99097`), which `pmset -g assertions` confirms is
"asserting on behalf of Process ID 99097" and has held for ~2 hours. So the
new form is correct; only its stated justification and its verification step
are broken.

Live-process PID/PPID relationships on this machine (99097's parent process
group 99083 is gone, 99100's reported PPID is 99097) looked like it might be
forensic evidence one way or the other; on reflection this is not resolvable
from `ps` output alone and I am not treating it as evidence for or against the
claim — noted only so a future reviewer doesn't retread it.

## Priority 4 — is the restructure justified?

Yes, on the merits the PR states, not the token-savings one. The board really
did duplicate `AUDIT.md` (verified: several rows I checked, e.g. SEC-02, run
850-3600+ raw characters, essentially copy of the audit entry), and the
GFM-fragility complaint is credible given this repo's own history (PROC-06's
audit entry itself documents the board breaking from three blank lines,
independent of this PR). The byte-size reduction is real (see Priority 5 for
the exact number). The PR is honest that the token-cost argument is weaker
than the byte count implies (`CLAUDE.md` is cached) and that `tasks/`'s file
*count* is unbounded — that's the correct thing to leave as an open, not
resolved, question; I would not upgrade it to a finding.

## Priority 5 — the record

- **AUDIT.md heading:** `### DOC-04 (2026-09-20): Split the task board out of
  CLAUDE.md into tasks/` — exact format match, confirmed by grep.
- **Rendering:** `gh api --method POST /markdown -f mode=gfm` on the full
  `CLAUDE.md` returns **4** `<table>` elements (model-tier table, PR
  review-tier log, Platform-coverage table, Versions table — none of them
  touched by this PR's diff, confirmed by line-range cross-check against the
  diff hunks) and **0** stray pipe characters outside `<table>`/`<pre>` blocks
  (the 2 raw `|` characters found by a crude filter are legitimate shell pipe
  operators inside fenced code blocks, confirmed by inspection).
- **Tests/clippy:** `cargo test --release` → **159 lib + 20 bin passed, 0
  failed, 2 ignored** (exact match to the PR's claim). `cargo clippy
  --all-targets --release -- -D warnings` → clean.

### Minor — two numbers in the record don't reproduce exactly

1. **`CLAUDE.md` byte count.** PR body and `AUDIT.md` both claim
   **39,856 bytes**. Measured directly (`wc -c` on the committed blob, `git
   show HEAD:CLAUDE.md | wc -c`, matches the working tree): **39,833 bytes** —
   23 bytes off. Small, but it's exactly the class of unverified number this
   repo's history flags repeatedly (RSS figures, the "~8 minute" JIT claim).

2. **File count / range.** PR body and `AUDIT.md`'s Result paragraph both say
   "Thirty-seven task files created" and cite the range `tasks/SYS-01.md`
   through `tasks/PROC-08.md`. **38** files were created in this commit
   (`ls tasks/*.md | grep -v README | wc -l` → 38), because `tasks/DOC-04.md`
   — this PR's own entry — is also new and falls outside the quoted range.
   "37 pre-existing tasks migrated verbatim" is correct and is what I verified
   in Priority 1; "37 files created" is off by one against the artifact the
   same commit produced.

### Minor — `AUDIT.md`'s "Files changed" field undersells and omits

The DOC-04 entry states: `**Files changed:** CLAUDE.md (Current task section
only), .claude/agents/audit-writer.md (rewritten...), tasks/DOC-04.md (new),
tasks/README.md (new line appended).` Checked against the actual diff
(`git diff origin/main..HEAD -- CLAUDE.md` → 4 separate hunks, not one):
`CLAUDE.md` also had its **caffeinate rule rewritten**, a **wording fix** in
the mutation-testing paragraph ("33 seconds" → "~31-33 seconds"), and
**Operational Protocol step 4 rewritten** — none of which is "the Current task
section." The field also omits the **37 newly-created `tasks/*.md` files**
covering the pre-existing tasks (only `DOC-04.md`, the 38th, is listed) —
arguably the PR's main artifact. This is the same self-contradicting-document
shape this repo has flagged before (the platform-coverage sections, DOC-02
round 3): the "Files changed" line is what a future `grep` trusts, and it
doesn't match either the diff or the entry's own prose two paragraphs below,
which correctly *does* describe the caffeinate and step-4 changes.

## Open question in the AUDIT.md entry — answer supplied here

The entry deliberately left "Review (tier and findings): not provided in the
brief" open. For the record:

- **Tier:** Sonnet.
- **Findings:** 1 major (broken `caffeinate.pid` verification step — real,
  reproduced, one-line fix), 4 minors (unsupported "does not survive nohup"
  claim; `CLAUDE.md` byte count off by 23; task-file count/range off by one;
  `AUDIT.md` Files-changed field inaccurate and incomplete against its own
  diff). 0 blockers.
- **What I could not verify:** whether the originally reported `caffeinate`
  failure genuinely occurred as described in some other invocation context
  (e.g. an interactive Terminal.app session closing) — my reproduction
  attempts covered the Bash-tool-driven path, which is the one this rule is
  written for, and did not reproduce it there.
- **False positives:** 0 — every finding above was independently reproduced
  (content check via a from-scratch extractor and normaliser, the pidfile bug
  via direct execution, the byte counts via `wc -c`/`ls`, the caffeinate
  survival tests via `ps`/`pmset` across separate shell invocations) before
  being written down here.

## Verdict

**NOT MERGEABLE** — one major, actionable in one line
(`echo $! > /tmp/caffeinate.pid` after the caffeinate `nohup` line in the
background-launch block, plus fixing the `grep` pattern's dependency on it).
Everything else is a minor accuracy correction to the PR body / `AUDIT.md`
text, not a blocker: fix the byte count, the file count, drop or soften the
unsupported "does not survive nohup" certainty claim, and correct the
Files-changed field to list the actual hunks and the 37 new task files.

Content preservation (Priority 1, the finding that mattered most per the
brief) is **sound**: 37/37 verified independently, 0 losses, 0 files missing,
2 escaped-pipe rows correctly unescaped. No stale references to the removed
table exist anywhere in the repo (Priority 2). The restructure is justified
on its stated merits (Priority 4).
