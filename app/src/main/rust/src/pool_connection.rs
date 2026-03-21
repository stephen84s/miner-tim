use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Job {
    pub blob: Vec<u8>,
    pub target: Vec<u8>,
    pub job_id: String,
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

pub struct PoolConnection {
    stream: Mutex<Option<TcpStream>>,
    current_job: Arc<Mutex<Option<Job>>>,
    connected: AtomicBool,
    request_id: AtomicU64,
    address: Mutex<String>,
}

impl PoolConnection {
    pub fn new() -> Self {
        Self {
            stream: Mutex::new(None),
            current_job: Arc::new(Mutex::new(None)),
            connected: AtomicBool::new(false),
            request_id: AtomicU64::new(1),
            address: Mutex::new(String::new()),
        }
    }

    pub fn connect(&self, address: &str) -> Result<(), String> {
        log::info!("Connecting to pool: {}", address);

        let stream = TcpStream::connect(address)
            .map_err(|e| format!("TCP connect failed: {}", e))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| format!("Set read timeout failed: {}", e))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| format!("Set write timeout failed: {}", e))?;
        stream
            .set_nodelay(true)
            .map_err(|e| format!("Set nodelay failed: {}", e))?;

        if let Ok(mut addr) = self.address.lock() {
            *addr = address.to_string();
        }

        if let Ok(mut s) = self.stream.lock() {
            *s = Some(stream);
        }

        self.connected.store(true, Ordering::SeqCst);
        log::info!("Connected to pool: {}", address);
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

        // Parse the login response for an initial job
        if let Some(result) = response.get("result") {
            if let Some(job_data) = result.get("job") {
                if let Some(job) = parse_job(job_data) {
                    if let Ok(mut current) = self.current_job.lock() {
                        *current = Some(job);
                    }
                }
            }
            log::info!("Login successful");
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
    ) -> Result<bool, String> {
        let params = serde_json::json!({
            "id": job_id,
            "job_id": job_id,
            "nonce": nonce,
            "result": result
        });

        let response = self.send_request("submit", params)?;

        if let Some(result) = response.get("result") {
            if let Some(status) = result.get("status") {
                return Ok(status.as_str() == Some("OK"));
            }
        }

        if response.get("error").is_some() {
            return Ok(false);
        }

        Ok(true)
    }

    pub fn start_receiver(&self) {
        let stream_clone = {
            let guard = match self.stream.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            match guard.as_ref() {
                Some(s) => match s.try_clone() {
                    Ok(c) => c,
                    Err(_) => return,
                },
                None => return,
            }
        };

        let current_job = self.current_job.clone();
        let connected = self.connected.load(Ordering::SeqCst);

        if !connected {
            return;
        }

        // Spawn receiver thread
        let connected_flag = AtomicBool::new(true);
        let _ = Arc::new(connected_flag);

        let stream_for_keepalive = match {
            let guard = self.stream.lock();
            match guard {
                Ok(g) => g.as_ref().and_then(|s| s.try_clone().ok()),
                Err(_) => None,
            }
        } {
            Some(s) => s,
            None => return,
        };

        // Receiver thread
        thread::Builder::new()
            .name("pool-receiver".into())
            .spawn(move || {
                let reader = BufReader::new(stream_clone);
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
                            // Check for job notification
                            let is_job = msg.get("method").and_then(|m| m.as_str()) == Some("job");
                            let job_params = if is_job {
                                msg.get("params")
                            } else {
                                // Could also be in result.job for login responses
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
                        }
                        Err(e) => {
                            log::warn!("Failed to parse pool message: {}", e);
                        }
                    }
                }
                log::info!("Pool receiver thread exiting");
            })
            .ok();

        // Keepalive thread
        thread::Builder::new()
            .name("pool-keepalive".into())
            .spawn(move || {
                let mut stream = stream_for_keepalive;
                loop {
                    thread::sleep(Duration::from_secs(60));

                    let keepalive = serde_json::json!({
                        "id": 0,
                        "jsonrpc": "2.0",
                        "method": "keepalive",
                        "params": {}
                    });

                    let mut msg = serde_json::to_string(&keepalive).unwrap_or_default();
                    msg.push('\n');

                    if stream.write_all(msg.as_bytes()).is_err() {
                        log::error!("Keepalive write failed, exiting");
                        break;
                    }
                    if stream.flush().is_err() {
                        log::error!("Keepalive flush failed, exiting");
                        break;
                    }

                    log::debug!("Keepalive sent");
                }
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
        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .map_err(|e| format!("Read failed: {}", e))?;

        log::debug!("Pool recv: {}", response_line.trim());

        serde_json::from_str(&response_line).map_err(|e| format!("Parse failed: {}", e))
    }
}

fn parse_job(data: &Value) -> Option<Job> {
    let blob_hex = data.get("blob")?.as_str()?;
    let target_hex = data.get("target")?.as_str()?;
    let job_id = data.get("job_id")?.as_str()?.to_string();

    let blob = hex_decode(blob_hex)?;
    let target = hex_decode(target_hex)?;

    Some(Job {
        blob,
        target,
        job_id,
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
