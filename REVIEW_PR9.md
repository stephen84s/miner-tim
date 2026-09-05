# Independent review — PR #9 "Repo-tuned reviewer agents"

Branch `chore/pr-reviewer-agents`, reviewed against `main`.
Reviewer spawned cold; did not write this change.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Agent files as agent definitions (YAML, name/description/tools, selection, gaps/overlaps) | **done** — F6 (sound), **F7 (major)** |
| 2 | `_shared-context.md` failure table — spot-check >=3 rows against history | **done** — all 9 rows checked, F13 (sound) |
| 3 | Untracking `settings.local.json` — correctness/completeness, pull consequence | **done** — F5 (sound, one nit) |
| 4 | `.gitignore` worktree entries actually match | **done** — F4 (sound) |
| 5 | `CLAUDE.md` steps 0/0b — contradictions, PR #7 numbering collision | **done** — F8, F10, F11 |
| 6 | AUDIT PROC-02/03/04 factual claims | **done** — F1, F2, F3, F9 |

## Findings

(appended as found)

---

### F1 (minor) — `AUDIT.md` PROC-04's pack-share figure does not reproduce

PROC-04's repo-size audit says: *"the ten largest objects in the repository are
all the same file, roughly half the pack."*

Measured:

```
$ git cat-file --batch-all-objects \
    --batch-check='%(objectname) %(objecttype) %(objectsize) %(objectsize:disk)' \
  | awk '$2=="blob"' | sort -k3 -rn | head -10 | awk '{s+=$4} END{print s}'
972478                      # 0.93 MiB on disk
$ ls -l .git/objects/pack/*.pack
4272022                     # 4.07 MiB
```

The ten largest blobs are **22.8 %** of the pack, not ~half. (All ten *are*
`AUDIT.md` revisions — that half of the claim holds.) Even the most generous
reading, *all* 75 `AUDIT.md` revisions, gives 3.01 MiB / 4.07 MiB = 74 %, which
is not "roughly half" either. The claim is wrong under both readings.

This is precisely the row `_shared-context.md` lists as *"Reported RSS figures
did not reproduce"* — an unreproduced measurement, in the PR whose thesis is
measurement discipline.

Sound in the same paragraph, verified: pack **4.10 MiB** (4,272,022 B), **54
tracked files** on `main` totalling 1.19 MiB, largest blob anywhere in history
223,876 B (so "no blob exceeds 500 KB" holds), and "every entry commits a fresh
full-size blob" — the ten revisions occupy 94–99 KB each on disk, i.e. they are
not delta-compressed against one another.

### F2 (minor) — the commit id `7851bdc` does not resolve, and PR #9 propagates it into two more files

```
$ git ls-tree 7851bdc .claude/worktrees/
fatal: Not a valid object name 7851bdc
```

`7851bdce4cf2e7c6…` is a **SHA-256** id from before MIGRATE-01's conversion. Its
SHA-1 equivalent is in the repo's own committed mapping:

```
$ grep 7851bdc SHA256_TO_SHA1_MAP.txt
7851bdce4cf2e7c680cb10424df3df933a328a1c60178da2e0def942811e5d27 3b2cc9d94b4076b1d7fd476b8dc48ddd01739b80
$ git log -1 --format='%h %s' 3b2cc9d
3b2cc9d feat: Initialize project management agent protocol
```

The id was already stale in `AUDIT.md:3573` (MIGRATE-01). This PR copies it into
`.gitignore:29` and `CLAUDE.md:26` — the two files a developer actually reads —
so a reader who follows the pointer gets `fatal: Not a valid object name`. The
*narrative* is corroborated (`AUDIT.md:3572-3573` records stripping
`M 160000 … .claude/worktrees/platform-neutral` from the fast-export stream) and
`git ls-tree 3b2cc9d .claude/worktrees/` is correctly empty, consistent with the
line having been stripped. Only the id is wrong. Substitute `3b2cc9d`.

