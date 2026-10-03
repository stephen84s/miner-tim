use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::donate::{Beneficiary, DonationSchedule};
use crate::hex::{hex_decode, hex_encode};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::{ClientConfig, RootCertStore};
use serde::Serialize;
use serde_json::Value;

use parking_lot::FairMutex;

/// The mutex type guarding `PoolConnection::stream`, and only that field —
/// see the field's doc comment for why it must be a *fair* lock. Every call
/// site goes through this alias rather than naming `FairMutex`, so the choice
/// is made in exactly one place.
type StreamLock<T> = FairMutex<T>;

/// How long the receiver blocks on a socket read (also the max time it holds the
/// stream lock per read). Because the stream lock is fair (see
/// `PoolConnection::stream`), a share submit waiting on it gets the lock after
/// the read in progress, or the one after that if it had not yet parked when
/// the first ended. So against the receiver's polling, this bounds
/// share-submit lock latency to about two intervals. That bound does not
/// cover `login()`'s synchronous request/response (`send_request`), which
/// holds the lock until the reply arrives, for up to its 30s read timeout, on
/// connect, reconnect and donation relogin. The receiver's polling did not
/// have this bound before #44
/// either: with an unfair lock the receiver could re-take the lock ahead of
/// a parked submit read after read, and a live run measured waits of up to
/// 81s. Kept short so that
/// under full-core mining, new jobs are picked up and shares submitted promptly —
/// large values here cause stale "Invalid job id" rejects.
const RECV_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Stratum keepalive interval.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(60);

/// How long the pool may say **nothing** before we treat the connection as
/// dead and reconnect.
///
/// Keepalives are *writes*: they prove the socket is still writable, which a
/// half-dead connection remains. Nothing watched the **receive** side, so a
/// pool that simply stopped talking left the miner hashing a job the pool had
/// long since replaced — every share found in that window submitted into a
/// connection that never answered, and discarded with no error, no warning and
/// no counter. Measured on a live run (GitHub #34): two windows of **121 and
/// 88 minutes**, about 60% of the session, ending only when the pool itself
/// closed the socket.
///
/// Three keepalive intervals. Real jobs arrive roughly every 15 s, so three
/// minutes of total silence is two orders of magnitude past normal and cannot
/// be mistaken for a quiet pool; anything much tighter would reconnect on an
/// ordinary lull.
const POOL_SILENCE_TIMEOUT: Duration = Duration::from_secs(3 * KEEPALIVE_INTERVAL.as_secs());
/// Delay between reconnection attempts.
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

/// Longest a donation rotation waits for outstanding submissions to be answered (#32).
/// 8x the largest reply latency observed live (641ms, LIVE7H run, median 306ms).
const ROTATION_SETTLE_LIMIT: Duration = Duration::from_secs(5);
/// Lock-free pause per deferred iteration, so a submitter parked on the stream
/// mutex can take it; the receiver otherwise re-locks within microseconds.
const ROTATION_SETTLE_YIELD: Duration = Duration::from_millis(10);

/// SHA-256 of a server certificate, as `tls-fingerprint` pins it.
pub type CertFingerprint = [u8; 32];

/// Parse a 64-character hex SHA-256 fingerprint, as printed by
/// `openssl x509 -noout -fingerprint -sha256`. Case-insensitive, and colons are
/// accepted because that is how openssl prints it.
pub fn parse_cert_fingerprint(text: &str) -> Option<CertFingerprint> {
    let cleaned: String = text.chars().filter(|c| *c != ':').collect();
    if cleaned.len() != 64 {
        return None;
    }
    let bytes = hex_decode(&cleaned)?;
    bytes.try_into().ok()
}

/// The Mozilla root store, from the `webpki-roots` crate already in `Cargo.toml`.
///
/// This was a dependency for the project's whole life and was never referenced:
/// `NoVerifier` bypassed it. Nothing new is vendored to verify properly.
fn webpki_verifier() -> Result<Arc<WebPkiServerVerifier>, String> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    WebPkiServerVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|e| format!("could not build the TLS certificate verifier: {e}"))
}

/// Choose the certificate verifier: pinned if the operator configured one,
/// otherwise standard WebPKI validation.
///
/// Factored out so a test can hold the *default* verifier and assert it actually
/// rejects something. Review found that swapping the default arm for an
/// accept-anything verifier left the whole suite green — the seven tests written
/// for this change all exercised the pinning path, so the security property that
/// matters most had no coverage at all.
fn server_verifier(fingerprint: Option<CertFingerprint>) -> Arc<dyn ServerCertVerifier> {
    let inner = webpki_verifier()
        .unwrap_or_else(|e| panic!("{e}; refusing to fall back to unverified TLS"));
    match fingerprint {
        Some(expected) => Arc::new(PinnedCertVerifier { inner, expected }),
        None => inner,
    }
}

/// Accepts exactly one certificate, identified by its SHA-256 fingerprint.
///
/// This exists because the Monero pool landscape mostly cannot satisfy WebPKI.
/// Surveyed 2026-09-13: `pool.supportxmr.com:443` and
/// `gulf.moneroocean.stream:20128` both present *self-signed* certificates whose
/// subject is the stock `C=IT, ST=Pool, L=Daemon, O=Mining Pool` shipped with pool
/// daemon software, named `CN=mining.pool` / `CN=mining.proxy` — so they fail on both
/// trust chain and hostname, and are valid for a century so they never rotate.
/// `monerohash.com:9999` is the opposite case: a genuine Let's Encrypt
/// certificate for the right host that had simply expired.
///
/// Pinning is the only mechanism that *authenticates* the first group rather
/// than waving it through. The operator records the certificate they expect; a
/// substituted one then fails, which is precisely what a blanket bypass cannot
/// detect.
///
/// **Signature verification is delegated, not skipped.** The verifier this
/// replaced also stubbed `verify_tls12_signature` and `verify_tls13_signature`
/// to `assertion()`, which is worse than accepting the certificate: it means the
/// handshake signature went unchecked, so there was no proof the peer even held
/// the private key. Those two methods defer to the real WebPKI verifier here, so
/// pinning narrows *which* certificate is acceptable without weakening the
/// handshake itself.
#[derive(Debug)]
struct PinnedCertVerifier {
    inner: Arc<WebPkiServerVerifier>,
    expected: CertFingerprint,
}

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let actual = ring::digest::digest(&ring::digest::SHA256, end_entity.as_ref());
        if actual.as_ref() == self.expected {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(format!(
                "pool certificate does not match the pinned fingerprint.\n                   expected: {}\n                   presented: {}\n                   The pool may have renewed its certificate — re-read it with \
                 `openssl s_client -connect <host>:<port> -servername <host> </dev/null \
                 | openssl x509 -noout -fingerprint -sha256` and update \
                 --tls-fingerprint. If you did not expect a change, treat this as a \
                 possible interception and do not simply overwrite the pin.",
                hex_encode(&self.expected),
                hex_encode(actual.as_ref()),
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

#[derive(Clone, Debug)]
pub struct Job {
    pub blob: Vec<u8>,
    pub target: Vec<u8>,
    pub job_id: String,
    pub seed_hash: Vec<u8>,
}

#[derive(Serialize)]
struct JsonRpcRequest {
    id: u64,
    jsonrpc: &'static str,
    method: String,
    params: Value,
}

/// A `submit` request written to the pool but not yet answered, keyed by its
/// JSON-RPC id in `PoolConnection::pending_shares`. Lets a response be paired
/// with the submission it belongs to instead of the previous "any id-bearing,
/// method-less message is a share response", which miscounted a keepalive
/// error as a rejected share (GitHub #17).
#[derive(Clone, Debug)]
struct PendingShare {
    job_id: String,
    nonce: String,
    sent_at: Instant,
}

/// Decrements `submits_in_flight` on drop, on every return path out of
/// `submit_share` — including the early `?`s — because it is dropped at
/// function exit regardless of how the function returns (#32).
struct InFlight<'a>(&'a AtomicU32);
impl Drop for InFlight<'_> {
    fn drop(&mut self) { self.0.fetch_sub(1, Ordering::SeqCst); }
}

/// Wraps either a plain TCP or TLS stream behind Read + Write.
/// A single long-lived value, so the variant size difference is irrelevant.
#[allow(clippy::large_enum_variant)]
enum PoolStream {
    Plain(TcpStream),
    Tls(rustls::StreamOwned<rustls::ClientConnection, TcpStream>),
}

impl PoolStream {
    fn tcp(&self) -> &TcpStream {
        match self {
            PoolStream::Plain(s) => s,
            PoolStream::Tls(s) => &s.sock,
        }
    }
}

impl Read for PoolStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            PoolStream::Plain(s) => s.read(buf),
            PoolStream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for PoolStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            PoolStream::Plain(s) => s.write(buf),
            PoolStream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            PoolStream::Plain(s) => s.flush(),
            PoolStream::Tls(s) => s.flush(),
        }
    }
}

/// Largest JSON-RPC line this miner will accept from a pool, in bytes.
///
/// Stratum messages are small — a job is a few hundred bytes — so a megabyte is
/// generous by three orders of magnitude. The point is that it is *bounded*:
/// without a limit, a peer that never sends a newline makes the receive buffer
/// grow until the process is killed by memory pressure. No wrong hashes, no
/// error, the miner simply dies.
///
/// `read_line` has always had this limit; the long-lived `receiver_loop` did
/// not, which is the asymmetry GitHub #21 records. Sharing the constant means
/// the two cannot disagree about *the number* — it says nothing about their
/// behaviour, which still differs: `read_line` refuses a single over-long line,
/// while `receiver_loop` checks what remains after draining complete ones, so
/// it tolerates a buffer up to `MAX_LINE_BYTES + 4096` mid-read. An earlier
/// version of this comment claimed the two "cannot drift apart" outright, which
/// overstated what one shared constant buys (PR #23 round 3, R3-F6).
const MAX_LINE_BYTES: usize = 1 << 20;

/// Well-known TLS ports for mining pools
const TLS_PORTS: &[u16] = &[443, 993, 995, 3333, 9999, 14433];

fn is_tls_port(address: &str) -> bool {
    address
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse::<u16>().ok())
        .map(|port| TLS_PORTS.contains(&port))
        .unwrap_or(true) // default to TLS if unsure
}

pub struct PoolConnection {
    /// Single shared session: the receiver thread, share submits, and
    /// keepalives all go through this one stream, guarded by the mutex.
    ///
    /// The mutex must be **fair** (`parking_lot::FairMutex`, via `StreamLock`)
    /// (#44). `receiver_loop` holds this lock for each read (up to
    /// `RECV_POLL_INTERVAL`, 50ms), releases it, and takes it again almost
    /// at once. `std::sync::Mutex` makes no fairness promise, and on both
    /// macOS and Linux the receiver re-took it ahead of a parked
    /// `submit_share` read after read. A live run measured the submit's wait
    /// at mean 11.6s and max 81.4s, and shares were rejected as stale.
    /// `FairMutex` hands the lock straight to a parked waiter on every unlock.
    /// So a submit waits for the read in progress, or for one more read if it
    /// was still in the lock's brief spin phase, not yet parked, when that
    /// read ended. Its wait behind the receiver's polling is bounded at about
    /// two reads. Other holders are not covered by that bound. In particular,
    /// `send_request` (login) holds the lock until the reply arrives, for up
    /// to its 30s read timeout.
    ///
    /// Do not "simplify" this to `parking_lot::Mutex`. That lock is only
    /// *eventually* fair. It hands off to a parked waiter only once a timer
    /// has run out: `parking_lot_core`'s `FairTimeout`, reset to a random
    /// 0-1ms after each fair handoff. Otherwise it barges. With 50ms hold
    /// times that timer has nearly always run out. During #44 it was
    /// swapped in, and it passed every test in this file, so nothing here
    /// can tell it apart from a real fix. Keep fairness explicit.
    stream: StreamLock<Option<PoolStream>>,
    current_job: Mutex<Option<Arc<Job>>>,
    connected: AtomicBool,
    request_id: AtomicU64,
    address: Mutex<String>,
    /// The wallet currently logged in with — may be the user's or, during a
    /// donation slice, the author's or XMRig's address.
    wallet: Mutex<String>,
    /// The user's own wallet, captured on the first login. Donation slices
    /// rotate away from and back to this.
    user_wallet: Mutex<String>,
    donation: DonationSchedule,
    tls_config: Arc<ClientConfig>,
    /// Set when the operator pinned a certificate. Kept so `connect` can detect
    /// the case where a pin was configured but the port is not treated as TLS —
    /// otherwise the pin is silently inert while the startup log says otherwise.
    pinned_fingerprint: Option<CertFingerprint>,
    session_id: Mutex<String>,
    accepted_shares: AtomicU32,
    rejected_shares: AtomicU32,
    /// Submissions written to the pool but not yet answered, keyed by the
    /// JSON-RPC id they were written with. A
    /// response is only ever counted against `accepted_shares` /
    /// `rejected_shares` if its id is found — and removed — here, which is
    /// what stops a keepalive response (or a late reply from a connection
    /// already replaced) from being miscounted as a share result (#17).
    pending_shares: Mutex<HashMap<u64, PendingShare>>,
    /// Submissions whose connection was torn down and replaced before the
    /// pool ever answered. Distinct from `rejected_shares`: the pool never
    /// said no, so counting it as a rejection would blame the pool for a
    /// local reconnect.
    lost_shares: AtomicU32,
    /// Shares `submit_share` was asked to send but never wrote — no stream
    /// or a write error. Distinct from `lost_shares` because
    /// the pool never saw these at all: issue #17 established that a
    /// never-written share must not be counted "lost" — its test
    /// `a_submission_blocked_on_the_stream_registers_nothing_until_it_writes`
    /// asserts exactly this (#32).
    unsent_shares: AtomicU32,
    /// Number of `submit_share` calls currently executing, including ones
    /// blocked waiting on the stream lock. Read by the rotation gate so a
    /// rotation can't proceed while a submit is parked on the lock but not
    /// yet registered (#32).
    submits_in_flight: AtomicU32,
    /// How long the pool may be silent before the connection is treated as
    /// dead, in **milliseconds**. Defaults to `POOL_SILENCE_TIMEOUT`; tests
    /// shorten it so the real `receiver_loop` can be driven to the timeout in
    /// a second rather than three minutes. Exercising the loop itself is the
    /// point — a test that called a helper would pass while the wiring was
    /// broken, which is how PR #22 shipped green three times.
    silence_timeout_ms: AtomicU64,
    /// Mirrors `silence_timeout_ms`: tests shorten this field directly;
    /// production uses `ROTATION_SETTLE_LIMIT`'s default (#32).
    rotation_settle_ms: AtomicU64,
    /// Set while a donation rotation is deferred waiting for outstanding
    /// submissions to settle; `None` otherwise. Tied to the beneficiary
    /// being deferred (checked in `rotation_settled`) AND cleared by
    /// `reconnect()` on every call (#32 round 2, R2-F2).
    ///
    /// A field, not a `receiver_loop` local: `reconnect()`'s retry loop is
    /// unbounded, so a real outage can span a FULL donation cycle or more
    /// (#34 recorded 121 minutes; the default cycle is 100). The schedule
    /// can then return to the *same* beneficiary that was originally being
    /// deferred, with no intervening beneficiary ever observed by
    /// `receiver_loop` to trigger the per-beneficiary mismatch check — the
    /// first fix for this (checking only whether the stored beneficiary
    /// differs from `want`) missed exactly this case, since nothing differs.
    /// Clearing on every `reconnect()` ties invalidation to an actual
    /// connection-discontinuity event instead of a beneficiary/time
    /// heuristic — the same event that already clears `current_job`,
    /// `stream` and `pending_shares`.
    rotation_wait_since: Mutex<Option<(Beneficiary, Instant)>>,
}

impl Default for PoolConnection {
    fn default() -> Self {
        Self::new(crate::donate::DEFAULT_DONATE_LEVEL)
    }
}

impl PoolConnection {
    pub fn new(donate_level: u8) -> Self {
        Self::with_tls_fingerprint(donate_level, None)
    }

