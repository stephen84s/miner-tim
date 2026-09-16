# REVIEW_PR22 — round 1, independent

PR #22 "security: verify pool TLS certificates by default; add fingerprint pinning"
Branch `security/tls-verify` @ `82e99ea`, base `origin/main` @ `6f5afdc`.
Reviewer: cold `pr-reviewer`, worktree `.claude/worktrees/tls-verify`.

**Verdict: NOT MERGEABLE.** No blocker by this repo's definition (no wrong hash,
no memory unsafety, no data loss). **Five majors, all five ACTIONABLE** before
merge, plus seven minors of which two are actionable.

The core cryptographic change is **correct** — I verified the default and both
pinning outcomes inside real TLS handshakes, which the PR states had never been
done. What is wrong is the wiring around it, the test coverage of it, and three
documents that describe it inaccurately. One of those inaccuracies is now proved
by measurement rather than argued: the README pins **monerohash's** certificate
under **supportxmr's** address (M-3).

## Scope check / handoff

Diff: `src/pool_connection.rs`, `src/bin/minertim.rs`, `src/miner.rs`,
`Cargo.toml`, `Cargo.lock`, `Makefile`, `README.md`, `mining.conf.example`,
`AUDIT.md`, `CLAUDE.md`.

- Nothing in `src/randomx/jit/`, the emitter, `vm.rs`'s native-loop path or
  `benches/`. No hashrate or speed claim anywhere in the diff. **`jit-reviewer`
  not required.**
- **`Makefile` is touched** (three lines). That file is `ci-reviewer`'s.
  **Handed off: the `Makefile` hunk.** I read it — it is a mechanical copy of the
  existing `NATIVE_LOOP`/`VERIFY_SHARES` passthroughs — and record one comment
  defect (m-6) for `ci-reviewer` to rule on. No `.github/workflows/`, `scripts/`
  or `.cargo/config.toml` change, so nothing else goes that way.

## Coverage ledger

| # | Brief item | State | Outcome |
|---|---|---|---|
| 1 | Is the new default actually secure? | done | **Yes** — verified by reading *and* by a live handshake. Composition hole: M-2. |
| 2 | Is the pinning verifier correct? | done | **Yes** — digest, comparison, delegation and both trait defaults all checked. Clean. |
| 3 | Fingerprint parsing | done | Sound for all realistic input; one panic path (m-2), one usability defect (m-3). |
| 4 | The CLI path | done | **M-1** — silently erases an env pin; does not match the house convention it claims to. |
| 5 | The panic in the constructor | done | **Defensible.** Unreachable; evidence recorded. Not a finding. |
| 6 | Accuracy of claims (PR / AUDIT / CLAUDE / README) | done | **M-3 proved by live cert read**; two survey rows reproduced exactly, one unreachable (M-4); numbers internally consistent. |
| 7 | Operator documentation | done | Strong prose; **M-3** value is wrong, **M-4** regression undisclosed. |
| — | Break-testing | done | Three mutations applied and reverted: two survived (**M-5**, **m-7**), one was caught. M-1, M-2, m-2, m-3 reproduced against the built binary. Tree clean afterwards. |
| — | `cargo test --release`, clippy | done | 138 + 10 pass; clippy clean. Both claims confirmed. |
| — | Concurrency (standing item) | done | Nothing new. `tls_config: Arc<ClientConfig>` is shared read-only across the receiver and submit paths. No change to job handoff or nonce interleaving. |
| — | Resource use (standing item) | done | No new allocation. `ring` was already in the graph via rustls; `Cargo.lock` gains one line (direct-dep edge), no new crate. |
| — | Orphaned doc comments (standing item) | done | `parse_tls_fingerprint` is spliced *above* `parse_switch`'s doc comment, not between it and its function. Checked; clean. |

---

# Majors

## M-1 — `parse_tls_fingerprint` silently erases an explicitly configured pin, and the claim that it matches the other switches is false

`src/bin/minertim.rs:283-296`:

```rust
let mut raw: Option<String> = std::env::var("MINERTIM_TLS_FINGERPRINT").ok();
...
    } else if args[i] == "--tls-fingerprint" {
        raw = args.get(i + 1).cloned();   // None at end of argv -> ERASES the env value
        i += 1;
    }
...
    Some(v) if v.trim().is_empty() => Ok(None),   // "" -> ERASES the env value, silently
```

Two shapes discard a pin set in `MINERTIM_TLS_FINGERPRINT`, neither with a
warning:

1. `--tls-fingerprint ""` — the `--flag "$UNSET_VAR"` idiom — sets `raw = Some("")`
   and resolves to `Ok(None)`.
2. `--tls-fingerprint` as the final argument — `args.get(i+1)` is `None`, so
   `raw = None` and it resolves to `Ok(None)`.

**Observed, against the built release binary:**

```
$ V=3d587c82…d420
$ MINERTIM_TLS_FINGERPRINT=$V ./target/release/minertim pool.invalid:443 w 1
WARN  minertim::pool_connection] TLS: pinned to certificate 3d587c82…d420. …

$ MINERTIM_TLS_FINGERPRINT=$V ./target/release/minertim pool.invalid:443 w 1 --tls-fingerprint ""
(no TLS line, no warning at all)

$ MINERTIM_TLS_FINGERPRINT=$V ./target/release/minertim pool.invalid:443 w 1 --tls-fingerprint
(no TLS line, no warning at all)

$ ./target/release/minertim pool.invalid:443 w 1 --native-loop ""
warning: --native-loop given an empty value - ignoring it; the previous setting or the default applies. Use on|off.
```

**This is R10-F2 recurring, in the same file that documents R10-F2.**
`parse_switch_with` (lines 405-433) deliberately writes `value = as_bool(v).or(value)`
precisely so an empty token cannot erase an earlier setting, and calls
`warn_if_empty` in **both** the flag and the env arms (R11-F1), and warns on the
bare-flag arm as well. `parse_tls_fingerprint` does none of the three: no
`.or(raw)`, no warning on any path.

The PR body, `AUDIT.md` SEC-02 and the code comment at `minertim.rs:293-295` all
state the empty case is *"treated as absent, **matching how the on/off switches
handle** `--flag "$UNSET_VAR"`"*. The last comparison shows it does not match, in
exactly the respect those switches were fixed for. **The claim is false as
written, and it is on its way into the append-only record.**

Direction of failure is towards full WebPKI verification, not towards "accept
anything", so this is not a wrong-trust bug. But the operator asked for a pin,
did not get one, and nothing told them — and on a self-signed pool the symptom is
an opaque certificate error that looks identical to having configured nothing.

**ACTIONABLE.** Preserve the previous value (`.or(raw)` on the empty arm and on
the missing-value arm) and warn on all three paths, or state in the code and in
`AUDIT.md` that this switch deliberately does *not* follow the convention and
why. What cannot stand is the code doing one thing and three documents asserting
the other.

## M-2 — a configured pin can be silently inert, while the log asserts it is active

`TLS_PORTS = [443, 993, 995, 3333, 9999, 14433]` (`pool_connection.rs:189`).
`is_tls_port()` decides TLS purely from the port. `mining.conf.example:42-44`
names **`gulf.moneroocean.stream:20128`** as a pin target — and 20128 is not in
that list, so `connect()` takes the `PoolStream::Plain` arm. No TLS, therefore no
certificate, therefore `PinnedCertVerifier` is never constructed into a live
handshake and the pin can never fire.

Meanwhile the constructor has already logged the pin as active.
**Observed** (local listener on 20128, so the TCP connect succeeds):

```
INFO  minertim] Connecting to 127.0.0.1:20128...
WARN  minertim::pool_connection] TLS: pinned to certificate 3d587c82…d420. The trust chain,
      hostname and expiry are NOT checked for this pool — only that the certificate is
      exactly the one pinned. Re-pin if the pool renews.
INFO  minertim::pool_connection] Connecting to pool: 127.0.0.1:20128
INFO  minertim::pool_connection] Connected to pool (plain TCP): 127.0.0.1:20128
```

Two lines apart: "pinned to certificate X" and "plain TCP". Nothing correlates
them, and the wallet address and shares then go over an unencrypted socket.

**And 20128 really is a TLS port.** Read live on 2026-09-13:

```
$ openssl s_client -connect gulf.moneroocean.stream:20128 -servername gulf.moneroocean.stream </dev/null 2>/dev/null \
    | openssl x509 -noout -subject -issuer -enddate -fingerprint -sha256
subject=C=IT, ST=Pool, L=Daemon, O=Mining Pool, CN=mining.proxy
issuer=C=IT, ST=Pool, L=Daemon, O=Mining Pool, CN=mining.proxy
notAfter=Aug 10 14:37:37 2117 GMT
sha256 Fingerprint=23:9D:AA:DD:5C:7D:0A:C0:97:37:6C:78:71:F7:87:73:88:26:EE:F1:C0:24:72:9E:FF:87:0E:47:3B:97:08:55
```

