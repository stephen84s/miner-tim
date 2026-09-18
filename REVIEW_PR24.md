# REVIEW_PR24 — round 1, independent

PR #24 "Adopt mutation testing; move break-testing into the author's protocol"
Base `main` (`f2abc1e`), head `e358581`. Reviewer spawned cold.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Advisory job cannot block a merge | DONE |
| 2 | `scripts/mutants.sh` works as documented | in progress |
| 3 | Cost claim honest / like-for-like | in progress |
| 4 | Equivalent-mutant claim + the reasoning from it | DONE |
| 5 | Rule text — behaviour change or intention | DONE |
| 6 | AUDIT PROC-06 / CLAUDE.md row vs the diff | DONE |
| 7 | Nothing else broken (`verify-jit.sh`) | DONE |
| 8 | clippy + test suite | pending |

---

## Findings

### F1 (MAJOR) — 102 files / 1.5 MB of cargo-mutants scratch output committed

`git diff origin/main...HEAD --stat` is **107 files, 15,816 insertions**. Of
those, **102 files are tool output**: `mutants.out/` (788 KB) and
`mutants.out.old/` (788 KB) — the second being nothing but the previous run,
which `cargo-mutants` rotates automatically. They contain `outcomes.json`
(1,425 lines), per-mutant build logs, and a byte-level diff per mutant.

`.gitignore` has **no `mutants.out` entry**, so this is not a one-off slip: it
recurs on every future run.

This is LEDGER-01 repeating four days later. That entry removed 530 KB of
`REVIEW_*.md` from the tree, on the grounds that it was "larger than the entire
Rust source (504 KB)". This PR adds 1.5 MB of a strictly less durable artifact,
and the entry that records the change does not mention it.

**Operational consequence.** `scripts/mutants.sh` offers no `--output` control
and `cargo mutants` defaults to writing `mutants.out/` in the working directory.
So the tool the PR asks the author to *prefer over judgement* dirties two
tracked paths every single time it is run, and rotates the older one. Any author
following the new `CLAUDE.md` rule ends with a dirty tree and a spurious 800 KB
diff to discard.

### F2 (MAJOR) — the "Files changed" list in `AUDIT.md` PROC-06 is false

PROC-06 states:

> **Files changed:** `CLAUDE.md`, `.claude/agents/_shared-context.md`,
> `scripts/mutants.sh`, `.github/workflows/ci.yml`, `AUDIT.md` (this entry).

Five files. The diff touches **107**. The `CLAUDE.md` task-board row omits them
too, as does the PR body.

This is a false claim in the permanent, append-only ledger, in the entry whose
entire subject is claims recorded as true at the moment they were false. Per
`CLAUDE.md` step 0 an entry on an unmerged branch may still be edited in place,
so this is correctable without an appended note.

### F3 (MAJOR) — the advisory job is red today and is red forever, and its
redness carries no information

Live evidence on head `e358581`:

* check run `mutation testing (advisory, hex::)` — **conclusion `failure`**
* workflow run `35334408664` (CI) — **conclusion `success`**
* job log: `21 mutants tested in 44s: 1 missed, 18 caught, 2 unviable`
  then `##[error]Process completed with exit code 2.`

`cargo-mutants` exits 2 when a mutant survives, so the script exits 2, so the
job fails. The survivor is the `|`→`^` mutant, which the PR itself proves is
**equivalent and therefore unkillable by any test** (F4). The job is therefore
red on this PR and will be red on every future PR until the scope changes.

The PR's own argument against making it required is:

> A gate that fails on equivalent mutants is a check people learn to ignore

— and it then ships exactly that, one severity down. `continue-on-error: true`
keeps the *workflow* green, but the *check run* is red, so the PR page shows
"Some checks were not successful" on every PR from here on.

The load-bearing consequence: **a permanently-red check cannot report anything.**
Nobody can distinguish "a new survivor appeared in a future change" from "the
usual `|`/`^`" without opening the log. The stated purpose of the job — to make
a missing break-test visible — cannot be discharged by a signal that is
constant.

Nothing in the PR body, `AUDIT.md` PROC-06, or the `ci.yml` comment says the job
is red. PROC-06's Verification paragraph says only "reports 18 caught / 1 missed
/ 2 unviable", omitting the exit status and the resulting check conclusion.

`advisory vs required` is presented as the only axis. It is not:
`cargo-mutants` supports suppressing a known-equivalent mutant, after which a
red job would mean something. That option is not discussed.

### F4 (verified, no defect) — the equivalent-mutant claim is correct

Checked exhaustively and independently, not by reading the PR:

* all 256 `(hi, lo)` nibble pairs give `(hi<<4)|lo == (hi<<4)^lo` — 0
  disagreements;
* the premise holds because `nibble()` can only return `0..=15` (`b'9'-b'0'=9`,
  `b'f'-b'a'+10=15`), so the two operands never share a set bit.

The claim is sound. What follows from it is F3.

### F7 (verified, no defect) — `scripts/verify-jit.sh` untouched

