//! Torn API v2 client in native Rust.
//! Includes sliding-window rate limiting, exponential backoff, concurrency caps,
//! in-memory caching, and dynamic Hardware TPM Vault integration.

use crate::vault;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};

const BASE_URL: &str = "https://api.torn.com/v2";
const CLIENT_USER_AGENT: &str = "MobHub-Rust/0.1 (sovereign native faction ops)";
const CACHE_TTL: Duration = Duration::from_secs(30);
const MAX_REQUESTS_PER_MINUTE: usize = 80;
const RATE_WINDOW: Duration = Duration::from_secs(60);
const MAX_CONCURRENT_REQUESTS: usize = 6;

#[derive(Debug)]
pub enum TornError {
    MissingKey,
    Network(String),
    Http { status: u16, detail: String },
    Api { code: i64, message: String },
    Serialization(String),
}

impl fmt::Display for TornError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TornError::MissingKey => write!(f, "Missing Torn API key in TPM vault or environment"),
            TornError::Network(s) => write!(f, "Network error: {}", s),
            TornError::Http { status, detail } => write!(f, "Torn API HTTP {}: {}", status, detail),
            TornError::Api { code, message } => write!(f, "Torn API error {}: {}", code, message),
            TornError::Serialization(s) => write!(f, "Serialization error: {}", s),
        }
    }
}

impl std::error::Error for TornError {}

struct CacheEntry {
    stored_at: Instant,
    data: serde_json::Value,
}

#[derive(Clone)]
pub struct TornClient {
    http: reqwest::Client,
    semaphore: Arc<Semaphore>,
    request_timestamps: Arc<Mutex<Vec<Instant>>>,
    cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
    mock_mode: bool,
    mock_responses: Arc<Mutex<HashMap<String, serde_json::Value>>>,
}

impl Default for TornClient {
    fn default() -> Self {
        Self::new()
    }
}

impl TornClient {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(CLIENT_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Failed to initialize HTTP client");

        Self {
            http,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS)),
            request_timestamps: Arc::new(Mutex::new(Vec::new())),
            cache: Arc::new(Mutex::new(HashMap::new())),
            mock_mode: false,
            mock_responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn new_mock() -> Self {
        let mut client = Self::new();
        client.mock_mode = true;
        client
    }

    pub async fn set_mock_response(&self, path: &str, data: serde_json::Value) {
        let mut map = self.mock_responses.lock().await;
        map.insert(path.to_string(), data);
    }

    async fn wait_for_rate_limit_slot(&self) {
        loop {
            let mut timestamps = self.request_timestamps.lock().await;
            let now = Instant::now();
            timestamps.retain(|&t| now.duration_since(t) < RATE_WINDOW);

            if timestamps.len() < MAX_REQUESTS_PER_MINUTE {
                timestamps.push(now);
                break;
            }

            let oldest = timestamps[0];
            let wait_time =
                RATE_WINDOW.saturating_sub(now.duration_since(oldest)) + Duration::from_millis(10);
            drop(timestamps);
            tokio::time::sleep(wait_time).await;
        }
    }

