# REVIEW_PR22 — round 1, independent

PR #22 "security: verify pool TLS certificates by default; add fingerprint pinning"
Branch `security/tls-verify` @ `82e99ea`, base `origin/main` @ `6f5afdc`.
Reviewer: cold `pr-reviewer`, worktree `.claude/worktrees/tls-verify`.

## Scope check / handoff

Diff touches: `src/pool_connection.rs`, `src/bin/minertim.rs`, `src/miner.rs`,
`Cargo.toml`, `Cargo.lock`, `Makefile`, `README.md`, `mining.conf.example`,
`AUDIT.md`, `CLAUDE.md`.

- **Nothing in `src/randomx/jit/`, the emitter, `vm.rs`'s native-loop path or
  `benches/`.** No hashrate or speed claim. `jit-reviewer` not required.
- **`Makefile` is touched** (three lines: a `TLS_FINGERPRINT ?=` default, its
  comment, and the `run` passthrough). Strictly that is `ci-reviewer` territory
  per my brief. **Handed off: the `Makefile` hunk.** I have read it and it is a
  mechanical copy of the existing `NATIVE_LOOP`/`VERIFY_SHARES` passthroughs —
  I record one comment defect below (M-6) but `ci-reviewer` owns the verdict on
  that file. No `.github/workflows/`, `scripts/` or `.cargo/config.toml` change.

## Coverage ledger

| # | Item | State |
|---|---|---|
| 1 | Is the new default actually secure? | done — secure, with a composition hole (B-1) |
| 2 | Is the pinning verifier correct? | done — correct; trait defaults safe |
| 3 | Fingerprint parsing | done — one panic path (m-3) |
| 4 | The CLI path | done — silent erasure (M-1) |
| 5 | The panic in the constructor | done — defensible, evidence recorded |
| 6 | Accuracy of claims (PR / AUDIT / CLAUDE / README) | done — M-2, M-4, m-5 |
| 7 | Operator documentation | done — M-2, M-4, m-4 |
| — | Break-testing | done — B-1 and M-5 both break-tested |
| — | `cargo test --release`, clippy | done |

---

## Findings

### B-1 (blocker) — the repo's own default `POOL` can no longer connect, and nothing says so

`README.md:31` and `mining.conf.example:5` both ship `POOL=pool.supportxmr.com:443`.
`README.md:49-50` repeats it as the direct-binary example. Port 443 is in
`TLS_PORTS` (`pool_connection.rs:189`), so that connection is TLS.

This PR's own survey (issue #20, PR body, `AUDIT.md` SEC-02, `pool_connection.rs:57-63`)
states that `pool.supportxmr.com:443` presents a **self-signed** certificate with
**`CN=mining.pool`**. Under the new default that certificate fails WebPKI on both
trust chain and hostname. **The configuration the repository ships as its
quick-start stops working with this commit.**

That is a defensible security decision — but it is a breaking behaviour change to
every existing user of the documented default, and:

- `README.md:20` still says `make run` with that `POOL` is "the whole setup",
  three paragraphs above the section explaining why it now cannot be.
- `mining.conf.example` keeps `POOL=pool.supportxmr.com:443` and
  `TLS_FINGERPRINT=` (blank) together — a combination the same file's own
  comment block says will fail.
- `AUDIT.md` SEC-02 has no "connections that worked yesterday now fail" line.
  Its "Not verified" paragraph covers the live-TLS gap but never states the
  regression. Under the project's own rule that `AUDIT.md` is the authoritative
  record, an omitted breaking change is the defect, not the missing test.
- The failure is **not** at `connect()`. `rustls::StreamOwned` handshakes lazily,
  so `connect()` returns `Ok` and logs `"Connected to pool (TLS): …"` (line 308)
  *before* any certificate is examined. See m-1 for what the operator actually
  sees.

Marked blocker rather than major because it is a shipped-default regression
combined with a documentation set that still instructs the broken path. The code
change may well be right; the release cannot go out claiming `make run` is the
whole setup.

### M-1 (major) — `parse_tls_fingerprint` silently erases an explicitly configured pin

`src/bin/minertim.rs:283-296`:

```rust
let mut raw: Option<String> = std::env::var("MINERTIM_TLS_FINGERPRINT").ok();
...
    } else if args[i] == "--tls-fingerprint" {
        raw = args.get(i + 1).cloned();   // None at end of argv -> ERASES env
        i += 1;
    }
...
    Some(v) if v.trim().is_empty() => Ok(None),   // "" -> ERASES env, no warning
```

