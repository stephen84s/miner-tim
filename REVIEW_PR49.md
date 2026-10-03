# REVIEW_PR49 — Fix #44: fair stream lock (parking_lot::FairMutex)

Reviewer: pr-reviewer (Opus tier), cold start. Branch `fix/issue-44-stream-lock-fairness`, head 8abb44d.
Base check: `git merge-base HEAD origin/main` == origin/main tip 646080f (after `git fetch`). Rebased: yes.

Scope: touches src/pool_connection.rs, Cargo.toml/lock, AUDIT.md, CLAUDE.md, tasks/. No jit/, benches/, workflows, Makefile, scripts/. Nothing to hand off.

## Coverage ledger
1. Correctness of change — in progress
2. Silent failure — pending
3. Safety switches — pending (none touched expected)
4. Tests / break-test — pending
5. Resource use — pending
6. Docs/audit accuracy — in progress
7. Concurrency — pending

## Findings