That is the survey's row reproduced exactly — self-signed, `CN=mining.proxy`, the
stock `C=IT, ST=Pool, L=Daemon, O=Mining Pool` subject, valid to 2117. So this is
not a hypothetical: an operator who follows `mining.conf.example` verbatim points
MinerTim at a **live TLS endpoint**, MinerTim speaks **plain TCP** to it because
20128 is not in `TLS_PORTS`, the pin they configured is never consulted, and the
log says it is. The connection then fails on protocol confusion rather than on
anything that names the real cause.

`TLS_PORTS` itself is pre-existing and out of scope. **The composition is new.**
This PR is the first thing that lets an operator ask for certificate pinning and
the first thing that documents `:20128` as a place to ask for it. A safety
control that cannot fire, with a log line claiming it is armed, is the exact
shape `_shared-context.md` lists first.

**ACTIONABLE.** Minimum fix is a diagnostic, not a port-list change: once the
stream is resolved, if a fingerprint is configured and the stream is `Plain`,
`error!` naming the port (arguably refuse to start — the operator explicitly
asked for authentication and is getting cleartext). Moving the existing `warn!`
out of the constructor and into `connect()`'s TLS arm would fix the false claim
at the same time, since only there is it true. Separately, `mining.conf.example`
should not name a port the miner will not use TLS on.

## M-3 — the README's worked pinning example carries a different pool's fingerprint

`README.md:73-75`:

```ini
POOL=pool.supportxmr.com:443
TLS_FINGERPRINT=3d587c824a6f6032e1767518f0f1db29cdf206ba29bd7cb1647f522f8ae3d420
```

`src/pool_connection.rs:817-820` documents that identical constant as:

> "The real **monerohash.com:9999** fingerprint, read 2026-09-13."

**Settled by measurement — the README is the wrong one.** Read live on
2026-09-13 from this host:

```
$ openssl s_client -connect monerohash.com:9999 -servername monerohash.com </dev/null 2>/dev/null \
    | openssl x509 -noout -subject -issuer -enddate -fingerprint -sha256
subject=CN=monerohash.com
issuer=C=US, O=Let's Encrypt, CN=E8
notAfter=Aug 10 23:14:06 2026 GMT
sha256 Fingerprint=3D:58:7C:82:4A:6F:60:32:E1:76:75:18:F0:F1:DB:29:CD:F2:06:BA:29:BD:7C:B1:64:7F:52:2F:8A:E3:D4:20
```

Lower-cased and stripped of colons that is
`3d587c824a6f6032e1767518f0f1db29cdf206ba29bd7cb1647f522f8ae3d420` — **byte for
byte the value the README prints under `POOL=pool.supportxmr.com:443`**. The test
comment is correct; the README attributes monerohash's certificate to supportxmr.
This is no longer a contradiction to be resolved, it is a documented wrong value.

(The same read confirms two of the survey's own rows exactly: monerohash is a
genuine Let's Encrypt certificate for the right host, expiring `Aug 10 2026` —
so expired as of today, 2026-09-13.)

What an operator who copies it sees (**observed**, wrong pin against a live
self-signed TLS server):

```
Failed to initialize: Login failed: Write failed: unexpected error: pool certificate
does not match the pinned fingerprint.
                   expected: 3d587c82…d420
                   presented: 5b4c6f36…c3ed
                   The pool may have renewed its certificate — … If you did not expect a
                   change, treat this as a possible interception and do not simply
                   overwrite the pin.
```

So the first thing the documentation teaches a new operator is that the
interception warning is routine noise to be clicked through. That is the
opposite of the section's purpose, and it degrades the one mechanism this PR
adds.

Note the `--help` text (`minertim.rs:25`) uses an obviously-fake `a1b2...`
placeholder and is fine. It is the README's realistic value that is the hazard.

**ACTIONABLE.** Either use a visibly-placeholder value in the README example, or
pair the real monerohash fingerprint with `POOL=monerohash.com:9999` and say when
it was read.

## M-4 — the repo's own default `POOL` stops working, and no document says so

`README.md:31` and `mining.conf.example:5` both ship
`POOL=pool.supportxmr.com:443`; `README.md:49-50` repeats it as the
direct-binary example. Port 443 is in `TLS_PORTS`, so that connection is TLS.

This PR's own survey — issue #20, the PR body, `AUDIT.md` SEC-02 and
`pool_connection.rs:57-63` — states that `pool.supportxmr.com:443` presents a
**self-signed** certificate named **`CN=mining.pool`**. Under the new default
that fails WebPKI on trust chain *and* hostname.

**I tried to confirm this and could not: `pool.supportxmr.com:443` would not
accept a TCP connection from this host today** (`nc -z -w 5` returns 1;
`openssl s_client` gives `BIO_connect: Operation timed out`, `errno=60`), while
`github.com:443`, `monerohash.com:9999` and `gulf.moneroocean.stream:20128` all
connected from the same shell in the same minute. So the supportxmr row is the one
row of the survey I cannot verify, and I am not going to assume it.

State it as the disjunction, which is airtight either way and which the PR must
resolve:

> **Either** the survey is right, and the configuration this repository ships as
> its quick-start cannot connect after this commit — an undisclosed breaking
> change; **or** the survey is wrong about supportxmr, and the PR's central
> justification for adding pinning at all is built on a bad reading.

Both are findings. The two rows I *could* read (monerohash, moneroocean)
reproduced the survey exactly, which makes the first branch much the likelier —
but "likelier" is not the standard this repo applies to a claim in `AUDIT.md`.

On the first branch, the decision is defensible and the silence about it is not:

- `README.md:20` still says `make run` with that `POOL` is "the whole setup" —
  three paragraphs above the section explaining why it now is not.
- `mining.conf.example` keeps `POOL=pool.supportxmr.com:443` alongside
  `TLS_FINGERPRINT=` (blank), a pairing its own comment block says will fail.
- `AUDIT.md` SEC-02 has no "connections that worked yesterday now fail" line.
  Its "Not verified" paragraph is honest about the live-TLS gap but never states
  the regression. Under this project's rule that `AUDIT.md` is the authoritative
  record, the omission *is* the defect.
- `CLAUDE.md`'s SEC-02 row inherits the same omission.

**ACTIONABLE.** Re-read `pool.supportxmr.com:443` from a host that can reach it
and settle the branch. Then: whatever the code does, the shipped example config
and the quick-start must be internally consistent with it, and if the default
pool stops connecting, `AUDIT.md` must record the behaviour change.

## M-5 — no test exercises the default path this PR exists to create (break-tested)

`the_default_configuration_verifies_certificates` (`pool_connection.rs:862-874`)
calls the free function `webpki_verifier()` directly. It never calls
`PoolConnection::with_tls_fingerprint(level, None)` or `PoolConnection::new()`.
So the `None =>` arm of `match fingerprint` — the wiring that decides whether the
*default connection* gets `with_webpki_verifier(...)` or something permissive —
has no coverage at all. The suite asserts a verifier *can be built*, not that the
default *uses* it. The assertion it does make
(`!verifier.supported_verify_schemes().is_empty()`) is about the rustls provider,
not about MinerTim.

**Break-test.** Replaced the `None` arm with
`.dangerous().with_custom_certificate_verifier(Arc::new(MutantAcceptAnything))`,
a byte-equivalent reintroduction of the deleted `NoVerifier` (all four trait
methods stubbed as before). Result:

```
test result: ok. 138 passed; 0 failed; 2 ignored   (lib)
test result: ok. 10 passed; 0 failed               (bin)
```

**Reintroducing the exact defect this PR exists to fix is invisible to the test
suite it ships.** Mutation reverted from a pre-taken copy; `git status --porcelain`
and `git diff --stat` both empty afterwards, verified.

This is the repo's documented failure shape — an assertion positioned where it
cannot fail on the thing it names.

**The seven new tests were also mutation-checked individually.** Two of the three
mutations were caught, one was not:

| mutation | tests that failed |
|---|---|
| `actual.as_ref() == self.expected` → `!=` in `verify_server_cert` | **both** `a_pinned_verifier_rejects_…` and `a_pinned_verifier_accepts_…` failed. The pinning pair is genuine. |
| `None` arm of `with_tls_fingerprint` → accept-anything verifier | **nothing failed** (M-5). |
| `cleaned.len() != 64` → `cleaned.len() < 2` | **nothing failed** (m-7). |

**Test-count arithmetic checks out and rules out silent coverage loss:** `main`
carries 131 lib + 10 bin (recorded in PLAT-01 and MEM-01); 131 + 7 new = **138
lib + 10 bin**, which is what I observed. No pre-existing test was removed or
renamed to keep the count flat.

