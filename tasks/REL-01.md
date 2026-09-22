# REL-01 — Release flow no longer contradicts itself (#11).

**Status:** Completed

`RELEASING.md` and `release.yml` both ran `gh release create` for the same tag, so following the documented steps collided; the doc also asserted the workflow "only creates an empty entry, if it runs at all", false on every clause. Latent only because no `v*` tag has been pushed since the migration and all three existing tags predate the workflow. Fixed by deciding **who owns the entry**: CI creates it **as a draft** (a published-but-empty release is worse than none, and a draft is invisible to the public), the operator does `gh release upload` then `--draft=false`. Workflow made idempotent so a re-run reports success. Also fixed: an unparseable half-migrated sentence, and an automation note asking for a self-hosted `macos-arm64` runner that `macos-14` made obsolete. `make verify-jit` added to the release checklist — the only check that exercises emitted ARM64. **Not executed end to end**: that needs a `v*` tag pushed to the public repo, which was not done without asking. Verified by reading, not observation. Round 1 review confirmed the load-bearing draft-visibility claim from GitHub's REST docs and that `gh` resolves drafts by tag from `gh`'s own source, and ran the workflow's shell against a stub `gh` (it can still go red). It caught two defects this change introduced — the bump step never said to merge the PR and return to `main`, though `make release` tags `HEAD`; and `ci.yml`/`Makefile` comments still described the pre-draft behaviour while pointing at the issue this closes. Ledger: `REVIEW_PR14.md` (removed from the tree; retrieval sha in LEDGER-01).

---

*Full record: the `REL-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
