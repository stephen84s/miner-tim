# REVIEW_PR43_R2 — round 2: lead's response to round-1 findings + rebase onto post-#42 main

Reviewer: pr-reviewer (Opus), cold. Branch `fix/issue-32-rotation-share-loss` @ 78b4850, base origin/main b4501f0.
Scope: diff touches AUDIT.md, CLAUDE.md, src/bin/minertim.rs, src/miner.rs, src/pool_connection.rs, tasks/. No jit/benches/workflows/Makefile/scripts/.cargo — nothing handed off.

## Coverage
- [ ] 1 correctness of new code (F1/F2 fixes)
- [ ] 2 silent failure
- [ ] 3 safety switches (n/a expected)
- [ ] 4 tests / break-tests (F1, F2, mutants)
- [ ] 5 resource use
- [ ] 6 docs/audit accuracy (F3, F4, ledger sha, rebase comment block)
- [ ] 7 concurrency
- [ ] up-to-date with main, CI on rebased head

## Findings

### R2-F1 (minor→major-for-the-record): recorded ledger sha 99bb076 is the PRE-rebase commit, reachable from no branch
`git branch -a --contains 99bb076` → empty. The rebase rewrote it to 995a1ee (which is on the branch and origin). CLAUDE.md step 0 makes the recorded sha the only retrieval mechanism after squash; an unreachable sha disappears at the next gc. Should read 995a1ee.
