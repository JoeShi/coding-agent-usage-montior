//! Kiro CLI subscription usage provider.
//!
//! Calls the CodeWhisperer control-plane `GetUsageLimits` — the same call
//! the Kiro CLI's `/usage` command makes. Plain Bearer auth with a `ksk_`
//! API key; no signing, no refresh. The edge rejects unrecognized
//! User-Agents with 403, so we present as the CLI's aws-sdk-rust UA.

use crate::credentials;
use crate::model::{DataSource, QuotaWindow, SourceStatus, UsageSnapshot};
use chrono::{DateTime, Utc};
use serde::Deserialize;

const URL: &str = "https://codewhisperer.us-east-1.amazonaws.com/?isEmailRequired=true";
const TARGET: &str = "AmazonCodeWhispererService.GetUsageLimits";
/// The service edge 403s unknown agents; mirror the Kiro CLI UA.
const API_UA: &str =
    "aws-sdk-rust/1.3.10 ua/2.1 api/codewhispererruntime os/cli lang/rust app/AmazonQ-For-CLI";
const BODY: &str = r#"{"isEmailRequired":true}"#;

#[derive(Debug, Deserialize)]
struct UsageLimitsResponse {
    #[serde(rename = "subscriptionInfo")]
    subscription_info: Option<SubscriptionInfo>,
    #[serde(rename = "usageBreakdownList", default)]
    usage_breakdown_list: Vec<UsageBreakdown>,
    #[serde(rename = "overageConfiguration")]
    overage_configuration: Option<OverageConfiguration>,
    #[serde(rename = "userInfo")]
    user_info: Option<UserInfo>,
    #[serde(rename = "nextDateReset")]
    next_date_reset: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SubscriptionInfo {
    #[serde(rename = "subscriptionTitle")]
    subscription_title: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageBreakdown {
    #[serde(rename = "resourceType")]
    resource_type: Option<String>,
    #[serde(rename = "currentUsageWithPrecision")]
    current_usage_with_precision: Option<f64>,
    #[serde(rename = "currentUsage")]
    current_usage: Option<f64>,
    #[serde(rename = "usageLimit")]
    usage_limit: Option<f64>,
    #[serde(rename = "nextDateReset")]
    next_date_reset: Option<f64>,
    #[serde(rename = "currentOverages")]
    current_overages: Option<f64>,
    #[serde(rename = "overageCharges")]
    overage_charges: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OverageConfiguration {
    #[serde(rename = "overageStatus")]
    overage_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    email: Option<String>,
}

fn epoch_secs(ts: Option<f64>) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(ts? as i64, 0)
}

fn parse(body: &str) -> Result<UsageSnapshot, String> {
    let resp: UsageLimitsResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut snap = UsageSnapshot::new(DataSource::KiroCli, SourceStatus::Ok);

    let credit = resp
        .usage_breakdown_list
        .iter()
        .find(|b| b.resource_type.as_deref() == Some("CREDIT"));
    if let Some(c) = credit {
        snap.windows.push(QuotaWindow {
            label: "monthly".into(),
            used: c.current_usage_with_precision.or(c.current_usage).unwrap_or(0.0),
            quota: c.usage_limit.unwrap_or(0.0),
            reset_at: epoch_secs(c.next_date_reset).or_else(|| epoch_secs(resp.next_date_reset)),
        });
        snap.extras.overage_credits = c.current_overages;
        snap.extras.overage_cost_usd = c.overage_charges;
    }

    snap.extras.plan_tier = resp
        .subscription_info
        .as_ref()
        .and_then(|s| s.subscription_title.clone().or(s.kind.clone()));
    snap.extras.overage_enabled = resp
        .overage_configuration
        .as_ref()
        .and_then(|o| o.overage_status.as_deref())
        .map(|s| s == "ENABLED");
    snap.extras.account_email = resp.user_info.and_then(|u| u.email);
    Ok(snap)
}

/// Public entry: fetch a Kiro usage snapshot.
pub async fn fetch_usage(http: &reqwest::Client) -> UsageSnapshot {
    let Some(key) = credentials::get_kiro_api_key() else {
        return UsageSnapshot::new(DataSource::KiroCli, SourceStatus::NotConfigured);
    };
    fetch_with_key(http, &key).await
}

/// Fetch with an explicit key (used by save-and-validate before persisting).
pub async fn fetch_with_key(http: &reqwest::Client, key: &str) -> UsageSnapshot {
    fetch_with_key_at(http, URL, key).await
}

async fn fetch_with_key_at(http: &reqwest::Client, url: &str, key: &str) -> UsageSnapshot {
    let resp = http
        .post(url)
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/x-amz-json-1.0")
        .header("x-amz-target", TARGET)
        .header("tokentype", "API_KEY")
        .header("user-agent", API_UA)
        .header("x-amz-user-agent", API_UA)
        .body(BODY)
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(_) => return UsageSnapshot::new(DataSource::KiroCli, SourceStatus::Stale),
    };
    if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
        return UsageSnapshot::new(DataSource::KiroCli, SourceStatus::AuthError);
    }
    if !resp.status().is_success() {
        return UsageSnapshot::new(DataSource::KiroCli, SourceStatus::Stale);
    }
    match resp.text().await {
        Ok(body) => match parse(&body) {
            Ok(snap) => snap,
            Err(_) => UsageSnapshot::new(DataSource::KiroCli, SourceStatus::Stale),
        },
        Err(_) => UsageSnapshot::new(DataSource::KiroCli, SourceStatus::Stale),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape per the reference implementation (kiro_account_usage.py).
    const SAMPLE: &str = r#"{
        "subscriptionInfo": {"subscriptionTitle": "Kiro Pro", "type": "Q"},
        "usageBreakdownList": [{
            "resourceType": "CREDIT",
            "currentUsageWithPrecision": 1234.56,
            "usageLimit": 10000.0,
            "nextDateReset": 1790000000,
            "currentOverages": 12.5,
            "overageCharges": 0.5
        }],
        "overageConfiguration": {"overageStatus": "ENABLED"},
        "userInfo": {"email": "user@example.com"}
    }"#;

