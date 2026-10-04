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

Main-only mutants run (detached worktree of origin/main e7f17c7, then removed): `./scripts/mutants.sh '<PR's corrected filter>' 'pool_connection::'` -> 7 tested, **5 caught, 2 MISSED: `>`->`==` and `>`->`>=`**. So on current main `<` and `<<` were already killed by the very test filter the issue names; the genuine survivors were `>=` and `==`. The PR does kill both (branch: 7/7 narrow, re-run here in 19 s). The entry never mentions `==` as a main survivor and lists `<`/`<<` as survivors without having re-run on main. Severity minor (record accuracy; the tests themselves are right), but it is the PR's central framing.

### Filter verification (item 2/3 of brief)
`cargo mutants -F '<corrected>' --list` on branch: exactly the 7 listed in AUDIT (291:33 `<<`, 1482 x2, 1488 `==`, 1492 x3). `$` and `^src/pool_connection\.rs:` anchors exclude every other `<<` in the tree (hex.rs, vm.rs, compiler.rs, superscalar ... checked via full --list grep). Issue's bare `read_line` filter: 6, missing 291:33 — confirmed. Cause confirmed via `git log -S'const MAX_LINE_BYTES'` -> 88c5d40 (#23).

### F3 (minor): AUDIT NET-09 contradicts itself on which test kills `>`->`<`
Implementation item 3 says the refuses-over-limit test "Kills the `>`->`<` mutation". Break-test 2 in the same entry says that test stays GREEN under `<` (correct — reproduced; identical error text). It's the accepts test (and 6 pre-existing tests) that kill it.

### F4 (minor): entry/task file stale relative to the correction commit
- "no PR has been opened yet, so no review and no CI have run" (AUDIT) / "no PR opened, no review run yet" (tasks/NET-09.md): PR #55 is open and CI has run (5 pass, jit-macos pending at review time).
- "**Commits** (2 ...)" — branch has 3; c27be82 (the correction itself) is missing, in the very paragraph that announces fixing an undercount.
- "Files Changed (from git diff main...HEAD --numstat)" lists only src/pool_connection.rs; numstat also shows AUDIT.md, tasks/NET-09.md, tasks/README.md.
- No prediction of expected review findings recorded before this review (task file or entry). Note, not blocker. Review paragraph expectedly absent until this round is folded in.

### F5 (nit): issue's 4th suggested bullet not acknowledged
#27 asks for "a line split across reads still assembles". read_line reads one byte per syscall, so the 1 MiB accepts test exercises this inherently, but neither PR body nor entry says so or says it was dropped.

### F6 (nit): wording
- PR body (inherited from #27): "shifted by twelve orders of magnitude" — `1 >> 20` is 0, not 2^-20 of the limit.
- AUDIT: "1 << 20 = 1048576 bits/bytes" (bytes); "One-byte allocations scale linearly" heading describes one-byte read syscalls, not allocations.

## Items 2, 3, 5, 7
- Silent failure: N/A (test-only). Tests themselves: refuses test has trailing newline, so guard-deleted mutant returns Ok rather than relying on EOF — verified (guard->false fails it). 30 s read timeout prevents hangs; server `let _ = write_all` is fine since the assertions on the client side are what fail. Fixtures self-check byte counts: accepts asserts len == MAX_LINE_BYTES; MAX+1 off-by-one variant of the guard is caught by the refuses test (reproduced).
- Safety switches: N/A.
- Resource: ~2 MiB Vec per test + ~1M 1-byte reads; whole suite still ~49-53 s release. Fine.
- Concurrency: test-local listener thread per test, ephemeral port; no shared state. Fine.
- Doc-comment placement: helper + tests inserted after a closing brace; next item (`parses_a_plain_hex_fingerprint`) had no doc comment displaced. OK.

## Other checks
- Base: merge-base == origin/main tip e7f17c7 after fetch.
- clippy --all-targets -D warnings: clean. (cargo fmt not gated, by recorded decision in ci.yml.)
- Full suite release: 198/0/3 lib, 20 bin — matches claim.
- NET-09 not on origin/main, so in-place edits are legitimate.

## Verdict
Mergeable after doc fixes. No blocker, no major. The tests are correct and do kill what matters (`>=`, `==`, guard deletion, MAX+1, and the literal). The fixes needed are all record accuracy: F1 (in-source "ONLY" comment + AUDIT + PR body), F2 (survivor list not re-derived on main), F3 (internal contradiction), F4 (stale status/commit/file lists). F5/F6 optional.

## Corrections to this ledger (after advisor pass)
- F2's "issue predates #23" was a guess; #27 says the survivors were confirmed on #23's branch. Traced instead (`git log -S'fn <test>'`): the `<<`-killing receiver tests (a_large_but_terminated..., splitting_a_message..., the_first_job_after_a_flood...) landed in #23's squash 88c5d40; the login tests that kill both `<` and `<<` landed in #42 (b4501f0) and #52 (68acb27). So the survivor list was accurate when filed and went stale as #23 (final), #42 and #52 merged. The PR never re-ran it on current main.
- F2 also appears in tasks/NET-09.md ("three uncovered mutants") and the PR body ("three survivors"); AUDIT's "critical mutants ... 1, 6, 7" should be 5 and 7 (`==`, `>=`), the ones this PR actually rescues.
- F1 suggested rewording of the src comment: keep "do not rewrite in terms of the constant"; replace "The ONLY test that kills" with "the only test that pins the value independently — receiver/login tests also kill `1 >> 20` today, but only incidentally, and the boundary tests below cannot".
- Verdict restated: NOT mergeable as-is; F1-F4 required (doc/comment only), F5/F6 optional. jit-macos was still pending at review time.
- Not verified: TLS path; mutants.sh exit codes (output piped through tail); broad filter on the branch (implied by narrow 7/7); jit-macos result.