Two shapes erase a pin set in `MINERTIM_TLS_FINGERPRINT`, both silently:

1. `--tls-fingerprint ""` (the `--flag "$UNSET_VAR"` idiom) → `raw = Some("")`
   → `Ok(None)`.
2. `--tls-fingerprint` as the last argument → `args.get(i+1)` is `None` →
   `raw = None` → `Ok(None)`.

**This is R10-F2 recurring, in the same file that documents R10-F2.**
`parse_switch_with` (lines 405-433) deliberately writes `value = as_bool(v).or(value)`
so an empty token cannot erase an earlier setting, and calls `warn_if_empty` in
**both** the flag and env arms (R11-F1), and warns on the bare-flag arm too.
`parse_tls_fingerprint` does neither: no `.or(raw)`, no warning on any of the
three paths.

The PR body, `AUDIT.md` SEC-02 and the code comment all assert the empty case is
"treated as absent, **matching how the on/off switches handle** `--flag
"$UNSET_VAR"`". It does not match, in exactly the respect the switches were
fixed for. The claim is false as written.

Direction of failure is towards full WebPKI verification, not towards "accept
anything", so this is not a wrong-trust bug — but the operator asked for a pin
and did not get one, and nothing told them. For a self-signed pool it surfaces
as a confusing connection failure; combined with B-1 it is indistinguishable
from the new default's failure.

### M-2 (major) — following `mining.conf.example` verbatim for moneroocean yields **cleartext with a "pinned" log line**

`mining.conf.example:42-44` names `gulf.moneroocean.stream:20128` as a pin
target. `TLS_PORTS = [443, 993, 995, 3333, 9999, 14433]` — **20128 is not in it**
— so `is_tls_port()` returns false and `connect()` takes the `PoolStream::Plain`
arm. No TLS, therefore no certificate, therefore the pin is never consulted.

Meanwhile `with_tls_fingerprint` has already logged, at `warn!`:

> `TLS: pinned to certificate <hex>. The trust chain, hostname and expiry are NOT checked for this pool — only that the certificate is exactly the one pinned.`

and `connect()` then logs `Connected to pool (plain TCP): …`. Nothing correlates
the two. An operator who configured a pin sees a log asserting the pin is active
while the wallet address and shares go over an unencrypted socket.

The `TLS_PORTS` list is pre-existing and out of this PR's scope. **The
composition is new**: this PR is the first thing that lets an operator ask for
certificate pinning, and the first thing that documents `:20128` as a place to
ask for it. A configured pin that cannot possibly fire, with no diagnostic, is
the "safety net that cannot fire" shape.

Minimum fix is a diagnostic, not a port-list change: if a fingerprint is set and
the resolved stream is plain TCP, that must be at least a `warn!` naming the
port, ideally a refusal.

### M-3 (major) — README's worked example pins one pool's fingerprint under another pool's address

`README.md:73-75`:

```ini
POOL=pool.supportxmr.com:443
TLS_FINGERPRINT=3d587c824a6f6032e1767518f0f1db29cdf206ba29bd7cb1647f522f8ae3d420
```

`src/pool_connection.rs:817-820` documents that identical constant as:

> "The real **monerohash.com:9999** fingerprint, read 2026-09-13."

The two documents contradict each other. One of them is wrong; I cannot
determine which without network access (see "Not verified"), and it does not
matter for the finding — the README presents a concrete, copy-pasteable value as
if it were `pool.supportxmr.com`'s, in the one section whose entire subject is
that the value must be exact.

Consequence for an operator who copies it: the handshake fails and
`PinnedCertVerifier` prints *"If you did not expect a change, treat this as a
possible interception and do not simply overwrite the pin."* The first thing the
documentation teaches is that this warning is routine noise. That is the
opposite of what the section is for.

Note the `--help` text (`minertim.rs:25`) uses an obviously-fake `a1b2...`
placeholder. The README's use of a real-looking 64-hex value is what makes it
copy-pasteable.

### M-4 (major) — no test exercises the default path this PR exists to create

`the_default_configuration_verifies_certificates` (`pool_connection.rs:862-874`)
calls the free function `webpki_verifier()` directly. It never calls
`PoolConnection::with_tls_fingerprint(level, None)` or `PoolConnection::new()`.

So the `None =>` arm of the `match fingerprint` — the wiring that decides whether
the default `ClientConfig` gets `with_webpki_verifier(...)` or something
permissive — has **no test coverage at all**. The suite asserts that a verifier
*can be built*, not that the default connection *uses* it.

Break-test (mutation applied, observed, reverted — see "Break-testing" below):
replacing the `None` arm with the pre-PR
`.dangerous().with_custom_certificate_verifier(Arc::new(AcceptAnything))` leaves
**138 lib + 10 bin tests passing, 0 failed**. Reintroducing the exact defect
this PR fixes is invisible to the test suite it ships.

This is the repo's documented failure shape: an assertion positioned so it
cannot fail on the thing it names.

### M-5 (major, accuracy) — `AUDIT.md` SEC-02 and the PR body both state a false parity claim

Covered under M-1: "An empty value is treated as absent, matching how the on/off
switches handle `--flag "$UNSET_VAR"`" appears in the PR body, in `AUDIT.md`
SEC-02, and in the code comment. The on/off switches preserve the earlier value
and warn; this one discards and is silent. `AUDIT.md` is append-only and
authoritative, so this lands in the record as a verified behaviour when it is
not.

`CLAUDE.md`'s SEC-02 row does not repeat the claim and is accurate as far as it
goes, but it inherits B-1's omission (no mention that the default pool stops
connecting).

### M-6 (minor→handoff) — `Makefile` comment points at a document that does not discuss this

`Makefile:17-19`:

```make
# TLS_FINGERPRINT unset by default: certificates are fully verified. Set it only
# for a pool whose certificate cannot pass standard validation; see RELEASING-
# adjacent notes in mining.conf.example for how to read one.
```

"RELEASING-adjacent notes" is meaningless — `RELEASING.md` says nothing about
TLS, and the phrase appears to be an editing artefact. The pointer to
`mining.conf.example` is correct; the `RELEASING-` prefix should not be there.
Flagged for `ci-reviewer`, who owns this file.

---

## What is correct (checked, not assumed)

### Item 1 — the default is genuinely WebPKI

- `webpki_verifier()` (line 46-53) builds `RootCertStore::empty()`, extends from
  `webpki_roots::TLS_SERVER_ROOTS`, and returns `WebPkiServerVerifier::builder(...).build()`.
- The `None` arm uses `ClientConfig::builder().with_webpki_verifier(verifier)` —
  the safe builder, **not** `.dangerous()`.
- `grep -rn "dangerous()\|assertion()\|NoVerifier" src/` returns exactly one
  live `.dangerous()` (line 252, inside the `Some(expected)` pinning arm) and
  exactly one live `ServerCertVerified::assertion()` (line 94, inside the
  fingerprint-match branch where it is load-bearing). The other three hits are
  prose in doc comments. **No residual permissive path.**
- No plaintext fallback on TLS failure: `connect()` chooses `Plain` vs `Tls`
  purely from `is_tls_port()`; a handshake error propagates out of
  `login()`/reads rather than downgrading. (The `is_tls_port` heuristic itself
  is the subject of M-2.)

### Item 2 — the pinning verifier

- **Right bytes.** `ring::digest::digest(&SHA256, end_entity.as_ref())` hashes
  the end-entity DER. That is what `openssl x509 -fingerprint -sha256` prints
  and what XMRig's `X509_digest` computes, so the value is portable between the
  two miners as claimed.
- **No partial match.** `actual.as_ref() == self.expected` compares a 32-byte
  slice against `[u8; 32]`; `PartialEq` on slices checks length first. A
  truncated or extended digest cannot compare equal. Not constant-time, which is
  irrelevant — a certificate fingerprint is public.
- **Delegation is genuine.** `verify_tls12_signature`, `verify_tls13_signature`
  and `supported_verify_schemes` all forward to `self.inner`, the real
  `WebPkiServerVerifier`. No re-stubbing. This closes the second defect the PR
  describes (the old `NoVerifier` returned `HandshakeSignatureValid::assertion()`,
  so the peer never had to prove key possession) — and that description is
  accurate: I checked `origin/main`'s version.
- **Trait defaults, checked against `rustls-0.23.37/src/verify.rs:141-155`.**
  Only two methods have defaults: `requires_raw_public_keys() -> false` and
  `root_hint_subjects() -> None`.
  - `requires_raw_public_keys = false` is the **safe** direction: with it false,
    RFC 7250 raw public keys are not negotiated, so `end_entity` is always a
    certificate and the pin always hashes a certificate. Had it been overridden
    to `true`, the pin would be hashing a SubjectPublicKeyInfo while the
    operator believed it was a certificate. Correct as left.
  - `root_hint_subjects = None` only suppresses the `certificate_authorities`
    extension in ClientHello (TLS 1.3 client-auth hint). `with_no_client_auth()`
    is in use, so it is inert. Not a security property. Correct as left.
  - Neither default should have been overridden. Item 2 clean.

### Item 5 — the panic

`webpki_verifier().unwrap_or_else(|e| panic!("{e}; refusing to fall back to unverified TLS"))`.

`WebPkiServerVerifier::builder(...).build()` returns
`Result<_, VerifierBuilderError>`, whose only variants are
`NoRootAnchors` and `InvalidCrl`. Neither is reachable here: the root store is
populated unconditionally from the compiled-in `webpki_roots::TLS_SERVER_ROOTS`
(a `const` slice, not a filesystem or network read), and no CRL is ever
supplied. **There is no realistic runtime input that reaches this panic** — it
would take a `webpki-roots` release shipping an empty slice.

So: the panic is unreachable in practice, and where it is reachable at all the
alternative (a `Result` threaded through `Default`, `new` and
`with_tls_fingerprint`) buys nothing. The PR's reasoning — that a silent
downgrade is worse — is sound, and the cost of being wrong is zero because the
condition cannot arise. **I do not consider this a finding.** `PoolConnection::default()`
and `new()` both routing through it is fine for the same reason.

---

## Minor findings

### m-1 (minor) — the operator's first symptom of B-1 is an undiagnosed rustls error, and the reconnect loop may hide it

Because the handshake is lazy, the sequence an operator sees on a self-signed
pool under the new default is:

```
Connecting to pool: pool.supportxmr.com:443
Connected to pool (TLS): pool.supportxmr.com:443     <- printed before any check
Login failed: <rustls error>
```

The success line is printed first, and the error that follows is a bare rustls
`InvalidCertificate(UnknownIssuer)` / `NotValidForName` with nothing pointing at
`--tls-fingerprint`. For the single change most likely to generate support
traffic, there is no mapping from the error to the remedy. (`PinnedCertVerifier`
does this well — its mismatch error is excellent. The default path has no
equivalent.) The "Connected to pool (TLS)" line predates this PR; it becomes
actively misleading because of it.

### m-2 (minor) — `parse_cert_fingerprint` panics on some non-ASCII input

`hex_decode` byte-slices (`&hex[i..i+2]`), so a `cleaned` string whose byte
length is 64 but which contains a 3-byte UTF-8 character can slice across a char
boundary and panic. Reproduced against the built binary — see "Break-testing".

The PR and `AUDIT.md` both state "a malformed `--tls-fingerprint` **exits 2**".
For this class of malformed value it does not; it panics (exit 101) with
`byte index N is not a char boundary`. The claim is too broad.

Severity minor: the process stops either way and the operator is at the keyboard;
there is no trust consequence. `hex_decode`'s byte-slicing is pre-existing and I
am deliberately not expanding into its other callers.

### m-3 (minor) — "paste what it prints" does not work

`README.md:81-86` and the `--help` text give the `openssl` command and say
"paste what it prints". What it prints is a **prefixed** line
(`sha256 Fingerprint=AA:BB:…` on OpenSSL 3.x, `SHA256 Fingerprint=…` on macOS
LibreSSL — see "Environment"). `parse_cert_fingerprint` strips only `:`, so the
prefix survives and the length check rejects it. Failure is loud (exit 2 with
the same command repeated), so this is a first-run friction defect, not a
security one — but the instruction as written is wrong. Piping through
`| cut -d= -f2` would make it literally true.

Note `parse_tls_fingerprint` does `.trim()` before parsing, so a trailing
newline or surrounding spaces are handled. Internal whitespace is not, and is
correctly rejected. A `0x` prefix is correctly rejected (length). Mixed case is
accepted (`u8::from_str_radix(_, 16)`). Colon-stripping is unconditional, so
`a:b:c…` with colons in odd places still parses if 64 hex chars remain — cosmetic
laxity, not a way to accept a *wrong* value.

### m-4 (nit) — the mismatch error message is malformed

`pool_connection.rs:97-105`. The format literal mixes escaped `\n` followed by 19
literal spaces with `\`-continuations, so the rendered message indents
inconsistently:

```
pool certificate does not match the pinned fingerprint.
                   expected: …
                   presented: …
                   The pool may have renewed its certificate — re-read it with `openssl …
```

