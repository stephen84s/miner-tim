# REVIEW_PR29 — round 1, independent

PR #29, branch `docs/delegation-default`, head `e1668e5`, base `origin/main`
(`f0c1c37`). Docs and agent definitions only — no Rust, no workflows, no
`benches/`. Nothing here belongs to `jit-reviewer` or `ci-reviewer`; the whole
diff is mine.

**Verdict: NOT MERGEABLE.** No blockers (docs-only; no wrong-hash, memory or
data-loss path). **Four majors, six minors, one nit.** Every major is closable
in one pass. All are **ACTIONABLE**.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Every number against its source | done — one stale, one internally inconsistent, the rest unverifiable by construction |
| 2 | Agent files: internal + cross-file consistency | done — three findings |
| 3 | Escalation table vs prose vs recorded outcomes | done — major |
| 4 | `caffeinate -dimsu` / `pmset -g assertions`, run for real | done — **sound** |
| 5 | Rebuild lost/duplicated nothing vs `b52b1b3` | done — **sound** |
| 6 | `AUDIT.md` entry, heading format, GFM board render | done — board sound, record of checking it false |
| 7 | Do the rules themselves misfire? | done — two majors, one minor |

## What I could not verify, stated up front

Every token figure in this change (101,608 / 111,957 / 94,350 / 95,394 /
104,304 / 91,085 / 76,575) came from this session's task notifications. **None
is re-derivable from the repository** and I did not attempt to. I checked those
numbers only for *internal consistency* between the three places they appear.
I also cannot verify the claim that the `rtk` hook mangles trailing `cargo
test` filters — that is an environment behaviour, not a repo fact. I did
confirm the prescribed workaround exists: `rtk 0.37.2`, and
`rtk proxy cargo --version` returns `cargo 1.97.1`, exit 0.

---

# Majors

## M1 — the `AUDIT.md` Verification paragraph measured `main`, not the branch, and its test is vacuous

`AUDIT.md` PROC-08 states:

> The task board was re-rendered through GitHub's own markdown API
> (`POST /markdown`, `mode: gfm`) and gives **3 tables, 46 rows, 0 stray
> pipes**, matching `main`.

Rendered myself, whole file, `gh api --method POST /markdown -f mode=gfm`:

| Tree | tables | `<tr>` per table | total |
|---|---|---|---|
| `origin/main` (`f0c1c37`) | 3 | 37, 4, 7 | 48 |
| `HEAD` (`e1668e5`) | 5 | 4, 3, **38**, 4, 7 | 56 |
| `b65bf86` (the *old* main) | 3 | 35, 4, 7 | **46** |

"3 tables, 46 rows" is `b65bf86` exactly — the base this branch sat on before
the rebuild. The figure is not merely stale: it is a measurement of a tree that
is not this branch, reported as this branch's result.

The second half is worse. **"matching `main`" is a check that cannot pass
honestly.** This change adds a board row and two tables; if the render matched
`main`, that *would be* the defect. This is the shape `_shared-context.md`
lists first — an assertion that cannot fail.

**The board itself is fine**, which is why this is a record defect rather than
a rendering one: the task board renders as **one** table, 38 `<tr>` against
`main`'s 37 — exactly the one row added, no blank line, no split. I confirmed
that directly. So the thing that was claimed is true; the sentence claiming it
is false.

Fix: re-render and record 5 tables / 56 rows whole-file, or state the board
figure (1 table, 38 vs 37 rows) and drop "matching `main`".

The PR body has a milder version of the same: "Whole-file render gives 4
tables, the fourth being the new tier table." It gives **5** — the bullet adds
*two* tables (the tier ladder and the review series). Its board figures
(36 vs 35) are the pre-rebuild pair; the current pair is 38 vs 37. The
conclusion — exactly one row added — survives.

## M2 — both "Not established" sections are false against the branch's own content

`AUDIT.md` PROC-08, Not established:

> Nor is there evidence about delegating to models between Haiku and Opus;
> only those two were used.

PR body, Not established:

> The Sonnet tier is **untested**: only Haiku and Opus were used this session.

Ten lines above the first sentence, the same entry says:

> Two points so far — Opus on #28 ... and **Sonnet on #30** (found the major by
> mutation, 3 correct unpredicted findings, 0 false positives, graded against a
> sealed list).

