# REVIEW_PR46 — round 2 (cold, pr-reviewer)

Head reviewed: `1ae4155` (origin matches). Base: origin/main. Scope: agent files,
CLAUDE.md, AUDIT.md, tasks/, research doc. No src/jit, benches, workflows,
Makefile, scripts, .cargo — nothing to hand off.

## Coverage ledger
| # | Item | Status |
|---|---|---|
| 1 | F1 indentation rule consistent | pending |
| 2 | F2 exits-141 direction | pending |
| 3 | F3 figures recomputed | pending |
| 4 | F4 contradiction + "at least three times" | pending |
| 5 | F5 pr-reviewer.md facts vs src | pending |
| 6 | F6 reordering note | pending |
| 7 | N1-N3, N5 | pending |
| 8 | N4 PR body | pending |
| 9 | Ledger removal + sha 2340ad1 | pending |
| 10 | CI status | pending |
| 11 | Fresh-eyes pass on fix diff | pending |

## Findings

### Verified, not findings
- Head 1ae4155 = origin; merge-base = origin/main tip 0be2000 (base current).
- F2: AUDIT prose now says a match yields 141, non-match a correct 1. Correct.
- F3 figures reproduce: 78a5f42 = 8 files/158+/8-; origin/main...1ae4155 = 12 files/1099+/9-;
  entry = 178 lines (7396-7573); tasks/PROC-09.md 54; research doc 647.
- REVIEW_PR46.md absent from 1ae4155 tree; `git show 2340ad1:REVIEW_PR46.md` works; intermediate
  shas ce1c05f/6089140/27e2041 match log.
- Research-doc status note: six bullets, intro "six items", closing "six items" — N1 fixed.
- N2 (rust-implementer Finishing), N3 (audit-writer Active rule), N5 (Not Established) present.
- F5 disarm *condition list* matches native_loop_applies (vm.rs:1167-1174) + miner.rs:657-658.
- Stratum facts: agent concat! (pool_connection.rs:511), algo rx/0, 60s keepalive (l.24) — correct.
- Seven drift-guard comments: lines 61/67/273 at indent 4, 425/589/641/661 at indent 0. Matches text.

### R2-F1 (minor) — F1 fix incomplete: two passages still state the list-nesting rule
AUDIT PROC-09 "Not Established" bullet 2: "list-nested vs. column-0 block is this entry's working
hypothesis" — directly contradicts the F1 correction paragraph above it. "Known, acknowledged gap"
paragraph: "a guard nested inside a list item costs real (if small) tokens while a column-0 guard
above a heading appears to cost none" — same superseded framing. Research doc is consistent; AUDIT
argues with itself (exactly the stale-claim failure mode).

### R2-F2 (minor) — F5 fix introduces a false rationale
pr-reviewer.md item 2: "in every one of those cases both the mined path and the reference path are
already the interpreter". False for switch-off / light / non-rx/0 with a working JIT: both paths are
the *body JIT* (reference = new_full + set_native_loop(false), miner.rs:462-465). The true reason is
that the mined path *is* the reference path. A reviewer trusting this could wrongly believe
`--native-loop off` mines on the interpreter. Also omits non-aarch64 build (native_loop_effective is
hard-false there, vm.rs:1837-1840; miner.rs:476 lists it).

### R2-F3 (minor) — F5 section copies an inaccurate CLAUDE.md fact
pr-reviewer.md: "thread 0 generates the shared dataset" / "Thread 0 generates". Source:
get_or_generate_dataset (miner.rs:872-888) — "The first thread to encounter a new seed_hash
generates"; whichever worker takes the lock first. Inherited from CLAUDE.md's Mining Flow step 4 /
Dataset section, but now lands in a reviewer's sole context labelled as fact.

### R2-F4 (minor) — F6 fix records only half the reordering
Note says step 4 ran "ahead of step 1". Round-1 F6 said it skipped steps 0-3, and step 0 (lesson
inventory, "Do this before moving a single line") plus §1 item 4 ("only after each agent's real
dependencies ... have been moved") is the doc's own mitigation for the exact sealed risk that then
materialised three times. Neither AUDIT nor the status note says step 0 was skipped; AUDIT
"Dependencies found..." frames step 0 as "the next stage".

### Nits
- R2-N1 AUDIT Files Changed (pr-reviewer bullet): "(no independent review has run on this PR yet"
  — now false; round 1 ran.
- R2-N2 Review para: F3 "fixed in the numbers below" — the numbers are above (Files Changed).
  Same dangling-direction class F3 was about.
- R2-N3 Diffstat-difference sentence attributes the residue to "the round-1-review fix commit";
  336e243/eddcdc8/09802cc (pr-reviewer +13/+15, audit-writer +10) and 1ae4155 also contribute.
  Totals correct; attribution incomplete.
- R2-N4 Known-gap paragraph's list of unguarded copied passages not updated for the new
  Mining-Flow/Stratum/Dataset copy in pr-reviewer.md (no drift guard added for it in CLAUDE.md).