    /// `fingerprint` pins the pool's certificate by SHA-256. `None` — the
    /// default — uses standard WebPKI validation: trust chain, hostname and
    /// expiry, all checked.
    ///
    /// A verifier that cannot be built is a hard failure rather than a silent
    /// downgrade. Falling back to "accept anything" on an error is how a
    /// security control quietly stops existing, and this one was absent for the
    /// project's whole life without anything reporting it.
    pub fn with_tls_fingerprint(donate_level: u8, fingerprint: Option<CertFingerprint>) -> Self {
        if let Some(expected) = fingerprint {
            log::warn!(
                "TLS: certificate pinned to {}. For a TLS connection this replaces the \
                 usual checks — trust chain, hostname and expiry are NOT verified; only \
                 that the certificate is exactly the pinned one. Re-pin if the pool \
                 renews. Whether TLS is used at all depends on the port, and is \
                 reported when the connection is made.",
                hex_encode(&expected)
            );
        }

        // One call site, deliberately. This was a `match` whose two arms differed
        // only in the argument to `server_verifier` — and that duplication was the
        // defect: review mutated the `None` arm to accept anything, left
        // `server_verifier` intact, and the suite stayed green, because the test
        // written for round 1's finding held the *helper* rather than the wiring.
        // With a single call there is no second place for the choice to be made,
        // so a test of `server_verifier` is a test of what the connection uses.
        let tls_config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(server_verifier(fingerprint))
            .with_no_client_auth();

        Self {
            stream: StreamLock::new(None),
            current_job: Mutex::new(None),
            connected: AtomicBool::new(false),
            request_id: AtomicU64::new(1),
            address: Mutex::new(String::new()),
            wallet: Mutex::new(String::new()),
            user_wallet: Mutex::new(String::new()),
            donation: DonationSchedule::new(donate_level),
            tls_config: Arc::new(tls_config),
            pinned_fingerprint: fingerprint,
            session_id: Mutex::new(String::new()),
            accepted_shares: AtomicU32::new(0),
            rejected_shares: AtomicU32::new(0),
            pending_shares: Mutex::new(HashMap::new()),
            lost_shares: AtomicU32::new(0),
            unsent_shares: AtomicU32::new(0),
            submits_in_flight: AtomicU32::new(0),
            silence_timeout_ms: AtomicU64::new(POOL_SILENCE_TIMEOUT.as_millis() as u64),
            rotation_settle_ms: AtomicU64::new(ROTATION_SETTLE_LIMIT.as_millis() as u64),
            rotation_wait_since: Mutex::new(None),
        }
    }

