//! Bounded, credential-safe JSON-RPC transport. Blocking calls run only on workers.
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::fmt;
use std::io::Read;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime};

pub const MAINNET_GENESIS: &str = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";
pub const REWARD_BATCH_SIZE: usize = 10;
const MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
static ACTIVE: Mutex<usize> = Mutex::new(0);
static AVAILABLE: Condvar = Condvar::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcError {
    pub code: &'static str,
    pub message: &'static str,
}
impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for RpcError {}
impl RpcError {
    pub fn invalid() -> Self {
        Self {
            code: "INVALID_RESPONSE",
            message: "Provider returned an unsupported or inconsistent response.",
        }
    }
}
#[derive(Clone)]
pub struct RequestContext {
    deadline: Instant,
    pub cancelled: Arc<AtomicBool>,
}
impl RequestContext {
    pub fn new(budget: Duration, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            deadline: Instant::now() + budget,
            cancelled,
        }
    }
    pub fn check(&self) -> Result<(), RpcError> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(RpcError {
                code: "INTERRUPTED",
                message: "Inspection cancelled.",
            });
        }
        if Instant::now() >= self.deadline {
            return Err(RpcError {
                code: "DEADLINE",
                message: "Inspection deadline reached; retry to continue missing data.",
            });
        }
        Ok(())
    }
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
    fn wait(&self, delay: Duration) -> Result<(), RpcError> {
        self.check()?;
        if delay >= self.remaining() {
            return Err(RpcError {
                code: "DEADLINE",
                message: "Provider backoff exceeds the remaining inspection budget.",
            });
        }
        let until = Instant::now() + delay;
        while Instant::now() < until {
            self.check()?;
            std::thread::sleep(
                until
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(25)),
            );
        }
        self.check()
    }
}
struct Permit;
impl Permit {
    fn acquire(ctx: &RequestContext) -> Result<Self, RpcError> {
        let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            ctx.check()?;
            if *active < 4 {
                *active += 1;
                return Ok(Self);
            }
            active = AVAILABLE
                .wait_timeout(active, Duration::from_millis(25))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        AVAILABLE.notify_one();
    }
}

// Intentionally no Debug implementation: the endpoint carries credentials.
#[derive(Clone)]
pub struct Helius {
    client: Client,
    endpoint: reqwest::Url,
    timeout: Duration,
}
impl Helius {
    pub fn new(key: &str) -> Result<Self, RpcError> {
        Self::with_endpoint(
            "https://mainnet.helius-rpc.com/",
            key,
            Duration::from_secs(10),
        )
    }
    /// Explicit constructor for local mock servers; no endpoint override is exposed by the CLI.
    pub fn with_endpoint(endpoint: &str, key: &str, timeout: Duration) -> Result<Self, RpcError> {
        let mut endpoint = reqwest::Url::parse(endpoint).map_err(|_| RpcError::invalid())?;
        endpoint.query_pairs_mut().append_pair("api-key", key);
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| RpcError {
                code: "INTERNAL",
                message: "Unable to initialize HTTP transport.",
            })?;
        Ok(Self {
            client,
            endpoint,
            timeout: timeout.min(Duration::from_secs(10)),
        })
    }
    pub fn verify_mainnet(&self, ctx: &RequestContext) -> Result<(), RpcError> {
        if self.call("getGenesisHash", json!([]), ctx)?.as_str() != Some(MAINNET_GENESIS) {
            return Err(RpcError {
                code: "WRONG_NETWORK",
                message: "Provider network does not match mainnet.",
            });
        }
        Ok(())
    }
    pub fn call(
        &self,
        method: &str,
        params: Value,
        ctx: &RequestContext,
    ) -> Result<Value, RpcError> {
        let _permit = Permit::acquire(ctx)?;
        for attempt in 0..=2u32 {
            ctx.check()?;
            let response = self
                .client
                .post(self.endpoint.clone())
                .timeout(self.timeout.min(ctx.remaining()))
                .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
                .send();
            let mut retry_after = None;
            let failure = match response {
                Err(error) => RpcError {
                    code: if error.is_timeout() {
                        "PROVIDER_TIMEOUT"
                    } else {
                        "PROVIDER_FAILURE"
                    },
                    message: "Provider connection failed or timed out.",
                },
                Ok(response) => {
                    let status = response.status();
                    if status.as_u16() == 401 || status.as_u16() == 403 {
                        return Err(RpcError {
                            code: "PROVIDER_AUTH",
                            message: "Helius rejected the configured API key or method entitlement.",
                        });
                    }
                    if status.as_u16() == 429 || status.is_server_error() {
                        retry_after = response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|h| h.to_str().ok())
                            .and_then(|h| {
                                h.parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
                                    httpdate::parse_http_date(h)
                                        .ok()
                                        .and_then(|t| t.duration_since(SystemTime::now()).ok())
                                })
                            });
                        RpcError {
                            code: if status.as_u16() == 429 {
                                "PROVIDER_RATE_LIMIT"
                            } else {
                                "PROVIDER_FAILURE"
                            },
                            message: "Provider is rate limited or temporarily unavailable.",
                        }
                    } else if !status.is_success() {
                        return Err(RpcError {
                            code: "PROVIDER_FAILURE",
                            message: "Provider rejected the request.",
                        });
                    } else {
                        'body: {
                            if response
                                .content_length()
                                .is_some_and(|n| n > MAX_RESPONSE_BYTES)
                            {
                                return Err(RpcError::invalid());
                            }
                            let mut bytes = Vec::new();
                            if response
                                .take(MAX_RESPONSE_BYTES + 1)
                                .read_to_end(&mut bytes)
                                .is_err()
                            {
                                break 'body RpcError {
                                    code: "PROVIDER_FAILURE",
                                    message: "Provider response was interrupted.",
                                };
                            }
                            ctx.check()?;
                            if bytes.len() as u64 > MAX_RESPONSE_BYTES {
                                return Err(RpcError::invalid());
                            }
                            let value: Value =
                                serde_json::from_slice(&bytes).map_err(|_| RpcError::invalid())?;
                            if value["jsonrpc"] != "2.0" || value["id"] != 1 {
                                return Err(RpcError::invalid());
                            }
                            if let Some(error) = value.get("error") {
                                let code = error["code"].as_i64();
                                if code == Some(-32001) {
                                    return Err(RpcError {
                                        code: "PROVIDER_AUTH",
                                        message: "Helius rejected the configured API key or method entitlement.",
                                    });
                                }
                                if !matches!(code, Some(-32005 | -32002 | -32003 | -32603)) {
                                    return Err(RpcError {
                                        code: "PROVIDER_FAILURE",
                                        message: "Provider rejected the RPC query.",
                                    });
                                }
                                RpcError {
                                    code: if code == Some(-32005) {
                                        "PROVIDER_RATE_LIMIT"
                                    } else {
                                        "PROVIDER_FAILURE"
                                    },
                                    message: "Provider RPC is temporarily unavailable.",
                                }
                            } else {
                                return value.get("result").cloned().ok_or_else(RpcError::invalid);
                            }
                        }
                    }
                }
            };
            if attempt == 2 {
                return Err(failure);
            }
            let jitter = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| u64::from(d.subsec_millis() % 101));
            ctx.wait(retry_after.unwrap_or(Duration::from_millis((100u64 << attempt) + jitter)))?;
        }
        unreachable!("bounded retry loop always returns")
    }
}