**ACTIONABLE.** A test that constructs `PoolConnection::with_tls_fingerprint(0, None)`
and asserts something about the resulting config's verifier would close it. If
rustls does not expose the verifier from a built `ClientConfig`, splitting the
`match` into a `fn build_tls_config(fingerprint) -> ClientConfig` and testing the
discriminant another way is still better than testing neither arm's wiring.

---

# Minors

## m-1 — the operator's first symptom of a certificate failure is undiagnosed, and a permanent failure is retried forever

Because `rustls::StreamOwned` handshakes lazily, `connect()` returns `Ok` and
logs success *before* any certificate is examined. **Observed**, default
configuration against a live self-signed TLS server on 127.0.0.1:9999:

```
INFO  minertim::pool_connection] Connected to pool (TLS): 127.0.0.1:9999
Failed to initialize: Login failed: Write failed: invalid peer certificate:
    Other(OtherError(CaUsedAsEndEntity))
```

The success line comes first, and the error is a bare rustls string with nothing
pointing at `--tls-fingerprint`. For the single change in this PR most likely to
generate support traffic, there is no mapping from symptom to remedy.
`PinnedCertVerifier`'s own mismatch error does this well (see M-3's transcript);
the default path has no equivalent.

Related: if a certificate error occurs *after* startup — a pool renewing under a
pin, which the README correctly says to expect — `reconnect()`
(`pool_connection.rs:545-563`) loops forever at `RECONNECT_DELAY`, logging at
`warn!`. A pin mismatch is permanent, not transient; the miner will sit at 0 H/s
retrying every 5s with a `warn!` an operator running at the default `RUST_LOG=info`
will see but may not distinguish from an ordinary blip. Pre-existing loop; newly
reachable by a permanent cause.

## m-2 — `parse_cert_fingerprint` panics on some non-ASCII input, contradicting "exits 2"

`hex_decode` byte-slices (`src/hex.rs:20`, `&hex[i..i+2]`), so a `cleaned` string
of byte-length 64 containing a 3-byte UTF-8 character slices across a char
boundary. **Observed:**

```
$ ./target/release/minertim pool.example:443 wallet 1 --tls-fingerprint "$(printf 'a%.0s' {1..61})€"
thread 'main' panicked at src/hex.rs:20:43:
end byte index 62 is not a char boundary; it is inside '€' (bytes 61..64 of string)
exit 101

$ ./target/release/minertim pool.example:443 wallet 1 --tls-fingerprint zz
exit 2
```

The PR and `AUDIT.md` both state "a malformed `--tls-fingerprint` **exits 2**".
For this class of malformed value it exits 101 with a panic. The claim is too
broad.

Minor, not major: the process stops either way, the operator is at the keyboard,
and there is no trust consequence. `hex_decode`'s byte-slicing is pre-existing;
this PR is the first to feed it free-form operator text. I am deliberately **not**
expanding into `hex_decode`'s other callers — but note for a future issue that
one of them parses pool-supplied JSON.

## m-3 — "paste what it prints" does not work

`README.md:81-86` and the `--help` text give an `openssl` command and say *"paste
what it prints"*. What it prints is prefixed. **Observed on both openssl binaries
on this host:**

```
OpenSSL 3.6.1   →  sha256 Fingerprint=EA:4E:00:…:CF
LibreSSL 3.3.6  →  SHA256 Fingerprint=EA:4E:00:…:CF
```

`parse_cert_fingerprint` strips only `:`, so the prefix survives and the length
check rejects it. **Observed** pasting verbatim:

```
--tls-fingerprint: expected 64 hex characters (SHA-256), got "sha256 Fingerprint=EA:4E:…:CF".
Read a pool's fingerprint with:
  openssl s_client -connect <host>:<port> -servername <host> </dev/null \
    | openssl x509 -noout -fingerprint -sha256
```

— the error helpfully repeats the command that produced the unusable output. The
command itself is shell-correct (I ran it; `</dev/null 2>/dev/null` and the pipe
are fine, and it works against a local `s_server`). It is the *instruction* that
is wrong. Appending `| cut -d= -f2` would make "paste what it prints" literally
true. Failure is loud, so this is first-run friction, not a security defect.

Also checked, all correct: `.trim()` in `parse_tls_fingerprint` handles a trailing
newline or surrounding spaces; internal whitespace is rejected; a `0x` prefix is
rejected on length; mixed case is accepted; colon-stripping is unconditional so
colons in odd positions still parse when 64 hex characters remain (cosmetic
laxity — it cannot make a *wrong* value accepted). Unicode lookalikes are rejected
by `from_str_radix` except for the panic in m-2.

## m-4 — survey transcription, and one honesty boundary

Cross-checked the five-pool table across the PR body, issue #20's comment,
`AUDIT.md` SEC-02 and `pool_connection.rs`'s doc comment. All five rows, both
`CN=` values, the 2126/2117 expiry years, the stock subject, the 2026-08-10
monerohash expiry and the "1 in 5" framing agree everywhere, and the XMRig quote
is reproduced identically in all three prose documents. **No internal
contradiction found.** Two notes:

- `pool_connection.rs:61` writes the subject as `C=IT, O=Mining Pool, L=Daemon`
  where the other three write `C=IT, ST=Pool, L=Daemon, O=Mining Pool`. Same
  claim, reordered and missing `ST=Pool`.
- The "no real pool connection was made" honesty **is** maintained in the PR body
  and in `AUDIT.md` SEC-02, including the sharp observation that LIVE-01 used
  plain TCP so the TLS path has never run live. It is not carried into the
  README, which says MinerTim "checks its certificate the way a browser does".
  I considered flagging that and decided **not** to: the statement is true of the
  policy, an operator does not need the caveat, and I have now verified the
  behaviour on a real handshake (below). Recorded as considered, not as a defect.

## m-5 — the pin-mismatch message renders correctly (a finding I withdrew)