### F3 (minor) — `_shared-context.md` and `AUDIT.md` state two different sizes for `AUDIT.md`

`_shared-context.md`: *"Never read `AUDIT.md` (~180 KB) … in full."*
PROC-04, same PR: *"`AUDIT.md` is 210 KB."*

```
$ git show main:AUDIT.md | wc -c
215635      # 210.6 KiB  -> PROC-04 is right
$ wc -c AUDIT.md         # on this branch
223876      # 218.6 KiB
```

`~180 KB` understates by ~17 % and is contradicted inside the same changeset.
`REVIEW_MR1_ARCHIVE.md (~175 KB)` in the same sentence is accurate
(175,768 B = 171.6 KiB). The *rule* ("don't read it in full") is unaffected, but
the number is the kind this PR's own `pr-reviewer` item 6 says must trace to a
measurement.

### F4 (verified sound) — the `.gitignore` entries do match

Checked with a path that exists and with a trailing slash on the query, per the
trailing-slash caveat:

```
$ git check-ignore -v .claude/worktrees/chore-pr-reviewer-agents
.gitignore:32:.claude/worktrees/	.claude/worktrees/chore-pr-reviewer-agents
$ git check-ignore -v '.worktrees/x/'
.gitignore:33:.worktrees/	.worktrees/x/
$ git check-ignore -v .claude/settings.local.json
.gitignore:40:.claude/settings.local.json	.claude/settings.local.json
$ git check-ignore .claude/worktrees ; echo $?
1           # bare path, directory-only pattern — the documented false negative
```

PROC-03's stated line number (`.gitignore:32`) is correct, and it flags the
bare-path false negative rather than papering over it. No finding.

### F5 (verified sound, one caveat) — untracking `settings.local.json`

- Removed from the index on the branch (`git ls-files --error-unmatch` →
  *did not match*), still tracked on `main` (`100644 blob da656cd9`). Correct
  `git rm --cached` + `.gitignore` shape; the file is still on disk in both the
  primary checkout and this worktree.
- **48 allow entries** — confirmed:
  `git show main:.claude/settings.local.json | python3 -c "…len(d['permissions']['allow'])"` → `48`.
- **"No secret was exposed"** — confirmed. The one Monero address in the blob,
  `49stQdfmRNQ…BbBZng`, is byte-identical to one of the two addresses in
  `src/donate.rs`, i.e. a published donation address. No other key material in
  the file (`permissions` is its only top-level key).
- **Backup exists**: `~/miner-tim-backups/settings.local.json.backup-20260905-224526`,
  2198 B, same size as the live file. Claim holds.
- **Pull consequence is stated and is correct**: the on-disk file is byte-identical
  to the `main` blob in both checkouts, so a post-merge `pull` deletes it silently
  — exactly as PROC-04 warns. Good.

*Caveat (nit):* PROC-04 also says the file *"had shown as modified in `git status`
for the life of the project."* It is clean right now in both checkouts, and it was
re-committed only 4 times in 181 commits (`cb96d56`, `3b2cc9d`, `9d2599c`,
`ca8db7a`). The two claims sit awkwardly together — and note it is the *clean*
state, not the dirty one, that makes the silent-delete warning true. A
locally-modified file removed upstream would abort the merge, not vanish.

### F6 (verified sound) — the three agents load and are discoverable; `_shared-context.md` does not break loading

This was the one thing that could have blocked. Settled empirically, from inside
the worktree, rather than by reading the frontmatter:

```
$ claude --print --model haiku \
    "Reply with ONLY a comma-separated list of the subagent_type values available to your Agent/Task tool."
ci-reviewer, claude, Explore, general-purpose, jit-reviewer, Plan,
pr-review-toolkit:code-reviewer, …, pr-reviewer, statusline-setup
```