Content is good; this is the message an operator reads while deciding whether
they are being intercepted, and it should not look corrupted.

### m-5 (nit) — internal consistency of the survey numbers

Cross-checked the five-pool table across the PR body, issue #20's comment,
`AUDIT.md` SEC-02 and `pool_connection.rs`'s doc comment. All five rows, both
`CN=` values, the 2126/2117 expiry years, the `C=IT, ST=Pool, L=Daemon, O=Mining Pool`
subject, the 2026-08-10 monerohash expiry and the "1 in 5" framing agree
everywhere. `CLAUDE.md`'s condensed row says "valid to 2126" for the
`CN=mining.pool` case, consistent. The XMRig quote is reproduced identically in
all three places. **No internal contradiction found.** Two notes:

- `pool_connection.rs:61` writes the subject as `C=IT, O=Mining Pool, L=Daemon`
  (reordered) where the other three write `C=IT, ST=Pool, L=Daemon, O=Mining Pool`
  and drop `ST=Pool`. Same claim, sloppily transcribed.
- The honesty about the live gap **is** maintained in the PR body and in
  `AUDIT.md` SEC-02 ("No connection to a real pool was made with this build", and
  the note that LIVE-01 used plain TCP so the TLS path has never run). It is
  **not** carried into `README.md`, which states flatly that MinerTim "checks its
  certificate the way a browser does" — true of the policy, untested on the wire.
  I do not think the README should carry that caveat; an operator does not need
  it, and the code is policy-correct. Recording it as considered, not as a defect.