I initially flagged the `rustls::Error::General` format literal at
`pool_connection.rs:97-105` as malformed — it mixes escaped `\n` plus 19 literal
spaces with `\`-continuations. **Rendered output checked against a live
handshake** (transcript in M-3): the indentation is consistent and the message
reads well. **Withdrawn.** The only residue is that `rustls` prefixes it with
"unexpected error:", which reads oddly for a deliberate policy rejection —
`rustls::Error::InvalidCertificate(CertificateError::Other(...))` would render
better. Nit, not actionable.

## m-7 — `rejects_wrong_length_and_non_hex` survives removal of the length check it names

Break-tested. Loosened the gate in `parse_cert_fingerprint` from
`if cleaned.len() != 64` to `if cleaned.len() < 2`, effectively deleting it.
Result: **all 7 new tests still pass**, `rejects_wrong_length_and_non_hex`
included.

The reason is that the real length guard is `bytes.try_into().ok()` into
`[u8; 32]`, one line below. Every case the test feeds it — `SAMPLE[..62]` (31
bytes), `SAMPLE + "ab"` (33 bytes), `""` (0 bytes) — is caught there instead. So
the explicit length check is untested, and the test's name claims coverage it
does not have.

**Not a production defect** — `try_into` is a correct guard and the two together
are defence in depth, which I would keep. It is a test-quality finding of the
shape this repo has been bitten by: an assertion that cannot fail on the thing it
names. It also shows the seven new tests were not mutation-checked before
submission (in contrast to the pinning pair, which are sound — see below).

## m-6 — `Makefile` comment points at a document that says nothing about this (→ `ci-reviewer`)

`Makefile:17-19`:

```make
# TLS_FINGERPRINT unset by default: certificates are fully verified. Set it only
# for a pool whose certificate cannot pass standard validation; see RELEASING-
# adjacent notes in mining.conf.example for how to read one.
```

"RELEASING-adjacent notes" is meaningless — `RELEASING.md` says nothing about
TLS. It looks like an editing artefact; the pointer to `mining.conf.example` is
correct on its own. Flagged for `ci-reviewer`, who owns this file.

---

# What is correct — checked by reading and by running, not assumed

## Item 1 — the default really is WebPKI, and it really rejects a bad certificate

By reading:

- `webpki_verifier()` (lines 46-53) builds `RootCertStore::empty()`, extends it
  from `webpki_roots::TLS_SERVER_ROOTS`, and returns
  `WebPkiServerVerifier::builder(...).build()`.
- The `None` arm uses `ClientConfig::builder().with_webpki_verifier(verifier)` —
  the safe builder, **not** `.dangerous()`.
- `rustls-0.23.37/src/webpki/server_verifier.rs:232-277`: `verify_server_cert`
  calls `verify_server_cert_signed_by_trust_anchor_impl(&cert, &self.roots,
  intermediates, revocation, now, ...)` — chain against the roots, with `now`
  supplying expiry — and then `verify_server_name(&cert, server_name)`. Chain,
  expiry and hostname are all enforced. Affirmed from the implementation, not
  from the method name.
- `grep -rn "dangerous()\|assertion()\|NoVerifier" src/` returns exactly **one**
  live `.dangerous()` (line 252, inside the `Some(expected)` pinning arm) and
  exactly **one** live `ServerCertVerified::assertion()` (line 94, inside the
  fingerprint-match branch, where it is load-bearing). The remaining three hits
  are prose in doc comments. **No residual permissive path.**
- No plaintext downgrade on TLS failure: `connect()` picks `Plain` vs `Tls`
  purely from `is_tls_port()`, and a handshake error propagates out rather than
  retrying unencrypted. (The heuristic itself is M-2.)

**By running — this closes a gap the PR declares open.** The PR states the TLS
path has never been exercised, before or after. I exercised it, locally, against
`openssl s_server` holding a self-signed `C=IT/ST=Pool/L=Daemon/O=Mining Pool/CN=mining.pool`
certificate with an IP SAN — deliberately shaped like the pools in the survey:

| configuration | result |
|---|---|
| default, no pin | **rejected**: `invalid peer certificate: Other(OtherError(CaUsedAsEndEntity))` |
| `--tls-fingerprint <correct>` | **handshake completed** — reached JSON parsing of the login response |
| `--tls-fingerprint <wrong>` | **rejected** with the explanatory pin-mismatch error |

So both arms behave correctly on a real wire, not only against synthetic DER.
This is *not* a substitute for a real-pool test — no Stratum exchange, no
public-CA chain, no real pool certificate — but "the policy has never run inside a
handshake" is no longer true.

## Item 2 — the pinning verifier

- **Right bytes.** `ring::digest::digest(&SHA256, end_entity.as_ref())` hashes
  the end-entity DER. That is what `openssl x509 -fingerprint -sha256` prints and
  what XMRig's `X509_digest` computes, so the portability claim holds.
- **No partial or truncated match.** `actual.as_ref() == self.expected` compares a
  32-byte slice with `[u8; 32]`; slice `PartialEq` checks length before content.
  Not constant-time, which is irrelevant — a certificate fingerprint is public.
- **Delegation is genuine.** `verify_tls12_signature`, `verify_tls13_signature`
  and `supported_verify_schemes` all forward to `self.inner`, the real
  `WebPkiServerVerifier`. Nothing re-stubbed. I checked `origin/main`'s
  `NoVerifier` and confirm the PR's second-defect description is accurate: it did
  return `HandshakeSignatureValid::assertion()` from both, so the peer never had
  to prove key possession.
- **Trait defaults** (`rustls-0.23.37/src/verify.rs:141-155`). Exactly two methods
  have defaults, and both are correct to leave alone:
  - `requires_raw_public_keys() -> false` is the **safe** direction. With it
    false, RFC 7250 raw public keys are not negotiated, so `end_entity` is always
    a certificate and the pin always hashes a certificate. Overridden to `true`,
    the pin would silently be hashing a SubjectPublicKeyInfo while the operator
    believed it was a certificate.
  - `root_hint_subjects() -> None` only suppresses the `certificate_authorities`
    extension in ClientHello — a TLS 1.3 client-auth hint, and
    `with_no_client_auth()` is in use, so it is inert. Not a security property.

  **Nothing in the trait should have been overridden and was not.** Item 2 clean.

## Item 5 — the panic is defensible, and here is why

`webpki_verifier().unwrap_or_else(|e| panic!("{e}; refusing to fall back to unverified TLS"))`.

`WebPkiServerVerifier::builder(...).build()` returns
`Result<_, VerifierBuilderError>`, and that enum
(`rustls-0.23.37/src/webpki/mod.rs:34-39`) has exactly two variants:
`NoRootAnchors` and `InvalidCrl(_)`. Neither is reachable here — the root store is
populated unconditionally from the compiled-in `webpki_roots::TLS_SERVER_ROOTS`
(a `const` slice; no filesystem or network read), and no CRL is ever supplied.
**There is no realistic runtime input that reaches this panic**; it would take a
`webpki-roots` release shipping an empty slice, which would break the build's
intent anyway.

So the panic is unreachable in practice, and where it is reachable at all the
alternative — threading a `Result` through `Default`, `new` and
`with_tls_fingerprint` — buys nothing. The PR's reasoning that a silent downgrade
is the worse failure is sound, and the cost of being wrong is zero because the
condition cannot arise. `PoolConnection::default()` and `new()` both routing
through it is fine for the same reason. **Not a finding.**

## Item 7 — the operator documentation, judged

The README section is the strongest part of this PR. Plain language, no
salesmanship, no jargon beyond "fingerprint" which it then defines. Both caveats
are stated in terms an operator can act on:

- Trust-on-first-use: *"If you read it over a connection someone was already
  interfering with, you have just pinned their certificate."* Clear, and it gives
  an action — read it from a trusted network, cross-check from elsewhere.
- Renewal: *"Mining stops until you update the line, which is the system
  working — but it means you should expect it."* Clear, with the ~90-day figure
  and the self-signed contrast.
- *"If a pool has a proper certificate, do not pin it"* is the right closing
  instruction and is missing from most miner documentation.

Someone acting on this section would **not** be misled about what pinning does or
does not give them. They **would** be handed a wrong value (M-3) and stranded by
the unchanged quick-start (M-4). `mining.conf.example` is of the same prose
quality and carries M-2.

---

# Verification I ran myself

- `cargo test --release` — **138 passed, 0 failed, 2 ignored** (lib);
  **10 passed, 0 failed** (bin); 0 doc. Matches the PR's claim exactly.
- `cargo clippy --all-targets --release -- -D warnings` — **clean**, no output
  beyond `Finished`. Matches the PR's claim.
- `cargo build --release` — clean.
- Live TLS exercise against a local `openssl s_server` (three configurations,
  table under *Item 1*).
- **Live certificate reads** of `monerohash.com:9999` and
  `gulf.moneroocean.stream:20128` (2026-09-13) — both reproduce the PR's survey
  rows exactly, and the monerohash read is what proves M-3.
  `pool.supportxmr.com:443` was unreachable; see *Not verified*.
- Binary-level reproduction of M-1, M-2, m-2 and m-3.
- Three break-test mutations (table under M-5), each applied, observed and
  restored from a pre-taken copy. `git status --porcelain` and `git diff --stat`
  both empty afterwards; `git diff HEAD -- src/` empty. Nothing mutated was ever
  committed.
- JIT gate **not** run: no `src/randomx/` change in this diff, so `make verify-jit`
  is not implicated. CI's `jit-macos` / `jit-linux-arm` run it regardless.

Environment: macOS 25.6.0, aarch64. `openssl` = OpenSSL 3.6.1 (Homebrew);
`/usr/bin/openssl` = LibreSSL 3.3.6. Both checked for m-3.

# Not verified — say it rather than imply it

- **`pool.supportxmr.com:443` — could not reach it.** TCP connect times out from
  this host (`errno=60`) while three other hosts connected in the same minute. So
  the one survey row that matters most for M-4 is unverified, and M-4 is stated as
  a disjunction rather than as a fact. I did **not** simply take the PR's word
  and I did not pretend to have checked.
- **The two rows I could read reproduced the survey exactly** (monerohash:9999 and
  gulf.moneroocean.stream:20128 — subject, issuer, expiry and fingerprint all as
  described), which is why I did not reopen the survey as a whole. The
  `xmr.2miners.com:12222` and `pool.hashvault.pro:443` "no TLS on that port" rows
  I did not test.
- **No Stratum exchange with a real pool.** Nothing here logged in, mined or
  submitted a share over TLS. My local `openssl s_server` exercise proves the
  verifier's behaviour inside a real handshake; it does not prove the miner works
  end to end against a pool over TLS. That gap predates this PR and the PR says so.
- **XMRig's `Tls.cpp`.** I did not fetch upstream source. The quoted line is taken
  on the PR's word; it is at least internally consistent across all four
  documents.
- **The `Makefile` hunk's verdict** — read, one defect noted, but `ci-reviewer`
  owns it.
- **Whether a public-CA pool handshake succeeds.** My live test covers rejection
  and pinning, not a successful WebPKI chain build against a real CA.

---

# Round 2 — review of the fixes (`d33972d..44e2dab`)

Fresh reviewer, cold context. Scope: commit `44e2dab` only. `make verify-jit`
not implicated (no `src/randomx/` change). The `Makefile` hunk is **ci-reviewer's**
— read, no defect seen, not my verdict.

`cargo test --release` 150 passed / 2 ignored; `cargo clippy --all-targets
--release -- -D warnings` clean. Tree restored to `7a88045f…` (md5) after every
mutation; `git status --porcelain` empty.

## Coverage

| # | Item | Verdict |
|---|---|---|
| 1 | `connect` refusal (R1 major 1) | correct, but **ordered after the socket opens** — R2-M2 |
| 2 | `parse_tls_fingerprint_with` (R1 major 2) | behaviour correct (10/10 matrix), **untested** — R2-M3, m-1, m-2 |
| 3 | New tests (R1 major 5) | one break-test discriminates; **the wiring still does not** — R2-M1 |
| 4 | Docs (R1 majors 3, 4) | literal gone, warning well placed; m-3, n-1, n-2 |
| 5 | Claims across four documents | **PR body never updated** — R2-M4; m-4, m-5, m-6 |

## R2-M1 (major) — round 1's M-5 is half-closed; the default *connection* is still uncovered

Round 1 was precise: "the wiring that decides whether the *default connection*
gets `with_webpki_verifier(...)` or something permissive has no coverage at all."
The fix added `the_default_verifier_rejects_what_it_cannot_validate`, which holds
`server_verifier(None)` — a helper — not the `ClientConfig` the connection uses.

**Break-test.** Left `server_verifier` untouched; replaced only the `None =>` arm
of `with_tls_fingerprint` with an accept-anything verifier (all four trait methods
delegating except `verify_server_cert`):

```
test result: ok. 140 passed; 0 failed; 2 ignored   (lib)
```

The exact defect round 1 found is still shippable with a green suite. The
*second* break-test — accept-anything inside `server_verifier` — does fail the new
test (`panicked at pool_connection.rs:939`), so the new test is not vacuous; it
just guards one call short of the path that matters. `AUDIT.md` and the
`CLAUDE.md` row present this finding as closed.

*Fix:* assert on `PoolConnection::with_tls_fingerprint(level, None)`, or on
something reachable from it, rather than on the helper.

## R2-M2 (major) — the refusal test's verdict is decided by a third party's routing

The guard sits **after** `TcpStream::connect`, so it cannot be reached without a
live connection to `gulf.moneroocean.stream:20128`.

Measured here: the name resolves to `205.172.58.170` (**SYN timeout**) and
`2402:1f00:8001:86d::1` (connects in 0.18 s). `std` tries them in resolver order,
so the test takes **75.2 s** (3/3 runs, `real 75.75 / 75.29 / 75.29`) — and in an
earlier run in this same session it took 0.19 s. On a host with no working route
it does not skip, it **fails**: the error is `TCP connect failed: …`, and
`err.contains("pinned")` is false. Round 1 recorded exactly that condition for a
different pool from this very machine. It is the sole automated guard for round
1's most serious finding.

Confirmed against the built binary: with nothing listening the refusal never runs
(`TLS: certificate pinned to aaaa…` then `TCP connect failed`); with a local
listener on `127.0.0.1:1234` it fires and names port, `TLS_PORTS` and both
remedies. Break-tested (`if false && …`): the test fails, so it does discriminate.

*Fix:* move the guard above `TcpStream::connect` — it reads only `address`. The
test becomes hermetic and instant, and refusing before opening a socket is better
behaviour anyway.

## R2-M3 (major) — the rewritten parser has no tests at all

`parse_tls_fingerprint_with` was split out "so the environment variable can be
supplied directly" — the testability refactor — and **zero tests call it**
(`grep -n '#\[test\]' src/bin/minertim.rs`: 10 tests, none for the fingerprint).
`parse_switch_with`, which solved the identical R10-F2 problem, has ten, including
`an_empty_value_does_not_erase_an_explicit_setting`. This class of defect has now
recurred twice in this repo; the second correction ships unguarded.

I verified the behaviour myself against the binary (oracle: the `TLS: certificate
pinned to <hex>` line plus the new refusal). All ten cases correct:

| case | result |
|---|---|
| `--tls-fingerprint A` | A |
| `--tls-fingerprint A --tls-fingerprint B` | **B** (last wins) |
| `--tls-fingerprint=A --tls-fingerprint ""` | **A kept**, warns |
| env=E + `--tls-fingerprint ""` | **E kept**, warns |
| env=E + bare flag at end | **E kept**, warns |
| env=E + `--tls-fingerprint A` | A (flag beats env) |
| env=E + argv malformed | exit 2 |
| env malformed + argv A | exit 2 (see m-1) |
| bare flag mid-argv (`--tls-fingerprint --native-loop off`) | exit 2; the `i += 1` correctly skips the absorbed token |
| colon-separated | accepted |

So this is a coverage gap, not a live bug — but it is the gap that let the bug in.

## R2-M4 (major) — the PR body was never updated, and `AUDIT.md` says otherwise

`AUDIT.md` (SEC-02, new text): "this entry, **the PR body** and the code comment
all claimed the behaviour 'matches how the on/off switches handle
`--flag \"$UNSET_VAR\"`' … and the false claim is withdrawn."