    pub fn connect(&self, address: &str) -> Result<(), String> {
        log::info!("Connecting to pool: {}", address);

        // Checked before the socket is opened: this reads only `address`, and
        // sitting behind `TcpStream::connect` made the test for it depend on a
        // third party's routing — 75 s when one A record blackholes, and a
        // *failure* rather than a skip with no route at all.
        //
        // A pin is a security decision, and TLS here is inferred from the port
        // rather than asked for. Connecting in plaintext while the log reports a
        // pin would leave the operator believing the pool is authenticated when
        // nothing is: worse than not offering pinning at all. Observed on
        // `gulf.moneroocean.stream:20128`, which really does speak TLS but is
        // not in TLS_PORTS — so the pin was accepted, announced, and ignored.
        if self.pinned_fingerprint.is_some() && !is_tls_port(address) {
            return Err(format!(
                "a TLS certificate is pinned, but port {} is not treated as a TLS port, so \
                 the connection would be plaintext and the pin would do nothing. Refusing \
                 rather than connecting unauthenticated.\n  \
                 If this pool speaks TLS on that port, it needs adding to TLS_PORTS \
                 ({:?}).\n  \
                 If it does not, remove the pin — there is no certificate to pin.",
                address.rsplit_once(':').map(|(_, p)| p).unwrap_or("?"),
                TLS_PORTS,
            ));
        }


        let tcp_stream = TcpStream::connect(address)
            .map_err(|e| format!("TCP connect failed: {}", e))?;

        tcp_stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| format!("Set read timeout failed: {}", e))?;
        tcp_stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| format!("Set write timeout failed: {}", e))?;
        tcp_stream
            .set_nodelay(true)
            .map_err(|e| format!("Set nodelay failed: {}", e))?;

        let pool_stream = if is_tls_port(address) {
            let host = address
                .rsplit_once(':')
                .map(|(h, _)| h)
                .unwrap_or(address);

            let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
                .map_err(|e| format!("Invalid server name '{}': {}", host, e))?;

            let tls_conn = rustls::ClientConnection::new(self.tls_config.clone(), server_name)
                .map_err(|e| format!("TLS setup failed: {}", e))?;

            log::info!("Connected to pool (TLS): {}", address);
            PoolStream::Tls(rustls::StreamOwned::new(tls_conn, tcp_stream))
        } else {
            log::info!("Connected to pool (plain TCP): {}", address);
            PoolStream::Plain(tcp_stream)
        };

        if let Ok(mut addr) = self.address.lock() {
            *addr = address.to_string();
        }

        *self.stream.lock() = Some(pool_stream);

        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn login(&self, wallet: &str) -> Result<(), String> {
        if let Ok(mut w) = self.wallet.lock() {
            *w = wallet.to_string();
        }
        // The very first login establishes the user's own wallet; donation
        // relogins (author/XMRig) must not overwrite it.
        if let Ok(mut uw) = self.user_wallet.lock()
            && uw.is_empty()
        {
            *uw = wallet.to_string();
        }

        let params = serde_json::json!({
            "login": wallet,
            "pass": "x",
            "agent": concat!("MinerTim/", env!("CARGO_PKG_VERSION")),
            "algo": "rx/0"
        });

        let response = self.send_request("login", params)?;

        // GitHub #37 review (PR #42, F1): check `error` before `result`.
        // sammy007/monero-stratum answers a rejected login with
        // `{"result":null,"error":{...}}` — `result` is present but null, so
        // checking it first (as the pre-review code did) swallowed the
        // pool's real rejection reason behind a generic "no session id"
        // error. xmrig's `Client::parseResponse` checks `error` first for
        // the same reason. `.filter(|v| !v.is_null())` on both treats an
        // explicit JSON `null` the same as an absent field.
        if let Some(error) = response.get("error").filter(|v| !v.is_null()) {
            Err(format!("Login error: {}", error))
        } else if let Some(result) = response.get("result").filter(|v| !v.is_null()) {
            // GitHub #37: a submit-acknowledgement reply read by mistake as
            // the login reply (e.g. `{"id":99,"result":{"status":"OK"}}`)
            // has no session id and must not be treated as a successful
            // login — this matches xmrig's `Client::parseLogin`, which
            // refuses a login with no rpc id. Checked before any job is
            // installed, so a misread reply cannot silently "succeed" with
            // no session id update and no job installed.
            let Some(id) = result.get("id").and_then(|v| v.as_str()) else {
                return Err(format!("Login response carried no session id: {}", result));
            };
            if let Ok(mut sid) = self.session_id.lock() {
                *sid = id.to_string();
            }
            log::info!("Login successful, session id: {}", id);
            if let Some(job_data) = result.get("job")
                && let Some(job) = parse_job(job_data)
            {
                let diff = target_to_difficulty(&job.target);
                log::info!(
                    "Initial job: {} (difficulty: {}, target: {})",
                    job.job_id,
                    diff,
                    hex_encode(&job.target),
                );
                if let Ok(mut current) = self.current_job.lock() {
                    *current = Some(Arc::new(job));
                }
            }
            Ok(())
        } else {
            Err("Unexpected login response".into())
        }
    }

    pub fn get_work(&self) -> Option<Arc<Job>> {
        self.current_job.lock().ok()?.clone()
    }

    pub fn submit_share(
        &self,
        job_id: &str,
        nonce: &str,
        result: &str,
    ) -> Result<(), String> {
        // Counted from entry so a submit parked on the stream lock (not yet
        // registered in `pending_shares`) is still visible to the rotation
        // gate. Dropped on every return path out of this function, including
        // the early `?`s inside `write_and_register` (#32).
        self.submits_in_flight.fetch_add(1, Ordering::SeqCst);
        let _in_flight = InFlight(&self.submits_in_flight);

        let sid = self.session_id.lock()
            .map(|s| s.clone())
            .unwrap_or_default();

        let params = serde_json::json!({
            "id": sid,
            "job_id": job_id,
            "nonce": nonce,
            "result": result
        });

        let rpc_id = self.next_request_id();

        let (lock_wait_ms, write_ms) =
            match self.write_and_register(rpc_id, params, job_id, nonce) {
                Ok(timings) => timings,
                Err(e) => {
                    self.unsent_shares.fetch_add(1, Ordering::Relaxed);
                    log::warn!(
                        "Share not sent: rpc_id={} job_id={} nonce={} — {}",
                        rpc_id,
                        job_id,
                        nonce,
                        e
                    );
                    return Err(e);
                }
            };

        log::info!(
            "Share submitted: rpc_id={} job_id={} nonce={} lock_wait_ms={:.3} write_ms={:.3}",
            rpc_id,
            job_id,
            nonce,
            lock_wait_ms,
            write_ms
        );

        Ok(())
    }

    // Write and register under the STREAM lock, as one step. Two earlier
    // orderings each left a race, both found by review:
    //
    //   write, then insert (round 1): the receiver could read and handle
    //     the reply in the gap, find nothing pending, discard it uncounted
    //     — and the entry inserted afterwards was later drained as "lost"
    //     although the pool had answered it.
    //   insert, then write (round 2): a reconnect could null the stream and
    //     drain the fresh entry as "lost" in the gap, and the write would
    //     then go out on the NEW connection with the old session id and no
    //     entry left to pair its reply with.
    //
    // Holding the stream lock across both closes each: the receiver reads
    // under this same lock, so no reply can be handled before the entry
    // exists; and `reconnect()`/`relogin_as()` must take this lock to null
    // the stream, so a drain runs wholly before (we see no stream and
    // insert nothing) or wholly after (the entry was written to the old
    // stream, where no reply will ever be read, so "lost" is correct).
    //
    // What this does NOT close: the caller (`submit_share`) reads `sid`
    // before taking any lock, and `connect()` installs the new stream
    // before `login()` updates the session id. A submit that takes the
    // lock in that window goes out on the new connection with the old
    // session id. It IS registered, so the pool's answer is paired and
    // counted — normally as a rejection, which is the pool's real verdict
    // on it. The miscounting is closed; the stale id is not (review round
    // 3, R3-1) — still open in general. What IS closed (#37): a submit-reply
    // that `login()` mistakenly reads as the login reply (no session id in
    // it) now fails the login outright instead of silently "succeeding"
    // with the old id left in place, and a failed `relogin_as` now clears
    // the stream so the receiver loop reconnects cleanly rather than being
    // left on a stale, unauthenticated connection.
    //
    // Lock order is stream -> pending_shares. Nothing takes them the other
    // way: `drain_pending_shares` and `handle_pool_message` take
    // `pending_shares` alone, after the stream guard has been released.
    //
    // A separate, independent order exists for `rotation_settled` (#32
    // round 3, R3-F2): it holds `rotation_wait_since` while calling
    // `get_pending_shares()`, i.e. rotation_wait_since -> pending_shares.
    // No deadlock risk — only the receiver thread ever takes
    // `rotation_wait_since`, and `reconnect()` releases it (via its own
    // short `if let Ok(...)` block) before `drain_pending_shares()` runs —
    // but documented here since it's a lock-order fact now true of the
    // code, not because anything currently depends on it being checked.
    /// Returns `(lock_wait_ms, write_ms)` on success — timing instrumentation
    /// for issue #40: time spent waiting for the stream lock, and time spent
    /// in the write itself, each in milliseconds. (R4-N1: merge-history
    /// detail on how these came to live here belongs in `AUDIT.md`, not
    /// this comment — see NET-05's entry.)
    fn write_and_register(
        &self,
        rpc_id: u64,
        params: Value,
        job_id: &str,
        nonce: &str,
    ) -> Result<(f64, f64), String> {
        let lock_requested = Instant::now();
        let mut stream_guard = self.stream.lock();
        let lock_acquired = Instant::now();
        let stream = stream_guard
            .as_mut()
            .ok_or_else(|| "Not connected".to_string())?;
        write_request(stream, rpc_id, "submit", params)?;
        let write_done = Instant::now();
        if let Ok(mut pending) = self.pending_shares.lock() {
            pending.insert(
                rpc_id,
                PendingShare {
                    job_id: job_id.to_string(),
                    nonce: nonce.to_string(),
                    sent_at: Instant::now(),
                },
            );
        }
        Ok((
            (lock_acquired - lock_requested).as_secs_f64() * 1000.0,
            (write_done - lock_acquired).as_secs_f64() * 1000.0,
        ))
    }

    pub fn get_accepted_shares(&self) -> u32 {
        self.accepted_shares.load(Ordering::Relaxed)
    }

    pub fn get_rejected_shares(&self) -> u32 {
        self.rejected_shares.load(Ordering::Relaxed)
    }

    /// Submissions whose connection was torn down and replaced (reconnect or
    /// donation relogin) before the pool ever answered. Not "rejected" — the
    /// pool never said no.
    pub fn get_lost_shares(&self) -> u32 {
        self.lost_shares.load(Ordering::Relaxed)
    }

    /// Submissions `submit_share` was asked to send but never wrote at all —
    /// distinct from `lost_shares`, which the pool at least received (#32).
    pub fn get_unsent_shares(&self) -> u32 {
        self.unsent_shares.load(Ordering::Relaxed)
    }

    /// Submissions written to the pool with no response yet — neither
    /// accepted, rejected, nor lost. Lets `submitted == accepted + rejected +
    /// lost + pending` be checked from a running log, rather than only ever
    /// balancing once every submission has finally been answered or drained.
    pub fn get_pending_shares(&self) -> usize {
        self.pending_shares.lock().map(|p| p.len()).unwrap_or(0)
    }

    /// Remove every outstanding submission and count + log it as lost. Called
    /// before the stream is replaced, wherever that happens — `reconnect()`
    /// and the donation `relogin_as()` path — so a share that is about to
    /// become unanswerable (its response, if it ever arrives, will be for a
    /// connection that no longer exists) is not simply forgotten.
    fn drain_pending_shares(&self) {
        let drained: Vec<(u64, PendingShare)> = match self.pending_shares.lock() {
            Ok(mut pending) => pending.drain().collect(),
            Err(_) => return,
        };
        for (rpc_id, entry) in drained {
            log::warn!(
                "Share lost: rpc_id={} job_id={} nonce={} — no response before the \
                 connection was replaced ({}s outstanding)",
                rpc_id,
                entry.job_id,
                entry.nonce,
                entry.sent_at.elapsed().as_secs(),
            );
            self.lost_shares.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn reset_share_counters(&self) {
        self.accepted_shares.store(0, Ordering::SeqCst);
        self.rejected_shares.store(0, Ordering::SeqCst);
        // The ledger identity is now complete: found == accepted + rejected +
        // lost + unsent + pending + withheld (verify_failures) — every way a
        // found share can go is now counted. `found` and `verify_failures`
        // live in `miner.rs`'s own stats, not here, so this function cannot
        // reset them itself — but it must reset every term that lives on
        // this struct, or the arithmetic that exposed #34 would silently
        // stop balancing. `unsent` is the newest term (#32): a share
        // `submit_share` never wrote at all, so it is not "lost" (the pool
        // never saw it), but it is still accounted for.
        self.lost_shares.store(0, Ordering::SeqCst);
        self.unsent_shares.store(0, Ordering::SeqCst);
    }

    /// Spawn the receiver thread. It polls the shared stream with a short
    /// read timeout so that share submissions can interleave on the same
    /// session, sends keepalives, and reconnects on connection loss.
    pub fn start_receiver(self: &Arc<Self>) {
        if !self.connected.load(Ordering::SeqCst) {
            return;
        }
        let conn = Arc::clone(self);
        thread::Builder::new()
            .name("pool-receiver".into())
            .spawn(move || conn.receiver_loop())
            .ok();
    }

    fn receiver_loop(&self) {
        // Raise this thread's priority so it keeps processing job updates and
        // share submissions promptly even when every core is busy mining.
        boost_current_thread_priority();
        self.set_read_timeout(RECV_POLL_INTERVAL);

        let mut pending: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        let mut last_keepalive = Instant::now();
        // Last time the pool sent us anything at all. Not "last job" — any
        // byte counts, so a chatty pool that happens not to be rotating jobs
        // does not trip the silence check.
        let mut last_recv = Instant::now();

        // Donation schedule reference point; the initial login is the user, so
        // we start in the User slice.
        let donation_start = Instant::now();
        let mut active = Beneficiary::User;
        // Tracks `want` across iterations for `note_donation_target` below —
        // see its doc comment (#32 round 3, R3-F1).
        let mut last_want = active;
        loop {
            // Rotate the login wallet between user/author/XMRig per the donation
            // schedule (see `crate::donate`). Switching re-logs-in on the same
            // pool with the target wallet. Deferral state for this now lives
            // in the `rotation_wait_since` field, not a loop local — see its
            // doc comment for why (#32 round 2, R2-F2).
            let want = self.donation.beneficiary_at(donation_start.elapsed().as_secs());
            self.note_donation_target(want, &mut last_want);
            if want != active && self.rotation_settled(want) {
                active = want;
                let addr = self.beneficiary_address(want);
                log::info!(
                    "Donation: mining to {:?} (donate-level {}%)",
                    want,
                    self.donation.level()
                );
                if let Ok(mut w) = self.wallet.lock() {
                    *w = addr.clone();
                }
                match self.relogin_as(&addr) {
                    Ok(()) => {
                        // A successful relogin performs a real read, which
                        // proves the peer is alive — count it (R1-F1b).
                        last_recv = Instant::now();
                        pending.clear();
                        continue;
                    }
                    Err(e) => {
                        // Stream is torn down (relogin_as now clears it on any
                        // failure, #37); the read below yields NotConnected
                        // and reconnect() re-establishes using self.wallet (= addr).
                        log::warn!("Donation switch failed: {} (reconnecting)", e);
                    }
                }
            }

            // Hold the stream lock only for the duration of one read so
            // submits/keepalives from other threads can interleave.
            let read_result = {
                let mut guard = self.stream.lock();
                match guard.as_mut() {
                    Some(s) => s.read(&mut chunk),
                    None => Err(std::io::Error::new(ErrorKind::NotConnected, "no stream")),
                }
            };

            match read_result {
                Ok(0) => {
                    log::warn!("Pool closed the connection");
                    if !self.reconnect() {
                        return;
                    }
                    last_recv = Instant::now();
                    pending.clear();
                }
                Ok(n) => {
                    last_recv = Instant::now();
                    pending.extend_from_slice(&chunk[..n]);
                    match take_complete_lines(&mut pending) {
                        Ok(lines) => {
                            for line in lines {
                                log::debug!("Pool recv: {}", line);
                                self.handle_pool_message(&line);
                            }
                        }
                        Err(overflow) => {
                            log::error!(
                                "Pool sent {overflow} bytes with no newline (limit \
                                 {MAX_LINE_BYTES}). This is not a valid Stratum message; \
                                 discarding the buffer and reconnecting."
                            );
                            pending.clear();
                            if !self.reconnect() {
                                return;
                            }
                            // After, not before. `Ok(n)` already refreshed
                            // `last_recv` when these bytes arrived, but
                            // `reconnect()` retries without a bound — if it
                            // takes longer than the silence window, the very
                            // next iteration would fire the check against a
                            // healthy new connection (R1-F1a).
                            last_recv = Instant::now();
                        }
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => {
                    log::error!("Pool read error: {}", e);
                    if !self.reconnect() {
                        return;
                    }
                    last_recv = Instant::now();
                    pending.clear();
                }
            }

            if last_keepalive.elapsed() >= KEEPALIVE_INTERVAL {
                last_keepalive = Instant::now();
                let sid = self.session_id.lock().map(|s| s.clone()).unwrap_or_default();
                if let Err(e) = self.send_message("keepalived", serde_json::json!({ "id": sid })) {
                    // Previously this only warned and carried on. A keepalive
                    // that cannot be written is a connection that cannot carry
                    // a share submission either, so treat it as lost (#34).
                    log::warn!("Keepalive failed, reconnecting: {}", e);
                    if !self.reconnect() {
                        return;
                    }
                    last_recv = Instant::now();
                    pending.clear();
                    continue;
                }
            }

            // The pool has said nothing for too long. See POOL_SILENCE_TIMEOUT:
            // the socket may still be writable, so only the receive side can
            // tell us this connection is finished.
            let silence_limit =
                Duration::from_millis(self.silence_timeout_ms.load(Ordering::Relaxed));
            if last_recv.elapsed() >= silence_limit {
                log::warn!(
                    "No data from pool for {}s (limit {}s) — treating the connection \
                     as dead and reconnecting. Shares found against the current job \
                     cannot be submitted on a dead connection.",
                    last_recv.elapsed().as_secs(),
                    silence_limit.as_secs()
                );
                if !self.reconnect() {
                    return;
                }
                last_recv = Instant::now();
                pending.clear();
            }
        }
    }

    /// Tear down the current stream and retry connect + login until it
    /// succeeds. Returns false if we don't have enough info to reconnect.
    fn reconnect(&self) -> bool {
        self.connected.store(false, Ordering::SeqCst);
        // Clear the job so workers idle (get_work() -> None) instead of mining and
        // submitting against the connection being torn down.
        if let Ok(mut job) = self.current_job.lock() {
            *job = None;
        }
        *self.stream.lock() = None;
        // Any donation-rotation deferral in progress is for a connection
        // epoch that just ended — discard it rather than let its timestamp
        // survive into whatever comes after reconnecting, however long that
        // takes (#32 round 2, R2-F2; see the field's own doc comment).
        if let Ok(mut w) = self.rotation_wait_since.lock() {
            *w = None;
        }
        // Every reconnect goes through this one function, so this is the single
        // place a submission on the old connection can be declared lost rather
        // than left pending forever against a stream that no longer exists.
        self.drain_pending_shares();

        let address = self.address.lock().map(|a| a.clone()).unwrap_or_default();
        let wallet = self.wallet.lock().map(|w| w.clone()).unwrap_or_default();
        if address.is_empty() || wallet.is_empty() {
            log::error!("Cannot reconnect: no pool address/wallet recorded");
            return false;
        }

        loop {
            thread::sleep(RECONNECT_DELAY);
            log::info!("Reconnecting to {}...", address);
            match self.connect(&address).and_then(|_| self.login(&wallet)) {
                Ok(()) => {
                    self.set_read_timeout(RECV_POLL_INTERVAL);
                    log::info!("Reconnected to pool");
                    return true;
                }
                Err(e) => {
                    *self.stream.lock() = None;
                    log::warn!(
                        "Reconnect failed: {} (retrying in {}s)",
                        e,
                        RECONNECT_DELAY.as_secs()
                    );
                }
            }
        }
    }

    /// The address for a donation beneficiary. `User` resolves to the wallet
    /// captured on first login; the others are the fixed donation addresses.
    fn beneficiary_address(&self, who: Beneficiary) -> String {
        match who {
            Beneficiary::User => self.user_wallet.lock().map(|w| w.clone()).unwrap_or_default(),
            Beneficiary::Author => crate::donate::AUTHOR_ADDRESS.to_string(),
            Beneficiary::Xmrig => crate::donate::XMRIG_ADDRESS.to_string(),
        }
    }

    /// Tear down the current session and log in again on the same pool with a
    /// different wallet. One attempt; on failure the caller falls through to
    /// the reconnect path. On return, either the stream is `None`, or the
    /// connection is logged in and has the 50ms poll-interval read timeout.
    fn relogin_as(&self, wallet: &str) -> Result<(), String> {
        let address = self.address.lock().map(|a| a.clone()).unwrap_or_default();
        if address.is_empty() {
            return Err("no pool address recorded".into());
        }
        // Drain before switching (xmrig's DonateStrategy uses a similar settle
        // step): clear the current job so workers stop mining and submitting
        // across the reconnect, otherwise in-flight shares are rejected as stale
        // "Invalid job id" or fail to send ("Not connected"). login() installs the
        // fresh job for the new wallet, and workers resume.
        if let Ok(mut job) = self.current_job.lock() {
            *job = None;
        }
        *self.stream.lock() = None;
        // This replaces the stream without going through `reconnect()`, so it
        // needs its own drain — otherwise a submission sent just before a
        // donation-slice switch is orphaned in the pending map forever.
        self.drain_pending_shares();
        // GitHub #37: a failed `login` used to return early here with the
        // stream `connect()` had just installed still in place: live,
        // unauthenticated, the session id still the previous connection's,
        // and on the 30s read timeout `connect`/`send_request` set. (A
        // failed `connect` never installed one, because the stream was
        // already cleared above.) Clearing it on failure instead sends the
        // receiver loop into its normal `reconnect()` path on the next read
        // (which retries cleanly using `self.wallet`, already set to the
        // donation address).
        if let Err(e) = self.connect(&address).and_then(|_| self.login(wallet)) {
            *self.stream.lock() = None;
            return Err(e);
        }
        self.set_read_timeout(RECV_POLL_INTERVAL);
        Ok(())
    }

    fn handle_pool_message(&self, line: &str) {
        let msg: Value = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("Failed to parse pool message: {}", e);
                return;
            }
        };

        // Handle job notifications
        let is_job = msg.get("method").and_then(|m| m.as_str()) == Some("job");
        let job_params = if is_job {
            msg.get("params")
        } else {
            msg.get("result").and_then(|r| r.get("job"))
        };

        // A job that will not parse is declined and the previous one stays in
        // force — which fails safe, but used to fail *silently*. A pool sending
        // only malformed jobs would pin the miner to a stale job with no
        // diagnostic at all, and stale work is exactly what looks like a JIT
        // fault from the share-reject side. Log it once per occurrence.
        if let Some(job_data) = job_params
            && parse_job(job_data).is_none()
        {
            log::warn!(
                "Pool sent a job that could not be parsed; keeping the previous job. \
                 Fields must be even-length hex: blob={:?} target={:?} seed_hash={:?}",
                job_data.get("blob").and_then(|v| v.as_str()).map(|s| s.len()),
                job_data.get("target").and_then(|v| v.as_str()).map(|s| s.len()),
                job_data.get("seed_hash").and_then(|v| v.as_str()).map(|s| s.len()),
            );
        }

        if let Some(job_data) = job_params
            && let Some(job) = parse_job(job_data)
        {
            let diff = target_to_difficulty(&job.target);
            log::info!(
                "New job: {} (difficulty: {}, target: {})",
                job.job_id,
                diff,
                hex_encode(&job.target),
            );
            if let Ok(mut current) = self.current_job.lock() {
                *current = Some(Arc::new(job));
            }
        }

        // A response candidate: has a numeric "id" and no "method" (a
        // notification, like "job", carries a method and never an id we
        // issued). That used to be sufficient to call it a share response,
        // but the only *other* id-bearing, method-less messages this handler
        // ever sees are keepalive responses — login goes through the
        // synchronous `send_request` and never reaches here — so an errored
        // keepalive was being counted as a rejected share. Pairing by id
        // against `pending_shares` is what tells the two apart (#17): only an
        // id this connection is actually waiting on for a *submit* is treated
        // as a share result.
        if msg.get("method").is_none()
            && let Some(rpc_id) = msg.get("id").and_then(|v| v.as_u64())
        {
            let entry = self
                .pending_shares
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&rpc_id));

            let Some(entry) = entry else {
                // A keepalive reply is unmatched *by design* — it is sent via
                // plain `send_message`, which never registers an id — and
                // arrives every `KEEPALIVE_INTERVAL`, so it stays at `debug`
                // to avoid flooding the log. Anything else unmatched (a late
                // reply after a reconnect, or an untracked request) is also
                // the only trace the race Finding 1 fixed would ever have
                // left, so it is worth `warn`.
                let status = msg
                    .get("result")
                    .and_then(|r| r.get("status"))
                    .and_then(|s| s.as_str());
                if status == Some("KEEPALIVED") {
                    log::debug!(
                        "Pool response for rpc_id={} matches no pending share submission \
                         (keepalive) — not counted",
                        rpc_id,
                    );
                } else {
                    log::warn!(
                        "Pool response rpc_id={} matches no pending share submission (late \
                         reply after a reconnect, or an untracked request)",
                        rpc_id,
                    );
                }
                return;
            };

            if let Some(error) = msg.get("error")
                && !error.is_null()
            {
                let err_msg = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                log::warn!(
                    "Share rejected: rpc_id={} job_id={} nonce={} — {}",
                    rpc_id,
                    entry.job_id,
                    entry.nonce,
                    err_msg,
                );
                self.rejected_shares.fetch_add(1, Ordering::Relaxed);
                return;
            }
            let status = msg
                .get("result")
                .and_then(|r| r.get("status"))
                .and_then(|s| s.as_str())
                .unwrap_or("");
            if status == "OK" {
                let latency_ms = entry.sent_at.elapsed().as_millis();
                log::info!(
                    "Share accepted: rpc_id={} job_id={} nonce={} latency_ms={}",
                    rpc_id,
                    entry.job_id,
                    entry.nonce,
                    latency_ms,
                );
                self.accepted_shares.fetch_add(1, Ordering::Relaxed);
            } else {
                // Neither an error nor `status == "OK"` — a shape no known
                // pool sends, but one that must still be accounted for:
                // the entry is already removed from `pending_shares` above,
                // so silently falling through here would make it vanish
                // from the found = accepted + rejected + lost + pending
                // ledger with no trace at all.
                log::warn!(
                    "Share rejected: rpc_id={} job_id={} nonce={} — unrecognised status \"{}\"",
                    rpc_id,
                    entry.job_id,
                    entry.nonce,
                    status,
                );
                self.rejected_shares.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn set_read_timeout(&self, timeout: Duration) {
        let guard = self.stream.lock();
        if let Some(s) = guard.as_ref()
            && let Err(e) = s.tcp().set_read_timeout(Some(timeout))
        {
            log::warn!("Failed to set read timeout: {}", e);
        }
    }

    /// Send a request and synchronously read the response line. Only used
    /// for login, before/while the receiver polls; reads byte-by-byte so no
    /// buffered data is lost to a throwaway reader.
    fn send_request(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut stream_guard = self.stream.lock();

        let stream = stream_guard
            .as_mut()
            .ok_or_else(|| "Not connected".to_string())?;

        write_request(stream, self.next_request_id(), method, params)?;

        // Login can race with the poll-interval timeout after a reconnect;
        // allow the pool a full window to respond.
        let _ = stream.tcp().set_read_timeout(Some(Duration::from_secs(30)));
        let response_line = read_line(stream)?;
        log::debug!("Pool recv: {}", response_line.trim());

        serde_json::from_str(&response_line).map_err(|e| format!("Parse failed: {}", e))
    }

    /// Write-only send — responses are handled by the receiver thread.
    ///
    /// The id is allocated here and deliberately not returned: callers of this
    /// (keepalive) never register it, which is exactly why their responses are
    /// *not* found in `pending_shares`. `submit_share` does its own write,
    /// because it must register the id under the stream lock — see there.
    fn send_message(&self, method: &str, params: Value) -> Result<(), String> {
        let mut stream_guard = self.stream.lock();
        let stream = stream_guard
            .as_mut()
            .ok_or_else(|| "Not connected".to_string())?;
        write_request(stream, self.next_request_id(), method, params)
    }

    fn next_request_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Clears any in-progress rotation deferral if `want` differs from the
    /// value seen on the *previous* call — called unconditionally from the
    /// top of `receiver_loop`, every iteration, before the `want != active`
    /// branch that calls `rotation_settled`.
    ///
    /// This has to be unconditional, not folded into `rotation_settled`'s
    /// own beneficiary-mismatch check, because that check only runs when
    /// `rotation_settled` is called at all — which the call site skips
    /// whenever `want == active`. That skip is exactly where a stale entry
    /// survives in a **2-value ring**: at `--donate-level 100`, `User`'s
    /// slice is zero-width, so the schedule only ever alternates
    /// Author<->Xmrig. A deferred entry for Xmrig can then survive a full
    /// trip back to Xmrig — the loop passes through `want == active` (the
    /// *other* beneficiary, which by then IS `active`) without ever calling
    /// `rotation_settled` to notice the mismatch, because at a 3-value ring
    /// the "other" value is a third, different beneficiary that would
    /// trigger it, but at a 2-value ring it's the exact one already stored
    /// going the other direction (#32 round 3, R3-F1 — found after both the
    /// beneficiary-mismatch check in `rotation_settled` and the `reconnect()`
    /// clear from rounds 1-2 were already in place and still missed this).
    ///
    /// Extracted as its own method, not left inline in `receiver_loop`,
    /// specifically so it has its own unit test — R3-F1's own review noted
    /// that loop-local clearing logic "nothing exercises" is exactly the
    /// shape of gap that let the first two rounds' fixes each miss something.
    fn note_donation_target(&self, want: Beneficiary, last_want: &mut Beneficiary) {
        if want != *last_want {
            if let Ok(mut w) = self.rotation_wait_since.lock() {
                *w = None;
            }
            *last_want = want;
        }
    }

    /// Whether a pending donation rotation to `want` may proceed now. Called
    /// from the top of `receiver_loop`, with no lock held.
    ///
    /// `rotation_wait_since` is `None` until the first deferral, then holds
    /// the beneficiary being deferred and when the wait started; cleared
    /// again once the rotation is allowed through, by every call to
    /// `reconnect()` (#32 round 2, R2-F2), and by `note_donation_target`
    /// above on every observed change of `want` (#32 round 3, R3-F1) — three
    /// independent mechanisms, each closing a gap the others didn't.
    ///
    /// The stored beneficiary is still checked against `want` here too, as
    /// a third, cheaper line of defence reached on the same call that would
    /// otherwise use the stale entry — redundant with `note_donation_target`
    /// in the 3-value-ring case, not redundant in the 2-value-ring case if
    /// `note_donation_target`'s own call site were ever changed.
    ///
    /// Read order matters: `submits_in_flight` is read BEFORE
    /// `get_pending_shares()`. Reading `pending == 0` first would let a
    /// submitter register and exit between the two reads, with `in_flight`
    /// then read as `0` too, letting a rotation proceed past a share that
    /// just got registered. Reading in-flight first is safe because a
    /// submitter stays counted in-flight until after it registers (#32).
    fn rotation_settled(&self, want: Beneficiary) -> bool {
        let mut wait_since = self.rotation_wait_since.lock().unwrap();

        if matches!(*wait_since, Some((b, _)) if b != want) {
            *wait_since = None;
        }

        let in_flight = self.submits_in_flight.load(Ordering::SeqCst);
        let pending = self.get_pending_shares();

        let waited = wait_since.map(|(_, t)| t.elapsed()).unwrap_or(Duration::ZERO);
        let limit = Duration::from_millis(self.rotation_settle_ms.load(Ordering::Relaxed));

        if !rotation_may_proceed(in_flight, pending, waited, limit) {
            if wait_since.is_none() {
                log::info!(
                    "Donation rotation to {:?} deferred: {} submission(s) in flight, {} \
                     awaiting a pool response",
                    want,
                    in_flight,
                    pending
                );
                *wait_since = Some((want, Instant::now()));
            }
            // No lock held here: the receiver has already released the
            // stream lock for this iteration by the time this is called.
            // The `rotation_wait_since` lock is dropped explicitly right
            // here (R3-F2: an earlier comment said "at function return",
            // which was wrong — it's this `drop()`, not scope exit) so it
            // isn't held across the sleep either.
            drop(wait_since);
            thread::sleep(ROTATION_SETTLE_YIELD);
            false
        } else {
            if wait_since.is_some() {
                if in_flight + pending as u32 > 0 {
                    log::warn!(
                        "Donation rotation proceeding after {}ms with {} submission(s) \
                         still unanswered; they will be counted lost/unsent",
                        waited.as_millis(),
                        in_flight + pending as u32
                    );
                } else {
                    log::info!("Donation rotation settled after {}ms", waited.as_millis());
                }
            }
            *wait_since = None;
            true
        }
    }
}

/// Pure decision: may a donation rotation proceed now? True once nothing is
/// outstanding, or once the wait has reached `limit` regardless (#32).
fn rotation_may_proceed(in_flight: u32, pending: usize, waited: Duration, limit: Duration) -> bool {
    (in_flight == 0 && pending == 0) || waited >= limit
}

fn write_request(
    stream: &mut PoolStream,
    id: u64,
    method: &str,
    params: Value,
) -> Result<(), String> {
    let request = JsonRpcRequest {
        id,
        jsonrpc: "2.0",
        method: method.to_string(),
        params,
    };

    let mut msg =
        serde_json::to_string(&request).map_err(|e| format!("Serialize failed: {}", e))?;
    msg.push('\n');

    log::debug!("Pool send: {}", msg.trim());

    stream
        .write_all(msg.as_bytes())
        .map_err(|e| format!("Write failed: {}", e))?;
    stream.flush().map_err(|e| format!("Flush failed: {}", e))
}

/// Read a single newline-terminated line without buffering past it.
fn read_line(stream: &mut PoolStream) -> Result<String, String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                line.push(byte[0]);
                if line.len() > MAX_LINE_BYTES {
                    return Err(format!(
                        "pool response line exceeded {MAX_LINE_BYTES} bytes without a newline"
                    ));
                }
            }
            Err(e) => return Err(format!("Read failed: {}", e)),
        }
    }
    String::from_utf8(line).map_err(|e| format!("Invalid UTF-8 from pool: {}", e))
}

