# Independent review — PR #10 "Retire the manual JIT gate; fix the 'every push' claim CI-03 falsified"

Round 1. Base `main` @ `9102bf3`, head `cf7fb89`. Files in diff: `CLAUDE.md`,
`Makefile`, `scripts/verify-jit.sh`, `AUDIT.md`. Docs + one `echo`; no Rust.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Can the gate still go red? | in progress (`./scripts/verify-jit.sh` running locally) |
| 2 | Commands/targets exist and mean what is claimed | done |
| 3 | Required-check names match exactly | done |
| 4 | Path filters / conditional execution | done |
| 5 | Live config vs description (`gh api`) | done |
| 6 | Platform assumptions | n/a (no build config touched) |
| 7 | Resource limits | n/a (no parallelism/dataset change) |
| 8 | Coverage given up | done |

## Verified true

- **Trigger keys.** Read directly: `ci.yml` and `jit.yml` are `pull_request:`
  (bare) + `workflow_dispatch:`; `release.yml` is `push: tags: v[0-9]*`. The
  PR's central claim holds.
- **"A push to a branch with no open PR is checked by nothing at all"** — true,
  and precise. A bare `pull_request:` defaults to `opened, synchronize,
  reopened`, so pushing to an *open* PR's head branch does re-run every check;
  the new step 6 says "with no open PR", which is exactly the uncovered case.
  Confirmed empirically: PR #10's own head push produced all five check runs.
- **"Both are required checks on a protected `main`."** Live API: the five
  required contexts are string-exact against the check-run names this PR
  produced, `strict: true`, `enforce_admins: true`.
- **Only three live "every push" instances existed on `main`** (step 6 —
  line-wrapped, so a naive grep misses it — and the two platform-coverage rows).
  A whitespace-normalised sweep of `CLAUDE.md`/`README.md`/`Makefile`/scripts/
  workflows finds no fourth. The count in the PR body is right.
- **Nothing consumes `verify-jit.sh`'s stdout.** `jit.yml` runs
  `make verify-jit` and `./scripts/verify-jit.sh` as plain `run:` steps with no
  `id:`, no capture, no `grep`/`tee`, no `$GITHUB_STEP_SUMMARY`. The Makefile
  invokes the script with `@`. Nothing in the repo greps `GATE PASSED`. The
  removed line was the last statement in the script, after the verdict, and
  both the old and new last statements are `echo`s exiting 0 under
  `set -uo pipefail` (no `set -e`). Behaviour-identical — this was the PR's one
  stated-but-untested assumption, and it holds.

## Findings

(filled in below as work proceeds)
