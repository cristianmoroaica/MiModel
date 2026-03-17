//! Fetch Claude API usage stats (5-hour and 7-day limits) via OAuth API.

use serde::Deserialize;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(60);
const CREDENTIALS_PATH: &str = ".claude/.credentials.json";

#[derive(Debug, Clone, Default)]
pub struct UsageStats {
    pub five_hour_pct: Option<f64>,
    pub seven_day_pct: Option<f64>,
    pub five_hour_reset: Option<String>,
    pub seven_day_reset: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthCredentials {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OAuthToken>,
}

#[derive(Debug, Deserialize)]
struct OAuthToken {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageResponse {
    five_hour: Option<UsageBucket>,
    seven_day: Option<UsageBucket>,
}

#[derive(Debug, Deserialize)]
struct UsageBucket {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

/// Shared usage state with built-in caching.
pub struct UsageMonitor {
    stats: Arc<Mutex<UsageStats>>,
    last_fetch: Arc<Mutex<Option<Instant>>>,
}

impl UsageMonitor {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(Mutex::new(UsageStats::default())),
            last_fetch: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the current cached stats.
    pub fn stats(&self) -> UsageStats {
        self.stats.lock().unwrap().clone()
    }

    /// Trigger a background refresh if the cache is stale.
    /// Returns immediately — the update happens in a spawned thread.
    pub fn maybe_refresh(&self) {
        let should_fetch = {
            let last = self.last_fetch.lock().unwrap();
            match *last {
                None => true,
                Some(t) => t.elapsed() >= CACHE_TTL,
            }
        };
        if !should_fetch {
            return;
        }

        // Mark as fetched now to prevent concurrent fetches
        *self.last_fetch.lock().unwrap() = Some(Instant::now());

        let stats = Arc::clone(&self.stats);
        std::thread::spawn(move || {
            if let Some(new_stats) = fetch_usage_stats() {
                *stats.lock().unwrap() = new_stats;
            }
        });
    }
}

/// Read OAuth token and fetch usage from the Anthropic API.
fn fetch_usage_stats() -> Option<UsageStats> {
    let home = dirs::home_dir()?;
    let creds_path = home.join(CREDENTIALS_PATH);
    let creds_data = std::fs::read_to_string(&creds_path).ok()?;
    let creds: OAuthCredentials = serde_json::from_str(&creds_data).ok()?;
    let token = creds.claude_ai_oauth?.access_token?;

    let output = Command::new("curl")
        .args([
            "-s",
            "--max-time", "3",
            "https://api.anthropic.com/api/oauth/usage",
            "-H", &format!("Authorization: Bearer {token}"),
            "-H", "anthropic-beta: oauth-2025-04-20",
            "-H", "Content-Type: application/json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let body = String::from_utf8(output.stdout).ok()?;
    let resp: UsageResponse = serde_json::from_str(&body).ok()?;

    Some(UsageStats {
        five_hour_pct: resp.five_hour.as_ref().and_then(|b| b.utilization),
        seven_day_pct: resp.seven_day.as_ref().and_then(|b| b.utilization),
        five_hour_reset: resp.five_hour.as_ref().and_then(|b| b.resets_at.clone()),
        seven_day_reset: resp.seven_day.as_ref().and_then(|b| b.resets_at.clone()),
    })
}

/// Format a reset timestamp as a relative duration (e.g. "1h22m", "45m").
pub fn format_reset_time(resets_at: &str) -> Option<String> {
    let reset_time = chrono::DateTime::parse_from_rfc3339(resets_at).ok()?;
    let now = chrono::Utc::now();
    let diff = reset_time.signed_duration_since(now);
    if diff.num_seconds() <= 0 {
        return None;
    }
    let total_min = diff.num_minutes();
    if total_min >= 60 {
        Some(format!("{}h{}m", total_min / 60, total_min % 60))
    } else {
        Some(format!("{}m", total_min))
    }
}
