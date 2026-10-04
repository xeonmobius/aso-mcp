use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

/// Shared HTTP client with global pacing across all store endpoints.
/// Apple's public APIs tolerate ~20 req/min; we stay well under with a
/// fixed minimum gap between any two outbound requests.
pub struct HttpClient {
    client: reqwest::Client,
    last_request: Mutex<Instant>,
}

const MIN_GAP: Duration = Duration::from_millis(1100);

impl HttpClient {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("AppStore/3.0 iOS/18.0 model/iPhone16,2 hwp/t8130 build/22A3354 (6; dt:326) AMS/1")
            .build()
            .context("building HTTP client")?;
        Ok(Self {
            client,
            last_request: Mutex::new(Instant::now() - Duration::from_secs(10)),
        })
    }

    pub async fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<String> {
        self.pace().await;
        let mut req = self.client.get(url);
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let resp = req.send().await.context("request failed")?;
        let status = resp.status();
        let body = resp.text().await.context("reading response body")?;
        if !status.is_success() {
            let snippet: String = body.chars().take(200).collect();
            anyhow::bail!("HTTP {status}: {snippet}");
        }
        Ok(body)
    }

    pub async fn post(&self, url: &str, headers: &[(&str, String)], body: String) -> Result<String> {
        self.pace().await;
        let mut req = self.client.post(url).body(body);
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let resp = req.send().await.context("request failed")?;
        let status = resp.status();
        let text = resp.text().await.context("reading response body")?;
        if !status.is_success() {
            let snippet: String = text.chars().take(200).collect();
            anyhow::bail!("HTTP {status}: {snippet}");
        }
        Ok(text)
    }

    async fn pace(&self) {
        loop {
            let wait = {
                let mut last = self.last_request.lock().unwrap();
                let elapsed = last.elapsed();
                if elapsed >= MIN_GAP {
                    *last = Instant::now();
                    None
                } else {
                    Some(MIN_GAP - elapsed)
                }
            };
            match wait {
                None => return,
                Some(d) => tokio::time::sleep(d).await,
            }
        }
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new().expect("HttpClient::new")
    }
}
