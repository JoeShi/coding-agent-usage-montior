//! Volcengine Ark AgentPlan (personal) usage provider.
//!
//! Control-plane endpoints, V4-signed with arkcli SSO-derived STS:
//! - GetAFPUsage:      5h/1w/1m rolling-window AFP quota (AFPDaily ignored:
//!                     a phantom always-zero field for personal plans)
//! - GetUsageDetails:  per-model usage details (Day/Hour granularity)

use crate::credentials::{self, ArkCredError};
use crate::model::{DataSource, QuotaWindow, SourceExtras, SourceStatus, UsageSnapshot};
use crate::volc_sigv4::{self, Credentials, SignRequest};
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;

const HOST: &str = "ark.cn-beijing.volcengineapi.com";
const REGION: &str = "cn-beijing";
const SERVICE: &str = "ark";
const VERSION: &str = "2024-01-01";

#[derive(Debug, Deserialize)]
struct TopResponse<T> {
    #[serde(rename = "Result")]
    result: Option<T>,
    #[serde(rename = "ResponseMetadata")]
    metadata: Option<ResponseMeta>,
}

#[derive(Debug, Deserialize)]
struct ResponseMeta {
    #[serde(rename = "Error")]
    error: Option<MetaError>,
}

#[derive(Debug, Deserialize)]
struct MetaError {
    #[serde(rename = "Code")]
    code: Option<String>,
    #[serde(rename = "Message")]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AfpResult {
    #[serde(rename = "AFPFiveHour")]
    five_hour: Option<AfpWindow>,
    #[serde(rename = "AFPWeekly")]
    weekly: Option<AfpWindow>,
    #[serde(rename = "AFPMonthly")]
    monthly: Option<AfpWindow>,
    #[serde(rename = "PlanType")]
    plan_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AfpWindow {
    #[serde(rename = "Quota")]
    quota: Option<f64>,
    #[serde(rename = "Used")]
    used: Option<f64>,
    #[serde(rename = "ResetTime")]
    reset_time: Option<i64>,
}

/// One row of per-model usage detail.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UsageDetail {
    pub billing_type: String,
    pub model: String,
    pub time: DateTime<Utc>,
    pub unit: String,
    pub usage: f64,
}

#[derive(Debug, Deserialize)]
struct DetailsResult {
    #[serde(rename = "Details")]
    details: Option<Vec<DetailRow>>,
}

#[derive(Debug, Deserialize)]
struct DetailRow {
    #[serde(rename = "BillingType")]
    billing_type: Option<String>,
    #[serde(rename = "ObjectName")]
    object_name: Option<String>,
    #[serde(rename = "Time")]
    time: Option<i64>,
    #[serde(rename = "Unit")]
    unit: Option<String>,
    #[serde(rename = "Usage")]
    usage: Option<f64>,
}

#[derive(Debug)]
pub enum FetchError {
    Status(SourceStatus),
    /// Transient error (network, 5xx, parse): caller keeps previous data (Stale).
    Transient(String),
}

fn epoch_ms(ms: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(ms).single()
}

async fn call<T: for<'de> Deserialize<'de>>(
    http: &reqwest::Client,
    creds: &Credentials,
    action: &str,
    body: &str,
) -> Result<T, FetchError> {
    let query = vec![
        ("Action".to_string(), action.to_string()),
        ("Version".to_string(), VERSION.to_string()),
    ];
    let signed = volc_sigv4::sign(
        creds,
        REGION,
        SERVICE,
        &SignRequest {
            method: "POST",
            host: HOST,
            path: "/",
            query: query.clone(),
            content_type: "application/json",
            body: body.as_bytes(),
            now: Utc::now(),
        },
    );
    let qs = query
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let url = format!("https://{HOST}/?{qs}");

    let mut builder = http
        .post(&url)
        .header("content-type", "application/json")
        .header("x-date", &signed.x_date)
        .header("x-content-sha256", &signed.x_content_sha256)
        .header("authorization", &signed.authorization)
        .body(body.to_string());
    if let Some(token) = &signed.session_token {
        builder = builder.header("x-security-token", token);
    }

    let resp = builder
        .send()
        .await
        .map_err(|e| FetchError::Transient(e.to_string()))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| FetchError::Transient(e.to_string()))?;

    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(FetchError::Status(SourceStatus::AuthError));
    }
    let parsed: TopResponse<T> =
        serde_json::from_str(&text).map_err(|e| FetchError::Transient(format!("parse: {e}")))?;
    if let Some(meta) = &parsed.metadata {
        if let Some(err) = &meta.error {
            let code = err.code.as_deref().unwrap_or("");
            let msg = err.message.as_deref().unwrap_or("").to_string();
            return Err(match code {
                "InvalidAuthorization" | "SignatureDoesNotMatch" | "InvalidAccessKey"
                | "AccessDenied" | "ExpiredToken" | "ExpiredTokenException" => {
                    FetchError::Status(SourceStatus::AuthError)
                }
                _ => FetchError::Transient(format!("{code}: {msg}")),
            });
        }
    }
    parsed
        .result
        .ok_or_else(|| FetchError::Transient("missing Result".into()))
}