---

## Item 7 — operator documentation, judged

The README section is the strongest part of this PR. Plain language, no
salesmanship, no jargon beyond "fingerprint" which it then defines. Both caveats
are stated in the terms an operator can act on:

- Trust-on-first-use: *"If you read it over a connection someone was already
  interfering with, you have just pinned their certificate."* Clear, and it gives
  an action (read it from a trusted network, cross-check from elsewhere).
- Renewal: *"Mining stops until you update the line, which is the system
  working — but it means you should expect it."* Clear, with the ~90-day figure
  and the self-signed contrast.
- *"If a pool has a proper certificate, do not pin it"* is the right closing
  instruction and is easy to miss in most miner documentation.

Someone acting on this section would **not** be misled about what pinning does.
They **would** be misled by the wrong fingerprint value (M-3), and they would be
stranded by the unchanged quick-start (B-1). `mining.conf.example` is of the same
quality and carries the same M-2 problem.

---

## Break-testing

Per `_shared-context.md`: mutations applied to a copy-protected file, observed,
and reverted. `git status` clean at end of review (verified).

1. **M-4, the untested default.** Replaced the `None =>` arm of
   `with_tls_fingerprint` with a permissive verifier equivalent to the deleted
   `NoVerifier`. Result: **138 lib + 10 bin passed, 0 failed.** The suite does
   not notice the PR's central defect being reintroduced. Reverted;
   `git diff` empty.