All three appear. `_shared-context.md`, which has no frontmatter, is **not**
surfaced as an agent and does not break the directory scan — so the PR's
"deliberately has none" is correct, not a latent parse failure. `name`,
`description` and `tools` are present on each of the three; the `tools` values
(`Bash, Read, Grep, Glob, Write, Edit`) are real tool names and cover everything
the bodies ask for (`gh api`, `gh pr view`, `git`, and writing a ledger).

PROC-02's "Verification" section only claimed the YAML parsed. That is a weaker
claim than the one that mattered, and the stronger one now holds.

### F7 (major) — the routing rules leave the highest-risk *non-JIT* hashing code with no equipped reviewer

`jit-reviewer`'s `description` claims `src/randomx/jit/`, the emitter, *"or
anything that can alter an emitted instruction **or a hash**."* That last clause
sweeps in `soft_aes.rs`, `aes_hash.rs`, `argon2d.rs`, `blake2b.rs`,
`blake2gen.rs`, `superscalar.rs` and `dataset.rs` — every one of which can
silently produce a wrong hash exactly as the JIT can. But `jit-reviewer`'s
*body* is entirely ARM64: instruction encodings, `imm19` signedness, AAPCS64,
`MAP_JIT`/W^X, FPCR, `native_loop_applies`. None of it applies to a change in
`argon2d.rs`.

Meanwhile `pr-reviewer`'s description says it takes "any PR that does not touch
the JIT", so `soft_aes.rs` also matches *it* — and its body has no wrong-hash
checklist at all (no known-answer vectors, no `make verify-jit`, no differential
testing).

So a change to the interpreter's hashing primitives either routes to an agent
whose checklist is about a different machine, or to one with no hashing
checklist. The wrong-hash framing that opens `_shared-context.md` has no agent
that operationalises it off the JIT path. Given the file's own severity ladder
("major = … a safety net that cannot fire"), this is a hole in the net.

Two narrower gaps of the same kind:

- **`benches/`** appears in no agent's path list, so `benches/nativeloop_ab.rs`
  routes to `pr-reviewer` by elimination — the only agent with **no** paired-A/B
  section, while `jit-reviewer` carries it. The harness that produced the
  retracted "+9.01%" is reviewed by the agent that was not told about it.
- **JIT ∩ CI is unarbitrated.** `jit-reviewer` says "use this rather than
  `pr-reviewer`"; it says nothing about `ci-reviewer`, and `ci-reviewer` says
  nothing about `jit-reviewer`. This repo's modal change hits both — PLAT-02
  added `scripts/verify-jit.sh` *and* the Makefile targets *and* touched
  `jit/memory.rs`; JIT-01 "also repaired CI". Nothing says to spawn both, and
  each description reads as exclusive.

`Cargo.toml`/`Cargo.lock` are likewise unlisted (a `rustls` advisory bump is
`pr-reviewer` by elimination, though `ci-reviewer` owns the `audit` job).

### F8 (minor) — `_shared-context.md` working rule 4 forbids what the break-testing section mandates

> **Break-testing is required, not optional.** … *mutate the production code
> that test guards and confirm the test fails.*

> **4.** Do not fix anything. Review only. **Do not touch the working tree apart
> from your ledger.**