`gh pr view 22` still carries it verbatim:

> **An empty value is treated as absent**, matching how the on/off switches handle `--flag "$UNSET_VAR"` …

The entry and the code comment were fixed; the PR body was not. So the
authoritative record asserts a correction that was not made, about the security
behaviour round 1 majored. The body also still says "138 lib + 10 bin tests",
"Seven new tests" (nine now) and does not mention the five majors at all. This is
the third round-2 in this repo to find an un-updated PR body (PROC-01, CI-03).

## Minors

- **m-1 — the parity claim is still not exact.** The doc comment says precedence
  and empty-value handling "deliberately mirror `parse_switch_with`" and that an
  "empty *or absent*" value declines to have an opinion. `parse_switch_with`'s
  absent case does the opposite: a bare flag at end of argv sets
  `value = Some(fail_safe)`, **overriding** the environment. And a malformed
  *environment* value here aborts with exit 2 before argv can override it, which
  contradicts "a later source wins". Both behaviours are defensible for a pin;
  the claim of mirroring is what is inaccurate — the same class round 1 majored.
- **m-2 — warning text names the wrong survivor.** Both empty-value warnings say
  "Any `MINERTIM_TLS_FINGERPRINT` setting still applies", but in
  `--tls-fingerprint=A --tls-fingerprint ""` what survives is the earlier *flag*,
  and with no env set nothing applies.
- **m-3 — README argues with itself.** Line 82 still reads "Read a pool's
  fingerprint with this, **and paste what it prints**", six lines above the new
  "Copy **only the hex after the `=`**". `AUDIT.md` lists that guidance as fixed.
- **m-4 — SEC-02's Verification paragraph is stale.** "138 lib + 10 bin" (now 140
  lib + 10 bin, 2 ignored) and "Seven new tests" (nine), directly above new text
  describing two more. Its files-changed list omits **`src/hex.rs`**, which this
  commit changed.
- **m-5 — no ledger sha recorded.** Neither `AUDIT.md` nor the `CLAUDE.md` row
  cites `REVIEW_PR22.md` or a retrieval sha. LEDGER-01 requires it; the branch is
  squash-merged, so both rounds become unretrievable once the branch ref goes.
- **m-6 — a disjunction promoted to a fact.** `AUDIT.md` now states "The first is
  true" for round 1's either/or about the quick-start pool, while conceding review
  could not re-reach the host. No new measurement backs it. From here TCP to
  `pool.supportxmr.com:443` succeeds on all three A records but the TLS handshake
  never completes (`openssl s_client` hangs; the miner hangs the same way) — which
  is consistent with the quick-start being broken, but *not* evidence for the
  stated reason, and equally consistent with the survey row being wrong.
- **m-7 — `hex_decode`'s rationale understates its own fix.** The comment
  attributes the panic to operator paste, but `parse_job` runs `hex_decode` on
  pool-supplied `blob`/`target`/`seed_hash` (`pool_connection.rs:819-821`), so a
  non-ASCII, even-byte-length field from the pool aborted the receiver thread.
  The guard is strictly tighter and valid hex is ASCII, so no regression; the
  write-up just sells it short.

## Nits

- **n-1** — the replacement example `SHA256 Fingerprint=3D:58:7C:…` is the first
  three bytes of the monerohash fingerprint removed as misattributed, still shown
  under a `pool.supportxmr.com` command. Unpasteable at 3 of 32 bytes.
- **n-2** — the README config block now uses inline `#` comments in a file the
  `Makefile` `-include`s, so `POOL` becomes `"pool.supportxmr.com:443   "`
  (verified with a stub makefile). Harmless only because `$(POOL)` is unquoted in
  the `run` recipe. `TLS_FINGERPRINT=  # …` is safe — `$(if …)` strips whitespace.

## Confirmed good

- **The refusal predicate is the exact complement of the plaintext arm**
  (`if pinned && !is_tls_port → Err`, then `if is_tls_port {tls} else {plain}`),
  so the plain branch is unreachable with a pin set. `is_tls_port`'s
  `unwrap_or(true)` routes a malformed address to TLS, where
  `ServerName::try_from` fails loudly — it cannot be used to reach plaintext.
- **The pin holds across reconnects**: `reconnect` (`:586`) and `:635` both go
  through `self.connect`, so the guard is not a startup-only check.
