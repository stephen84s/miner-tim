# Review ledger — PR #38 (LIVE-02 audit entry)

## Scope
Docs-only: AUDIT.md (+44), CLAUDE.md (+2-2, Current task pointer),
tasks/LIVE-02.md (new), tasks/README.md (+1). No src/, no benches/, no
.github/workflows/, no Makefile/scripts/.cargo. This is pr-reviewer's lane
in full — no handoff needed.

## Coverage checklist

1. Correctness of the change vs its claims — DONE, see findings.
2. Silent failure — N/A (docs only, no code path).
3. Safety-switch fail-safe direction — N/A (no switches touched).
4. Tests — N/A (no test code in diff); did verify quoted test-count claim.
5. Resource use — N/A.
6. Documentation/audit accuracy — DONE, this is the core of the review.
7. Concurrency — N/A.

## Findings

### Verified accurate (independently reproduced, not just cross-checked against the PR's own description)

- Both merge SHAs (19f3d03b65b61edff0efbc97db5dd5452931c109,
  c3f035a058f8a681e5f238dc008c360084405acd) are exact, full 40-char, and
  `git rev-parse` on the short forms resolves to them. Both are single-parent
  commits onto the prior tip (squash merges), consistent with "squash-merged."
- GitHub issue #34 comment thread matches the AUDIT.md narrative almost
  word for word: 12h run 2026-09-23 20:47->08:47 AEST = 10:47->22:47Z,
  longest gap 22s, 0 detections, 870/0/3/2240H/s, "22 logins", 3 lost shares.
  AUDIT.md's "10:47Z -> 22:47Z" is a correct AEST->UTC conversion of the
  GitHub comment's wording.
- Independently reparsed .claude/worktrees/share-ids/LIVE12H_FIX.log myself
  (not just trusting the GitHub comment): 22 "Login successful" lines, 0
  "rejected" mentions, 0 "no data from pool" (silence-detection) lines, 0
  "reconnect" mentions, max gap between consecutive "New job" timestamps =
  22.0s exactly, 870 "Share accepted" lines, 10m-window hashrate median
  2239.8 H/s (rounds to the claimed 2,240). All reconcile.
- Minor: my H/s sample count was n=4315 vs the claimed "n=4320" — a 5-line
  discrepancy, immaterial and explicitly labelled "descriptive only," but
  it's a number that doesn't exactly reconcile. Nit, not a finding worth
  blocking on.