`git diff origin/main...HEAD -- scripts/verify-jit.sh` is empty;
`EXPECTED_PASSES=92` present at line 65 and asserted at line 110. The five
required checks were all green on head `e358581`, including both `jit-*` jobs.

### Priority 1 — the advisory job cannot block a merge (confirmed)

* Live branch protection required contexts are exactly the five:
  `lint (clippy, x86_64 linux)`, `audit (cargo-audit / RustSec)`,
  `test (cargo test --release, x86_64 linux)`,
  `jit-macos (aarch64 darwin, make verify-jit)`,
  `jit-linux-arm (aarch64 linux, scripts/verify-jit.sh)`.
  `strict: true`, `enforce_admins: true`. The new job's name,
  `mutation testing (advisory, hex::)`, is **not** among them.
* `continue-on-error: true` is set at job level (ci.yml:290) — empirically the
  workflow run concluded `success` with the job red.
* **Nothing shared.** The `mutants` job has no `actions/cache` step at all, so
  no key can collide with `target-lint`/`target-test`/`cargo-*`. No `needs:`.
  It reads workflow-level `env` and the workflow-level `concurrency` group
  read-only; adding a job to the group changes nothing for the other five.
* CI-03 respected: no new trigger — `pull_request` + `workflow_dispatch` only.

### F5 (MAJOR) — "a permanent fix" is claimed for a mechanism that is out of
scope of every defect it cites

The PR opens "A permanent fix for the defect this session produced repeatedly";
PROC-06 says "Fixed three ways". Inventory what actually lands:

| Part | What it is | Enforces anything? |
|---|---|---|
| `CLAUDE.md` bullet | prose | no |
| `_shared-context.md` sharpening | prose | no |
| `scripts/mutants.sh` | opt-in, run only by an author who remembers | no |
| CI job | runs `hex::` only | see below |

**The advisory job's scope excludes all three named motivating defects.** Every
one is in `pool_connection.rs` / TLS-verifier territory — the all-zeros
fingerprint (verifier wiring), the flooding socket the test server dropped, the
third-reconnect assertion. The job mutates `hex_decode` and nothing else. It
could not, even in principle, have caught any of them.

So the new mechanism is: two prose rules, plus a script nobody is obliged to
run, plus a check that is permanently red on one 30-line pure function. Issue
**#19** — open, filed by this same repo — names exactly this pattern:

> The new rule describes an intention with nothing enforcing it — **the
> recurrence being fixed came from a rule followed exactly as written.**

The PR's defence is that the tool is the enforcement and the prose only the
pointer. That does not hold on this configuration: nothing makes a missing
break-test visible. Declaring the defect permanently fixed in the append-only
ledger is the same false-completion pattern the entry is about.

The prose itself is good — `CLAUDE.md`'s new bullet is specific, names the
failure mode, and quotes three real examples. As documentation it is an
improvement. As a *fix* it is the thing #19 says does not work, and the entry
should say so rather than claim closure.

### F6 (MINOR) — script exit 2 collides with cargo-mutants' own exit 2

`scripts/mutants.sh` exits 2 for a usage error, and `cargo-mutants` exits 2 for
"mutants survived" (measured: `./scripts/mutants.sh hex_decode 'hex::'` exits
**2**). A typo'd argument and a genuine survivor are indistinguishable by exit
code. Harmless while advisory; not harmless if the job is ever promoted, which
the entry explicitly leaves open. `$# -lt 2` also silently ignores extra
arguments (`./scripts/mutants.sh --help` prints usage, which is fine by
accident).

### F8 (MINOR) — `cargo install cargo-mutants --locked` is unpinned

ci.yml:299. It resolved to 27.1.0 and rebuilt from source in 1m06s with no
cache. A future release can change the generated mutant set, or raise its MSRV
above the deliberately pinned `RUST_VERSION: 1.97.1`. The ci.yml comment calls
the job "a standing demonstration that the tooling works"; an unpinned installer
undercuts that, and every other toolchain input in this workflow is pinned on
purpose. `timeout-minutes: 20` against a ~2 min observed run is ample headroom.

### F9 (MINOR) — no `make mutants` target

Every other verification tool in the repo is reachable from the `Makefile`
(`verify-jit`, `verify-jit-linux`, `audit`, `test`, `bench`). `mutants.sh` is
not, and is absent from `README.md`. Its only pointer is the `CLAUDE.md` prose
the PR argues is insufficient on its own.

### F10 (NIT) — 3,196 is an aarch64 figure quoted without its platform

Reproduced exactly on this Mac (`arm64`): **3,196** total, `randomx/vm.rs`
**855**, `randomx/jit/aarch64.rs` **710**, `src/pool_connection.rs` **88**. All
four quoted figures check out, including the corrected 88.

Caveat the entry does not give: 967 of those 3,196 (30%) are in `jit/aarch64.rs`
+ `jit/compiler.rs`, which are `cfg`'d out on x86_64 — so the `ubuntu-24.04`
runner could never test them even if the job were widened crate-wide. The scale
figure is right; what it would cost to act on it is platform-split.