/// Raise the calling thread's scheduling priority. On macOS the pool receiver
/// runs at USER_INTERACTIVE QoS so the scheduler preempts a mining worker to run
/// it — without this, 12 mining threads saturate all cores, the receiver is
/// starved, `current_job` goes stale, and shares are rejected as "Invalid job id".
#[cfg(target_os = "macos")]
fn boost_current_thread_priority() {
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    unsafe {
        pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
    }
}

#[cfg(not(target_os = "macos"))]
fn boost_current_thread_priority() {}

/// Take every complete line out of `pending`, leaving any partial one behind.
///
/// `Err(len)` means the remainder — which by construction is a single unfinished
/// line — is already longer than [`MAX_LINE_BYTES`]. The caller discards the
/// buffer and reconnects: a peer that has sent a megabyte without a newline is
/// not speaking Stratum, and continuing to buffer is how an endless newline-free
/// stream kills the process (GitHub #21).
///
/// Splitting messages across reads is normal and must keep working, which is why
/// the check is on the *remainder after draining* rather than on the buffer as it
/// arrives. Returning whole lines only is also what keeps a truncated message
/// from ever reaching `handle_pool_message`, so an oversized message cannot leave
/// a partial job active.
fn take_complete_lines(pending: &mut Vec<u8>) -> Result<Vec<String>, usize> {
    let mut lines = Vec::new();
    while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
        let raw: Vec<u8> = pending.drain(..=pos).collect();
        let line = String::from_utf8_lossy(&raw).trim().to_string();
        if !line.is_empty() {
            lines.push(line);
        }
    }
    if pending.len() > MAX_LINE_BYTES {
        return Err(pending.len());
    }
    Ok(lines)
}

fn parse_job(data: &Value) -> Option<Job> {
    let blob_hex = data.get("blob")?.as_str()?;
    let target_hex = data.get("target")?.as_str()?;
    let job_id = data.get("job_id")?.as_str()?.to_string();
    let seed_hash_hex = data.get("seed_hash")?.as_str()?;

    let blob = hex_decode(blob_hex)?;
    let target = hex_decode(target_hex)?;
    let seed_hash = hex_decode(seed_hash_hex)?;

    Some(Job {
        blob,
        target,
        job_id,
        seed_hash,
    })
}

/// Convert a Stratum target (4-byte compact or 8-byte full, little-endian)
/// to a pool difficulty.
pub fn target_to_difficulty(target: &[u8]) -> u64 {
    if target.len() >= 8 {
        let t = u64::from_le_bytes(target[0..8].try_into().unwrap());
        if t == 0 {
            return 0;
        }
        return u64::MAX / t;
    }
    if target.len() >= 4 {
        let t = u32::from_le_bytes(target[0..4].try_into().unwrap());
        if t == 0 {
            return u64::MAX;
        }
        return 0xFFFFFFFF_u64 / t as u64;
    }
    0
}

#[cfg(test)]
mod tls_tests {
    use super::*;

    /// The real monerohash.com:9999 fingerprint, read 2026-09-13. Used as a
    /// realistic shape rather than as a live expectation — the pool rotates via
    /// Let's Encrypt, so the value will change and that is fine: nothing here
    /// contacts the network.
    const SAMPLE: &str = "3d587c824a6f6032e1767518f0f1db29cdf206ba29bd7cb1647f522f8ae3d420";

    /// The reason the `hex_decode` fix matters beyond the CLI. `parse_job` runs
    /// it on three pool-supplied fields, so before the fix a pool could abort
    /// the miner by putting one non-ASCII byte in a job, or smuggle a byte past
    /// it with a `+` sign. A malformed job must be *declined* — `None` — leaving
    /// the previous job in force, which is how the receiver already handles
    /// anything it cannot parse.
    ///
    /// **Fixture lengths are load-bearing, and the first version got them
    /// wrong.** `"ff€ff"` is *seven* bytes, so it was caught by the odd-length
    /// check that every version of `hex_decode` has had — the test was green
    /// against both unfixed implementations while asserting "must be declined,
    /// not panic" of an input that never panicked. `"ff€f"` is six: it passes the
    /// length check, reaches the byte-index slice, and panics on the old code.
    /// Round 4 caught that; it was one character from being a real regression
    /// test.
    #[test]
    fn a_malformed_job_from_the_pool_is_declined_not_fatal() {
        let good = serde_json::json!({
            "blob": "0f0f", "target": "ffffffff",
            "job_id": "j1", "seed_hash": "abcd",
        });
        assert!(parse_job(&good).is_some(), "the control case must parse");

        for field in ["blob", "target", "seed_hash"] {
            // Even length, so it reaches the slicer: this is the panic case.
            let mut hostile = good.clone();
            hostile[field] = serde_json::json!("ff\u{20AC}f");
            assert_eq!(
                hostile[field].as_str().unwrap().len(),
                6,
                "fixture must be even-length or it only tests the length check"
            );
            assert!(
                parse_job(&hostile).is_none(),
                "a non-ASCII {field} must be declined, not panic"
            );

            // The sign defect: even length, all ASCII, and `from_str_radix`
            // used to decode "+f" as 0x0f. This is its only caller-level cover.
            let mut signed = good.clone();
            signed[field] = serde_json::json!("+f+f");
            assert!(
                parse_job(&signed).is_none(),
                "a signed {field} is not hex and must be declined"
            );

            let mut odd = good.clone();
            odd[field] = serde_json::json!("abc");
            assert!(parse_job(&odd).is_none(), "an odd-length {field} must be declined");
        }
    }