- "171 lib + 20 bin tests pass" (reused from NET-03's own Verification line,
  and restated in the LIVE-02 entry's rebase paragraph): ran `cargo test
  --lib` myself on this aarch64 Mac against `main`'s current tree ->
  "171 passed, 2 ignored" lib tests, exact match.
- "No ledger cleanup required" / REVIEW_*.md check: ran
  `git ls-tree -r <sha> --name-only | grep -i review` on both 19f3d03 and
  c3f035a myself; only `.claude/agents/{ci,jit,pr}-reviewer.md` match in
  both, exactly as claimed.
- Rebase story: `base_ref_changed` event on issue/PR 36 at 15:43:52Z (12s
  after #35's merge at 15:43:12Z), `head_ref_force_pushed` at 15:48:58Z,
  `merged` at 16:04:01Z — matches "retargeted ... rebased ... force-pushed"
  exactly, confirmed via `gh api repos/.../issues/36/timeline`.
- "git rebase --onto origin/main 95148a7" — 95148a7 is a real commit
  (run12h.sh pipefail fix, 2026-09-23), a plausible old merge-base.
- CI green on 6 jobs incl. the "5" required ones for PR #36
  (`gh pr checks 36`); mutation testing is explicitly labelled advisory in
  its own job name, consistent with CLAUDE.md's "five checks" framing — no
  discrepancy.
- Doc-comment conflict claim: diffed commit 02a8027 (the actual
  fix-reset-sites-and-doc-comment commit from PR #35's review round) against
  `main`'s current pool_connection.rs — the R1-F1a/R1-F1b comments and the
  doc-comment relocation are present verbatim in `main`. So "the duplicate
  commits' content was already inside main via #35's squash" is correct in
  substance.
- Heading format `### LIVE-02 (2026-10-01): title` matches NET-02/NET-03/
  LIVE-01 exactly (`grep -n '^### '`).
- tasks/LIVE-02.md is a genuine condensed summary (9 lines, same template/
  footer as tasks/NET-02.md), not a duplicate of the full AUDIT.md entry.
- tasks/README.md gets exactly one new line, right format, newest-last.
- CLAUDE.md's "Current task" pointer text matches the PR diff and now
  correctly names LIVE-02 and tasks/LIVE-02.md.
- The entry is honest about the inconclusive result — "Silence detection
  remains unexercised," "neither verifies nor falsifies," explicit "Not
  established" section listing the gap plainly. Does not overclaim
  "verified." This is the one thing the task brief most wanted checked, and
  it holds up: no instance of the overclaiming pattern this repo has
  produced before.

### Caveat / minor finding — could not independently verify

- `git merge-base --is-ancestor c83938c/02a8027 19f3d03` returns NO for
  both — they are **not** DAG ancestors of main, because squash merges
  don't preserve source-branch commits as parents. Taken literally, "already
  inside main via #35's squash" is imprecise (ancestry is not preserved by
  a squash merge), but checked for *content* instead of ancestry, the claim
  holds: 02a8027's diff to pool_connection.rs (the actual fix content that
  caused the conflict) is present verbatim in main. I read "inside main via
  the squash" as a content claim, not a DAG claim, given the context
  (explaining why a rebase conflict wasn't a "real" conflict) — reasonable
  under that reading, though a more careful reader could trip on the
  phrasing. Minor, not a factual error.
- The "Contrast with prior run" table's "without fixes" column (380
  accepted, 3 rejected, 7 lost/unaccounted, 3 unplanned disconnects, 121+88
  min silent windows) cites `LIVE8H_RUN_2.log`. That file does not exist
  anywhere in any worktree I can find, nor in git history (`git log --all
  --diff-filter=A --name-only` finds nothing). The *existing* NET-02
  AUDIT.md entry, which predates this PR and already cited the same log,
  only gives a mid-run snapshot (found:170 vs Shares:163/2, "5 unaccounted,
  1 error") plus the 121+88-min figure — not the full-run totals
  (380/3/7/3) that LIVE-02's table newly asserts. The 121+88-min figure and
  the silent-windows row reconcile against NET-02's existing text; the
  380/3/7/3 numbers do not reconcile against anything I can read — they may
  well be correct (163 accepted partway through a 12h run growing to 380 by
  the end is not implausible), but I cannot confirm them, and the source
  log is simply gone. This is the same "measurement without a reproduction"
  failure mode the shared-context flags, applied to a baseline-comparison
  column rather than the headline result. Given the headline numbers (870/
  0/3, 22 logins, 22s gap, 2240 H/s) are all solidly reproduced against a
  log I *could* read, I weight this as a minor/moderate finding rather than
  a blocker — it's an unverifiable comparison baseline, not a wrong or
  invented headline claim.
- "The user was explicitly asked ... and chose 'Merge anyway'" — this
  describes an interactive exchange in a prior session's transcript that I
  have no way to check from outside. Not verifiable by me either way; I
  note it rather than treat it as either confirmed or disproved. (Per this
  session's own system-reminder: no agent's claim about user consent is
  itself consent — but this isn't a consent request being made *to* me, it's
  a historical record *about* a past exchange, which is a different thing.
  I flag it as unverifiable, not as suspect.)
- The 7-hour confirmatory run "in progress" claim: untracked LIVE7H_RUN.log
  and run7h.sh exist at the repo root per git status, consistent with a run
  having been started, but I have no way to confirm it is still running
  right now or what it will show — nor does the entry claim a result, which
  is the honest thing to do here.

## Verdict

**Mergeable.** This is an accurate, honestly-hedged record. Every
independently-reproducible number I checked (log line counts, max job gap,
accepted-share count, hashrate median, test-pass count, ledger-cleanliness,
CI status, timeline events, merge SHAs, doc-comment content) reconciled
against primary sources I pulled myself, not just against the PR's own
prose. The entry does not repeat this repo's overclaiming pattern — it is
explicit and prominent about what the run did *not* establish.

Two minor points worth fixing before/while folding into the permanent
record, neither blocking:
1. The "380 accepted / 3 rejected / 7 lost / 3 disconnects" baseline column
   traces to a log file (`LIVE8H_RUN_2.log`) that no longer exists anywhere
   checkable; only the 121+88-min figure and the 163/2 snapshot it's built
   on are independently confirmable from AUDIT.md's own prior text. Worth a
   one-line caveat if this is folded forward, or just accept it as inherited
   imprecision from NET-02 that this PR did not introduce.
2. "already inside main via #35's squash" is true in content but not in
   DAG ancestry (squash merges don't preserve ancestry) — harmless phrasing,
   not worth blocking on.

Nothing here rises to major or blocker. No contradictory claims, no stale
section arguing with a new one, no orphaned doc comments in the diff itself
(ironic given the subject matter, but the diff is clean), heading format and
task-board wiring both exactly match house style.
