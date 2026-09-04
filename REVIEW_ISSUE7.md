# Independent review — `fix/test-dataset-memory` (`20fa11e`, issue #7)

Reviewer: independent (no prior context on the implementation).
Scope: `git diff main..HEAD` — `src/randomx/dataset.rs`, `src/randomx/tests.rs`,
`scripts/verify-jit.sh`, `CLAUDE.md`, `AUDIT.md`.

## Coverage ledger

| # | Item | Status |
|---|------|--------|
| 1 | Is the key swap genuinely lateral? | in progress |
| 2 | Is `zeroed_for_test()` sound for the ShareVerifier test? | not started |
| 3 | Does the dummy pointer keep the zero-iteration test meaningful? | not started |
| 4 | Reproduce the RSS numbers | not started |
| 5 | Is coverage really unchanged (92 / 131+10)? | not started |
| 6 | The decided-against list | not started |

## Findings

_(none yet)_
