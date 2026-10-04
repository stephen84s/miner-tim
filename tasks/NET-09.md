# NET-09: Add `read_line`'s missing 1 MiB limit coverage (#27)

**Status:** Active

**Summary**

Issue #27: `read_line` function in `src/pool_connection.rs` enforces a 1 MiB length guard with zero dedicated test coverage. The issue named three mutants as surviving on `main` (`>`→`>=`, `>`→`<`, `<<`→`>>`), but that framing had gone stale by the time this PR was written — later, unrelated PRs (#23, #37/#41) already closed the `<` and `<<` gaps incidentally through their own tests. Review round 1 caught this and the lead independently reproduced it against unmodified `main`: the real survivors were only `>`→`==` and `>`→`>=`. Added three test functions to the `tls_tests` module — a line at the limit, rejection of over-limit lines, and a literal pin on `MAX_LINE_BYTES`'s value — which between them close both real gaps and pin the constant independently of any incidental coverage.

**Key Points**

- **Three new tests**: `read_line_limit_is_exactly_one_mebibyte` (literal pin), `read_line_accepts_a_line_of_exactly_the_limit` (boundary), `read_line_refuses_one_byte_over_the_limit_and_names_it` (overflow).
- **No production-code changes**: only test additions, no changes to `read_line` implementation.
- **Break-tests**: four mutations hand-verified (>=, <, >> shift, delete guard block); all killed as expected.
- **Mutation testing**: 7 mutants tested, 7 caught, exit 0 on both narrow and broad filters.
- **Test count**: 195 (main) → 198 (+3 new tests).

PR #55 reviewed (Opus, round 1): no blockers/majors, 4 minors + 2 nits, all fixed. Not yet merged — `jit-macos` was still pending at review time.