- **Refuse rather than warn is right.** The operator set a pin because they do not
  trust the path; a log line they may not be reading leaves them on plaintext
  under a guarantee that is not there — the worst outcome for this change. The
  cost is real and should be stated: a globally-set `TLS_FINGERPRINT` now blocks
  any plaintext-port pool. That is the right price.
- New tests break-tested twice; `the_default_verifier_rejects_what_it_cannot_validate`
  is not vacuous. Precedence matrix 10/10. Clippy clean, 150 pass.

## Not verified

- No handshake with `pool.supportxmr.com:443` completed from this host, so the
  survey row behind m-6 is still unconfirmed by me.
- No connection to a real pool with this build — unchanged, and SEC-02 says so.
- XMRig's `Tls.cpp` not fetched (same as round 1).

# Round 2 verdict

**NOT MERGEABLE. ACTIONABLE.** Four majors — but all four are small edits, not
rework: one test assertion moved up a level (R2-M1), one guard moved above
`TcpStream::connect` (R2-M2), tests written for a function that was split out to
be testable (R2-M3), and the PR body brought in line with the record it claims
(R2-M4). The security behaviour itself, exercised against the built binary, is
correct in every case I could construct.

**R2-M1 addendum.** The `AUDIT.md` sentence is narrower than it reads:
"Break-tested twice … A true accept-anything mutation fails it." The mutation
that *found* the bug — round 1's, on the `None` arm of `with_tls_fingerprint` —
still passes (mutation C above). Same defect class as R2-M4, in the authoritative
document rather than the PR body.

---

# Round 3

Fresh reviewer, cold. Scope: `8ff30e3..HEAD` — `b01ab21` (round-2 fixes),
`014fb63` (rustls bump), `c78fe80` (live-run write-up). Worktree
`.claude/worktrees/tls-verify`, head `c78fe80`.

**Handed off, nothing in scope:** the diff touches no `src/randomx/jit/`, no
emitter, no `vm.rs` native-loop path, no `benches/`, no `.github/workflows/`,
`Makefile`, `scripts/` or `.cargo/config.toml`. Nothing for `jit-reviewer` or
`ci-reviewer`.

## Coverage ledger

| # | Item | Result |
|---|---|---|
| 1 | In-memory handshake tests genuinely discriminate | **PASS** — break-tested at the wiring |
| 2 | Hoisted refusal guard | **PASS** — fires, runs first, 0.00 s |
| 3 | Nine parser tests | **PASS** — both named tests break-tested |
| 4 | Live-run figures re-derived from the log | **PASS** — every figure reproduces |
| 5 | rustls bump | **PASS** — 0.23.45, audit clean, nothing else moved |
| 6 | Documentation / audit accuracy | **ONE MAJOR** + minors |
| 7 | Concurrency / resource use | **PASS** — reconnect loop unaffected by the hoist |

Self-run: `cargo test --release` **161 passed, 2 ignored** (49 s);
`cargo clippy --all-targets --release -- -D warnings` **clean**;
`cargo audit` **exit 0**. All five CI checks SUCCESS on `c78fe80`; branch has
nothing behind `origin/main`.

## 1. The handshake tests — genuine (R3-V1)

Break-tested at the **production wiring**, not the helper. Replaced
`.with_custom_certificate_verifier(server_verifier(fingerprint))` in
`with_tls_fingerprint` with an `Arc::new(MutAcceptAnything)` that returns
`ServerCertVerified::assertion()` unconditionally (signature methods delegated so
the handshake still completes):

```
test ...a_wrong_pin_fails_a_real_handshake ... FAILED
test ...by_default_a_self_signed_pool_certificate_is_rejected_in_a_real_handshake ... FAILED
test result: FAILED. 141 passed; 2 failed
```

Round 1's and round 2's defect is closed. Confirmed that
`with_tls_fingerprint` is the **only** production `ClientConfig::builder()` site
(`pool_connection.rs:279`) and `PoolConnection::new` delegates to it
(`:248`), so there is no second wiring the mutation could have missed.

Second mutation, pinning disabled (`Some(_) => inner` in `server_verifier`):
`the_pinned_certificate_completes_a_real_handshake` **FAILED**. The pinned-accept
test is therefore not vacuous — it requires `Ok(())`, so it also guards the
byte-pump.

Fixture verified: `subject=C=IT, ST=Pool, L=Daemon, O=Mining Pool, CN=mining.pool`,
self-signed, `notAfter=Aug 22 07:51:30 2126`, SHA-256
`bd:d5:…:0c:7d` — exactly the value pinned in
`the_pinned_certificate_completes_a_real_handshake`. Tests need no network and
no ports.

## R3-MAJOR-1 — `AUDIT.md` SEC-02 contradicts itself; a stale paragraph was welded on, not rewritten

`c78fe80` deleted the heading **"Still not verified, and it matters. No
connection to a real pool was made with this build."** but left its **body**,
which is now spliced onto the end of the new hardware-correction paragraph
(`AUDIT.md:5560-5574`). Mid-sentence, the entry says:

> …which are append-only. The tests exercise the verifier's decision logic with
> synthetic DER, not a TLS handshake … the TLS path in this codebase has **never
> been exercised against a pool at all — before or after this change**. That gap
> predates this work and is not closed by it. A live check against a pinned
> self-signed pool and against a normally-verifying pool is the remaining
> verification.

All three clauses are false as of this same commit, and are contradicted ~20
lines above by **"Verified live, 2026-09-16 — this section previously said the
opposite"** and by the three in-memory handshake tests. `AUDIT.md` is the
project's authoritative record; a future session grepping "TLS path … never been
exercised" will trust it. This is CLAUDE.md item 6 verbatim — *when a premise
changes, the section is rewritten* — and the same accretion defect PROC-01 round 3
and CI-03 round 3 both found.

SEC-02 is on an unmerged branch, so this is an in-place edit, permitted. Swept
the whole entry for other survivors of the splice: **this is the only one**.

**ACTIONABLE.** Delete or rewrite the welded text.

## 2. The hoisted guard (R3-V2)

Now the first statement in `connect()` (`pool_connection.rs:315`), above
`TcpStream::connect`. Mutation `if false && …`:
`a_pin_and_a_plaintext_port_is_refused_rather_than_silently_ignored` **FAILED**.
On clean code the test runs in **0.00 s** and resolves no hostname — hermetic.

**R3-minor-1.** Only the *refusing* direction is covered. Nothing calls
`connect()` with a pin **and** a TLS port, so inverting the guard to always
refuse would ship green. It fails closed (no false security), and the 6-hour run
on `:9999` covers it empirically, so: minor.

**R3-nit-1.** Double blank line left at `pool_connection.rs:326-327` by the hoist.

## 3. The parser tests (R3-V3)

Both named tests break-tested:

- `resolved = parse(v)?.or(resolved)` → `resolved = parse(v)?` →
  **`an_empty_value_never_erases_a_pin` FAILED**.
- bare flag made to decline when the next argv item starts with `--` →
  **`a_bare_flag_followed_by_another_flag_is_an_error_not_a_silent_swallow` FAILED**.

Neither is vacuous. The eight (not nine — see R3-minor-10) cover env-only,
argv-over-env, last-flag-wins, all three empty forms, bare-flag-at-end,
malformed-with-recipe, and case/colon survival. The withdrawn `parse_switch_with`
parity claim is now stated precisely and matches the code.

## 4. The live run — every figure reproduces (R3-V4)

`LIVE6H_TLS_RUN.log` is **byte-identical** to the raw run log still in the
session scratchpad (`diff` empty), so it was not edited after the fact.

| Claim | Re-derived | Verdict |
|---|---|---|
| 354 accepted | `grep -c 'Share accepted by pool'` = **354** | ✓ |
| 0 rejected | final stats line `0 rejected`; no reject lines | ✓ |
| 355 found | `grep -c 'SHARE FOUND'` = **355** | ✓ |
| 0 withheld / 0 errors | `grep -c ERROR` = **0** | ✓ |
| 10 TLS connections | `Connected to pool (TLS)` = **10** | ✓ |
| 10 logins | `Login successful` = **10** | ✓ |
| 9 donation rotations | 9 rotation lines (+1 startup disclosure) | ✓ |
| 0 plain-TCP fallbacks | `plain TCP` = **0** | ✓ |
| 0 unplanned disconnects | 10 connects = 1 initial + 9 rotations; no reconnect/fail lines | ✓ |
| median 2267.4, range 2090.8-2333.1, n=2098 | dropping the first 59 `10m:` samples: **n=2098, median 2267.45, min 2090.8, max 2333.1** | ✓ |

Internal consistency of the negative half's quoted error: verification time
`1789516258` = 2026-09-15 23:50:58 UTC, 27 s before the positive run started;
`1786403646` = 2026-08-10 23:14:06 UTC, matching "expired 2026-08-10"; the
3112612 s delta checks out. Header `commit 014fb63` was authored 93 s before the
run started — the binary did include the round-2 fixes and the bump.

