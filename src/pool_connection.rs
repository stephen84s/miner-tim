use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::donate::{Beneficiary, DonationSchedule};
use crate::hex::{hex_decode, hex_encode};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::ClientConfig;
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

/// Certificate verifier that accepts all certificates.
/// Mining pool data (wallet address, shares) is public, and many pools
/// run expired or self-signed certs on their Stratum ports.
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
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
        let tls_config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
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
            session_id: Mutex::new(String::new()),
            accepted_shares: AtomicU32::new(0),
            rejected_shares: AtomicU32::new(0),
        }
    }

    pub fn connect(&self, address: &str) -> Result<(), String> {
        log::info!("Connecting to pool: {}", address);

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
                    while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = pending.drain(..=pos).collect();
                        let line = String::from_utf8_lossy(&line);
                        let line = line.trim();
                        if !line.is_empty() {
                            log::debug!("Pool recv: {}", line);
                            self.handle_pool_message(line);
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
                if line.len() > 1 << 20 {
                    return Err("Pool response line too long".into());
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
