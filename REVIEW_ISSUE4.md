# Independent review — `fix/jit-alloc-failure-visible` (issues #4, #3)

Reviewer: independent (did not write this code). Base: `main`, head `049db1d`.
Diff reviewed: `git diff main..HEAD` — 446 insertions / 30 deletions across
`AUDIT.md`, `CLAUDE.md`, `src/bin/minertim.rs`, `src/miner.rs`, `src/randomx/vm.rs`.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Hot path semantically unchanged (`execute_vm_inner`) | in progress |
| 2 | Arming holds across seed rotation | not started |
| 3 | `native_loop_effective()` in every mode / cfg arm | not started |
| 4 | Are the six new tests testing what is claimed | not started |
| 5 | x86_64 behaviour change, honesty in AUDIT.md | not started |
| 6 | Self-reported limits | not started |
| R | Reproduce: clippy (both targets), `make check`, `make test` | not started |

## Findings

_(none yet)_
