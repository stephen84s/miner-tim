# NET-09: Add `read_line`'s missing 1 MiB limit coverage (#27)

**Status:** Active

**Summary**

Issue #27: `read_line` function in `src/pool_connection.rs` enforces a 1 MiB length guard with zero test coverage. Mutation testing identified three uncovered mutants: `>`→`>=`, `>`→`<`, and `<<`→`>>` at the guard and constant. Added three test functions to the `tls_tests` module, covering the exact boundary, a line at the limit, and rejection of over-limit lines. The constant-pinning test ensures the `<<`→`>>` mutation is caught (the two boundary tests would otherwise shrink with the mutation and pass). All three mutants are now killed.

**Key Points**

- **Three new tests**: `read_line_limit_is_exactly_one_mebibyte` (literal pin), `read_line_accepts_a_line_of_exactly_the_limit` (boundary), `read_line_refuses_one_byte_over_the_limit_and_names_it` (overflow).
- **No production-code changes**: only test additions, no changes to `read_line` implementation.
- **Break-tests**: four mutations hand-verified (>=, <, >> shift, delete guard block); all killed as expected.
- **Mutation testing**: 7 mutants tested, 7 caught, exit 0 on both narrow and broad filters.
- **Test count**: 195 (main) → 198 (+3 new tests).

Implemented on `test/issue-27-read-line-limit` (`db8e344`); not yet merged — no PR opened, no review run yet.