A reviewer cannot both mutate production code and not touch the working tree.
There is no reconciliation — no "mutate, observe, revert", no "use a scratch
copy", no `git stash`/`git checkout --` instruction. The two rules are three
screens apart and a cold-spawned reviewer will hit the contradiction and pick
one, unrecorded. `ci-reviewer` item 1 ("Break something … and confirm the
pipeline fails") inherits the same problem, and additionally is stated
unconditionally, so it is unfollowable for a docs- or config-only infra change.

### F9 (minor) — the AUDIT correction was made *in place*, in the entry that claims otherwise

`pr-reviewer` item 6, added by this PR: *"`AUDIT.md` is append-only: corrections
are appended, not edited in place."* Commit `0a11083` did the opposite:

```
-`CLAUDE.md` gains Operational Protocol step 0a naming which agent covers what,
-and repeating the cold-spawn rule.
+`CLAUDE.md` gains an Operational Protocol step 0 naming which agent covers what,
+and repeating the cold-spawn rule. (Numbered 0 here, not 0a: …)
```

The original sentence — the one that made the false claim — was **deleted**, and
a paragraph reading *"Correction, appended per the append-only rule"* was added
below it. The rule is asserted in the same commit that breaks it, and the
appended correction now describes a sentence a reader can no longer see. The
honest form is to leave the original standing and append the correction beneath.

### F10 (minor) — the PR #7 collision is a merge **conflict**, not a renumbering

The PR body: *"Whichever merges second needs renumbering."* Understated. PR #7
(`chore/branch-protection`) inserts its own step `0.` at the identical position
in `CLAUDE.md`, and `### PROC-01` at the identical position in `AUDIT.md`
(line 3798 on both branches). Verified read-only:

```
$ git merge-tree --write-tree --messages chore/pr-reviewer-agents chore/branch-protection
CONFLICT (content): Merge conflict in AUDIT.md
CONFLICT (content): Merge conflict in CLAUDE.md
```

Second consequence, unmentioned: if #9 merges first, `PROC-01` lands *after*
`PROC-04` in a file whose entire discipline is chronological append-only, so the
ledger's IDs read out of order forever. Worth stating in the PR, since the
recorded mitigation ("recorded so the collision is not a surprise") is what the
next session will act on.

The collision *is* flagged, in both the PR body and PROC-02, and PROC-02's claim
that the colliding step lives on `chore/branch-protection` is correct
(`gh pr view 7 --json headRefName` → `chore/branch-protection`). So the task's
literal question — is it flagged — is yes.

### F11 (nit) — `0b.` is not a list marker, so step 0b folds into step 0

`CLAUDE.md:20` is `0b. **Worktrees for concurrent branches.**`, at column 0, with
**no blank line** after step 0's last continuation line. CommonMark ordered-list
markers are 1–9 digits followed by `.` or `)`; `0b.` is neither, so the line is a
lazy continuation and renders as trailing prose inside step 0. `1.` on the next
line then interrupts and starts a *fresh* list, so the rendered protocol reads
0, then 1–6 as a separate list. Cosmetic — CLAUDE.md is consumed mostly as raw
text — but it is one blank line and an `0a.`/`0b.` → sub-bullet away from
rendering as written. (Also collides with PR #7, per F10.)

### F12 (minor) — the shared context's commit trailer is incomplete

Working rule 7 gives future reviewers:

```
Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
```

The repo's actual convention is two lines — 28 of the last 30 commits on `main`
carry `Claude-Session:` alongside `Co-Authored-By:`, in equal number. A reviewer
following this file exactly will produce commits missing the session pointer.

### F13 (verified sound) — the failure table is accurate; all nine rows corroborate

Spot-checked against source and history, not taken on trust:

| Row | Evidence |
|---|---|
| benchmark measured the new path against itself; "+9.01%" retracted | `CLAUDE.md` JIT-01 (round 5) |
| `assert!(…contains("requested"))` could not fail | `AUDIT.md:2762` F3, verbatim |
| bound 2× too loose — `imm19` is signed | `compiler.rs:816` `(-(1 << 18)..(1 << 18))`, `:822` comment "CBZ's imm19 is SIGNED", `:828` `skip < (1 << 18)` |
| inverted fail-safe on `--verify-shares` | `AUDIT.md` R7-F2 |
| empty value erased an explicit setting | `AUDIT.md:2289` R10-F2 (MAJOR), test `an_empty_value_does_not_erase_an_explicit_setting` |
| 256 MiB Argon2d per VM never read — 2.75 GiB at 11 workers | `AUDIT.md:1924-1932` R7-F1, verbatim including the 2.75 GiB |
| orphaned doc comments | `CLAUDE.md` VIS-01 ("two orphaned doc comments") |
| RSS did not reproduce — 2.7× overstatement | `AUDIT.md:3352` "the original overstated it 2.7x"; `REVIEW_ISSUE7.md:450` |
| filter matched nothing, libtest called it success | `CLAUDE.md` PLAT-02; `scripts/verify-jit.sh:57` `EXPECTED_PASSES=92` |

Agent-body technical claims also check out exactly, which is where this class of
document usually rots:

- `EXPECTED_PASSES` — real variable name, real value `92`, in
  `scripts/verify-jit.sh:57`. `jit-reviewer` item 7 names it correctly.
- `native_loop_applies(use_native_loop, version, has_dataset, has_jit)` —
  `vm.rs:1167`, signature exact, parameter order exact.
- `native_loop_diff_tests` — real module, `src/randomx/tests.rs:1052`.
- AAPCS64 (`x19–x28` callee-saved, low 64 bits of `v8–v15`, `x18` reserved on
  Darwin) — correct as stated.
- `macos-14` 7 GB, 4.07 GB at 3 threads vs 6.23 GB at 12, *"a floor, not a
  budget"* — matches `AUDIT.md:3390` almost verbatim.
- 560k-token context death — `AUDIT.md:2501` ("35k → 560k across rounds 5–12"),
  `REVIEW_MR1.md:8`.
- ~15% stale rejects at 12 threads — `AUDIT.md:738`.
- the unsupported "~3× faster" README claim — `AUDIT.md:3759`.
- PROC-03's ledger-on-the-wrong-branch story — corroborated by this branch's own
  `dd4df09` "remove the PR #7 review ledger from this branch".

Zero fabricated lessons found. This is the strongest part of the PR.

## What I did not verify

- **That the agents behave as intended when invoked.** The PR says so itself and
  does not claim otherwise. F6 proves they *load*; nothing here proves the
  bodies steer a review well.
- **`make verify-jit` was not run.** No Rust changed in this PR; running the
  ~6-minute gate would have proved nothing about the diff. Stated rather than
  implied, per the shared context's own rule 6.
- **PROC-04's "showed as modified in `git status` for the life of the project."**
  Not reconstructible from history; it is false *now* (the file is byte-identical
  to the `main` blob in both checkouts) and the file was committed only 4 times
  in 181 commits. See the caveat under F5.
- **PROC-03's "`cargo check` clean in the worktree"** — not re-run.
- `.git` at **6.1 MB**: measures **6.3 MB** now, but commits have landed since;
  not a discrepancy worth calling.

## Verdict

**Mergeable.** No blockers. The one thing that could have blocked — whether
`_shared-context.md` sitting frontmatter-less inside `.claude/agents/` breaks
agent discovery — is empirically clear (F6), and the failure table, which is the
substance of the change, is accurate on every row I checked (F13). The
`git rm --cached` is correct, complete, and its silent-delete-on-pull
consequence is stated and true (F5); the `.gitignore` patterns genuinely match
(F4).

Worth fixing before or shortly after merge: **F7** (major — no agent is equipped
for wrong-hash review off the JIT path; `benches/` and JIT∩CI unrouted), then
the minors **F1** (pack figure ~23%, not "half"), **F2** (`7851bdc` is a dead
SHA-256 id now copied into `.gitignore` and `CLAUDE.md`; use `3b2cc9d`), **F3**
(`~180 KB` vs `210 KB` for the same file), **F8** (break-testing vs working
rule 4), **F9** (in-place AUDIT edit), **F10** (real merge conflict, and PROC IDs
land out of order), **F12** (missing `Claude-Session:` trailer), and the nit
**F11** (`0b.`).

F1, F2, F3 and F9 are all instances of the discipline this PR exists to enforce,
applied to itself — which is the fairest test of it, and it does not fully pass.
