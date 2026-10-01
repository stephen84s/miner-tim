# REVIEW_PR39.md — docs-only LIVE-03 follow-up entry

Scope: AUDIT.md, tasks/LIVE-03.md, tasks/README.md, CLAUDE.md "Current task" pointer.
No src/randomx/jit/, no benches/, no .github/workflows, Makefile, scripts/, .cargo/config.toml touched.
Confirmed via `git diff main...pr39 --name-only`: AUDIT.md, CLAUDE.md, tasks/LIVE-03.md, tasks/README.md only.
No handoff needed — squarely pr-reviewer scope.

## Coverage ledger

1. Correctness of the change vs claim — DONE, see findings.
2. Silent failure — N/A (docs only, no code).
3. Safety-switch fail-safe direction — N/A (no switches touched).
4. Tests — N/A (no tests touched).
5. Resource use — N/A.
6. Documentation/audit accuracy — DONE, this is the whole review. See findings.
7. Concurrency — N/A.

## Findings

### Minor: "longest job gap 22 seconds" does not match the log

Entry text: "No silence-detection event fired — the pool remained responsive
throughout (longest job gap 22 seconds, same as LIVE-02)."

Independently measured the gap between consecutive `New job:` log lines in
`LIVE7H_RUN.log` two ways (Python datetime diff, and an independent awk-based
seconds-of-day diff). Both agree: **max gap is 24 seconds**, not 22:

```
24.0 between 2026-10-01T21:16:38Z (New job: Fw4aBDCd0XDyxoQ0) and
            2026-10-01T21:17:02Z (New job: 3HHT3yiy59pHSDjZ)
```

The "22 seconds" figure is suspicious on its face: it is the *exact* figure
LIVE-02 reported for a completely different 12-hour run against a different
log. The phrasing "same as LIVE-02" reads as if it were copied from LIVE-02's
entry rather than independently recomputed for this run's log. The real
number (24s) is close enough that the substance of the claim doesn't change —
still nowhere near the 180s silence threshold, still no detection event — but
the entry asserts a specific measured value that does not reproduce. Given
this repo's own stated standard ("every number traces to a measurement"), this
should be corrected before merge: either recompute and state 24s, or soften to
avoid a false precise match with LIVE-02. The entry is still on an unmerged
branch, so CLAUDE.md permits fixing it in place rather than appending a
correction.

## Verified as accurate (direct log checks, not trusting the PR prose)

- Run window: log starts `2026-10-01T16:06:04Z`, ends `2026-10-01T23:05:59Z`
  (6h59m55s, "7 hours" as stated).
- Final stats line: `Shares: 465/0 (lost:1) (found:467)` — matches "465
  accepted, 0 rejected, 1 lost, 467 found" exactly.
- Anomaly grep (`ERROR|panic|withheld|Failed to submit|[Rr]ejected|Pool
  closed|No data from pool|Keepalive failed`) against the full log:
  independently re-run, **zero matches** — confirmed.
- "465+1=466≠467" reasoning holds up under direct inspection, not just
  plausibility:
  - `grep -c "Share submitted"` = 466, `grep -c "Share accepted"` = 465,
    `grep -c "SHARE FOUND"` = 467.
  - The single "lost" share is an explicit, named event:
    `21:03:35Z WARN ... Share lost: rpc_id=638 job_id=u3UExt83JQdPT1Gn
    nonce=ce524202 — no response before the connection was replaced`.
  - The 467th found share (`Worker 1 SHARE FOUND! job_id=DMY1wInkFbDQVg7V,
    nonce=e1d52e03`, found at `23:05:48Z`) has no corresponding "Share
    submitted" line anywhere in the log — it was found 11 seconds before the
    alarm fired and the process exited mid-submission-cycle. That is a real,
    verified "snapshot mid-cycle" artifact, not hand-waving: 465 accepted + 1
    explicitly-lost = 466, and the 467th is simply in flight when the log
    ends. Submission latency elsewhere in the log is not always sub-second
    (one earlier share took 20s from found to submitted), so an 11s-old
    in-flight share at process-exit is unsurprising.
- No `minertim` process remains running (`ps aux` after the fact — none
  found), matching the entry's claim.
- `run7h.sh`'s perl-alarm self-terminate mechanism (`SECONDS_TO_RUN=25200` =
  exactly 7h, `perl -e 'alarm shift; exec @ARGV'`) matches the entry's
  description, and is a deliberate improvement over an earlier external
  `sleep; kill` helper that the script's own comment says once overran by 4h.
- No overclaiming repeated from LIVE-02's corrected mistake: this entry
  explicitly says "Like LIVE-02, this run shows the fixes do not create
  problems when they are not needed, and nothing more," and separately: "Not
  established... Whether the silence-detection path itself works — the
  condition it exists to catch still did not occur." Correctly does not
  upgrade "no harm" into "did good," and correctly does not claim the
  silence-detection fix "works" despite it never firing.
- Heading format: `### LIVE-03 (2026-10-02): Confirmatory 7-hour live run with
  both fixes deployed` — matches `### LIVE-02 (2026-10-01): ...` immediately
  above it exactly (verified via `git show pr39:AUDIT.md | grep -n
  "^### LIVE-03"`).
- `tasks/README.md` gets exactly one new bullet, appended last, same format as
  siblings (`- **[LIVE-03](LIVE-03.md)** — ... *(Completed)*`).
- `CLAUDE.md`'s "Current task" section is updated from LIVE-02 to LIVE-03,
  pointing at `tasks/LIVE-03.md`.
- `tasks/LIVE-03.md` is a genuine short summary (9 lines, one paragraph), not
  a duplicate of the AUDIT.md entry — condenses to the headline numbers and a
  pointer back to AUDIT.md as authoritative, consistent with DOC-04's stated
  convention.
- Binary provenance claim ("`main` at `461008a`'s predecessor, carrying both
  PR #35 and #36") checked against git history: `461008a^` is exactly
  `c3f035a` (the #36 merge, itself rebased onto the #35 merge), so this is
  precise and correct, not approximate.
- Timeline sanity: run started `16:06:04Z` on 2026-10-01, after both #35's
  merge (`2026-10-02 01:43:12+10` = `2026-10-01 15:43:12Z`) and #36's merge
  (`2026-10-02 02:04:01+10` = `2026-10-01 16:04:01Z`) — consistent with
  building the binary right after #36 landed and starting the run ~2 minutes
  later.

## Not independently verified

- Whether the binary actually used for the run was built from exactly that
  commit (no build log / binary hash captured in the PR to check against); the
  timeline and the script's own guard (`strings "$BIN" | grep ...` for both
  fixes' marker strings) make it plausible, but this review did not have
  access to the actual binary used at run time to hash-compare.

## Verdict

**Mergeable with one fix requested**: correct the "longest job gap 22
seconds" figure to match the log (24 seconds), or otherwise stop asserting a
specific number that doesn't reproduce. Everything else in the entry —
headline share counts, the mid-cycle-snapshot arithmetic, the anomaly grep,
the no-overclaim discipline, heading format, and task-board wiring — checks
out against direct re-derivation from the primary log and git history.

Severity: minor (an inaccurate number in an otherwise-accurate record; does
not change any safety or merge conclusion, but AUDIT.md is supposed to be the
trustworthy record precisely because people cite it without re-deriving it).
0 blockers, 0 majors, 1 minor, 0 nits.
