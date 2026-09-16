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
/// not, which is the asymmetry GitHub #21 records. The value lives here so the
/// two cannot drift apart.
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

        self.send_message("submit", params)
    }

    pub fn get_accepted_shares(&self) -> u32 {
        self.accepted_shares.load(Ordering::Relaxed)
    }

    pub fn get_rejected_shares(&self) -> u32 {
        self.rejected_shares.load(Ordering::Relaxed)
    }

    pub fn reset_share_counters(&self) {
        self.accepted_shares.store(0, Ordering::SeqCst);
        self.rejected_shares.store(0, Ordering::SeqCst);
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
                    pending.clear();
                }
                Ok(n) => {
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
                        }
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => {
                    log::error!("Pool read error: {}", e);
                    if !self.reconnect() {
                        return;
                    }
                    pending.clear();
                }
            }

            if last_keepalive.elapsed() >= KEEPALIVE_INTERVAL {
                last_keepalive = Instant::now();
                let sid = self.session_id.lock().map(|s| s.clone()).unwrap_or_default();
                if let Err(e) = self.send_message("keepalived", serde_json::json!({ "id": sid })) {
                    log::warn!("Keepalive failed: {}", e);
                }
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

        // Handle submit responses (has an "id" but no "method")
        if msg.get("id").is_some() && msg.get("method").is_none() {
            if let Some(error) = msg.get("error")
                && !error.is_null()
            {
                let err_msg = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                log::warn!("Share rejected: {}", err_msg);
                self.rejected_shares.fetch_add(1, Ordering::Relaxed);
                return;
            }
            if let Some(result) = msg.get("result") {
                let status = result.get("status").and_then(|s| s.as_str()).unwrap_or("");
                if status == "OK" {
                    log::info!("Share accepted by pool");
                    self.accepted_shares.fetch_add(1, Ordering::Relaxed);
                }
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
        let mut fed = 0usize;
        loop {
            pending.extend_from_slice(&chunk);
            fed += chunk.len();
            match take_complete_lines(&mut pending) {
                Ok(_) => {
                    assert!(
                        pending.len() <= MAX_LINE_BYTES,
                        "buffer grew past the limit without being refused: {} bytes",
                        pending.len()
                    );
                    assert!(fed <= MAX_LINE_BYTES + chunk.len(), "should have refused by now");
                }
                Err(overflow) => {
                    assert!(overflow > MAX_LINE_BYTES);
                    // The caller clears and reconnects; the point is that it is
                    // told to, rather than the allocation continuing.
                    return;
                }
            }
        }
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
}