    #[test]
    fn parses_credit_window_and_extras() {
        let snap = parse(SAMPLE).unwrap();
        assert_eq!(snap.status, SourceStatus::Ok);
        assert_eq!(snap.windows.len(), 1);
        let w = &snap.windows[0];
        assert_eq!(w.label, "monthly");
        assert_eq!(w.used, 1234.56);
        assert_eq!(w.quota, 10000.0);
        assert!(w.reset_at.is_some());
        assert_eq!(snap.extras.plan_tier.as_deref(), Some("Kiro Pro"));
        assert_eq!(snap.extras.overage_enabled, Some(true));
        assert_eq!(snap.extras.overage_credits, Some(12.5));
        assert_eq!(snap.extras.overage_cost_usd, Some(0.5));
        assert_eq!(snap.extras.account_email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn tolerates_missing_sections() {
        let snap = parse(r#"{"userInfo":{}}"#).unwrap();
        assert!(snap.windows.is_empty());
        assert_eq!(snap.extras.plan_tier, None);
        assert_eq!(snap.extras.overage_enabled, None);
    }

    #[test]
    fn falls_back_to_top_level_reset() {
        let snap = parse(
            r#"{"usageBreakdownList":[{"resourceType":"CREDIT","currentUsage":1.0,"usageLimit":100.0}],"nextDateReset":1790000000}"#,
        )
        .unwrap();
        assert!(snap.windows[0].reset_at.is_some());
    }

    #[test]
    fn ignores_non_credit_breakdowns() {
        let snap = parse(
            r#"{"usageBreakdownList":[{"resourceType":"OTHER","currentUsage":5.0,"usageLimit":10.0}]}"#,
        )
        .unwrap();
        assert!(snap.windows.is_empty());
    }

    // ---- error mapping against a mock endpoint ----

    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

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
                "HTTP/1.1 {status}\r\ncontent-type: application/x-amz-json-1.0\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        });
        format!("http://{addr}/")
    }

    #[tokio::test]
    async fn auth_error_on_403() {
        // The edge 403s unknown UAs / invalid keys alike.
        let url = mock_server("403 Forbidden", r#"{"message":"token is invalid"}"#);
        let http = reqwest::Client::new();
        let snap = fetch_with_key_at(&http, &url, "ksk_bad").await;
        assert_eq!(snap.status, SourceStatus::AuthError);
    }

    #[tokio::test]
    async fn ok_on_valid_response() {
        let url = mock_server("200 OK", SAMPLE);
        let http = reqwest::Client::new();
        let snap = fetch_with_key_at(&http, &url, "ksk_ok").await;
        assert_eq!(snap.status, SourceStatus::Ok);
        assert_eq!(snap.windows[0].label, "monthly");
    }

    #[tokio::test]
    async fn stale_on_server_error() {
        let url = mock_server("500 Internal Server Error", "boom");
        let http = reqwest::Client::new();
        let snap = fetch_with_key_at(&http, &url, "ksk_ok").await;
        assert_eq!(snap.status, SourceStatus::Stale);
    }
}
