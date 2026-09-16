use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Which upstream service a snapshot belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    KimiCode,
    ArkAgentPlan,
    KiroCli,
    Codex,
}

/// Health of a data source's last refresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceStatus {
    /// Fresh data.
    Ok,
    /// Last refresh failed; data shown is from an earlier successful fetch.
    Stale,
    /// Credentials were rejected (invalid or insufficient permission).
    AuthError,
    /// No credentials available at all.
    NotConfigured,
    /// Credentials exist but must be refreshed by re-login (refresh token dead,
    /// arkcli STS expired, ...).
    NeedRelogin,
}

/// One rolling quota window (5h / daily / weekly / monthly).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaWindow {
    /// Human label, e.g. "5h", "daily", "weekly", "monthly".
    pub label: String,
    pub used: f64,
    pub quota: f64,
    /// When the window resets, if known.
    pub reset_at: Option<DateTime<Utc>>,
}

impl QuotaWindow {
    /// Fraction used in `[0, 1]`. Zero-quota windows report 0.
    pub fn ratio(&self) -> f64 {
        if self.quota > 0.0 {
            (self.used / self.quota).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Source-specific extra info shown in the detail view.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceExtras {
    /// Kimi: membership level, e.g. "LEVEL_INTERMEDIATE".
    pub membership: Option<String>,
    /// Kimi Extra Usage wallet: remaining balance in CNY.
    pub extra_balance_cny: Option<f64>,
    /// Kimi Extra Usage wallet: total topped-up amount in CNY.
    pub extra_total_cny: Option<f64>,
    /// Ark: subscribed plan tier, e.g. "large".
    /// Kiro: subscription title, e.g. "Kiro Power".
    /// Codex: ChatGPT plan type, e.g. "plus".
    pub plan_tier: Option<String>,
    /// Kiro: whether overage billing is enabled.
    pub overage_enabled: Option<bool>,
    /// Kiro: credits consumed beyond the plan allowance.
    pub overage_credits: Option<f64>,
    /// Kiro: estimated overage charges in USD.
    pub overage_cost_usd: Option<f64>,
    /// Kiro: account email.
    pub account_email: Option<String>,
}

/// Unified provider output consumed by the frontend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub source: DataSource,
    pub status: SourceStatus,
    pub windows: Vec<QuotaWindow>,
    pub extras: SourceExtras,
    /// Optional actionable, secret-free status detail for the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub fetched_at: DateTime<Utc>,
}

impl UsageSnapshot {
    pub fn new(source: DataSource, status: SourceStatus) -> Self {
        Self {
            source,
            status,
            windows: Vec::new(),
            extras: SourceExtras::default(),
            message: None,
            fetched_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_serde_roundtrip() {
        let mut snap = UsageSnapshot::new(DataSource::KimiCode, SourceStatus::Ok);
        snap.windows.push(QuotaWindow {
            label: "weekly".into(),
            used: 10.0,
            quota: 100.0,
            reset_at: Some(Utc::now()),
        });
        snap.extras.membership = Some("LEVEL_INTERMEDIATE".into());
        snap.extras.extra_balance_cny = Some(44.48);

        let json = serde_json::to_string(&snap).unwrap();
        let back: UsageSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source, DataSource::KimiCode);
        assert_eq!(back.status, SourceStatus::Ok);
        assert_eq!(back.windows.len(), 1);
        assert_eq!(back.extras.extra_balance_cny, Some(44.48));
    }

    #[test]
    fn codex_serializes_as_codex() {
        assert_eq!(
            serde_json::to_string(&DataSource::Codex).unwrap(),
            r#""codex""#
        );
    }

    #[test]
    fn status_serializes_as_tagged() {
        let s = serde_json::to_string(&SourceStatus::NeedRelogin).unwrap();
        assert_eq!(s, r#"{"kind":"need_relogin"}"#);
    }

    #[test]
    fn zero_quota_ratio_is_zero() {
        let w = QuotaWindow {
            label: "x".into(),
            used: 5.0,
            quota: 0.0,
            reset_at: None,
        };
        assert_eq!(w.ratio(), 0.0);
    }
}