    /// **The wiring test**, not just the helper. PR #22 taught this repo three
    /// times that a test holding an extracted function passes happily while the
    /// production call site is mutated away — so this drives the real
    /// `receiver_loop` over a real socket.
    ///
    /// A pool that accepts the connection, answers the login, and then says
    /// **nothing** must be detected as dead and reconnected to.
    ///
    /// This is GitHub #34, observed live: the socket stays open and writable,
    /// so keepalives keep succeeding and prove nothing, while the miner hashes
    /// a job the pool replaced long ago and every share it finds is submitted
    /// into a connection that never answers. Two windows of 121 and 88 minutes
    /// in one session, ending only when the pool finally closed the socket.
    ///
    /// The listener holds every socket open for the life of the test. That is
    /// the load-bearing detail: if the server dropped the first socket the
    /// miner would see EOF and reconnect for that reason instead, and the test
    /// would pass against the unfixed code — exactly how the first attempt at
    /// the flood test below was worthless. Here the **only** route to a second
    /// accept is the silence timeout firing.
    ///
    /// Hermetic: `127.0.0.1` on an ephemeral port, which is not in `TLS_PORTS`,
    /// so this is plain TCP with no certificate involved.
    #[test]
    fn a_silent_pool_is_detected_and_reconnected_to() {
        use std::io::Write as _;
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel::<u8>();

        let server = thread::spawn(move || {
            let mut held = Vec::new();
            for (n, incoming) in listener.incoming().enumerate() {
                let mut sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let _ = tx.send(n as u8);
                // Answer the login so the miner reaches its receive loop, then
                // go quiet for good. Never close: silence, not EOF, is the
                // condition under test.
                let _ = sock.write_all(
                    b"{\"id\":1,\"result\":{\"id\":\"sess\",\"job\":{\"blob\":\"00\",\
                      \"job_id\":\"j1\",\"target\":\"ffffffff\"},\"status\":\"OK\"}}\n",
                );
                let _ = sock.flush();
                held.push(sock);
                if n >= 1 {
                    // Second connection observed — that is the assertion.
                    thread::sleep(Duration::from_millis(300));
                    return;
                }
            }
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        // Drive the real loop to the timeout in about a second rather than
        // three minutes. Only the window changes; the code under test is the
        // shipping `receiver_loop`.
        conn.silence_timeout_ms.store(600, Ordering::Relaxed);
        conn.connect(&addr).expect("connect to the local listener");
        // reconnect() needs both recorded, or it bails without retrying.
        *conn.address.lock().unwrap() = addr.clone();
        *conn.wallet.lock().unwrap() = "4test".to_string();

        let worker = conn.clone();
        thread::spawn(move || worker.receiver_loop());

        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(0), "first connection");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(20)),
            Ok(1),
            "a pool that goes silent past the timeout must be treated as dead and \
             reconnected to; no second connection means the silence went undetected, \
             which is #34"
        );
        let _ = server.join();
    }

    /// Shared body of the #40/#44 reproducers: does `receiver_loop` starve
    /// `submit_share` of the `stream` lock when the pool is quiet? The
    /// receiver re-takes the lock every `RECV_POLL_INTERVAL` (50ms) to
    /// attempt a read; if the mutex hands the lock back to the receiver ahead
    /// of a parked `submit_share` call, a share can sit for seconds before it
    /// reaches the wire — the #40 symptom (mean 7.8s, max 34s in a live run;
    /// #44 measured mean 11.6s, max 81.4s).
    ///
    /// `busy_threads` CPU-bound spinners run alongside, to reproduce the
    /// full-core mining load the live runs were under. Returns how long the
    /// `submit_share` call took. The spinners are stopped and joined before
    /// anything here can panic, so a failing submit cannot leave them
    /// spinning for the rest of the suite.
    fn quiet_receiver_submit_latency(busy_threads: usize) -> Duration {
        use std::io::Write as _;
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel::<()>();

        let server = thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            // Hold the socket open, silent, for up to 3s — never close it.
            // Silence on an open connection is the condition under test, not
            // EOF (that is #34's scenario, covered elsewhere). Under the bug,
            // this keepalive at ~3s is what finally let the submit through
            // (hence the 2.80s baseline); a teardown signal ends the wait
            // early so a passing run does not cost 3s.
            if rx.recv_timeout(Duration::from_secs(3)).is_ok() {
                return;
            }
            let _ = sock.write_all(
                b"{\"id\":999999,\"result\":{\"status\":\"KEEPALIVED\"}}\n",
            );
            let _ = sock.flush();
            // Keep holding the socket until the test signals teardown.
            let _ = rx.recv();
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        // Far above the 3s the server stays quiet, so the silence-timeout
        // reconnect path (#34) cannot fire and confound this measurement.
        conn.silence_timeout_ms.store(60_000, Ordering::Relaxed);
        conn.connect(&addr).expect("connect to the local listener");
        *conn.address.lock().unwrap() = addr.clone();
        *conn.wallet.lock().unwrap() = "4test".to_string();

        let stop = Arc::new(AtomicBool::new(false));
        let spinners: Vec<_> = (0..busy_threads)
            .map(|_| {
                let stop = Arc::clone(&stop);
                thread::spawn(move || {
                    let mut x: u64 = 0;
                    while !stop.load(Ordering::Relaxed) {
                        x = std::hint::black_box(x.wrapping_add(1));
                    }
                })
            })
            .collect();

        let worker = conn.clone();
        thread::spawn(move || worker.receiver_loop());
        thread::sleep(Duration::from_millis(200));

        let start = Instant::now();
        let result = conn.submit_share("j", "deadbeef", &"a".repeat(64));
        let elapsed = start.elapsed();

        stop.store(true, Ordering::Relaxed);
        for s in spinners {
            let _ = s.join();
        }
        // An empty address makes `reconnect()` return false, so the receiver
        // loop exits on the EOF that follows teardown instead of retrying
        // forever against a dead port for the rest of the test process.
        conn.address.lock().unwrap().clear();
        let _ = tx.send(());
        let _ = server.join();

        eprintln!(
            "quiet_receiver_submit_latency(busy_threads={}): submit_share took {:?}",
            busy_threads, elapsed
        );
        result.expect("submit_share must succeed against a connected stream");
        elapsed
    }

    /// Regression test for #40/#44 on an otherwise idle machine. See
    /// `quiet_receiver_submit_latency` for the scenario.
    #[test]
    fn a_submit_is_not_held_hostage_by_a_quiet_receiver() {
        let elapsed = quiet_receiver_submit_latency(0);
        assert!(
            elapsed < Duration::from_millis(500),
            "submit took {:?}, expected well under 500ms if the stream lock isn't starved",
            elapsed
        );
    }

    /// The same reproducer with every core busy, as it is while mining —
    /// the condition both live runs (#40, #44) measured the starvation under.
    #[test]
    fn a_submit_is_not_held_hostage_under_full_cpu_load() {
        let n = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let elapsed = quiet_receiver_submit_latency(n);
        assert!(
            elapsed < Duration::from_millis(500),
            "submit took {:?} with {} busy threads, expected well under 500ms if the \
             stream lock isn't starved",
            elapsed,
            n
        );
    }

    /// #44: proves the stream lock is *handed* to a parked submitter on
    /// release, not merely that submits happen to be fast. The test thread
    /// holds the lock, lets a `submit_share` park on it, releases, and then
    /// immediately asks for the lock again with a **blocking** `lock()`. A
    /// fair lock gives it to the parked submitter first, so by the time the
    /// test thread holds it again the share is already registered in
    /// `pending_shares`. An unfair lock lets the releasing thread barge
    /// straight back in — exactly what `receiver_loop` does to a submit in
    /// production — and the share is not there yet.
    ///
    /// Deliberately not `try_lock()` after the release: that would race the
    /// test thread's own scheduling against the handoff and flake on a loaded
    /// runner. The blocking re-lock has no such race.
    ///
    /// The only timing dependency is the settle sleep after the submitter is
    /// seen in flight, to let it get from `submit_share`'s entry to parked on
    /// the lock. It can only fail in one direction: too short and the
    /// submitter is not yet parked (still running, or still spinning in the
    /// lock's adaptive phase), there is no handoff, and the test goes red on
    /// a correct lock. It cannot turn a barging lock green.
    #[test]
    fn a_released_stream_lock_goes_to_the_parked_submitter_not_back_to_the_releaser() {
        use std::net::TcpListener;
        use std::sync::mpsc;

        const ITERATIONS: usize = 10;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel::<()>();
        let server = thread::spawn(move || {
            // Accept and hold the socket open, silent, for the whole test.
            let (_sock, _) = listener.accept().expect("accept");
            let _ = rx.recv();
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener");

        for i in 0..ITERATIONS {
            let guard = conn.stream.lock();

            let submitter_conn = Arc::clone(&conn);
            let submitter = thread::spawn(move || {
                submitter_conn.submit_share("j", &format!("{:08x}", i), &"a".repeat(64))
            });

            let deadline = Instant::now() + Duration::from_secs(5);
            while conn.submits_in_flight.load(Ordering::SeqCst) == 0
                && Instant::now() < deadline
            {
                thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(
                conn.submits_in_flight.load(Ordering::SeqCst),
                1,
                "iteration {i}: the submitter never entered submit_share"
            );
            // Settle: from entry to parked on the stream lock is a session-id
            // read and a JSON build — microseconds. 100ms is a wide margin.
            thread::sleep(Duration::from_millis(100));
            assert_eq!(
                conn.pending_shares.lock().unwrap().len(),
                i,
                "iteration {i}: the submitter registered without the stream lock"
            );

            drop(guard);
            let guard = conn.stream.lock();
            assert_eq!(
                conn.pending_shares.lock().unwrap().len(),
                i + 1,
                "iteration {i}: the released stream lock went back to the releasing \
                 thread instead of the submitter parked on it — the barging that lets \
                 receiver_loop starve a share submission (#44)"
            );
            drop(guard);

            submitter
                .join()
                .expect("submitter thread")
                .expect("submit_share must succeed against a connected stream");
        }

        assert_eq!(conn.pending_shares.lock().unwrap().len(), ITERATIONS);
        let _ = tx.send(());
        let _ = server.join();
    }

    /// Control for the reproducer above: same shape, but the server writes a
    /// keepalive-shaped line every ~100ms instead of staying quiet for 3s. If
    /// #40 is specifically about a *quiet* connection — the receiver blocked
    /// in its read for the full `RECV_POLL_INTERVAL` with nothing to do other
    /// than re-contend for the lock — a chatty connection should behave
    /// differently, since each incoming line gives the receiver (and hence
    /// the lock) something else to do between read attempts. Same assertion
    /// as T1; this one is expected to stay fast.
    ///
    /// `#[ignore]`d because it gates nothing: it was green both before and
    /// after the #44 fair-lock fix, so it cannot catch a regression. Kept as a
    /// diagnostic. Run manually with:
    /// `rtk proxy cargo test --release --lib -- --ignored --nocapture control_a_chatty`
    #[test]
    #[ignore = "diagnostic control for #44's reproducer; green before and after the \
                fix, so it gates nothing"]
    fn control_a_chatty_receiver_lets_a_submit_through() {
        use std::io::Write as _;
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel::<()>();

        let server = thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            loop {
                let _ = sock.write_all(
                    b"{\"id\":999999,\"result\":{\"status\":\"KEEPALIVED\"}}\n",
                );
                let _ = sock.flush();
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    _ => break,
                }
            }
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.silence_timeout_ms.store(60_000, Ordering::Relaxed);
        conn.connect(&addr).expect("connect to the local listener");
        *conn.address.lock().unwrap() = addr.clone();
        *conn.wallet.lock().unwrap() = "4test".to_string();

        let worker = conn.clone();
        thread::spawn(move || worker.receiver_loop());
        thread::sleep(Duration::from_millis(200));

        let start = Instant::now();
        let result = conn.submit_share("j", "deadbeef", &"a".repeat(64));
        let elapsed = start.elapsed();
        eprintln!("control_a_chatty_receiver_lets_a_submit_through: submit_share took {:?}", elapsed);
        result.expect("submit_share must succeed against a connected stream");

        assert!(
            elapsed < Duration::from_millis(500),
            "submit took {:?}, expected well under 500ms — a chatty connection \
             should not starve the lock",
            elapsed
        );

        let _ = tx.send(());
        let _ = server.join();
    }

    /// A local listener accepts, then sends a megabyte and a half with no
    /// newline. If the buffer is bounded, the loop gives up on the stream and
    /// calls `reconnect`, which the listener observes as a **second accept**. If
    /// it is unbounded, the loop simply keeps buffering and no second connection
    /// ever arrives.
    ///
    /// Hermetic: `127.0.0.1` on an ephemeral port — which is not in `TLS_PORTS`,
    /// so this is plain TCP and no certificate is involved.
    #[test]
    fn the_receiver_loop_really_drops_a_newline_free_stream() {
        use std::io::Write as _;
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel::<u8>();

        let server = thread::spawn(move || {
            let mut held = Vec::new(); // keep sockets alive — see below
            for (n, incoming) in listener.incoming().enumerate() {
                let mut sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let _ = tx.send(n as u8);
                if n == 0 {
                    // Flood: no newline, comfortably past the limit.
                    let junk = vec![b'x'; 64 * 1024];
                    for _ in 0..24 {
                        if sock.write_all(&junk).is_err() {
                            break;
                        }
                    }
                    let _ = sock.flush();
                    // CRITICAL: hold this socket open. The first version of this
                    // test let it drop here, which closed the connection — the
                    // miner then saw EOF and reconnected, so the second accept
                    // arrived for a reason with nothing to do with the buffer
                    // bound. It passed against the unbounded implementation.
                    // Now the only way a second connection happens is if the
                    // *miner* gives up on the stream.
                    held.push(sock);
                } else {
                    return;
                }
            }
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener");
        // reconnect() needs both recorded, or it bails without retrying.
        *conn.address.lock().unwrap() = addr.clone();
        *conn.wallet.lock().unwrap() = "4test".to_string();

        let worker = conn.clone();
        thread::spawn(move || worker.receiver_loop());

        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(0), "first connection");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(30)),
            Ok(1),
            "the loop must drop a newline-free flood and reconnect; no second connection \
             means it is still buffering, which is the defect this guards"
        );
        let _ = server.join();
    }

    /// F1 from review: `pending.clear()` in the overflow arm had **no coverage**
    /// — removing it shipped green.
    ///
    /// Review predicted the consequence would be an infinite reconnect loop.
    /// **That is not what happens, and this test was rewritten twice before it
    /// measured anything.** Without the clear, the stale flood survives the
    /// reconnect; the next read appends data that *does* contain a newline, so
    /// `take_complete_lines` drains the whole megabyte-plus-message as a single
    /// bogus line and the buffer self-clears. No second overflow, no loop.
    ///
    /// What is actually lost is that first real message: it arrives concatenated
    /// onto a megabyte of `x`, cannot parse, and is silently swallowed. So the
    /// observable is not "does it reconnect again" but **"does the first job
    /// after a flood survive"** — asserted here through `get_work()`.
    #[test]
    fn the_first_job_after_a_flood_is_not_swallowed_by_the_stale_buffer() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel::<u8>();

        let server = thread::spawn(move || {
            let mut held = Vec::new();

            // Flood, holding the socket open so the miner's own bound is the
            // only thing that can end this connection.
            let (mut sock, _) = listener.accept().expect("first accept");
            let _ = tx.send(0);
            let junk = vec![b'x'; 64 * 1024];
            for _ in 0..24 {
                if sock.write_all(&junk).is_err() {
                    break;
                }
            }
            let _ = sock.flush();
            held.push(sock);

            // The reconnect: answer the login, then send one real job.
            let (mut sock, _) = listener.accept().expect("second accept");
            let _ = tx.send(1);
            let mut req = [0u8; 4096];
            let _ = sock.read(&mut req);
            let _ = sock.write_all(
                b"{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":{\"id\":\"sess\",\"status\":\"OK\"}}\n",
            );
            let _ = sock.write_all(
                b"{\"jsonrpc\":\"2.0\",\"method\":\"job\",\"params\":{\"blob\":\"0f0f\",\
                  \"target\":\"ffffffff\",\"job_id\":\"after-flood\",\"seed_hash\":\"abcd\"}}\n",
            );
            let _ = sock.flush();
            thread::sleep(Duration::from_secs(3));
            held.push(sock);
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener");
        *conn.address.lock().unwrap() = addr.clone();
        *conn.wallet.lock().unwrap() = "4test".to_string();

        let worker = conn.clone();
        thread::spawn(move || worker.receiver_loop());

        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(0), "first connection");
        assert_eq!(rx.recv_timeout(Duration::from_secs(30)), Ok(1), "the flood is dropped");

        // Give the job time to arrive and be applied.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = None;
        while Instant::now() < deadline {
            if let Some(job) = conn.get_work() {
                seen = Some(job.job_id.clone());
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            seen.as_deref(),
            Some("after-flood"),
            "the first job after a flood must arrive intact; if the oversized buffer was not \
             cleared it is prepended to this message, which then cannot parse and is lost"
        );
        let _ = server.join();
    }

    // --- GitHub #21: the receive buffer must be bounded -------------------

    #[test]
    fn splitting_a_message_across_reads_still_parses() {
        // The behaviour the limit must not break. Stratum messages routinely
        // arrive in pieces, so a naive "reject anything without a newline"
        // would break normal mining.
        let mut pending = Vec::new();
        pending.extend_from_slice(b"{\"id\":1,");
        assert_eq!(take_complete_lines(&mut pending).unwrap(), Vec::<String>::new());
        pending.extend_from_slice(b"\"x\":2}\n");
        assert_eq!(
            take_complete_lines(&mut pending).unwrap(),
            vec!["{\"id\":1,\"x\":2}".to_string()]
        );
        assert!(pending.is_empty(), "a consumed line must leave nothing behind");
    }

    #[test]
    fn several_lines_in_one_read_all_come_out_in_order() {
        let mut pending = b"one\ntwo\nthree\n".to_vec();
        assert_eq!(take_complete_lines(&mut pending).unwrap(), vec!["one", "two", "three"]);
        assert!(pending.is_empty());
    }

    #[test]
    fn a_trailing_partial_line_is_kept_for_the_next_read() {
        let mut pending = b"done\npartial".to_vec();
        assert_eq!(take_complete_lines(&mut pending).unwrap(), vec!["done"]);
        assert_eq!(pending, b"partial", "the unfinished line must survive");
    }

    /// The regression. A peer that never sends a newline must be cut off rather
    /// than buffered forever.
    #[test]
    fn a_newline_free_stream_is_refused_instead_of_buffered() {
        let mut pending = Vec::new();
        let chunk = vec![b'x'; 4096]; // the real read size
        // Bounded by an ABSOLUTE byte count, not by `MAX_LINE_BYTES`.
        //
        // The first attempt at this cap was `(MAX_LINE_BYTES * 2) / chunk.len()`,
        // which is derived from the very quantity a "raise the limit" mutation
        // moves — so the cap scaled with the mutation, the `panic!` below became
        // unreachable, and the test *passed* rather than failing. Measured at
        // 16x: passes in 18.3 s, still O(n^2). The audit entry recorded that cap
        // as fixing the problem; it did not, and round 2 caught it.
        //
        // Why it needs bounding at all: with an open `loop`, raising the limit
        // makes this spin, rescanning an ever-growing buffer for a newline that
        // never comes, and exhaust memory instead of failing. On the 7 GB
        // `macos-14` runner that is an OOM rather than a verdict.
        //
        // 4 MiB is four times the real limit and independent of it, so a
        // sufficiently raised limit runs out of iterations and fails cleanly.
        // "Sufficiently" is the honest word: measured in release, 1x passes in
        // 0.06 s and 2x still *passes* in 0.18 s (the buffer overflows within
        // the 1024 iterations), while 4x, 16x and 1024x all fail in 0.66 s. So
        // this test's detection threshold is 4x, and the mutation at 2x is
        // caught by the two socket tests instead, not here.
        const FEED_CEILING_BYTES: usize = 4 * 1024 * 1024;
        let max_iterations = FEED_CEILING_BYTES / chunk.len();
        for _ in 0..max_iterations {
            pending.extend_from_slice(&chunk);
            match take_complete_lines(&mut pending) {
                Ok(_) => assert!(
                    pending.len() <= MAX_LINE_BYTES,
                    "buffer grew past the limit without being refused: {} bytes",
                    pending.len()
                ),
                Err(overflow) => {
                    assert!(overflow > MAX_LINE_BYTES);
                    // The caller clears and reconnects; the point is that it is
                    // told to, rather than the allocation continuing.
                    return;
                }
            }
        }
        panic!(
            "fed {} bytes without being refused; the limit is not being enforced",
            max_iterations * chunk.len()
        );
    }

    #[test]
    fn the_limit_is_exact_and_not_off_by_one() {
        // At the limit: still acceptable, because a legitimate message could be
        // exactly this long and its newline may be in the next read.
        let mut at = vec![b'x'; MAX_LINE_BYTES];
        assert!(take_complete_lines(&mut at).is_ok(), "exactly the limit must be allowed");
        // One byte over: refused.
        let mut over = vec![b'x'; MAX_LINE_BYTES + 1];
        assert_eq!(take_complete_lines(&mut over), Err(MAX_LINE_BYTES + 1));
    }

    /// A huge *complete* message is fine — the limit is on an unfinished line,
    /// not on throughput. Draining first is what makes this true.
    #[test]
    fn a_large_but_terminated_message_is_accepted() {
        let mut pending = vec![b'x'; MAX_LINE_BYTES];
        pending.push(b'\n');
        pending.extend_from_slice(b"next");
        let lines = take_complete_lines(&mut pending).expect("a terminated line is not an overflow");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), MAX_LINE_BYTES);
        assert_eq!(pending, b"next");
    }

    /// What happens to a good line that arrives immediately before a flood: it
    /// is **dropped**, because the overflow is reported instead of the drained
    /// lines. That is deliberate — the caller is about to clear the buffer and
    /// reconnect, so delivering one message from a peer being disconnected for
    /// protocol abuse buys nothing and complicates the contract.
    ///
    /// The property that matters is the one this does guarantee: nothing
    /// *partial* is ever delivered, so an oversized message cannot leave a
    /// half-applied job. `handle_pool_message` only ever sees whole lines.
    #[test]
    fn a_good_line_before_a_flood_is_dropped_with_the_flood() {
        let mut pending = b"{\"job_id\":\"real\"}\n".to_vec();
        pending.extend_from_slice(&vec![b'x'; MAX_LINE_BYTES + 1]);
        assert!(
            take_complete_lines(&mut pending).is_err(),
            "the oversized remainder must be reported even though a good line preceded it"
        );
    }

    #[test]
    fn parses_a_plain_hex_fingerprint() {
        let fp = parse_cert_fingerprint(SAMPLE).expect("should parse");
        assert_eq!(hex_encode(&fp), SAMPLE);
    }

    #[test]
    fn accepts_the_colon_separated_form_openssl_prints() {
        // `openssl x509 -fingerprint` emits AA:BB:CC..., and operators paste it
        // verbatim. Rejecting that would send them to a text editor for no
        // reason.
        let colons = SAMPLE
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(":");
        assert_eq!(parse_cert_fingerprint(&colons), parse_cert_fingerprint(SAMPLE));
    }

    #[test]
    fn is_case_insensitive() {
        assert_eq!(
            parse_cert_fingerprint(&SAMPLE.to_uppercase()),
            parse_cert_fingerprint(SAMPLE)
        );
    }

    #[test]
    fn rejects_wrong_length_and_non_hex() {
        // Truncation is the realistic paste error, and a short pin that silently
        // "worked" would pin nothing.
        // Exercise the length guard across the boundary. Review found the old
        // version still passed with `!= 64` loosened to `< 2`, because
        // `try_into()` was doing the real work — so the test named a guard it
        // was not actually testing.
        for n in [0usize, 1, 2, 30, 62, 63, 65, 66, 128] {
            let candidate: String = SAMPLE.chars().cycle().take(n).collect();
            assert!(
                parse_cert_fingerprint(&candidate).is_none(),
                "{n} hex characters must be rejected; only 64 is a SHA-256"
            );
        }
        assert!(parse_cert_fingerprint(SAMPLE).is_some(), "64 must still be accepted");
        // Non-ASCII must parse-fail, not panic: hex_decode used to slice on a
        // byte index that could land inside a multi-byte character.
        assert!(parse_cert_fingerprint(&format!("{}€", &SAMPLE[..61])).is_none());
        let mut bad = SAMPLE.to_string();
        bad.replace_range(0..1, "z");
        assert!(parse_cert_fingerprint(&bad).is_none());
    }

    /// Drive a **real TLS handshake** against the config the connection actually
    /// uses, in memory — no sockets, no ports, no network.
    ///
    /// Two weaker attempts did not hold. Round 1 found that swapping the default
    /// verifier for accept-anything left the suite green. The fix tested
    /// `server_verifier(None)` — a helper — and round 2 showed the same mutation
    /// applied to the *wiring* still passed. Collapsing the two call sites into
    /// one did not close it either: a mutation at the call site simply bypasses
    /// the helper the test holds.
    ///
    /// No structural trick can close that gap, because nothing can inspect a
    /// built `ClientConfig` to learn what it will accept. Only exercising it
    /// can.
    fn handshake_against_self_signed(fingerprint: Option<CertFingerprint>) -> Result<(), rustls::Error> {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};

        let cert = CertificateDer::from(
            include_bytes!("../tests/fixtures/selfsigned-mining-pool.crt.der").to_vec(),
        );
        let key = PrivateKeyDer::try_from(
            include_bytes!("../tests/fixtures/selfsigned-mining-pool.key.der").to_vec(),
        )
        .expect("fixture key must parse");

        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("fixture cert/key must load");
        let mut server = rustls::ServerConnection::new(Arc::new(server_config)).unwrap();

        // Build the client from the connection's OWN config, so this tests what
        // `PoolConnection` will really use rather than a parallel construction.
        let conn = PoolConnection::with_tls_fingerprint(
            crate::donate::DEFAULT_DONATE_LEVEL,
            fingerprint,
        );
        let name = rustls::pki_types::ServerName::try_from("mining.pool").unwrap();
        let mut client = rustls::ClientConnection::new(conn.tls_config.clone(), name).unwrap();

        // Pump bytes between the two in memory until the handshake settles.
        for _ in 0..16 {
            let mut buf = Vec::new();
            client.write_tls(&mut buf).ok();
            if !buf.is_empty() {
                server.read_tls(&mut buf.as_slice()).ok();
                server.process_new_packets().map_err(|e| rustls::Error::General(e.to_string()))?;
            }
            let mut buf = Vec::new();
            server.write_tls(&mut buf).ok();
            if !buf.is_empty() {
                client.read_tls(&mut buf.as_slice()).ok();
                // This is the call that runs the certificate verifier.
                client.process_new_packets()?;
            }
            if !client.is_handshaking() {
                return Ok(());
            }
        }
        Err(rustls::Error::General("handshake did not complete".into()))
    }

    #[test]
    fn by_default_a_self_signed_pool_certificate_is_rejected_in_a_real_handshake() {
        let err = handshake_against_self_signed(None)
            .expect_err("the default must reject a self-signed certificate");
        // Any rejection is the point; naming it keeps the failure legible.
        assert!(
            format!("{err:?}").contains("Certificate") || format!("{err:?}").contains("Invalid"),
            "expected a certificate rejection, got: {err:?}"
        );
    }

    #[test]
    fn the_pinned_certificate_completes_a_real_handshake() {
        let pin = parse_cert_fingerprint(
            "bdd5fe3031d2729cb79dc027441988f7b4c6a2c0258e0d382f7eb1a308890c7d",
        )
        .unwrap();
        handshake_against_self_signed(Some(pin))
            .expect("the pinned certificate must be accepted — this is the whole point of a pin");
    }

    #[test]
    fn a_wrong_pin_fails_a_real_handshake() {
        let pin = parse_cert_fingerprint(SAMPLE).unwrap();
        handshake_against_self_signed(Some(pin))
            .expect_err("a certificate that is not the pinned one must be rejected");
    }

    /// A cheap companion to the handshake tests: the default verifier must reject
    /// input it cannot validate at all. Kept because it fails fast and names the
    /// property in one line; the handshake tests are what actually prove the
    /// wiring.
    #[test]
    fn the_default_verifier_rejects_what_it_cannot_validate() {
        let verifier = server_verifier(None);
        let der = rustls::pki_types::CertificateDer::from(vec![0u8; 64]);
        let name = rustls::pki_types::ServerName::try_from("pool.supportxmr.com").unwrap();
        assert!(
            verifier
                .verify_server_cert(&der, &[], &name, &[], rustls::pki_types::UnixTime::now())
                .is_err(),
            "the default verifier accepted a certificate it cannot validate — TLS would be \
             encrypted but unauthenticated, which is the defect SEC-02 exists to fix"
        );
    }

    #[test]
    fn a_pin_and_a_plaintext_port_is_refused_rather_than_silently_ignored() {
        // gulf.moneroocean.stream:20128 speaks TLS but is not in TLS_PORTS, so
        // the pin was accepted, announced in the log, and then ignored while the
        // connection went out in plaintext. Refusing is the only honest
        // outcome: the alternative tells the operator they are authenticated
        // when nothing is.
        let conn = PoolConnection::with_tls_fingerprint(
            crate::donate::DEFAULT_DONATE_LEVEL,
            parse_cert_fingerprint(SAMPLE),
        );
        let err = conn
            .connect("gulf.moneroocean.stream:20128")
            .expect_err("a pinned connection to a non-TLS port must not succeed");
        assert!(
            err.contains("pinned") && err.contains("plaintext"),
            "the error must explain why, got: {err}"
        );
    }

    #[test]
    fn the_default_configuration_verifies_certificates() {
        // The regression this guards: for the project's whole life the default
        // was `NoVerifier`, which accepted every certificate. Building the
        // verifier proves the webpki root store is present and usable, so a
        // default connection has something to check against.
        let verifier = webpki_verifier().expect("webpki verifier must build");
        assert!(
            !verifier.supported_verify_schemes().is_empty(),
            "a verifier with no signature schemes would accept nothing and is not a working default"
        );
    }

    #[test]
    fn a_pinned_verifier_rejects_a_certificate_that_is_not_the_pinned_one() {
        let verifier = PinnedCertVerifier {
            inner: webpki_verifier().unwrap(),
            expected: parse_cert_fingerprint(SAMPLE).unwrap(),
        };
        // Any DER that is not the pinned certificate must fail. The point of the
        // pin is that a *substituted* certificate is detected, which is exactly
        // what a blanket bypass cannot do.
        let other = rustls::pki_types::CertificateDer::from(vec![0u8; 64]);
        let name = rustls::pki_types::ServerName::try_from("monerohash.com").unwrap();
        let result = verifier.verify_server_cert(
            &other,
            &[],
            &name,
            &[],
            rustls::pki_types::UnixTime::now(),
        );
        assert!(result.is_err(), "a non-matching certificate must be rejected");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("pinned fingerprint"),
            "the error should say why it failed, got: {msg}"
        );
    }

    #[test]
    fn a_pinned_verifier_accepts_exactly_the_pinned_certificate() {
        // Pin whatever this DER hashes to, then present that same DER.
        let der = rustls::pki_types::CertificateDer::from(vec![7u8; 128]);
        let digest = ring::digest::digest(&ring::digest::SHA256, der.as_ref());
        let expected: CertFingerprint = digest.as_ref().try_into().unwrap();
        let verifier = PinnedCertVerifier {
            inner: webpki_verifier().unwrap(),
            expected,
        };
        let name = rustls::pki_types::ServerName::try_from("mining.pool").unwrap();
        assert!(
            verifier
                .verify_server_cert(&der, &[], &name, &[], rustls::pki_types::UnixTime::now())
                .is_ok(),
            "the pinned certificate itself must be accepted, self-signed or not"
        );
    }

    // --- GitHub #17: pair share responses by JSON-RPC id -------------------

    fn pending_share(job_id: &str, nonce: &str) -> PendingShare {
        PendingShare {
            job_id: job_id.to_string(),
            nonce: nonce.to_string(),
            sent_at: Instant::now(),
        }
    }

    #[test]
    fn a_share_response_with_a_pending_id_is_accepted_and_the_entry_removed() {
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        conn.pending_shares
            .lock()
            .unwrap()
            .insert(7, pending_share("job-a", "aabbccdd"));

        conn.handle_pool_message(
            r#"{"id":7,"jsonrpc":"2.0","result":{"status":"OK"}}"#,
        );

        assert_eq!(conn.get_accepted_shares(), 1, "the accepted counter must increment");
        assert_eq!(conn.get_rejected_shares(), 0);
        assert!(
            !conn.pending_shares.lock().unwrap().contains_key(&7),
            "the entry must be removed once its response arrives"
        );
    }

    /// Many pools answer an accepted share with `"error": null` alongside the
    /// result. That is a success, and must not be read as a rejection just
    /// because an `error` key is present. Round 2's mutation run showed this
    /// was handled correctly but untested: deleting the `!` in
    /// `!error.is_null()` left the suite green.
    #[test]
    fn a_success_reply_carrying_error_null_is_accepted_not_rejected() {
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        conn.pending_shares
            .lock()
            .unwrap()
            .insert(9, pending_share("job-n", "01020304"));

        conn.handle_pool_message(
            r#"{"id":9,"jsonrpc":"2.0","error":null,"result":{"status":"OK"}}"#,
        );

        assert_eq!(conn.get_accepted_shares(), 1, "error:null with status OK is an accept");
        assert_eq!(conn.get_rejected_shares(), 0, "error:null must not count as a rejection");
    }

    #[test]
    fn an_error_response_with_a_pending_id_is_rejected() {
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        conn.pending_shares
            .lock()
            .unwrap()
            .insert(9, pending_share("job-b", "11223344"));

        conn.handle_pool_message(
            r#"{"id":9,"jsonrpc":"2.0","error":{"code":-1,"message":"Low difficulty share"}}"#,
        );

        assert_eq!(conn.get_rejected_shares(), 1, "the rejected counter must increment");
        assert_eq!(conn.get_accepted_shares(), 0);
        assert!(!conn.pending_shares.lock().unwrap().contains_key(&9));
    }

    /// The latent bug from GitHub #17: a keepalive is written through
    /// `send_message`, which never registers a `pending_shares` entry — its id
    /// is never a share's id. Before pairing by id, `handle_pool_message`
    /// treated *any* id-bearing, method-less message as a share response, so
    /// an errored keepalive answer was counted as a rejected share.
    ///
    /// This must FAIL on that old behaviour: break-tested by temporarily
    /// restoring "any id without method is a share response" (dropping the
    /// pending-map lookup) and confirming this assertion goes red before
    /// restoring the real code.
    #[test]
    fn an_error_response_whose_id_is_not_pending_does_not_count_as_a_rejected_share() {
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        // No insert into pending_shares: id 42 was never a submission — this is
        // the shape of an errored keepalive response.

        conn.handle_pool_message(
            r#"{"id":42,"jsonrpc":"2.0","error":{"code":-1,"message":"keepalive not recognised"}}"#,
        );

        assert_eq!(
            conn.get_rejected_shares(),
            0,
            "a response whose id was never a pending share submission (e.g. a keepalive) \
             must not be counted as a rejected share"
        );
        assert_eq!(conn.get_accepted_shares(), 0);
    }

    #[test]
    fn draining_pending_shares_on_reconnect_counts_and_empties_them() {
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        {
            let mut pending = conn.pending_shares.lock().unwrap();
            pending.insert(1, pending_share("job-c", "00000001"));
            pending.insert(2, pending_share("job-c", "00000002"));
            pending.insert(3, pending_share("job-d", "00000003"));
        }

        // `drain_pending_shares` is the exact call `reconnect()` and
        // `relogin_as()` make before tearing down the stream — see those two
        // functions in `pool_connection.rs`. Driving the real `reconnect()`
        // here would require a live socket loop; that wiring is verified by
        // reading the two call sites, not by this test.
        conn.drain_pending_shares();

        assert_eq!(conn.get_lost_shares(), 3, "every drained entry must be counted as lost");
        assert!(
            conn.pending_shares.lock().unwrap().is_empty(),
            "the pending map must be empty after draining"
        );
    }

    /// A response whose id matches but is neither `error` nor
    /// `result.status == "OK"` — a shape no known pool sends, but one the
    /// accounting must not lose silently now that a matched id always
    /// removes its `pending_shares` entry.
    #[test]
    fn a_result_with_an_unrecognised_status_is_counted_rejected_not_dropped() {
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        conn.pending_shares
            .lock()
            .unwrap()
            .insert(11, pending_share("job-e", "55667788"));

        conn.handle_pool_message(
            r#"{"id":11,"jsonrpc":"2.0","result":{"status":"WEIRD"}}"#,
        );

        assert_eq!(
            conn.get_rejected_shares(),
            1,
            "an unrecognised status must be counted, not silently dropped"
        );
        assert_eq!(conn.get_accepted_shares(), 0);
        assert!(!conn.pending_shares.lock().unwrap().contains_key(&11));
    }

    // --- submit_share itself (review round 2 of #17/PR #36) ----------------
    //
    // The four tests above all construct a `PendingShare` by hand and drive
    // `handle_pool_message`/`drain_pending_shares` directly — none of them
    // calls `submit_share`, so none of them could ever notice a defect in
    // how it registers or writes a submission. Mutation testing confirmed
    // the gap: `./scripts/mutants.sh 'submit_share|send_message_with_id'` (a
    // helper since folded into `send_message`)
    // with `'pool_connection::'` found 3 MISSED mutants that gutted `submit_share`
    // and that helper to a no-op `Ok(...)` — i.e. nothing written
    // to the socket, no `pending_shares` entry ever created — and the full
    // suite stayed green. These tests close that: they call `submit_share`
    // itself, against a real local socket.

    #[test]
    fn submit_share_registers_the_same_id_it_writes_to_the_wire() {
        use std::io::Read as _;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();

        let server = thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            // Bounded, not blocking forever: a `submit_share` that silently
            // writes nothing must fail this test promptly, not hang the
            // whole suite. Mutation testing found exactly this gap.
            sock.set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set_read_timeout");
            let mut buf = [0u8; 4096];
            let n = sock
                .read(&mut buf)
                .expect("read (or a timeout: submit_share wrote nothing to the socket)");
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });

        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        conn.connect(&addr).expect("connect to the local listener");

        conn.submit_share("job-wire", "deadbeef", &"a".repeat(64))
            .expect("submit_share must succeed against a connected stream");

        let line = server.join().expect("server thread panicked");
        let msg: Value =
            serde_json::from_str(line.trim()).expect("the line on the wire must be valid JSON");
        assert_eq!(
            msg.get("method").and_then(|m| m.as_str()),
            Some("submit"),
            "submit_share must write a \"submit\" request"
        );
        let wire_id = msg
            .get("id")
            .and_then(|v| v.as_u64())
            .expect("the request must carry a numeric id");

        let pending = conn.pending_shares.lock().unwrap();
        let entry = pending.get(&wire_id).unwrap_or_else(|| {
            panic!(
                "pending_shares must contain the exact id written to the wire ({}); has: {:?}",
                wire_id,
                pending.keys().collect::<Vec<_>>()
            )
        });
        assert_eq!(entry.job_id, "job-wire");
        assert_eq!(entry.nonce, "deadbeef");
    }

    /// `send_message` is the write path keepalives take. Mutation testing
    /// showed it could be replaced by a no-op `Ok(())` with the whole suite
    /// still green — nothing checked that a keepalive actually reaches the
    /// wire, and a silent keepalive is half of what #34 was about.
    #[test]
    fn send_message_writes_a_request_to_the_wire() {
        use std::io::{BufRead as _, BufReader};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let server = thread::spawn(move || {
            let (sock, _) = listener.accept().expect("accept");
            sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut line = String::new();
            BufReader::new(sock).read_line(&mut line).expect("a request line");
            line
        });

        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        conn.connect(&addr).expect("connect to the local listener");
        conn.send_message("keepalived", serde_json::json!({ "id": "sess" }))
            .expect("write");

        let line = server.join().expect("server thread");
        let v: serde_json::Value = serde_json::from_str(line.trim()).expect("valid JSON");
        assert_eq!(v["method"], "keepalived", "the request must reach the wire: {line}");
        assert!(v["id"].is_u64(), "every request carries a numeric JSON-RPC id: {line}");
    }

    #[test]
    fn submit_share_write_failure_leaves_nothing_pending() {
        // Never connected — `PoolConnection::new` alone, no `connect()` — so
        // the write inside `submit_share` must fail with no stream to write
        // to.
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);

        let err = conn
            .submit_share("job-fail", "cafebabe", &"b".repeat(64))
            .expect_err("submit_share must fail when there is no stream to write to");
        assert!(
            err.contains("Not connected"),
            "unexpected error: {err}"
        );
        assert!(
            conn.pending_shares.lock().unwrap().is_empty(),
            "a submission that never left the machine must not leave an orphan entry \
             in pending_shares"
        );
        assert_eq!(conn.get_unsent_shares(), 1);
        assert_eq!(conn.get_lost_shares(), 0);
    }

    /// The complement of the test below: this one pins round 1's ordering.
    /// Round 1 found that writing, releasing the stream lock, and only then
    /// registering let the receiver handle the reply in the gap. So registration
    /// must happen while the stream lock is still held. Hold `pending_shares`
    /// so the submission blocks exactly at its insert, wait until the request
    /// has reached the wire, and check the stream is still locked at that
    /// moment. Under round 1's ordering the stream is already released there.
    /// (Approach from review round 3, which prototyped it; R3-3.)
    #[test]
    fn a_submission_still_holds_the_stream_while_it_registers() {
        use std::io::{BufRead as _, BufReader};
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (read_tx, read_rx) = mpsc::channel::<()>();
        let _server = thread::spawn(move || {
            let (sock, _) = listener.accept().expect("accept");
            sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut line = String::new();
            let _ = BufReader::new(sock).read_line(&mut line);
            let _ = read_tx.send(());
            thread::sleep(Duration::from_secs(2));
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener");

        let pending_guard = conn.pending_shares.lock().unwrap();
        let submitter_conn = Arc::clone(&conn);
        let submitter =
            thread::spawn(move || submitter_conn.submit_share("j1", "00000000", "ff"));

        read_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the submission should reach the wire");
        thread::sleep(Duration::from_millis(100)); // let it arrive at the insert

        assert!(
            conn.stream.try_lock().is_none(),
            "the stream must still be locked while the share is being registered — \
             releasing it first lets the receiver handle the reply before the entry \
             exists, so it goes uncounted and the share is later counted lost"
        );

        drop(pending_guard);
        submitter.join().expect("submitter thread").expect("submit succeeds");
        assert_eq!(conn.pending_shares.lock().unwrap().len(), 1);
    }

    /// Round 2 of review found that registering a share *before* writing it
    /// let a reconnect drain the entry as "lost" in the gap, after which the
    /// write went out on the new connection with nothing left to pair its
    /// reply with. The fix does write-and-register as one step under the
    /// stream lock — which, unlike the earlier orderings, can be tested
    /// deterministically: stand in for `reconnect()` by holding that lock,
    /// start a submission that has to block on it, and check nothing is
    /// registered while it waits. Then null the stream, as a reconnect does.
    ///
    /// Under the round-2 ordering the entry exists during the wait and the
    /// first assertion fails. (Round 1's race — a reply handled between the
    /// write and the insert — is closed by the same lock, since the receiver
    /// reads under it, but is not what this test exercises.)
    ///
    /// An earlier attempt at an ordering test was dropped for good reason: it
    /// raced a real socket round trip against a nanosecond gap and passed on
    /// both orderings. Holding the lock removes the timing from the question.
    #[test]
    fn a_submission_blocked_on_the_stream_registers_nothing_until_it_writes() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let _server = thread::spawn(move || {
            let _held = listener.accept();
            thread::sleep(Duration::from_secs(3));
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener");

        // Stand in for reconnect(): it must hold this lock to null the stream.
        let mut guard = conn.stream.lock();
        let submitter_conn = Arc::clone(&conn);
        let submitter =
            thread::spawn(move || submitter_conn.submit_share("j1", "00000000", "ff"));
        thread::sleep(Duration::from_millis(200));

        assert!(
            conn.pending_shares.lock().unwrap().is_empty(),
            "a submission must not be registered before it can be written — \
             otherwise a reconnect in the gap drains it as lost and the write \
             then lands on the new connection with nothing to pair its reply with"
        );

        *guard = None; // ...the reconnect nulls the stream,
        drop(guard);
        conn.drain_pending_shares(); // ...and drains.

        assert!(
            submitter.join().expect("submitter thread").is_err(),
            "with the stream gone the submission must fail, not be written anywhere"
        );
        assert!(conn.pending_shares.lock().unwrap().is_empty());
        assert_eq!(
            conn.lost_shares.load(Ordering::Relaxed),
            0,
            "a share that was never written must not be counted lost"
        );
        assert_eq!(
            conn.get_unsent_shares(),
            1,
            "a never-written share is counted unsent, not lost (#32)"
        );
    }

    /// Complement of `submit_share_write_failure_leaves_nothing_pending`: there
    /// the stream was never established at all, so the failure is "Not
    /// connected". Here a real connection is made and then its write half is
    /// shut down, so the write itself fails partway through
    /// `write_and_register` — a different code path to the same `unsent`
    /// counter (#32).
    #[test]
    fn a_write_failure_on_an_established_stream_counts_as_unsent() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let _server = thread::spawn(move || {
            let _held = listener.accept();
            thread::sleep(Duration::from_secs(3));
        });

        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        conn.connect(&addr).expect("connect to the local listener");

        // Shut down the write half of the underlying TCP stream so the next
        // write fails, without touching the mutex (submit_share needs to lock
        // it itself).
        conn.stream
            .lock()
            .as_ref()
            .unwrap()
            .tcp()
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown write half");

        let err = conn
            .submit_share("j1", "00000000", &"c".repeat(64))
            .expect_err("a write to a write-shutdown socket must fail");
        assert!(
            err.contains("Write failed") || err.contains("Flush failed"),
            "unexpected error: {err}"
        );
        assert_eq!(conn.get_unsent_shares(), 1);
        assert_eq!(conn.get_lost_shares(), 0);
        assert!(
            conn.pending_shares.lock().unwrap().is_empty(),
            "a write that failed must not leave an entry registered"
        );
    }

    /// Pure unit test on the free function: no sockets, no threads. Pins the
    /// exact boundary condition (`waited >= limit`, not `>`) and that either
    /// `in_flight` or `pending` alone blocks the gate (#32).
    #[test]
    fn rotation_may_proceed_truth_table() {
        let l = Duration::from_secs(1);
        assert!(rotation_may_proceed(0, 0, Duration::ZERO, l));
        assert!(!rotation_may_proceed(1, 0, Duration::ZERO, l));
        assert!(!rotation_may_proceed(0, 1, Duration::ZERO, l));
        assert!(rotation_may_proceed(1, 1, l, l), "exact boundary must count as settled");
        assert!(!rotation_may_proceed(1, 0, l - Duration::from_millis(1), l));
    }

    /// Direct, deterministic unit-level test on `rotation_settled` itself: no
    /// sockets, no timing races. Exercises the exact line a mutation could
    /// replace with a hardcoded `0u32` (`submits_in_flight.load(...)`) and
    /// pins that the gate defers while a submission is outstanding (#32).
    #[test]
    fn rotation_settled_defers_while_a_submission_is_in_flight() {
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        conn.submits_in_flight.fetch_add(1, Ordering::SeqCst);

        let proceeded = conn.rotation_settled(Beneficiary::Author);

        assert!(
            !proceeded,
            "a rotation must defer while a submission is in flight, not proceed \
             immediately and leave the share to be lost or go uncounted"
        );
        assert!(
            conn.rotation_wait_since.lock().unwrap().is_some(),
            "deferring for the first time must record when the wait started"
        );
    }

    /// A deferral episode's timestamp must not survive into an unrelated
    /// rotation *within the same no-reconnect wait*. This is the cheaper,
    /// second line of defence described in `rotation_settled`'s doc comment
    /// — the real fix for long outages is the field being cleared by
    /// `reconnect()`, tested separately below.
    #[test]
    fn a_stale_wait_timestamp_for_a_different_beneficiary_is_discarded() {
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        // Simulate an old deferral episode for Author that started an hour ago --
        // long enough that `waited >= ROTATION_SETTLE_LIMIT` would trivially hold
        // if the stale timestamp were reused.
        *conn.rotation_wait_since.lock().unwrap() =
            Some((Beneficiary::Author, Instant::now() - Duration::from_secs(3600)));
        // One share still outstanding for a DIFFERENT, unrelated rotation to Xmrig.
        conn.submits_in_flight.fetch_add(1, Ordering::SeqCst);

        let proceeded = conn.rotation_settled(Beneficiary::Xmrig);

        assert!(
            !proceeded,
            "a stale hour-old timestamp from a different beneficiary's deferral must \
             not let a new rotation skip its own deferral just because it reads as \
             already-waited-long-enough"
        );
        assert_eq!(
            conn.rotation_wait_since.lock().unwrap().map(|(b, _)| b),
            Some(Beneficiary::Xmrig),
            "the stale entry must be discarded and replaced with a fresh one for the \
             beneficiary actually being deferred now"
        );

        conn.submits_in_flight.fetch_sub(1, Ordering::SeqCst);
    }

    /// GitHub #32 round 2, R2-F2: the fix above (discard on beneficiary
    /// mismatch) is NOT sufficient on its own. `reconnect()`'s retry loop is
    /// unbounded, so a real outage can span a FULL donation cycle or more
    /// (#34 recorded 121-minute outages; the default cycle is 100 minutes).
    /// The schedule can then return to the exact SAME beneficiary that was
    /// originally being deferred, with no intervening beneficiary ever
    /// observed to trigger the mismatch check in the test above — so an
    /// hour-old timestamp for the SAME beneficiary must also be discarded,
    /// and the mechanism that does it is `reconnect()` clearing the field,
    /// not a beneficiary comparison (which sees no mismatch here).
    ///
    /// This drives `reconnect()` itself, not a hand-set field, so it proves
    /// the actual call path — not just that clearing-on-mismatch logic
    /// would also happen to clear a same-beneficiary entry (it wouldn't).
    #[test]
    fn a_reconnect_discards_a_same_beneficiary_stale_wait_timestamp() {
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        // No address/wallet recorded, so `reconnect()` logs and returns
        // `false` immediately after its teardown steps -- including the
        // one under test -- without blocking on a real retry loop.
        *conn.rotation_wait_since.lock().unwrap() =
            Some((Beneficiary::Author, Instant::now() - Duration::from_secs(3600)));

        let reconnected = conn.reconnect();

        assert!(!reconnected, "no address/wallet recorded: reconnect() must fail fast");
        assert!(
            conn.rotation_wait_since.lock().unwrap().is_none(),
            "reconnect() must discard any rotation-deferral timestamp, since it \
             represents a connection epoch that has just ended — otherwise an \
             hour-old entry for the SAME beneficiary survives into a later, \
             unrelated deferral and lets it skip its own wait (R2-F2)"
        );
    }

    /// GitHub #32 round 3, R3-F1: at `--donate-level 100` the rotation ring
    /// has only two values (`User`'s slice is zero-width, so the schedule
    /// alternates Author<->Xmrig). A deferred entry for Xmrig can survive a
    /// full trip back to Xmrig — `want` passes through Author (which is
    /// `active`) without ever calling `rotation_settled` to notice the
    /// mismatch, since at a 2-value ring "the other beneficiary" IS the one
    /// already stored, not a third, different one. Neither the beneficiary-
    /// mismatch check inside `rotation_settled` nor `reconnect()`'s clear
    /// (rounds 1-2) catches this — proven by the review that found it via a
    /// direct call to `rotation_settled` with a backdated same-beneficiary
    /// entry, which returned `true` with a share still in flight.
    ///
    /// This test drives `note_donation_target` directly rather than the full
    /// `receiver_loop` (a real `CYCLE_SECS` cycle is 6000s minimum — not
    /// something a test can wait out), but it's the exact method the loop
    /// calls unconditionally every iteration, not a reimplementation of it.
    #[test]
    fn a_stale_entry_does_not_survive_a_two_value_ring_round_trip() {
        let conn = PoolConnection::new(crate::donate::MAX_DONATE_LEVEL);
        // Reflects a prior loop iteration having observed `want == Xmrig`,
        // which is what led to the stale entry below being created.
        let mut last_want = Beneficiary::Xmrig;

        // An old deferral for Xmrig, hours in the past -- long enough that
        // `waited >= ROTATION_SETTLE_LIMIT` would trivially hold if reused.
        *conn.rotation_wait_since.lock().unwrap() =
            Some((Beneficiary::Xmrig, Instant::now() - Duration::from_secs(3600)));

        // The schedule moves to the ring's only OTHER value (Author, which
        // is `active` at this point) -- this is exactly the transition
        // `rotation_settled`'s own mismatch check cannot see, because the
        // call site skips calling it whenever `want == active`.
        conn.note_donation_target(Beneficiary::Author, &mut last_want);
        assert!(
            conn.rotation_wait_since.lock().unwrap().is_none(),
            "a transition to the ring's other value must clear a stale \
             deferral for the value being left behind"
        );
        assert_eq!(last_want, Beneficiary::Author);

        // The schedule comes back around to Xmrig -- the SAME beneficiary
        // the (now-cleared) stale entry was originally for.
        conn.note_donation_target(Beneficiary::Xmrig, &mut last_want);

        // End-to-end consequence: a fresh rotation to Xmrig with a share in
        // flight must still defer -- proving the system doesn't just clear
        // the field once, but behaves correctly on the next real rotation
        // attempt for the value that was previously stale.
        conn.submits_in_flight.fetch_add(1, Ordering::SeqCst);
        let proceeded = conn.rotation_settled(Beneficiary::Xmrig);
        assert!(
            !proceeded,
            "a fresh rotation to a value seen before in the ring must still \
             defer for an in-flight submission, not inherit a stale timestamp \
             from the previous visit to that same value"
        );
    }

    /// Sets up a `PoolConnection` whose donation level (100%) rotates to the
    /// Author address on the very first `receiver_loop` iteration, with no
    /// timing trickery needed to hit the rotation path. Mirrors the setup
    /// `a_silent_pool_is_detected_and_reconnected_to` uses for `address` /
    /// `wallet`, plus `user_wallet` since a real login would set it and these
    /// tests never call `login()` (#32).
    fn new_rotation_conn(addr: &str) -> Arc<PoolConnection> {
        let conn = Arc::new(PoolConnection::new(100));
        conn.connect(addr).expect("connect to the local listener");
        *conn.address.lock().unwrap() = addr.to_string();
        *conn.wallet.lock().unwrap() = "4user".to_string();
        *conn.user_wallet.lock().unwrap() = "4user".to_string();
        conn
    }

    /// The core end-to-end defect test for #32 (Gap B): a share found right as
    /// a donation rotation starts must be answered on the OLD session before
    /// the rotation tears it down, not lost or left unsent.
    ///
    /// `submit_share` is called, and so is registered in `pending_shares`,
    /// BEFORE `receiver_loop` is even spawned — so without the rotation gate,
    /// the very first loop iteration (donate level 100%, so `want != active`
    /// immediately) rotates straight into `relogin_as`, which drains the
    /// pending entry as lost before the pool ever gets a chance to answer it.
    ///
    /// Must FAIL against the unfixed code: `accepted == 0, lost == 1`. If
    /// reverting the fix does not reproduce that, the test is not covering
    /// the defect.
    #[test]
    fn a_share_outstanding_at_a_rotation_is_answered_before_the_relogin() {
        use std::io::{BufRead as _, BufReader, Write as _};
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel::<u8>();

        let server = thread::spawn(move || {
            let mut held = Vec::new();
            for (n, incoming) in listener.incoming().enumerate() {
                let sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let _ = tx.send(n as u8);
                if n == 0 {
                    // Answer the queued submit on the OLD session.
                    let mut reader = BufReader::new(sock.try_clone().expect("clone"));
                    let mut line = String::new();
                    reader
                        .read_line(&mut line)
                        .expect("a submit line from the queued share");
                    let v: serde_json::Value =
                        serde_json::from_str(line.trim()).expect("valid JSON-RPC");
                    let id = v["id"].as_u64().expect("numeric rpc id");
                    let mut w = sock.try_clone().expect("clone for write");
                    let _ = w.write_all(
                        format!(
                            "{{\"id\":{},\"jsonrpc\":\"2.0\",\"error\":null,\
                             \"result\":{{\"status\":\"OK\"}}}}\n",
                            id
                        )
                        .as_bytes(),
                    );
                    let _ = w.flush();
                    held.push(sock);
                } else {
                    // The relogin on the Author wallet.
                    let mut reader = BufReader::new(sock.try_clone().expect("clone"));
                    let mut line = String::new();
                    let _ = reader.read_line(&mut line); // login line
                    let mut w = sock.try_clone().expect("clone for write");
                    let _ = w.write_all(
                        b"{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":{\"id\":\"sess2\",\
                          \"job\":{\"blob\":\"00\",\"job_id\":\"j2\",\"target\":\"ffffffff\"},\
                          \"status\":\"OK\"}}\n",
                    );
                    let _ = w.flush();
                    held.push(sock);
                    thread::sleep(Duration::from_secs(1));
                    return;
                }
            }
        });

        let conn = new_rotation_conn(&addr);
        conn.submit_share("j1", "00000000", &"a".repeat(64))
            .expect("queue the share before the receiver loop even starts");

        let worker = conn.clone();
        thread::spawn(move || worker.receiver_loop());

        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(0), "first connection (old session)");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(10)),
            Ok(1),
            "second connection (the Author relogin)"
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        while conn.get_accepted_shares() == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }

        assert_eq!(
            conn.get_accepted_shares(),
            1,
            "the share queued before the rotation must be accepted, not dropped by it"
        );
        assert_eq!(conn.get_lost_shares(), 0, "a share answered on the old session is not lost");
        assert_eq!(conn.get_unsent_shares(), 0);

        let _ = server.join();
    }

    /// The rotation gate must not wait forever: if the pool never answers,
    /// the rotation proceeds anyway once `rotation_settle_ms` elapses, and
    /// the outstanding share is counted lost when the stream is torn down.
    ///
    /// This also passes against the unfixed code — it guards the time
    /// limit's existence, not the Gap-B defect itself (which this suite's
    /// other new test establishes). Do not read a pass here as evidence the
    /// core defect is fixed.
    #[test]
    fn a_rotation_waits_at_most_the_settle_limit() {
        use std::sync::mpsc;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel::<u8>();

        let server = thread::spawn(move || {
            let mut held = Vec::new();
            for (n, incoming) in listener.incoming().enumerate() {
                let sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let _ = tx.send(n as u8);
                if n == 0 {
                    // Never answer the submit; just hold the socket open.
                    held.push(sock);
                } else {
                    use std::io::{BufRead as _, BufReader, Write as _};
                    let mut reader = BufReader::new(sock.try_clone().expect("clone"));
                    let mut line = String::new();
                    let _ = reader.read_line(&mut line);
                    let mut w = sock.try_clone().expect("clone for write");
                    let _ = w.write_all(
                        b"{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":{\"id\":\"sess2\",\
                          \"job\":{\"blob\":\"00\",\"job_id\":\"j2\",\"target\":\"ffffffff\"},\
                          \"status\":\"OK\"}}\n",
                    );
                    let _ = w.flush();
                    held.push(sock);
                    thread::sleep(Duration::from_secs(1));
                    return;
                }
            }
        });

        let conn = new_rotation_conn(&addr);
        conn.rotation_settle_ms.store(300, Ordering::SeqCst);
        conn.submit_share("j1", "00000000", &"a".repeat(64))
            .expect("queue the share before the receiver loop even starts");

        let worker = conn.clone();
        thread::spawn(move || worker.receiver_loop());

        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(0), "first connection");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(10)),
            Ok(1),
            "the rotation must proceed once the settle limit elapses, even with no reply"
        );

        assert_eq!(conn.get_lost_shares(), 1);
        assert_eq!(conn.get_accepted_shares(), 0);

        let _ = server.join();
    }

    /// Pins the in-flight term specifically: a submitter blocked on the
    /// stream lock (not yet registered in `pending_shares`) must still be
    /// visible to the rotation gate via `submits_in_flight` (#32).
    #[test]
    fn a_submitter_blocked_on_the_stream_lock_counts_as_in_flight() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let _server = thread::spawn(move || {
            let _held = listener.accept();
            thread::sleep(Duration::from_secs(3));
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener");

        let guard = conn.stream.lock();
        let submitter_conn = Arc::clone(&conn);
        let submitter =
            thread::spawn(move || submitter_conn.submit_share("j1", "00000000", "ff"));

        let deadline = Instant::now() + Duration::from_secs(5);
        while conn.submits_in_flight.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(conn.submits_in_flight.load(Ordering::SeqCst), 1);
        assert_eq!(
            conn.get_pending_shares(),
            0,
            "still blocked on the stream lock, so not yet registered"
        );

        drop(guard);
        submitter.join().expect("submitter thread").expect("submit succeeds");

        assert_eq!(conn.submits_in_flight.load(Ordering::SeqCst), 0);
        assert_eq!(conn.get_pending_shares(), 1);
    }

    /// Every return path out of `submit_share` must release the in-flight
    /// count, including the earliest possible failure — no connection at
    /// all (#32).
    #[test]
    fn in_flight_is_released_on_every_error_path() {
        let conn = PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL);
        let err = conn.submit_share("j1", "00000000", "ff");
        assert!(err.is_err());
        assert_eq!(conn.submits_in_flight.load(Ordering::SeqCst), 0);
    }

    /// Does not deterministically distinguish "the gate checks pending only"
    /// from "the gate also checks in-flight", since which thread wins the
    /// lock race between the submitter and `receiver_loop` is
    /// non-deterministic — `rotation_may_proceed_truth_table` is what pins
    /// the in-flight term specifically. What this test does establish is that
    /// a submission blocked on the stream lock at the moment a rotation
    /// fires is still sent and answered on the old session (#32).
    #[test]
    fn a_blocked_submitter_at_rotation_is_sent_and_answered_on_the_old_session() {
        use std::io::{BufRead as _, BufReader, Write as _};
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel::<u8>();

        let server = thread::spawn(move || {
            let mut held = Vec::new();
            for (n, incoming) in listener.incoming().enumerate() {
                let sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let _ = tx.send(n as u8);
                if n == 0 {
                    let mut reader = BufReader::new(sock.try_clone().expect("clone"));
                    let mut line = String::new();
                    reader
                        .read_line(&mut line)
                        .expect("a submit line from the queued share");
                    let v: serde_json::Value =
                        serde_json::from_str(line.trim()).expect("valid JSON-RPC");
                    let id = v["id"].as_u64().expect("numeric rpc id");
                    let mut w = sock.try_clone().expect("clone for write");
                    let _ = w.write_all(
                        format!(
                            "{{\"id\":{},\"jsonrpc\":\"2.0\",\"error\":null,\
                             \"result\":{{\"status\":\"OK\"}}}}\n",
                            id
                        )
                        .as_bytes(),
                    );
                    let _ = w.flush();
                    held.push(sock);
                } else {
                    let mut reader = BufReader::new(sock.try_clone().expect("clone"));
                    let mut line = String::new();
                    let _ = reader.read_line(&mut line);
                    let mut w = sock.try_clone().expect("clone for write");
                    let _ = w.write_all(
                        b"{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":{\"id\":\"sess2\",\
                          \"job\":{\"blob\":\"00\",\"job_id\":\"j2\",\"target\":\"ffffffff\"},\
                          \"status\":\"OK\"}}\n",
                    );
                    let _ = w.flush();
                    held.push(sock);
                    thread::sleep(Duration::from_secs(1));
                    return;
                }
            }
        });

        let conn = new_rotation_conn(&addr);

        let guard = conn.stream.lock();
        let submitter_conn = Arc::clone(&conn);
        let submitter = thread::spawn(move || {
            submitter_conn.submit_share("j1", "00000000", &"a".repeat(64))
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        while conn.submits_in_flight.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(conn.submits_in_flight.load(Ordering::SeqCst), 1);

        let worker = conn.clone();
        thread::spawn(move || worker.receiver_loop());

        drop(guard);
        submitter.join().expect("submitter thread").expect("submit succeeds");

        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(0), "first connection (old session)");
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(1), "second connection (relogin)");

        let deadline = Instant::now() + Duration::from_secs(5);
        while conn.get_accepted_shares() == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }

        assert_eq!(conn.get_accepted_shares(), 1);
        assert_eq!(conn.get_lost_shares(), 0);
        assert_eq!(conn.get_unsent_shares(), 0);

        let _ = server.join();
    }

    // --- GitHub #37: a failed relogin must not leave a live stream ---

    /// Isolates the `relogin_as` fix: a failed `connect`/`login` must clear
    /// the stream rather than leave a live, unauthenticated socket with a
    /// stale session id and up to a 30s read timeout.
    ///
    /// `.is_err()` alone would also pass against the unfixed code — `?` still
    /// propagates the login error either way. The stream-is-none check is
    /// what actually covers the bug: the unfixed code returns `Err` with the
    /// just-opened, now-unauthenticated stream still installed.
    #[test]
    fn a_failed_relogin_login_leaves_no_stream_behind() {
        use std::io::{BufRead as _, BufReader, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();

        let server = thread::spawn(move || {
            let mut held = Vec::new();
            for (n, incoming) in listener.incoming().enumerate() {
                let mut sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                if n == 1 {
                    let mut reader = BufReader::new(sock.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("a login request line");
                    let _ = sock.write_all(
                        b"{\"id\":1,\"jsonrpc\":\"2.0\",\
                          \"error\":{\"code\":-1,\"message\":\"denied\"}}\n",
                    );
                    let _ = sock.flush();
                    // Hold the socket open and silent — proves the test isn't
                    // passing via EOF.
                    thread::sleep(Duration::from_secs(3));
                    return;
                }
                held.push(sock);
            }
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener (accept 0)");

        let result = conn.relogin_as("4test");

        assert!(result.is_err(), "a denied login must fail relogin_as");
        assert!(
            conn.stream.lock().is_none(),
            "a failed relogin must clear the stream, not leave the new, \
             unauthenticated connection installed"
        );

        let _ = server.join();
    }

    /// Covers the `login()` fix and reuses the `relogin_as` fix: a
    /// submit-acknowledgement reply read by mistake as the login reply (no
    /// `result.id`) must fail the login outright, not be mistaken for success.
    #[test]
    fn a_submit_reply_read_as_the_login_reply_fails_the_relogin() {
        use std::io::{BufRead as _, BufReader, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();

        let server = thread::spawn(move || {
            let mut held = Vec::new();
            for (n, incoming) in listener.incoming().enumerate() {
                let mut sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                if n == 1 {
                    let mut reader = BufReader::new(sock.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("a login request line");
                    // Shape of an accepted submit reply: a top-level JSON-RPC
                    // envelope id (99, unrelated to the login request's own
                    // id) and a "result" with no "id" field inside it — which
                    // is what login() actually reads (result.id), not the
                    // envelope id.
                    let _ = sock.write_all(
                        b"{\"id\":99,\"jsonrpc\":\"2.0\",\"result\":{\"status\":\"OK\"}}\n",
                    );
                    let _ = sock.flush();
                    thread::sleep(Duration::from_secs(3));
                    return;
                }
                held.push(sock);
            }
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener (accept 0)");
        *conn.session_id.lock().unwrap() = "old".to_string();

        let result = conn.relogin_as("4test");

        assert!(
            result.is_err(),
            "a login response with no session id must fail, not succeed silently"
        );
        assert!(
            conn.stream.lock().is_none(),
            "a failed relogin must clear the stream"
        );
        assert_eq!(
            *conn.session_id.lock().unwrap(),
            "old",
            "a login response with no session id must not overwrite the existing one"
        );

        let _ = server.join();
    }

    /// GitHub #37 review (PR #42, F1/F2): a real pool rejects a login this
    /// way — sammy007/monero-stratum answers with `result` present but
    /// `null` alongside a populated `error`, not with `error` alone.
    /// Before the F1 fix (but after #37's base fix in this same PR), login()
    /// already checked `result` first and failed on a missing session id —
    /// so this didn't silently succeed, but the error it raised was the
    /// generic "no session id: null", not the pool's real rejection reason.
    /// (`Ok(())` is `main`'s behaviour, two states further back — `main`'s
    /// login() doesn't fail on a missing id at all.) This pins that `error`
    /// is now checked first, so the real rejection reason reaches the
    /// caller instead of being replaced with that generic message.
    #[test]
    fn a_login_rejection_with_a_null_result_reports_the_real_reason() {
        use std::io::{BufRead as _, BufReader, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();

        let server = thread::spawn(move || {
            let mut held = Vec::new();
            for (n, incoming) in listener.incoming().enumerate() {
                let mut sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                if n == 1 {
                    let mut reader = BufReader::new(sock.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("a login request line");
                    let _ = sock.write_all(
                        b"{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":null,\
                          \"error\":{\"code\":-1,\"message\":\"Unauthenticated\"}}\n",
                    );
                    let _ = sock.flush();
                    thread::sleep(Duration::from_secs(3));
                    return;
                }
                held.push(sock);
            }
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener (accept 0)");

        let err = conn.relogin_as("4test").expect_err("a rejected login must fail relogin_as");

        assert!(
            err.contains("Unauthenticated"),
            "the pool's real rejection reason must reach the caller, not be \
             replaced with a generic \"no session id\" message; got: {err}"
        );
        assert!(
            conn.stream.lock().is_none(),
            "a failed relogin must clear the stream"
        );

        let _ = server.join();
    }

    /// Guards against someone deleting the `set_read_timeout` line on the
    /// success path: a successful relogin must leave the connection polling
    /// at the normal short interval, not the 30s login-wait timeout.
    #[test]
    fn a_successful_relogin_restores_the_poll_interval() {
        use std::io::{BufRead as _, BufReader, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();

        let server = thread::spawn(move || {
            let mut held = Vec::new();
            for (n, incoming) in listener.incoming().enumerate() {
                let mut sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                if n == 1 {
                    let mut reader = BufReader::new(sock.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("a login request line");
                    let _ = sock.write_all(
                        b"{\"id\":1,\"result\":{\"id\":\"sess2\",\"job\":{\"blob\":\"00\",\
                          \"job_id\":\"j1\",\"target\":\"ffffffff\"},\"status\":\"OK\"}}\n",
                    );
                    let _ = sock.flush();
                    thread::sleep(Duration::from_secs(1));
                    return;
                }
                held.push(sock);
            }
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener (accept 0)");

        let result = conn.relogin_as("4test");

        assert!(result.is_ok(), "a valid login response must succeed: {result:?}");
        assert_eq!(*conn.session_id.lock().unwrap(), "sess2");

        let timeout = {
            let guard = conn.stream.lock();
            guard
                .as_ref()
                .expect("stream must be installed on success")
                .tcp()
                .read_timeout()
                .expect("read_timeout query")
        };
        assert!(
            timeout < Some(Duration::from_secs(1)),
            "a successful relogin must restore the short poll interval, not leave \
             the 30s login-wait timeout in place; got {timeout:?}"
        );

        let _ = server.join();
    }

    /// GitHub #37 review round 2 (R2-F1): both reference pool implementations
    /// send `"error":null` on a *successful* login, not an absent `error`
    /// field. The F1 fix's `.filter(|v| !v.is_null())` on the `error` check
    /// matters for every real-pool login, not just the rejection case R2's
    /// other new test covers — without the filter, `response.get("error")`
    /// would return `Some(Null)` on every success and `login()` would fail
    /// every real login with `"Login error: null"`. No prior test used this
    /// shape; `a_successful_relogin_restores_the_poll_interval` omits `error`
    /// entirely, so it can't catch a missing filter here.
    #[test]
    fn a_successful_login_with_an_explicit_null_error_still_succeeds() {
        use std::io::{BufRead as _, BufReader, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();

        let server = thread::spawn(move || {
            let mut held = Vec::new();
            for (n, incoming) in listener.incoming().enumerate() {
                let mut sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                if n == 1 {
                    let mut reader = BufReader::new(sock.try_clone().unwrap());
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("a login request line");
                    let _ = sock.write_all(
                        b"{\"id\":1,\"jsonrpc\":\"2.0\",\"error\":null,\"result\":\
                          {\"id\":\"sess3\",\"job\":{\"blob\":\"00\",\"job_id\":\"j1\",\
                          \"target\":\"ffffffff\"},\"status\":\"OK\"}}\n",
                    );
                    let _ = sock.flush();
                    thread::sleep(Duration::from_secs(1));
                    return;
                }
                held.push(sock);
            }
        });

        let conn = Arc::new(PoolConnection::new(crate::donate::DEFAULT_DONATE_LEVEL));
        conn.connect(&addr).expect("connect to the local listener (accept 0)");

        let result = conn.relogin_as("4test");

        assert!(
            result.is_ok(),
            "an explicit `\"error\":null` alongside a real result must not be \
             mistaken for a rejection: {result:?}"
        );
        assert_eq!(*conn.session_id.lock().unwrap(), "sess3");

        let _ = server.join();
    }

    /// End-to-end through the real `receiver_loop`: a failed donation relogin
    /// must not leave the loop stuck re-reading a dead, unauthenticated
    /// stream — it must fall into `reconnect()`, which retries using
    /// `self.wallet` (the donation address), not a stale path.
    ///
    /// Donate level 100 means the very first rotation targets the Author
    /// address at elapsed time ~0 (see `src/donate.rs`: `user` time is 0 when
    /// `level == 100`), so no test hook is needed to trigger the switch.
    #[test]
    fn a_failed_donation_relogin_reconnects_through_the_real_receiver_loop() {
        use std::io::{BufRead as _, BufReader, Write as _};
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = mpsc::channel::<String>();

        let server = thread::spawn(move || {
            let mut held = Vec::new();
            for (n, incoming) in listener.incoming().enumerate() {
                let mut sock = match incoming {
                    Ok(s) => s,
                    Err(_) => return,
                };
                sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                match n {
                    0 => {
                        // Held open, silent — the initial connection the
                        // receiver loop starts on.
                        held.push(sock);
                    }
                    1 => {
                        let mut reader = BufReader::new(sock.try_clone().unwrap());
                        let mut line = String::new();
                        reader.read_line(&mut line).expect("a login request line");
                        // Submit-OK shape again: no result.id, so login()
                        // fails. Never closed — closing it would let the
                        // unfixed code reach EOF and reconnect for the wrong
                        // reason (see CLAUDE.md's break-testing section).
                        let _ = sock.write_all(
                            b"{\"id\":99,\"jsonrpc\":\"2.0\",\"result\":{\"status\":\"OK\"}}\n",
                        );
                        let _ = sock.flush();
                        held.push(sock);
                    }
                    2 => {
                        let mut reader = BufReader::new(sock.try_clone().unwrap());
                        let mut line = String::new();
                        reader.read_line(&mut line).expect("a login request line");
                        let _ = tx.send(line);
                        let _ = sock.write_all(
                            b"{\"id\":1,\"result\":{\"id\":\"sess3\",\"job\":{\"blob\":\"00\",\
                              \"job_id\":\"j1\",\"target\":\"ffffffff\"},\"status\":\"OK\"}}\n",
                        );
                        let _ = sock.flush();
                        held.push(sock);
                        return;
                    }
                    _ => return,
                }
            }
        });

        // Donate level 100: the very first rotation targets Author.
        let conn = Arc::new(PoolConnection::new(100));
        conn.connect(&addr).expect("connect to the local listener (accept 0)");
        // reconnect() needs both recorded, or it bails without retrying.
        *conn.address.lock().unwrap() = addr.clone();
        *conn.wallet.lock().unwrap() = "4test".to_string();
        // Leave silence_timeout_ms at its default — shortening it would let
        // the unfixed code reconnect for a different reason entirely.

        let worker = conn.clone();
        thread::spawn(move || worker.receiver_loop());

        let login_line = rx
            .recv_timeout(Duration::from_secs(15))
            .expect("a third accept (and login attempt) must happen within 15s");
        let msg: Value = serde_json::from_str(login_line.trim())
            .expect("the login line on the wire must be valid JSON");
        assert_eq!(
            msg.get("params").and_then(|p| p.get("login")).and_then(|l| l.as_str()),
            Some(crate::donate::AUTHOR_ADDRESS),
            "reconnect() must retry using self.wallet (the donation address), \
             proving it is not stuck on a stale path: {login_line}"
        );

        let _ = server.join();
    }
}
