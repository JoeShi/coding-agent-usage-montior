//! Dev probe: exercises all provider paths against real services and
//! prints results. Run with: cargo run --example probe
//!
//! Also verifies the AK/SK signing path using VOLCENGINE_ACCESS_KEY /
//! VOLCENGINE_SECRET_KEY env vars (validation only, nothing persisted).

use agent_plan_monitor_lib::providers;
use agent_plan_monitor_lib::volc_sigv4::{self, Credentials, SignRequest};

#[tokio::main]
async fn main() {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap();

    println!("=== Kimi Code (shared CLI credentials) ===");
    let kimi = providers::kimi::fetch_usage(&http).await;
    println!("{}", serde_json::to_string_pretty(&kimi).unwrap());

    println!("\n=== Ark AgentPlan (auto: keychain AK/SK -> arkcli STS) ===");
    let ark = providers::ark::fetch_afp_usage(&http).await;
    println!("{}", serde_json::to_string_pretty(&ark).unwrap());

    println!("\n=== Ark GetUsageDetails (yesterday, Day) ===");
    let yesterday = (chrono::Utc::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    match providers::ark::fetch_usage_details(&http, &yesterday, &yesterday, "Day").await {
        Ok(rows) => println!(
            "{} rows; first 3:\n{}",
            rows.len(),
            serde_json::to_string_pretty(&rows.iter().take(3).collect::<Vec<_>>()).unwrap()
        ),
        Err(e) => println!("error: {:?}", e),
    }

    println!("\n=== Ark via env AK/SK (validation-only signing path) ===");
    match (
        std::env::var("VOLCENGINE_ACCESS_KEY"),
        std::env::var("VOLCENGINE_SECRET_KEY"),
    ) {
        (Ok(ak), Ok(sk)) => {
            let creds = Credentials {
                access_key: ak,
                secret_key: sk,
                session_token: None,
            };
            let body = "{}";
            let query = vec![
                ("Action".to_string(), "GetAFPUsage".to_string()),
                ("Version".to_string(), "2024-01-01".to_string()),
            ];
            let signed = volc_sigv4::sign(
                &creds,
                "cn-beijing",
                "ark",
                &SignRequest {
                    method: "POST",
                    host: "ark.cn-beijing.volcengineapi.com",
                    path: "/",
                    query,
                    content_type: "application/json",
                    body: body.as_bytes(),
                    now: chrono::Utc::now(),
                },
            );
            let resp = http
                .post("https://ark.cn-beijing.volcengineapi.com/?Action=GetAFPUsage&Version=2024-01-01")
                .header("content-type", "application/json")
                .header("x-date", &signed.x_date)
                .header("x-content-sha256", &signed.x_content_sha256)
                .header("authorization", &signed.authorization)
                .body(body.to_string())
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let text = r.text().await.unwrap_or_default();
                    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                    match v.pointer("/ResponseMetadata/Error") {
                        Some(e) => println!("rejected: {e}"),
                        None => println!(
                            "accepted: PlanType={:?}",
                            v.pointer("/Result/PlanType").and_then(|p| p.as_str())
                        ),
                    }
                }
                Err(e) => println!("network error: {e}"),
            }
        }
        _ => println!("skipped (VOLCENGINE_ACCESS_KEY/SECRET_KEY not set)"),
    }

    println!("\n=== Keychain roundtrip (self-cleaning) ===");
    let entry = keyring::Entry::new("com.agentplanmonitor.app", "probe_test");
    match entry {
        Ok(e) => {
            let r = e
                .set_password("probe-secret")
                .and_then(|_| e.get_password())
                .and_then(|v| e.delete_credential().map(|_| v));
            match r {
                Ok(v) => println!(
                    "ok: wrote, read back ({}), deleted",
                    if v == "probe-secret" {
                        "match"
                    } else {
                        "MISMATCH"
                    }
                ),
                Err(e) => println!("error: {e}"),
            }
        }
        Err(e) => println!("error: {e}"),
    }

    println!("\n=== Codex (ChatGPT login via local app-server; live, not CI-safe) ===");
    let started = std::time::Instant::now();
    let codex = providers::codex::fetch_usage().await;
    println!(
        "status: {:?}; plan: {:?}; elapsed: {:.2?}",
        codex.status,
        codex.extras.plan_tier,
        started.elapsed()
    );
    if let Some(message) = codex.message.as_deref() {
        println!("message: {message}");
    }
    for window in &codex.windows {
        println!(
            "window: {} = {:.1}/{:.1} ({:.0}%); reset: {}",
            window.label,
            window.used,
            window.quota,
            window.ratio() * 100.0,
            window
                .reset_at
                .as_ref()
                .map(chrono::DateTime::to_rfc3339)
                .unwrap_or_else(|| "unknown".into())
        );
    }

    println!("\n=== Kiro (keychain kiro_api_key, or KIRO_API_KEY env) ===");
    let kiro = providers::kiro::fetch_usage(&http).await;
    println!("keychain path -> status: {:?}", kiro.status);
    if let Ok(key) = std::env::var("KIRO_API_KEY") {
        let snap = providers::kiro::fetch_with_key(&http, &key).await;
        println!(
            "env key path ->\n{}",
            serde_json::to_string_pretty(&snap).unwrap()
        );
    }
}