2. **B-1 / M-2 composition.** Confirmed `is_tls_port("gulf.moneroocean.stream:20128")`
   is `false` and `is_tls_port("pool.supportxmr.com:443")` is `true` by direct
   evaluation against `TLS_PORTS`.
3. **m-2, the unicode panic.** Ran the built binary with a 61-ASCII + one 3-byte
   character argument. Observed panic, not exit 2.

(Details and exact observed output are recorded in the sections above; the
mutation for (1) was never committed.)

## Verification run by me, not taken from the PR

- `cargo test --release` — **138 passed, 0 failed, 2 ignored** (lib);
  **10 passed, 0 failed** (bin); 0/0 doc. Matches the PR's claim exactly.
- `cargo clippy --all-targets --release -- -D warnings` — recorded below.
- JIT gate not run: no `src/randomx/` change in this diff, so `make verify-jit`
  is not implicated. CI's `jit-macos` / `jit-linux-arm` will run it regardless.

## Not verified

- **No live TLS connection to any pool.** I did not contact
  `pool.supportxmr.com`, `gulf.moneroocean.stream` or `monerohash.com`. I
  therefore cannot confirm or refute the five-pool survey, and cannot say which
  of the two contradictory attributions in M-3 is correct. The PR is honest that
  it did not do this either.
- **The claim that the new default actually completes a handshake against a
  well-configured pool.** Nothing in this repo has ever exercised the TLS path
  against a real pool (LIVE-01 used plain TCP). Policy verified by reading;
  behaviour on the wire unverified.
- **XMRig's `Tls.cpp`.** I did not fetch upstream source; the quoted line is
  taken on the PR's word and is internally consistent across all four documents.
