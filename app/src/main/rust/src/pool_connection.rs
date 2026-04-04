use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::ClientConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[derive(Deserialize)]
#[allow(dead_code)]
struct JsonRpcResponse {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<Value>,
    method: Option<String>,
    params: Option<Value>,
}

/// Wraps either a plain TCP or TLS stream behind Read + Write.
enum PoolStream {
    Plain(TcpStream),
    Tls(rustls::StreamOwned<rustls::ClientConnection, TcpStream>),
}

impl PoolStream {
    fn try_clone_tcp(&self) -> Option<TcpStream> {
        match self {
            PoolStream::Plain(s) => s.try_clone().ok(),
            PoolStream::Tls(s) => s.sock.try_clone().ok(),
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
    stream: Mutex<Option<PoolStream>>,
    current_job: Arc<Mutex<Option<Job>>>,
    connected: AtomicBool,
    request_id: AtomicU64,
    address: Mutex<String>,
    use_tls: AtomicBool,
    tls_config: Arc<ClientConfig>,
    session_id: Mutex<String>,
    accepted_shares: Arc<AtomicU32>,
    rejected_shares: Arc<AtomicU32>,
}

impl PoolConnection {
    pub fn new() -> Self {
        let tls_config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
            .with_no_client_auth();

        Self {
            stream: Mutex::new(None),
            current_job: Arc::new(Mutex::new(None)),
            connected: AtomicBool::new(false),
            request_id: AtomicU64::new(1),
            address: Mutex::new(String::new()),
            use_tls: AtomicBool::new(false),
            tls_config: Arc::new(tls_config),
            session_id: Mutex::new(String::new()),
            accepted_shares: Arc::new(AtomicU32::new(0)),
            rejected_shares: Arc::new(AtomicU32::new(0)),
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

        let use_tls = is_tls_port(address);
        self.use_tls.store(use_tls, Ordering::SeqCst);

        let pool_stream = if use_tls {
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
        let params = serde_json::json!({
            "login": wallet,
            "pass": "android",
            "agent": "MinerTim/1.0",
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
            if let Some(job_data) = result.get("job") {
                if let Some(job) = parse_job(job_data) {
                    if let Ok(mut current) = self.current_job.lock() {
                        *current = Some(job);
                    }
                }
            }
            Ok(())
        } else if let Some(error) = response.get("error") {
            Err(format!("Login error: {}", error))
        } else {
            Err("Unexpected login response".into())
        }
    }

    pub fn get_work(&self) -> Option<Job> {
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

    pub fn start_receiver(&self) {
        let tcp_clone = {
            let guard = match self.stream.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            match guard.as_ref() {
                Some(s) => match s.try_clone_tcp() {
                    Some(c) => c,
                    None => return,
                },
                None => return,
            }
        };

        let address = self.address.lock().ok().map(|a| a.clone()).unwrap_or_default();
        let current_job = self.current_job.clone();
        let connected = self.connected.load(Ordering::SeqCst);
        let use_tls = self.use_tls.load(Ordering::SeqCst);
        let tls_config = self.tls_config.clone();
        let accepted = self.accepted_shares.clone();
        let rejected = self.rejected_shares.clone();

        if !connected {
            return;
        }

        thread::Builder::new()
            .name("pool-receiver".into())
            .spawn(move || {
                // Build a reader over either plain or TLS stream
                let boxed_read: Box<dyn Read + Send> = if use_tls {
                    let host = address
                        .rsplit_once(':')
                        .map(|(h, _)| h)
                        .unwrap_or(&address);

                    let server_name = match rustls::pki_types::ServerName::try_from(host.to_string()) {
                        Ok(sn) => sn,
                        Err(e) => {
                            log::error!("Receiver: invalid server name: {}", e);
                            return;
                        }
                    };

                    let tls_conn = match rustls::ClientConnection::new(tls_config, server_name) {
                        Ok(c) => c,
                        Err(e) => {
                            log::error!("Receiver: TLS setup failed: {}", e);
                            return;
                        }
                    };

                    Box::new(rustls::StreamOwned::new(tls_conn, tcp_clone))
                } else {
                    Box::new(tcp_clone)
                };

                let reader = BufReader::new(boxed_read);

                for line in reader.lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(e) => {
                            log::error!("Pool read error: {}", e);
                            break;
                        }
                    };

                    if line.is_empty() {
                        continue;
                    }

                    log::debug!("Pool recv: {}", line);

                    match serde_json::from_str::<Value>(&line) {
                        Ok(msg) => {
                            // Handle job notifications
                            let is_job = msg.get("method").and_then(|m| m.as_str()) == Some("job");
                            let job_params = if is_job {
                                msg.get("params")
                            } else {
                                msg.get("result").and_then(|r| r.get("job"))
                            };

                            if let Some(job_data) = job_params {
                                if let Some(job) = parse_job(job_data) {
                                    log::info!("New job received: {}", job.job_id);
                                    if let Ok(mut current) = current_job.lock() {
                                        *current = Some(job);
                                    }
                                }
                            }

                            // Handle submit responses (has an "id" but no "method")
                            if msg.get("id").is_some() && msg.get("method").is_none() {
                                if let Some(error) = msg.get("error") {
                                    if !error.is_null() {
                                        let err_msg = error
                                            .get("message")
                                            .and_then(|m| m.as_str())
                                            .unwrap_or("unknown");
                                        log::warn!("Share rejected: {}", err_msg);
                                        rejected.fetch_add(1, Ordering::Relaxed);
                                    }
                                } else if let Some(result) = msg.get("result") {
                                    let status = result
                                        .get("status")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("");
                                    if status == "OK" {
                                        log::info!("Share accepted by pool");
                                        accepted.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to parse pool message: {}", e);
                        }
                    }
                }
                log::info!("Pool receiver thread exiting");
            })
            .ok();
    }

    fn send_request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);

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

        let mut stream_guard = self
            .stream
            .lock()
            .map_err(|_| "Stream mutex poisoned".to_string())?;

        let stream = stream_guard
            .as_mut()
            .ok_or_else(|| "Not connected".to_string())?;

        stream
            .write_all(msg.as_bytes())
            .map_err(|e| format!("Write failed: {}", e))?;
        stream
            .flush()
            .map_err(|e| format!("Flush failed: {}", e))?;

        // Read response
        let mut response_line = String::new();
        let mut buf_reader = BufReader::new(stream);
        buf_reader
            .read_line(&mut response_line)
            .map_err(|e| format!("Read failed: {}", e))?;

        log::debug!("Pool recv: {}", response_line.trim());

        serde_json::from_str(&response_line).map_err(|e| format!("Parse failed: {}", e))
    }

    /// Write-only send — used for submit after the receiver thread is running.
    fn send_message(&self, method: &str, params: Value) -> Result<(), String> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);

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

        let mut stream_guard = self
            .stream
            .lock()
            .map_err(|_| "Stream mutex poisoned".to_string())?;

        let stream = stream_guard
            .as_mut()
            .ok_or_else(|| "Not connected".to_string())?;

        stream
            .write_all(msg.as_bytes())
            .map_err(|e| format!("Write failed: {}", e))?;
        stream
            .flush()
            .map_err(|e| format!("Flush failed: {}", e))
    }
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

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16).ok()?;
        bytes.push(byte);
    }
    Some(bytes)
}
