# Independent review — PR #9 "Repo-tuned reviewer agents"

Branch `chore/pr-reviewer-agents`, reviewed against `main`.
Reviewer spawned cold; did not write this change.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Agent files as agent definitions (YAML, name/description/tools, selection, gaps/overlaps) | in progress |
| 2 | `_shared-context.md` failure table — spot-check >=3 rows against history | pending |
| 3 | Untracking `settings.local.json` — correctness/completeness, pull consequence | pending |
| 4 | `.gitignore` worktree entries actually match | pending |
| 5 | `CLAUDE.md` steps 0/0b — contradictions, PR #7 numbering collision | pending |
| 6 | AUDIT PROC-02/03/04 factual claims | pending |

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
