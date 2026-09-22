# LIVE-01 — Eight-hour live pool run — 600 accepted, 1 rejected, 0 withheld, 0 errors.

**Status:** Completed

First end-to-end run against a real pool since the migration. The 92-test gate proves emitted ARM64 agrees with the *interpreter*; only this shows a **pool** accepts it (no block found, so not "what Monero accepts"). Hashrate median **2226.9 H/s** (n=2817, autocorrelated — descriptive only), 0 unplanned disconnects. **Two review rounds; round 2 returned NOT MERGEABLE.** Round 1 found the rejection timeline unsupported — three submissions outstanding, all found while their job was current, and `pool_connection.rs` logs responses with **no identifier** (now GitHub #17). Round 2 found the *correction* had turned a true weak claim into a **vacuous** strong one: "601 found = 601 responses, so none was lost across any rotation" — but outstanding was **0 at all 12 rotations**, so the case never arose and the identity would hold if the path were broken. Withdrawn to *Not established*. Round 2 also found the wrong-hash argument omitted the verifier's own documented limit (`miner.rs:740-747`: both paths run `emit_body`, so a shared-emitter defect passes) — the exact error the entry's opening criticises. "0 withheld" is directly observable (withholds log at `error!`; 0 ERROR lines), not inferred. Full log committed as `LIVE8H_RUN.log`, internally consistent 2877/2877. Not established: rotation-window survival, 12-thread behaviour, the TLS path, the `off` switch paths, multi-day stability. Ledger: `REVIEW_PR16.md` (removed from the tree; retrieval sha in LIVE-01).

---

*Full record: the `LIVE-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