And `main` already carries two Sonnet reviews in `AUDIT.md`: line 6423
(DOC-03 / PR #30, "MERGEABLE, one major and two minors") and line 6539
(TEST-01 / PR #31, "NOT MERGEABLE — one blocker, one major ... right on every
count"). DOC-03's entry says so explicitly: "this PR was the deliberate trial
of reviewing at the Sonnet tier rather than Opus (PROC-08's ladder listed
Sonnet as reasoned but **untested**)."

So the Sonnet tier has been tested twice and both results are recorded on
`main`. The "Not established" paragraphs were written before that and were
never revisited when the tier-series table was added to the same bullet — the
premise changed and the section was edited around rather than rewritten, which
is the documented `AUDIT.md` failure mode. Both need rewriting, not patching.

## M3 — the PR body omits two of the five rules being merged

`grep -ci caffeinate` against the PR body: **0**. `grep -ci 'tier and'`: **0**.

The branch adds five rules to Operational Protocol step 0: delegate to Haiku;
never a general-purpose agent; the escalation ladder; **log every review's tier
and outcome**; and **`caffeinate -dimsu` for long runs**. The PR body describes
three of them and is headed "Three user instructions, one rule."

The `caffeinate` rule is also absent from the `AUDIT.md` entry — the entry's
"Files changed" line says "`CLAUDE.md` (two bullets in Operational Protocol
step 0...)" and then describes only one of them. A new mandatory operational
rule is landing with no entry in the authoritative record and no mention in the
PR description. The task-board row omits both the caffeinate rule and the
tier-logging rule too.

Under this repo's own mandate — "No implementation is complete until it is
committed to `AUDIT.md`" — that is not a presentation problem. An un-updated PR
body has been a round-2 finding three times in this repo already.

## M4 — the rule "every independent review is Opus" is contradicted by the evidence printed beneath it, and the series is already two divergent records

The normative table:

> | **Opus** | ... **And every independent review**, whatever the diff touched |

Two paragraphs later, the same bullet's evidence table:

> | #30 (wording + docs) | Sonnet | Mergeable. Found the major ... **0 false positives**. |

So the rule forbids exactly what the repo did, deliberately and successfully,
on PR #30 — and again on PR #31, where a Sonnet review returned NOT MERGEABLE
with a blocker and was "right on every count". **#31 is missing from the series
table**, and it is the strongest data point the series has: the omitted row is
the one that most undercuts the Opus-only rule.

Worse, the bullet prescribes a *mechanism* — "`grep -n 'Review (' AUDIT.md`
then reads as the running series" — and that mechanism does not agree with the
table beside it:

- CLAUDE.md's table: **#28, #30**
- `grep -n 'Review (' AUDIT.md`: **#30, #31** (PR #28 is still open; its entry
  does not exist)

Two records of one series, disagreeing on two of three entries on the day they
are created, one of them citing a PR that has not merged.

Two things to fix, both concrete:

