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