/// Fetch the AFP rolling windows (5h / weekly / monthly) as a unified
/// snapshot. The response's AFPDaily field is a phantom for AgentPlan
/// personal plans (always 0 used) and is ignored.
pub async fn fetch_afp_usage(http: &reqwest::Client) -> UsageSnapshot {
    // Credential resolution may spawn the arkcli refresh subprocess (up to
    // 30s), so keep it off the async runtime.
    let creds = match tokio::task::spawn_blocking(credentials::resolve_ark_credentials).await {
        Ok(Ok(c)) => c,
        Ok(Err(ArkCredError::NotConfigured)) | Err(_) => {
            return UsageSnapshot::new(DataSource::ArkAgentPlan, SourceStatus::NotConfigured)
        }
        Ok(Err(ArkCredError::ArkCliExpired)) => {
            return UsageSnapshot::new(DataSource::ArkAgentPlan, SourceStatus::NeedRelogin)
        }
    };

    match call::<AfpResult>(http, &creds, "GetAFPUsage", "{}").await {
        Ok(r) => {
            let mut snap = UsageSnapshot::new(DataSource::ArkAgentPlan, SourceStatus::Ok);
            let mut push = |label: &str, w: Option<AfpWindow>| {
                if let Some(w) = w {
                    snap.windows.push(QuotaWindow {
                        label: label.into(),
                        used: w.used.unwrap_or(0.0),
                        quota: w.quota.unwrap_or(0.0),
                        reset_at: w.reset_time.and_then(epoch_ms),
                    });
                }
            };
            push("5h", r.five_hour);
            push("weekly", r.weekly);
            push("monthly", r.monthly);
            snap.extras = SourceExtras {
                plan_tier: r.plan_type,
                ..Default::default()
            };
            snap
        }
        Err(FetchError::Status(s)) => UsageSnapshot::new(DataSource::ArkAgentPlan, s),
        Err(FetchError::Transient(_)) => {
            UsageSnapshot::new(DataSource::ArkAgentPlan, SourceStatus::Stale)
        }
    }
}

/// Fetch per-model usage details for a date range (YYYY-MM-DD, inclusive).
pub async fn fetch_usage_details(
    http: &reqwest::Client,
    start: &str,
    end: &str,
    interval: &str,
) -> Result<Vec<UsageDetail>, FetchError> {
    let creds = tokio::task::spawn_blocking(credentials::resolve_ark_credentials)
        .await
        .unwrap_or(Err(ArkCredError::NotConfigured))
        .map_err(|e| match e {
            ArkCredError::NotConfigured => FetchError::Status(SourceStatus::NotConfigured),
            ArkCredError::ArkCliExpired => FetchError::Status(SourceStatus::NeedRelogin),
        })?;
    let body = serde_json::json!({
        "QueryInterval": interval,
        "Filter": { "StartTime": start, "EndTime": end }
    })
    .to_string();
    let r = call::<DetailsResult>(http, &creds, "GetUsageDetails", &body).await?;
    Ok(r.details
        .unwrap_or_default()
        .into_iter()
        .filter_map(|d| {
            Some(UsageDetail {
                billing_type: d.billing_type?,
                model: d.object_name?,
                time: epoch_ms(d.time?)?,
                unit: d.unit.unwrap_or_else(|| "Tokens".into()),
                usage: d.usage.unwrap_or(0.0),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_afp_response_and_ignores_daily() {
        // AFPDaily stays in the fixture on purpose: the server returns it
        // (always 0 used for AgentPlan personal) and we must ignore it.
        let body = r#"{"ResponseMetadata":{"Action":"GetAFPUsage","Region":"cn-beijing","RequestId":"x","Service":"ark","Version":"2024-01-01"},"Result":{"AFPDaily":{"Quota":87500,"ResetTime":1788710400000,"SubscribeTime":1788624000000,"Used":0},"AFPFiveHour":{"Quota":25000,"ResetTime":1788696456000,"SubscribeTime":1788678456000,"Used":16319.764},"AFPMonthly":{"Quota":246566.5901,"ResetTime":1791215999000,"SubscribeTime":1788622484000,"Used":19827.5754},"AFPWeekly":{"Quota":125000,"ResetTime":1788710400000,"SubscribeTime":1788105600000,"Used":19827.5754},"PlanType":"large"}}"#;
        let parsed: TopResponse<AfpResult> = serde_json::from_str(body).unwrap();
        let r = parsed.result.unwrap();
        assert_eq!(r.plan_type.as_deref(), Some("large"));
        assert_eq!(r.five_hour.unwrap().quota, Some(25000.0));
        assert_eq!(r.weekly.unwrap().quota, Some(125000.0));
        assert_eq!(r.monthly.unwrap().used, Some(19827.5754));
    }

    #[test]
    fn parses_details_response() {
        let body = r#"{"ResponseMetadata":{"Action":"GetUsageDetails","Region":"cn-beijing","RequestId":"x","Service":"ark","Version":"2024-01-01"},"Result":{"Details":[{"BillingType":"WithinPlan","ObjectName":"kimi-k3","Time":1788624000000,"Unit":"Tokens","Usage":23680038}]}}"#;
        let parsed: TopResponse<DetailsResult> = serde_json::from_str(body).unwrap();
        let rows = parsed.result.unwrap().details.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].object_name.as_deref(), Some("kimi-k3"));
        assert_eq!(rows[0].usage, Some(23680038.0));
    }

    #[test]
    fn parses_error_response() {
        let body = r#"{"ResponseMetadata":{"RequestId":"x","Action":"GetAFPUsage","Version":"2024-01-01","Error":{"CodeN":100024,"Code":"InvalidAuthorization","Message":"Invalid 'Authorization' header"}}}"#;
        let parsed: TopResponse<AfpResult> = serde_json::from_str(body).unwrap();
        assert!(parsed.result.is_none());
        let code = parsed
            .metadata
            .and_then(|m| m.error)
            .and_then(|e| e.code)
            .unwrap();
        assert_eq!(code, "InvalidAuthorization");
    }
}
