//! Kimi Code usage provider.
//!
//! Shares OAuth credentials with the Kimi Code CLI at
//! `~/.kimi-code/credentials/kimi-code.json`. Access tokens live ~15
//! minutes; on expiry we refresh via https://auth.kimi.com/api/oauth/token
//! and atomically write the new tokens back to the same file. Because the
//! CLI may refresh concurrently (refresh-token rotation), write-back
//! re-reads and compares first, and `invalid_grant` triggers a re-read +
//! single retry.

use crate::model::{DataSource, QuotaWindow, SourceExtras, SourceStatus, UsageSnapshot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const USAGES_URL: &str = "https://api.kimi.com/coding/v1/usages";
const TOKEN_URL: &str = "https://auth.kimi.com/api/oauth/token";
/// Public OAuth client id used by the Kimi Code CLI.
const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiCredentials {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Epoch seconds.
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    /// Preserve unknown fields so write-back doesn't drop CLI data.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl KimiCredentials {
    fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(ts) => ts <= Utc::now().timestamp() + 30, // 30s safety margin
            None => false,
        }
    }
}

fn credentials_path() -> Option<std::path::PathBuf> {
    // Overridable for tests / sandboxing.
    if let Ok(p) = std::env::var("KIMI_CODE_CREDENTIALS_PATH") {
        return Some(std::path::PathBuf::from(p));
    }
    Some(dirs::home_dir()?.join(".kimi-code/credentials/kimi-code.json"))
}

fn read_credentials() -> Option<KimiCredentials> {
    read_credentials_from(&credentials_path()?)
}

fn read_credentials_from(path: &std::path::Path) -> Option<KimiCredentials> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Atomically write credentials back: write temp file in the same
/// directory, then rename over the original.
fn write_credentials_atomic_to(path: &std::path::Path, creds: &KimiCredentials) -> Result<(), String> {
    let dir = path.parent().ok_or("no parent dir")?;
    let tmp = dir.join(format!(".kimi-code.json.{}.tmp", std::process::id()));
    let mut value = serde_json::to_value(creds).map_err(|e| e.to_string())?;
    // Drop nulls for absent optional fields to keep the file tidy.
    if let Some(obj) = value.as_object_mut() {
        obj.retain(|_, v| !v.is_null());
    }
    std::fs::write(&tmp, serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[derive(Debug)]
enum RefreshOutcome {
    /// New tokens obtained and written back.
    Refreshed(KimiCredentials),
    /// Server said the refresh token is dead.
    InvalidGrant,
    /// Network/other transient failure.
    Transient(String),
}

async fn do_refresh(http: &reqwest::Client, token_url: &str, refresh_token: &str) -> RefreshOutcome {
    let resp = http
        .post(token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CLIENT_ID),
        ])
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return RefreshOutcome::Transient(e.to_string()),
    };
    let status = resp.status();
    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return RefreshOutcome::Transient(e.to_string()),
    };
    if !status.is_success() {
        let err = body
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("")
            .to_string();
        return if err == "invalid_grant" {
            RefreshOutcome::InvalidGrant
        } else {
            RefreshOutcome::Transient(format!("{status}: {err}"))
        };
    }
    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    match access_token {
        Some(at) => {
            let expires_in = body.get("expires_in").and_then(|v| v.as_i64());
            let new_refresh = body
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            RefreshOutcome::Refreshed(KimiCredentials {
                access_token: at,
                refresh_token: new_refresh.or_else(|| Some(refresh_token.to_string())),
                expires_at: expires_in.map(|s| Utc::now().timestamp() + s),
                scope: body.get("scope").and_then(|v| v.as_str()).map(|s| s.to_string()),
                token_type: body
                    .get("token_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                expires_in,
                extra: Default::default(),
            })
        }
        None => RefreshOutcome::Transient("no access_token in refresh response".into()),
    }
}

/// Refresh tokens and write back, handling CLI refresh races:
/// re-read the file before writing; if the file's refresh_token changed,
/// the CLI already refreshed — prefer the file's tokens.
async fn refresh_and_persist(http: &reqwest::Client, current: &KimiCredentials) -> Result<KimiCredentials, SourceStatus> {
    let path = credentials_path().ok_or(SourceStatus::NotConfigured)?;
    refresh_and_persist_at(http, TOKEN_URL, &path, current).await
}