1. Either carve out a graded trial at a lower tier ("a deliberate deviation,
   recorded as one") or record #30 and #31 as deviations. As written the rule
   is dead letter on arrival, and a step-0 rule the lead openly ignores erodes
   the ones that matter.
2. Keep **one** series. It should live in `AUDIT.md`, where the prescribed
   grep runs; `CLAUDE.md` carries the rule and the grep, not a duplicate table
   that must be hand-synchronised and already is not. LEDGER-01 named this
   exact accretion one level up — the task board is 46% of `CLAUDE.md` — and
   this bullet's own premise is that `CLAUDE.md` bytes are the expensive kind.

---

# Minors

## m1 — "the agent definitions default to Haiku" is false for half of them

`CLAUDE.md`: "The agent definitions default to Haiku; the `model` argument
overrides that per call, so escalation is a deliberate act each time."

The three new files carry `model: haiku`. The three reviewer files —
`pr-reviewer.md`, `jit-reviewer.md`, `ci-reviewer.md` — carry **no `model:`
key at all**, so they inherit rather than defaulting to Haiku. The sentence is
true of the new files and false of the existing ones.

The consequence is the one this PR argues against in its own words ("a
correction you make by hand fixes one instance; a correction written into
`.claude/agents/<name>.md` fixes every future one"): the *only* rule the bullet
calls load-bearing — every review at Opus — is enforced by nothing but the lead
remembering to pass `model` on each call, when `model: opus` in the three
reviewer files would make it structural. Fix the sentence, and consider
pinning the reviewers.

## m2 — the tier-logging rule is not in `audit-writer.md`, the agent that writes entries

`grep -in 'tier|Review (' .claude/agents/audit-writer.md` → nothing.
`audit-writer.md`'s "What a good entry contains" lists request/goal, files
changed, behaviour changes, verification, Not established. `CLAUDE.md` now
makes a review paragraph naming **tier / found / missed / false positives** a
required part of every entry. The new requirement was written into the protocol
and not into the file whose whole purpose is to carry the protocol's formats —
the same gap the PR exists to close. One bullet in `audit-writer.md` fixes it.

## m3 — `_shared-context.md`'s new header omits `rust-implementer`

> the reviewers (`jit-reviewer`, `ci-reviewer`, `pr-reviewer`) and the
> implementers (`audit-writer`, `break-tester`) alike

Three implementer agents are added; two are listed. This is in the file every
agent is told to read first.

## m4 — `_shared-context.md` still mandates `caffeinate -i` while `CLAUDE.md` now mandates `-dimsu`

`.claude/agents/_shared-context.md:76`: "Wrap long runs in `caffeinate -i` so
the Mac does not sleep." The PR's new rule says any long run "must be launched
under **`caffeinate -dimsu`**". The header of that file was edited in this diff
and this line was not. Two flag sets, both normative, both read by the same
agents.

## m5 — the `AUDIT.md` entry was extended without its earlier sentences being updated

Four instances, each individually closable:

1. "**Two** implementer agents now exist, covering exactly the work delegated
   this session:" — followed by three bullets (`audit-writer`,
   `break-tester`, `rust-implementer`).
2. "**Fourth instruction: monitor the tiers...**" appears *before* "**Third
   instruction, and the one that makes the rest self-maintaining...**".
3. The escalation ladder and the JIT bar are indented as **sub-paragraphs of
   the `rust-implementer` list item**, so two global rules read as properties
   of one agent. ("It is barred by default from `src/randomx/jit/`" is
   correct there; "Escalation ladder, added on the user's instruction" is
   not.) This is the orphaned-doc-comment shape from `_shared-context.md`,
   applied to prose.
4. The task-board row summarises three instructions and stops, missing the
   tier-log and caffeinate rules.

## m6 — `59,967 bytes on main` does not reproduce, and two ranges for the same quantity coexist

`git cat-file -s origin/main:CLAUDE.md` → **62,625**. 59,967 was correct at
`b65bf86` (2026-09-19); `main` has moved twice since (`2fd056d` 60,238,
`f0c1c37` 62,625). The claim appears as a precise byte count "on `main`" in
`CLAUDE.md`, in the `AUDIT.md` entry and in the PR body; it is stale by 2,658
bytes in all three. The argument ("~60 KB, re-sent every turn") is unaffected.

Separately, and inside the same paragraph: "cold reviewers **94,350**,
**95,394** and **104,304**" and later "Opus reviews at **76,575-104,304**".
Those cannot both be the complete Opus set. 76,575 does trace to DOC-03's entry
on `main` (line 6435), so it is sourced — but then the three-value list is
incomplete, and the entry should say which.

## nit — the file arguing that `CLAUDE.md` bytes are expensive grows 23%

`HEAD:CLAUDE.md` is **77,003** bytes against `main`'s 62,625: +14,378, +23.0%,
re-sent on every lead turn. Not a reason to reject — the rule is worth its
bytes — but the entry makes file size load-bearing and never mentions that its
own bullet is the largest single addition to that file, or that the review
series is designed to grow without bound inside it.

---

# Checked and sound

## The `caffeinate` rule is accurate — I ran it

- `caffeinate -dimsu /usr/bin/true` → **exit 0**. `-m` is genuinely accepted,
  not silently ignored: the usage synopsis prints `[-disu]`, but
  `man caffeinate` documents `-m  Create an assertion to prevent the disk from
  idle sleeping`.
- `caffeinate -dimsu /bin/sh -c 'pmset -g assertions'` lists
  `PreventUserIdleSystemSleep 1`, and under "Listed by owning process" the
  caffeinate pid holds `PreventUserIdleSystemSleep`,
  `PreventUserIdleDisplaySleep` and `PreventSystemSleep` "on behalf of" the
  utility. The rule's verification advice works as written.
- The flag-by-flag gloss matches the man page on all five, including
  "`-s` holds the system awake while on mains power" ("valid only when system
  is running on AC power").
- The one caveat that could have bitten does not: `-u` defaults to a 5-second
  timeout "if a timeout is not specified with `-t`", but the man page also says
  "Timeout value is not used when an utility is invoked with this command" —
  and the documented launch shape invokes a utility. No finding.

## The rebuild lost and duplicated nothing

Against `b52b1b3` (last pre-rebuild commit):

- All seven `.claude/agents/*.md` blobs are **byte-identical** (compared by
  blob sha, not by diff).
- `git diff --stat b52b1b3 HEAD -- AUDIT.md` → **+237, −0**. A pure insertion,
  so the PROC-08 entry text survived the rebuild unchanged; the 237 lines are
  `main`'s DOC-03 and TEST-01 entries arriving.
- `PROC-08` appears **once** in `CLAUDE.md` (the board row) and once as an
  `AUDIT.md` heading; three total occurrences in `AUDIT.md`, the other two
  being in-text references. The five-fold duplication is gone.
- `CLAUDE.md` `b52b1b3..HEAD` is +35/−7: the caffeinate bullet added, and
  `main`'s DOC-03 row, TEST-01 row and Runtime-switches correction pulled in.
  Nothing from the pre-rebuild state was dropped.

## Formats

- `### PROC-08 (2026-09-20): delegate the mechanical work...` matches the
  house heading exactly, and matches the six most recent entries.
- The task board renders as one table, +1 row (M1).
- `rust-implementer`'s JIT boundary is stated twice and unambiguously — in the
  frontmatter `description` ("Barred from src/randomx/jit/ unless the lead says
  otherwise") and in a dedicated section naming `src/randomx/jit/`, the ARM64
  emitter and `vm.rs`'s native-loop path, with the reason (silent wrong
  hashes) and the instruction to stop and report if the plan requires it. An
  agent can obey this.
- `break-tester`'s `scripts/mutants.sh` facts check out against the script:
  `--lib` is on line 166, and the exit codes 64/65/66 are at lines 55, 133/143
  and 187.

# Scope overlap worth a decision, not a finding

`rust-implementer` is told to break-test ("reintroduce the defect ... then run
`./scripts/mutants.sh`") — which is `break-tester`'s entire brief, restated in
shorter form. That is defensible (the implementer proves its own work; the
break-tester is the independent second pass) but nothing says which to use
when. One sentence in either file would settle it.

# Obligation for the lead, not a defect now

PROC-08 has no `Ledger: \`REVIEW_PR29.md\` ... retrieve with
\`git show <sha>:REVIEW_PR29.md\`` line, where DOC-03 (6436) and TEST-01 (6561)
both have one. It cannot have one at round 1. Per `CLAUDE.md` step 0 and
LEDGER-01 it must be added in the same commit that `git rm`s this file.

# Verdict

**NOT MERGEABLE**, round 1. Nothing is dangerous; everything is closable in a
single pass. The reason for the negative verdict is not any one item but its
concentration: in a PR whose subject is making the project's record
self-maintaining, the record contains a verification of the wrong tree (M1),
two "Not established" sections falsified by the branch's own content (M2), a PR
body describing three of five rules (M3), a normative rule contradicted by the
evidence printed under it (M4), and two records of one series that disagree on
arrival (M4). The rules themselves are sound and worth having — the
`caffeinate` rule is correct to the flag, the agent files are usable, the JIT
bar is unambiguous, and the rebuild is clean.

On priority 7 — is this too much process on one session's evidence? The
disclosure is adequate in shape and wrong in content: the entry does say the
saving is directional and unquantified, which is the honest framing. What it
gets wrong is the specific claim that Sonnet is untested (M2), and what it
over-reaches on is the one rule that has no evidence behind it at all — Opus
for *every* review — which the branch's own two Sonnet data points contradict
(M4). Fix that rule and the disclosure is fine.
