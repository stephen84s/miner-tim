# REVIEW_PR55 — read_line 1 MiB limit coverage (#27), NET-09

Reviewer: pr-reviewer (Opus tier, round 1). Head reviewed: c27be82. Base: e7f17c7 (= origin/main tip, verified after fetch).

Scope: test-only diff in src/pool_connection.rs tls_tests (#[cfg(test)]), AUDIT.md, tasks/. Nothing for jit-reviewer or ci-reviewer.

## Coverage ledger
1. Correctness of change — in progress
2. Silent failure — pending
3. Safety switches — pending (expected N/A)
4. Tests / break-tests — pending
5. Resource use — pending
6. Docs / audit accuracy — pending
7. Concurrency — pending

## Findings

### Break-tests (reviewer-run, full release lib suite each, file restored from scratchpad copy + cmp + git diff --quiet after every one: all RESTORED-OK)
Baseline: 198 passed / 0 failed / 3 ignored (49 s).
- 291 `1 << 20`->`1 >> 20`: 10 FAILED — the pin test PLUS 9 pre-existing tests (a_large_but_terminated_message_is_accepted, a_trailing_partial_line_is_kept_for_the_next_read, splitting_a_message_across_reads_still_parses, the_first_job_after_a_flood_..., and 5 login/generation tests that go through read_line). Both new boundary tests stay green (that part of the claim holds).
- 1492 `>`->`<`: 7 FAILED — accepts-at-limit PLUS 5 pre-existing login/generation tests and the_first_job_after_a_flood. 
- 1492 `>`->`>=`: 1 FAILED (accepts-at-limit only). 
- 1492 `>`->`==`: 1 FAILED (accepts-at-limit only).
- guard -> `false`: 1 FAILED (refuses-over-limit only).
- guard `> MAX+1` (not a cargo mutant; off-by-one the other way): 1 FAILED (refuses-over-limit only). Boundary pair pins the edge exactly.

### F1 (minor, in-code comment + AUDIT + PR body): "ONLY test that kills `<<`->`>>`" is false
Source comment on read_line_limit_is_exactly_one_mebibyte says it is "The ONLY test that kills `1 << 20` -> `1 >> 20`". Nine pre-existing tests also kill it, because #23 made MAX_LINE_BYTES shared with receiver_loop/take_complete_lines and the login path calls read_line. AUDIT NET-09 break-test 3 ("ONLY ... FAILED") and the PR body ("each turns the right test red, and only that one") repeat it. True only under the `read_line_` test filter, which the entry does not say. The pin test is still worth keeping (it pins the number independently of those tests); the stated reason is wrong.

### F2 (minor->major for audit record): "three mutants survive on main" not re-derived — `<` and `<<` are already killed on main by the suite the issue itself names
The issue's survivor list predates #23 (filed against pre-#23 main). On current main, the full pool_connection:: suite kills `>`->`<` (5 login tests + flood test) and `<<`->`>>` (9 tests). See main-only mutants run below.