async fn refresh_and_persist_at(
    http: &reqwest::Client,
    token_url: &str,
    path: &std::path::Path,
    current: &KimiCredentials,
) -> Result<KimiCredentials, SourceStatus> {
    let Some(rt) = current.refresh_token.clone() else {
        return Err(SourceStatus::NeedRelogin);
    };
    match do_refresh(http, token_url, &rt).await {
        RefreshOutcome::Refreshed(mut new_creds) => {
            // Re-read: if the CLI rotated tokens meanwhile, the file wins.
            if let Some(on_disk) = read_credentials_from(path) {
                if on_disk.refresh_token.as_deref() != current.refresh_token.as_deref() {
                    return Ok(on_disk);
                }
                // Preserve unknown fields from the on-disk record.
                new_creds.extra = on_disk.extra.clone();
            }
            write_credentials_atomic_to(path, &new_creds).map_err(|_| SourceStatus::Stale)?;
            Ok(new_creds)
        }
        RefreshOutcome::InvalidGrant => {
            // Likely a rotation race: re-read the file and use whatever is
            // there now. If it still carries the same dead refresh token,
            // the user must log in again.
            match read_credentials_from(path) {
                Some(on_disk)
                    if on_disk.refresh_token.as_deref() != current.refresh_token.as_deref()
                        && !on_disk.is_expired() =>
                {
                    Ok(on_disk)
                }
                _ => Err(SourceStatus::NeedRelogin),
            }
        }
        RefreshOutcome::Transient(m) => {
            eprintln!("[kimi] token refresh failed: {m}");
            Err(SourceStatus::Stale)
        }
    }
}

// ---- /usages response parsing (defensive, mirrors the CLI's parser) ----

#[derive(Debug, Deserialize)]
struct UsagesResponse {
    user: Option<UsagesUser>,
    usage: Option<UsageRow>,
    #[serde(default)]
    limits: Vec<LimitEntry>,
    #[serde(rename = "boosterWallet")]
    booster_wallet: Option<BoosterWallet>,
}

#[derive(Debug, Deserialize)]
struct UsagesUser {
    membership: Option<Membership>,
}