"0 withheld" is **not** vacuous: all four workers log
`share verification on` at 23:52:10, so the verifier was effective, and
withholds log at `error!` with `ERROR` count 0.

Hardware correction verified on this host: `hw.memsize` 34359738368 (**32 GiB**),
`hw.model` **Mac14,5**, 12 cores, Apple M2 Max. Correct.

**R3-minor-2 — the negative half has no committed artifact.** The entry says
"tested **both halves** … Full log committed as `LIVE6H_TLS_RUN.log`", but that
log is the *positive* half only (its own header says so). The negative half
exists solely as a quoted error string with no log, and none is in the
scratchpad. It is highly plausible — the timestamps decode correctly — but it is
the one headline claim resting on a transcription rather than an artifact, and
the sentence reads as if the log covers both.

**R3-minor-3 — the hashrate figure's filter is unstated.** The log has 2158
`10m:` samples; n=2098 only follows if the first 59 (the 10-minute window fill)
are dropped. The rule is nowhere in the entry, the PR body or the log. It does
reproduce once guessed — but "every number traces to a measurement" means the
derivation too. (Also 2267.45 is reported as 2267.4.)

**R3-minor-5 — one submission has no recorded response.** 355 found, 354
accepted, run stopped 9 s after the last find: one share was still outstanding at
SIGINT. Max concurrent outstanding reached **3** (02:42:53Z), so #17's
"responses carry no identifier" was live during this run. The arithmetic in the
table is honest, but "355 found / 354 accepted" is stated without noting the
unanswered one — and LIVE-01's round 2 was specifically about over-reading this.

## 5. The rustls bump (R3-V5)

`Cargo.lock`: rustls 0.23.37 → **0.23.45**, rustls-webpki 0.103.13 → **0.103.15**.
RUSTSEC-2026-0285 in the local advisory DB: dated 2026-09-14, `patched = [">= 0.23.45"]`,
CVSS string yields **5.3** — all three as the entry states. `cargo audit` exit 0.
`main` does pin 0.23.37, so "not caused by this change" holds. Diff vs `main` in
`Cargo.lock` is those four lines plus the `ring` dependency edge from the earlier
commit — **nothing else moved**.

## 6. Documentation accuracy — minors

**R3-minor-4 — orphaned doc comment, and its content is now false.**
`handshake_against_self_signed` was spliced directly beneath the doc comment
belonging to `the_default_verifier_rejects_what_it_cannot_validate`
(`pool_connection.rs:921-948`), so that test is now undocumented and two doc
blocks are concatenated. This is the named repo failure mode, and it has now
orphaned three. Worse, the stranded text is *stale*: it says the test "uses input
WebPKI cannot accept rather than a well-formed self-signed certificate, because
generating one would mean vendoring a certificate builder" — a well-formed
self-signed fixture sits twelve lines below it.

**R3-minor-6 — "the exact shape of the real article" is unsupported, and the
rejection is not the one the write-up attributes.** Printing the error the
default test actually observes:

```
InvalidCertificate(Other(OtherError(CaUsedAsEndEntity)))
```

webpki rejects the fixture for `CA:TRUE` used as an end-entity, **before** it
reaches trust chain or hostname — the two grounds the PR body's survey table
names. Both scratchpad certs (`c.pem`, `c2.pem`) are locally generated with
`openssl req -x509` defaults (hence `CA:TRUE`, no SAN); neither is a captured
pool certificate, and the real endpoint is unreachable from this host, so
"the exact shape of the real article" is not established. The test still
discriminates (mutation A kills it), so this is precision, not coverage.

**R3-minor-10 — `AUDIT.md` says "nine tests now pin them"; there are eight.**
`tls_fingerprint_absent_means_normal_verification`, `…env_is_used…`,
`…flag_overrides_env_and_the_last_flag_wins`, `an_empty_value_never_erases_a_pin`,
`a_bare_flag_at_the_end_declines_rather_than_erasing`,
`a_bare_flag_followed_by_another_flag_is_an_error_not_a_silent_swallow`,
`a_malformed_value_is_an_error_with_the_openssl_recipe`,
`colons_and_case_survive_the_cli_path`. Same class as R3-minor-7 and -8. Batch
this into R3-MAJOR-1's edit so the lead makes one pass over `AUDIT.md`, not two —
and while there, note that the Verification paragraph says the test counts are
"left out rather than restated, because they went stale inside this entry twice"
and the rustls paragraph then restates them as "143 lib + 18 bin" (the figures
are correct; the entry is arguing with itself).

**R3-minor-7 — `CLAUDE.md`'s SEC-02 row still says "138+10 tests".** The actual
count is 143 lib + 18 bin. `AUDIT.md` deliberately dropped these figures because
they went stale twice inside the entry; the task-board row kept the stale pair.

**R3-minor-8 — the PR body says "Twenty-one new tests"; there are 20.**
`main` had 10 test fns across `pool_connection.rs` (0) and `bin/minertim.rs` (10);
head has 12 + 18 = 30.

**R3-minor-9 — `LIVE6H_TLS_RUN.log` is not in SEC-02's files-changed list**, and
neither `AUDIT.md` nor the `CLAUDE.md` row names `REVIEW_PR22.md` or its
retrieval sha, which LEDGER-01 requires before the ledger is removed at merge.

**Honest about limits — yes, with one gap.** The "What the run still does not
establish" paragraph names the renewal failure mode, the single pool/certificate,
the self-signed case being test-only, and 4 threads not 12. All three items the
brief asked about are stated clearly. The gap is R3-MAJOR-1: that honest
paragraph is immediately followed by a stale one claiming far more is unverified
than is true.

## 7. Concurrency (R3-V6)

The hoist changes `connect()`'s timing profile — a pinned/non-TLS-port failure
now returns before any syscall rather than after one. Checked the only retry
path: `reconnect()` (`pool_connection.rs:591`) sleeps `RECONNECT_DELAY` at the
**top** of each iteration, before `connect`, so it paces itself independently of
socket latency and the hoist cannot turn it into a hot loop. The case is also
unreachable there: the address is fixed for the process, so a pin/port mismatch
fails at startup and `main` exits — the ten connections in the live log are all
to the same `monerohash.com:9999`. No worker, receiver or nonce behaviour is
touched by this diff.

## Round 3 verdict

**NOT MERGEABLE. ACTIONABLE — one major.**

The engineering is sound and, for the first time in this PR, the coverage is
genuine: the accept-anything mutation at the production wiring now kills two
tests, the guard is hermetic and fires, both named parser tests break-test
cleanly, and **every one of the eleven live-run figures re-derives exactly from a
log that is byte-identical to the raw one**. The rustls bump is minimal and
correct. Said plainly, because it is a legitimate outcome: the record's numbers
are accurate and the coverage is real.

The single blocker to merge is R3-MAJOR-1 — a deleted heading whose body survived,
leaving `AUDIT.md` asserting the TLS path "has never been exercised against a pool
at all" twenty lines after recording six hours of doing exactly that. One edit.

**Could not verify:** the real `pool.supportxmr.com` / `gulf.moneroocean.stream`
certificates (both unreachable from this host), so the survey table and the
fixture's fidelity to it rest on the 2026-09-13 survey; and the negative half of
the live run, which has no committed artifact.

---

# Round 4 — scope: commit `83a8d7e` only

Fresh reviewer, cold. Reviewed `git diff 291a739..HEAD` and nothing else; the TLS
work of rounds 1–3 was not re-reviewed. Nothing in the diff touches
`src/randomx/jit/`, the emitter, `vm.rs`'s native-loop path, `benches/`,
`.github/workflows/`, the `Makefile`, `scripts/` or `.cargo/config.toml`, so
nothing is handed to `jit-reviewer` or `ci-reviewer`.

## Coverage ledger

| # | Item | Done | Outcome |
|---|---|---|---|
| 1 | Correctness of the new `hex_decode` | yes | sound — see below |
| 2 | Do the tests discriminate (mutation) | yes | 7 mutations; `hex.rs`'s five all discriminate, the `parse_job` one does not — **R4-MAJOR-1** |
| 3 | `parse_job` / `None` handling traced | yes | claim true but silent — R4-minor-4 |
| 4 | `AUDIT.md` / `CLAUDE.md` / PR-body claims | yes | test counts right, three claims wrong — R4-minor-1/2/3 |
| 5 | Call sites / behaviour change | yes | no legitimate caller affected |
| 6 | Silent failure, fail-safe direction | yes | no new fallback; direction of both switches untouched by this commit |
| 7 | Concurrency, resource use | yes | n/a — pure function, no allocation change of note |

## The implementation is correct, and that was checked exhaustively

Not by reading. A standalone harness built the pre-PR decoder, the `291a739`
decoder and the new one side by side:

