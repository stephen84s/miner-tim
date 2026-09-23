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

/// How long the receiver blocks on a socket read (also the max time it holds the
/// stream lock, i.e. the worst-case share-submit latency). Kept short so that
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
    stream: Mutex<Option<PoolStream>>,
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
    /// How long the pool may be silent before the connection is treated as
    /// dead, in **milliseconds**. Defaults to `POOL_SILENCE_TIMEOUT`; tests
    /// shorten it so the real `receiver_loop` can be driven to the timeout in
    /// a second rather than three minutes. Exercising the loop itself is the
    /// point — a test that called a helper would pass while the wiring was
    /// broken, which is how PR #22 shipped green three times.
    silence_timeout_ms: AtomicU64,
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
            stream: Mutex::new(None),
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
            silence_timeout_ms: AtomicU64::new(POOL_SILENCE_TIMEOUT.as_millis() as u64),
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

        if let Ok(mut s) = self.stream.lock() {
            *s = Some(pool_stream);
        }

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

        if let Some(result) = response.get("result") {
            if let Some(id) = result.get("id").and_then(|v| v.as_str()) {
                if let Ok(mut sid) = self.session_id.lock() {
                    *sid = id.to_string();
                }
                log::info!("Login successful, session id: {}", id);
            }
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
        } else if let Some(error) = response.get("error") {
            Err(format!("Login error: {}", error))
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
        let sid = self.session_id.lock()
            .map(|s| s.clone())
            .unwrap_or_default();

        let params = serde_json::json!({
            "id": sid,
            "job_id": job_id,
            "nonce": nonce,
            "result": result
        });

        // The id must be registered in `pending_shares` *before* the request
        // is written, not after. The write releases
        // the stream lock before returning; if the insert happened only once
        // that call returned, the receiver thread could read and process the
        // pool's reply for this exact id in the gap, find nothing pending,
        // discard it uncounted, and then have this function insert an entry
        // for a submission that had already been fully answered — later
        // drained and miscounted as "lost" even though the pool responded.
        // Registering first closes that window; on a write failure the entry
        // is removed again, since the request never left the machine.
        let rpc_id = self.next_request_id();

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
        // the stream, so a drain can only run wholly before (we see no stream
        // and insert nothing) or wholly after (the entry was written to the old
        // stream, where no reply will ever be read, so "lost" is correct).
        //
        // Lock order is stream -> pending_shares. Nothing takes them the other
        // way: `drain_pending_shares` and `handle_pool_message` take
        // `pending_shares` alone, after the stream guard has been released.
        {
            let mut stream_guard = self
                .stream
                .lock()
                .map_err(|_| "Stream mutex poisoned".to_string())?;
            let stream = stream_guard
                .as_mut()
                .ok_or_else(|| "Not connected".to_string())?;
            write_request(stream, rpc_id, "submit", params)?;
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
        }

        log::info!(
            "Share submitted: rpc_id={} job_id={} nonce={}",
            rpc_id,
            job_id,
            nonce
        );

        Ok(())
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
        // Accepted, rejected and lost are one ledger: for *submitted* shares,
        // submitted == accepted + rejected + lost + still-pending. This is
        // not the same population as the stats line's "found" count, which
        // increments the moment a hash clears the target — before the share
        // verifier or `submit_share` itself has had a chance to withhold or
        // fail it, so a found share is not guaranteed ever to become a
        // submitted one. Resetting two of these three would let the
        // arithmetic that exposed #34 silently stop balancing.
        self.lost_shares.store(0, Ordering::SeqCst);
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

        loop {
            // Rotate the login wallet between user/author/XMRig per the donation
            // schedule (see `crate::donate`). Switching re-logs-in on the same
            // pool with the target wallet.
            let want = self.donation.beneficiary_at(donation_start.elapsed().as_secs());
            if want != active {
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
                        // Stream is torn down; the read below yields NotConnected
                        // and reconnect() re-establishes using self.wallet (= addr).
                        log::warn!("Donation switch failed: {} (reconnecting)", e);
                    }
                }
            }

            // Hold the stream lock only for the duration of one read so
            // submits/keepalives from other threads can interleave.
            let read_result = {
                let mut guard = match self.stream.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
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
        if let Ok(mut s) = self.stream.lock() {
            *s = None;
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
                    if let Ok(mut s) = self.stream.lock() {
                        *s = None;
                    }
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
    /// the reconnect path.
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
        if let Ok(mut s) = self.stream.lock() {
            *s = None;
        }
        // This replaces the stream without going through `reconnect()`, so it
        // needs its own drain — otherwise a submission sent just before a
        // donation-slice switch is orphaned in the pending map forever.
        self.drain_pending_shares();
        self.connect(&address)?;
        self.login(wallet)?;
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
        if let Ok(guard) = self.stream.lock()
            && let Some(s) = guard.as_ref()
            && let Err(e) = s.tcp().set_read_timeout(Some(timeout))
        {
            log::warn!("Failed to set read timeout: {}", e);
        }
    }

    /// Send a request and synchronously read the response line. Only used
    /// for login, before/while the receiver polls; reads byte-by-byte so no
    /// buffered data is lost to a throwaway reader.
    fn send_request(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut stream_guard = self
            .stream
            .lock()
            .map_err(|_| "Stream mutex poisoned".to_string())?;

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
        let mut stream_guard = self
            .stream
            .lock()
            .map_err(|_| "Stream mutex poisoned".to_string())?;
        let stream = stream_guard
            .as_mut()
            .ok_or_else(|| "Not connected".to_string())?;
        write_request(stream, self.next_request_id(), method, params)
    }

    fn next_request_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }
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
        let mut guard = conn.stream.lock().unwrap();
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
    }
}