#[derive(Debug, Deserialize)]
struct Membership {
    level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageRow {
    limit: Option<serde_json::Value>,
    used: Option<serde_json::Value>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LimitEntry {
    window: Option<LimitWindow>,
    detail: Option<UsageRow>,
}

#[derive(Debug, Deserialize)]
struct LimitWindow {
    duration: Option<i64>,
    #[serde(rename = "timeUnit")]
    time_unit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BoosterWallet {
    balance: Option<BoosterBalance>,
}

#[derive(Debug, Deserialize)]
struct BoosterBalance {
    #[serde(rename = "type")]
    kind: Option<String>,
    amount: Option<serde_json::Value>,
    #[serde(rename = "amountLeft")]
    amount_left: Option<serde_json::Value>,
}

/// Values arrive as JSON strings of fixed-point integers (1e-8 = 1 unit).
fn fixed_to_f64(v: Option<&serde_json::Value>) -> Option<f64> {
    match v? {
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        serde_json::Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// 1e-8 fixed-point currency → CNY yuan.
fn fixed_to_cny(v: Option<&serde_json::Value>) -> Option<f64> {
    fixed_to_f64(v).map(|x| x / 1e8)
}

fn parse_reset(s: Option<&String>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s?).ok().map(|d| d.with_timezone(&Utc))
}

fn window_label(window: Option<&LimitWindow>) -> String {
    match window {
        Some(w) => {
            let dur = w.duration.unwrap_or(0);
            match (w.time_unit.as_deref(), dur) {
                (Some("TIME_UNIT_MINUTE"), 300) => "5h".into(),
                (Some("TIME_UNIT_MINUTE"), d) => format!("{d}min"),
                (Some("TIME_UNIT_HOUR"), 1) | (Some("TIME_UNIT_DAY"), 1) => "daily".into(),
                (Some("TIME_UNIT_WEEK"), 1) => "weekly".into(),
                (Some("TIME_UNIT_MONTH"), 1) => "monthly".into(),
                (Some(unit), d) => format!("{d} {unit}"),
                _ => "window".into(),
            }
        }
        None => "window".into(),
    }
}

fn parse_usages(body: &str) -> Result<UsageSnapshot, String> {
    let resp: UsagesResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut snap = UsageSnapshot::new(DataSource::KimiCode, SourceStatus::Ok);

    // Weekly summary ("usage" section).
    if let Some(u) = &resp.usage {
        snap.windows.push(QuotaWindow {
            label: "weekly".into(),
            used: fixed_to_f64(u.used.as_ref()).unwrap_or(0.0),
            quota: fixed_to_f64(u.limit.as_ref()).unwrap_or(0.0),
            reset_at: parse_reset(u.reset_time.as_ref()),
        });
    }
    // Rolling rate windows ("limits" array).
    for entry in &resp.limits {
        if let Some(detail) = &entry.detail {
            snap.windows.push(QuotaWindow {
                label: window_label(entry.window.as_ref()),
                used: fixed_to_f64(detail.used.as_ref()).unwrap_or(0.0),
                quota: fixed_to_f64(detail.limit.as_ref()).unwrap_or(0.0),
                reset_at: parse_reset(detail.reset_time.as_ref()),
            });
        }
    }

    let mut extras = SourceExtras::default();
    if let Some(user) = &resp.user {
        extras.membership = user
            .membership
            .as_ref()
            .and_then(|m| m.level.clone())
            .map(|l| l.trim_start_matches("LEVEL_").to_string());
    }
    if let Some(wallet) = &resp.booster_wallet {
        if let Some(balance) = &wallet.balance {
            if balance.kind.as_deref() == Some("BOOSTER") {
                extras.extra_total_cny = fixed_to_cny(balance.amount.as_ref());
                extras.extra_balance_cny = fixed_to_cny(balance.amount_left.as_ref());
            }
        }
    }
    snap.extras = extras;
    Ok(snap)
}

async fn get_usages(http: &reqwest::Client, access_token: &str) -> Option<reqwest::Response> {
    http.get(USAGES_URL).bearer_auth(access_token).send().await.ok()
}

/// Public entry: fetch a Kimi Code usage snapshot, refreshing the shared
/// OAuth token when needed.
pub async fn fetch_usage(http: &reqwest::Client) -> UsageSnapshot {
    let Some(mut creds) = read_credentials() else {
        return UsageSnapshot::new(DataSource::KimiCode, SourceStatus::NotConfigured);
    };

    if creds.is_expired() {
        match refresh_and_persist(http, &creds).await {
            Ok(c) => creds = c,
            Err(status) => return UsageSnapshot::new(DataSource::KimiCode, status),
        }
    }

    match get_usages(http, &creds.access_token).await {
        Some(resp) if resp.status().as_u16() == 401 => {
            // Token rejected despite appearing fresh: try one refresh cycle.
            match refresh_and_persist(http, &creds).await {
                Ok(c) => match get_usages(http, &c.access_token).await {
                    Some(resp2) => finish(resp2).await,
                    None => UsageSnapshot::new(DataSource::KimiCode, SourceStatus::Stale),
                },
                Err(status) => UsageSnapshot::new(DataSource::KimiCode, status),
            }
        }
        Some(resp) => finish(resp).await,
        None => UsageSnapshot::new(DataSource::KimiCode, SourceStatus::Stale),
    }
}

async fn finish(resp: reqwest::Response) -> UsageSnapshot {
    if resp.status().as_u16() == 401 {
        return UsageSnapshot::new(DataSource::KimiCode, SourceStatus::NeedRelogin);
    }
    if !resp.status().is_success() {
        return UsageSnapshot::new(DataSource::KimiCode, SourceStatus::Stale);
    }
    match resp.text().await {
        Ok(body) => match parse_usages(&body) {
            Ok(snap) => snap,
            Err(_) => UsageSnapshot::new(DataSource::KimiCode, SourceStatus::Stale),
        },
        Err(_) => UsageSnapshot::new(DataSource::KimiCode, SourceStatus::Stale),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real response captured 2026-09-06 (tokens redacted).
    const REAL_RESPONSE: &str = r#"{"user":{"userId":"u","region":"REGION_CN","membership":{"level":"LEVEL_INTERMEDIATE"},"businessId":""},"usage":{"limit":"100","used":"10","remaining":"90","resetTime":"2026-09-08T16:19:14.792728Z"},"limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"limit":"100","used":"1","remaining":"99","resetTime":"2026-09-06T11:19:14.792728Z"}}],"parallel":{"limit":"20"},"totalQuota":{},"authentication":{"method":"METHOD_ACCESS_TOKEN","scope":"FEATURE_CODING"},"subType":"TYPE_PURCHASE","boosterWallet":{"id":"w","userId":"u","balance":{"id":"b","feature":"FEATURE_OMNI","type":"BOOSTER","amount":"7500000000","amountLeft":"4447582000","unit":"UNIT_CURRENCY"},"status":"STATUS_DISABLED"},"domain":"DOMAIN_NEXUS","version":"GOODS_VERSION_V1"}"#;

    #[test]
    fn parses_real_response() {
        let snap = parse_usages(REAL_RESPONSE).unwrap();
        assert_eq!(snap.status, SourceStatus::Ok);
        assert_eq!(snap.extras.membership.as_deref(), Some("INTERMEDIATE"));
        assert_eq!(snap.extras.extra_total_cny, Some(75.0));
        let bal = snap.extras.extra_balance_cny.unwrap();
        assert!((bal - 44.47582).abs() < 1e-6);

        let weekly = snap.windows.iter().find(|w| w.label == "weekly").unwrap();
        assert_eq!(weekly.quota, 100.0);
        assert_eq!(weekly.used, 10.0);
        assert!(weekly.reset_at.is_some());

        let five_h = snap.windows.iter().find(|w| w.label == "5h").unwrap();
        assert_eq!(five_h.used, 1.0);
        assert_eq!(five_h.quota, 100.0);
    }

    #[test]
    fn tolerates_missing_sections() {
        // Unsubscribed / no-extra-usage shape: everything optional.
        let snap = parse_usages(r#"{"user":{"membership":{"level":"LEVEL_FREE"}}}"#).unwrap();
        assert!(snap.windows.is_empty());
        assert_eq!(snap.extras.extra_balance_cny, None);
    }

    #[test]
    fn ignores_non_booster_wallet() {
        let snap = parse_usages(
            r#"{"boosterWallet":{"balance":{"type":"OTHER","amount":"100000000","amountLeft":"1"}}}"#,
        )
        .unwrap();
        assert_eq!(snap.extras.extra_total_cny, None);
    }

    #[test]
    fn credential_expiry_check() {
        let mut c = KimiCredentials {
            access_token: "x".into(),
            refresh_token: None,
            expires_at: Some(Utc::now().timestamp() - 10),
            scope: None,
            token_type: None,
            expires_in: None,
            extra: Default::default(),
        };
        assert!(c.is_expired());
        c.expires_at = Some(Utc::now().timestamp() + 3600);
        assert!(!c.is_expired());
    }

    // ---- refresh flow against a mock token endpoint ----

    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    /// Minimal one-shot HTTP server returning a fixed response.
    fn mock_server(status: &str, body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let status = status.to_string();
        let body = body.to_string();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        });
        format!("http://{addr}/api/oauth/token")
    }

    fn dead_creds() -> KimiCredentials {
        KimiCredentials {
            access_token: "old-at".into(),
            refresh_token: Some("old-rt".into()),
            expires_at: Some(1), // long expired
            scope: None,
            token_type: None,
            expires_in: None,
            extra: Default::default(),
        }
    }

    #[tokio::test]
    async fn refresh_success_writes_back_atomically() {
        let url = mock_server(
            "200 OK",
            r#"{"access_token":"new-at","refresh_token":"new-rt","expires_in":900,"token_type":"Bearer"}"#,
        );
        let dir = std::env::temp_dir().join(format!("apm-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kimi-code.json");
        std::fs::write(&path, serde_json::to_string(&dead_creds()).unwrap()).unwrap();

        let http = reqwest::Client::new();
        let out = refresh_and_persist_at(&http, &url, &path, &dead_creds()).await.unwrap();
        assert_eq!(out.access_token, "new-at");
        assert_eq!(out.refresh_token.as_deref(), Some("new-rt"));
        // File updated in place, no temp files left behind.
        let on_disk = read_credentials_from(&path).unwrap();
        assert_eq!(on_disk.access_token, "new-at");
        assert!(!on_disk.is_expired());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn invalid_grant_with_stale_file_needs_relogin() {
        let url = mock_server("400 Bad Request", r#"{"error":"invalid_grant"}"#);
        let dir = std::env::temp_dir().join(format!("apm-test-ig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kimi-code.json");
        std::fs::write(&path, serde_json::to_string(&dead_creds()).unwrap()).unwrap();

        let http = reqwest::Client::new();
        let err = refresh_and_persist_at(&http, &url, &path, &dead_creds())
            .await
            .unwrap_err();
        assert_eq!(err, SourceStatus::NeedRelogin);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn invalid_grant_but_cli_rotated_uses_file() {
        let url = mock_server("400 Bad Request", r#"{"error":"invalid_grant"}"#);
        let dir = std::env::temp_dir().join(format!("apm-test-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kimi-code.json");
        // The CLI refreshed concurrently: file has NEW tokens, fresh expiry.
        let mut on_disk = dead_creds();
        on_disk.access_token = "cli-at".into();
        on_disk.refresh_token = Some("cli-rt".into());
        on_disk.expires_at = Some(Utc::now().timestamp() + 3600);
        std::fs::write(&path, serde_json::to_string(&on_disk).unwrap()).unwrap();

        let http = reqwest::Client::new();
        let out = refresh_and_persist_at(&http, &url, &path, &dead_creds()).await.unwrap();
        assert_eq!(out.access_token, "cli-at", "file's rotated tokens win");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