- **All 16 384 two-character ASCII inputs**: exactly **22** differ, and every one
  is of the form `+<hexdigit>` — old `Some(n)`, new `None`. Nothing else moved.
- **20 971 520 four-character inputs** (`128³ × 10` over a boundary-focused last
  character): **0** cases where the new function accepts and the old one either
  rejected or produced different bytes. New ⊆ old, byte-identical on the
  intersection. So no input that should decode is now rejected.
- 300 000 random mixed ASCII/Unicode strings: no panic; on ASCII the only
  mismatches are `+`-forms (152 of 179 450).
- All 256 byte values round-trip through `hex_encode`, in lower, upper and mixed
  case. Boundary characters `/ : \` g @ G` and `\0 \x7f + -` and space all reject
  in both implementations.
- `chunks_exact(2)` cannot drop a trailing byte: the `is_multiple_of(2)` guard
  above it returns `None` first, so the remainder is always empty.
- `<< 4` cannot overflow: `nibble` returns `0..=15`, so the shift is at most
  `15u8 << 4 == 240`, and a `u8 << 4` is not a shift-overflow even in debug.
  Precedence is right — `?` binds tighter than `<<`, `<<` tighter than `|`.

`cargo test --release`: **149 lib + 18 bin, 0 failed, 2 ignored**. That matches
`CLAUDE.md`'s "167 tests (149 lib + 18 bin)" exactly, and the +6 lib delta from
the previous 143 is the five new `hex` tests plus the one `parse_job` test.
`cargo clippy --all-targets --release -- -D warnings` clean.

`scripts/verify-jit.sh` is unaffected: all six `JIT_FILTERS` are `randomx::`
prefixes, so neither `hex::tests` nor `pool_connection::tls_tests` can be swept
into the `EXPECTED_PASSES=92` assertion. The gate was not run (nothing in the
diff is in its scope).

## Mutation results

Each mutation applied to a pristine copy of `src/hex.rs`, run, then restored
from a backup taken before any edit. `git status` clean at the end.

| Mutation | Result |
|---|---|
| A: `<< 4` → `<< 3` | **2 fail** (`round_trips`, `every_byte_round_trips`) |
| B: nibble accepts `b'g'` | **1 fail** (`rejects_odd_length_and_non_hex`, `"0g"` → `Some([16])`) |
| C: drop the odd-length check | **2 fail** — `rejects_odd_length_and_non_hex` *and* the `parse_job` test |
| D: reintroduce `from_str_radix` (the `291a739` body) | **1 fail** (`"+1"` → `Some([1])`); `parse_job` test **green** |
| E: revert `hex.rs` to `origin/main` (pre-PR, panicking) | **2 fail** — sign case, and `non_ascii_…` **panics at `src/hex.rs:20`**, the slicing line; `parse_job` test **green** |
| F: uppercase arm off-by-one (`+ 11`) | **1 fail** (`decodes_either_case_and_encodes_lower`) |
| G: uppercase arm removed | **1 fail** (same test) |

No test passes vacuously. Worth recording for a future editor:
`decodes_either_case_and_encodes_lower` is the **only** guard on uppercase
decoding — `hex_encode` emits lowercase, so neither round-trip test touches that
match arm — and it is the test that fires against both F and G. Do not delete it
as redundant.

## R4-MAJOR-1 — the `parse_job` test covers neither defect it is said to cover

`a_malformed_job_from_the_pool_is_declined_not_fatal` is green against **both**
unfixed implementations (mutations D and E above). Its two fixtures only ever
exercise the odd-length branch, which every version of `hex_decode` ever shipped
has had:

```
"ff€ff"  = 7 bytes  → ODD → rejected by the length check, in every version
"abc"    = 3 bytes  → ODD → same
```

Measured against the pre-PR decoder: `orig("ff€ff")` returns `None`;
`orig("ff€f")` (6 bytes, even) **panics**. The fixture is one character away from
being a real regression test and is not one.

What makes this a major rather than a nit is the claim attached to it. The test's
own doc comment says *"before the fix a pool could abort the miner by putting one
non-ASCII byte in a job"* and its assertion message reads *"must be declined, not
panic"* — of an input that never panicked. `AUDIT.md` goes further: *"`parse_job`
gained the test that matters more than any of those."* `AUDIT.md` is this
project's authoritative record, and a wrong claim in it is trusted later rather
than re-derived.

**Stated fairly, and this differs from the round-2 precedent:** the defect is
*not* shippable green here. `hex.rs`'s `non_ascii_returns_none_rather_than_panicking`
genuinely reproduces the panic (mutation E panics inside it), so suite-level
coverage of the regression exists. The severity rests on the false coverage claim
plus a non-discriminating fixture, not on absent coverage.

**Closes with two edits:** `"ff€ff"` → `"ff€f"` (verified to panic pre-fix), and
the `AUDIT.md`/doc-comment sentences adjusted to what the fixtures actually
demonstrate. Adding a `"+f"`-style fixture would additionally give the *sign*
defect caller-level coverage, which nothing currently has.

## Minors

**R4-minor-1 — two errors in one `AUDIT.md` sentence.** *"Seven tests cover it —
… and six non-ASCII inputs including one whose length passes the even check and
only then straddles a character boundary."* There are **five** `#[test]` fns in
`hex.rs` (six counting the `parse_job` one), and **three** of the six non-ASCII
inputs are even-length and reach the slicing: `"a€"` (4), `"€€"` (6), `"😀"` (4) —
the claim understates its own coverage. Fix both halves in one edit; this repo's
documented pattern is a correction that introduces a new error into the sentence
being corrected.

**R4-minor-2 — `hex_decode` has four call sites in `pool_connection.rs`, not
six.** `AUDIT.md` says six. Actual: `:38` (`parse_cert_fingerprint`) and
`:827/:828/:829` (`parse_job`). "Three of them pool-supplied" is correct. Four is
also the whole crate — `src/randomx/tests.rs` has its own private `hex_decode`
helper, untouched and unrelated.

**R4-minor-3 — the PR body was not updated for this commit.** It still says
*"143 lib + 18 bin tests pass"* (now 149 + 18) and does not mention the sign
defect, the rewrite or `hex.rs`'s first tests at all; its "A second defect"
heading now competes with two other second defects. This is the same finding
round 2 made on this PR, recurring.

**R4-minor-4 — a declined job is declined silently.** Traced as claimed: in
`handle_pool_message` the `if let Some(job_data) = … && let Some(job) =
parse_job(…)` chain simply fails, `*current` is never written, and the previous
job stays in force. So the claim is literally true and the failure direction is
the safe one. But **nothing is logged** — a pool sending only malformed jobs
pins the miner to a stale job with no diagnostic until stale rejects appear.
Pre-existing and unchanged by this commit; in scope only because the new test and
the `AUDIT.md` paragraph put this behaviour on the record as correct without
noting that it is silent. A `log::warn!` on the `None` arm would close it.

## Claims checked and confirmed true

- *"Reachable from pool-supplied `blob`/`target`/`seed_hash` for the project's
  life."* Verified to the CLI pivot itself (`e78376d`, = `791815f^`): the
  identical `from_str_radix` decoder sat in `pool_connection.rs:529` and
  `parse_job` at `:504` ran it on all three fields. `791815f` moved it to
  `hex.rs` unchanged. Confirmed by execution, not reading: the pre-PR function
  returns `Some([1])` for `"+1"`.
- *"Fixes both defects at once."* True. Sign: `new("+1") == None`. Panic: no
  byte-index slicing remains; every byte ≥ 0x80 fails the nibble match, so the
  dropped `is_ascii()` guard is fully subsumed.
- Test counts `149 lib + 18 bin` — reproduced exactly.
- No legitimate caller relied on the old laxity. The only non-`parse_job` caller
  is `parse_cert_fingerprint`, which strips colons and requires exactly 64
  characters; a `+`-bearing fingerprint was never valid input. Rejecting signs is
  strictly a narrowing, and the 21M-input scan shows the accepted set shrank by
  exactly the `+` forms.

## Round 4 verdict

**NOT MERGEABLE. ACTIONABLE — one major (R4-MAJOR-1) and four minors.**

Said plainly, because it is the expected outcome and it holds: **the rewritten
`hex_decode` is correct.** It was verified exhaustively rather than by reading —
over 21 million inputs, new ⊆ old with byte-identical results on the
intersection — and its five tests discriminate every mutation put to them,
including the two this commit fixes. That half of the commit needs nothing.

What blocks it is the one test outside `hex.rs` and the sentences written about
it: a caller-level regression test that passes against both unfixed
implementations, described in the authoritative record as the test that matters
most. Two fixture characters and three sentences.

**Could not verify:** nothing material was left unchecked in this commit's scope.
Not attempted, and out of scope by instruction: the TLS work of rounds 1–3, the
live-run logs, and `scripts/verify-jit.sh` (which this diff cannot reach).