    pub async fn get(&self, path: &str, use_cache: bool) -> Result<serde_json::Value, TornError> {
        if self.mock_mode {
            let map = self.mock_responses.lock().await;
            if let Some(val) = map.get(path) {
                return Ok(val.clone());
            }
            return Ok(serde_json::json!({ "status": "mock_default", "path": path }));
        }

        let cache_key = path.to_string();
        if use_cache {
            let cache = self.cache.lock().await;
            if let Some(entry) = cache.get(&cache_key) {
                if entry.stored_at.elapsed() < CACHE_TTL {
                    return Ok(entry.data.clone());
                }
            }
        }

        let api_key = vault::get_torn_api_key().ok_or(TornError::MissingKey)?;
        let sep = if path.contains('?') { "&" } else { "?" };
        let full_url = format!("{}{}{}key={}", BASE_URL, path, sep, api_key);

        let mut attempt = 0;
        let max_attempts = 4;
        let mut last_error = None;

        while attempt < max_attempts {
            attempt += 1;
            let _permit = self
                .semaphore
                .acquire()
                .await
                .map_err(|e| TornError::Network(e.to_string()))?;
            self.wait_for_rate_limit_slot().await;

            let response = match self.http.get(&full_url).send().await {
                Ok(res) => res,
                Err(err) => {
                    let redacted = vault::redact_secrets(&err.to_string());
                    last_error = Some(TornError::Network(redacted));
                    tokio::time::sleep(Duration::from_millis(500 * (1 << attempt))).await;
                    continue;
                }
            };

            let status = response.status();
            if status == 429 || status.is_server_error() {
                let wait_ms = 1000 * (1 << attempt);
                tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                continue;
            }

            let text = response
                .text()
                .await
                .map_err(|e| TornError::Network(vault::redact_secrets(&e.to_string())))?;
            let json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| TornError::Serialization(e.to_string()))?;

            if let Some(err_obj) = json.get("error").and_then(|e| e.as_object()) {
                let code = err_obj.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                let msg = err_obj
                    .get("error")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown Torn API error");
                if (code == 5 || code == 17) && attempt < max_attempts {
                    tokio::time::sleep(Duration::from_millis(1500 * attempt as u64)).await;
                    continue;
                }
                return Err(TornError::Api {
                    code,
                    message: msg.to_string(),
                });
            }

            if let Some(err_name) = json.get("error_name").and_then(|e| e.as_str()) {
                let code = json
                    .get("error_code")
                    .and_then(|c| c.as_i64())
                    .unwrap_or(status.as_u16() as i64);
                return Err(TornError::Api {
                    code,
                    message: err_name.to_string(),
                });
            }

            if !status.is_success() {
                return Err(TornError::Http {
                    status: status.as_u16(),
                    detail: format!("Torn API HTTP status {}", status.as_u16()),
                });
            }

            if use_cache {
                let mut cache = self.cache.lock().await;
                cache.insert(
                    cache_key.clone(),
                    CacheEntry {
                        stored_at: Instant::now(),
                        data: json.clone(),
                    },
                );
            }

            return Ok(json);
        }

        Err(last_error.unwrap_or(TornError::Network(
            "Max retry attempts exceeded".to_string(),
        )))
    }

    pub async fn get_ranked_wars(&self) -> Result<serde_json::Value, TornError> {
        self.get("/faction/rankedwars", true).await
    }

    pub async fn get_ranked_war_report(&self, war_id: i64) -> Result<serde_json::Value, TornError> {
        self.get(&format!("/faction/{}/rankedwarreport", war_id), true)
            .await
    }

    pub async fn get_faction_members(&self) -> Result<serde_json::Value, TornError> {
        self.get("/faction/members", true).await
    }

    pub async fn get_faction_balance(&self) -> Result<serde_json::Value, TornError> {
        self.get("/faction/balance", true).await
    }

    pub async fn get_bounties(&self) -> Result<serde_json::Value, TornError> {
        self.get("/torn/bounties", true).await
    }

    pub async fn get_faction_news(
        &self,
        category: &str,
        from_unix: Option<i64>,
        to_unix: Option<i64>,
        limit: Option<usize>,
    ) -> Result<serde_json::Value, TornError> {
        let lim = limit.unwrap_or(100);
        let mut path = format!("/faction/news?cat={}&limit={}", category, lim);
        if let Some(from) = from_unix {
            path.push_str(&format!("&from={}", from));
        }
        if let Some(to) = to_unix {
            path.push_str(&format!("&to={}", to));
        }
        self.get(&path, false).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_mock_client_operations() {
        let client = TornClient::new_mock();
        client
            .set_mock_response("/faction/rankedwars", json!({ "wars": [40711] }))
            .await;

        let res = client.get_ranked_wars().await.unwrap();
        assert_eq!(res["wars"][0], 40711);

        client
            .set_mock_response(
                "/faction/news?cat=depositFunds&limit=100",
                json!({ "news": [{ "id": "1", "text": "deposited" }] }),
            )
            .await;

        let news = client
            .get_faction_news("depositFunds", None, None, None)
            .await
            .unwrap();
        assert_eq!(news["news"][0]["id"], "1");
    }
}
