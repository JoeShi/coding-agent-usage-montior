//! Codex ChatGPT plan usage via the local Codex app-server.
//!
//! Codex remains the sole owner of authentication and token refresh. This
//! provider never reads auth.json or logs protocol payloads.

use crate::model::{DataSource, QuotaWindow, SourceStatus, UsageSnapshot};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const SESSION_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchFailure {
    MissingCli,
    Unsupported,
    SignedOut,
    ApiKeyOnly,
    NeedRelogin,
    AuthError,
    Transient,
}

#[derive(Debug, Deserialize)]
struct RpcEnvelope {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Debug)]
enum SessionError {
    Timeout,
    Closed,
    Protocol,
    Rpc(RpcError),
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountReadResult {
    account: Option<AccountInfo>,
    #[serde(default)]
    requires_openai_auth: bool,
    #[serde(default, alias = "planType")]
    chatgpt_plan_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountInfo {
    #[serde(rename = "type")]
    account_type: Option<String>,
    #[serde(default, alias = "chatgptPlanType")]
    plan_type: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitsReadResult {
    rate_limits: Option<RateLimitBucket>,
    #[serde(default)]
    rate_limits_by_limit_id: BTreeMap<String, RateLimitBucket>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitBucket {
    #[serde(default)]
    limit_id: Option<String>,
    #[serde(default)]
    limit_name: Option<String>,
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    primary: Option<RateLimitWindow>,
    #[serde(default)]
    secondary: Option<RateLimitWindow>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RateLimitWindow {
    used_percent: Option<f64>,
    #[serde(default)]
    window_duration_mins: Option<i64>,
    #[serde(default)]
    resets_at: Option<i64>,
}

pub async fn fetch_usage() -> UsageSnapshot {
    match tokio::task::spawn_blocking(fetch_usage_blocking).await {
        Ok(snapshot) => snapshot,
        Err(_) => failure_snapshot(FetchFailure::Transient),
    }
}

fn fetch_usage_blocking() -> UsageSnapshot {
    let Some(binary) = find_codex_binary() else {
        return failure_snapshot(FetchFailure::MissingCli);
    };
    match fetch_from_binary(&binary, SESSION_TIMEOUT) {
        Ok(snapshot) => snapshot,
        Err(error) => failure_snapshot(error),
    }
}

fn fetch_from_binary(binary: &Path, timeout: Duration) -> Result<UsageSnapshot, FetchFailure> {
    let (account, limits) = query_app_server(binary, timeout)?;
    let plan = classify_account(&account)?;
    normalize_limits(plan, limits)
}

fn query_app_server(
    binary: &Path,
    timeout: Duration,
) -> Result<(AccountReadResult, RateLimitsReadResult), FetchFailure> {
    let mut child = codex_command(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| FetchFailure::Transient)?;
    let mut stdin = child.stdin.take().ok_or(FetchFailure::Transient)?;
    let stdout = child.stdout.take().ok_or(FetchFailure::Transient)?;
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || read_protocol(stdout, tx));
    let deadline = Instant::now() + timeout;

    let result = (|| {
        send_message(
            &mut stdin,
            &json!({
                "method": "initialize",
                "id": 0,
                "params": {
                    "clientInfo": {
                        "name": "agent_plan_monitor",
                        "title": "Agent Plan Monitor",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            }),
        )?;
        wait_for_response(&rx, 0, deadline).map_err(classify_initialize_error)?;

        send_message(&mut stdin, &json!({"method": "initialized", "params": {}}))?;
        send_message(
            &mut stdin,
            &json!({
                "method": "account/read",
                "id": 1,
                "params": {"refreshToken": false}
            }),
        )?;
        let account_value = wait_for_response(&rx, 1, deadline).map_err(classify_account_error)?;
        let account: AccountReadResult =
            serde_json::from_value(account_value).map_err(|_| FetchFailure::Transient)?;

        send_message(
            &mut stdin,
            &json!({"method": "account/rateLimits/read", "id": 2}),
        )?;
        let limits_value = wait_for_response(&rx, 2, deadline).map_err(classify_account_error)?;
        let limits: RateLimitsReadResult =
            serde_json::from_value(limits_value).map_err(|_| FetchFailure::Transient)?;
        Ok((account, limits))
    })();

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    result
}

fn send_message(stdin: &mut ChildStdin, message: &Value) -> Result<(), FetchFailure> {
    serde_json::to_writer(&mut *stdin, message).map_err(|_| FetchFailure::Transient)?;
    stdin
        .write_all(b"\n")
        .map_err(|_| FetchFailure::Transient)?;
    stdin.flush().map_err(|_| FetchFailure::Transient)
}

fn read_protocol(stdout: impl std::io::Read, tx: mpsc::Sender<Result<String, SessionError>>) {
    let mut reader = BufReader::new(stdout);
    let mut total = 0usize;
    loop {
        let mut bytes = Vec::new();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) => {
                let _ = tx.send(Err(SessionError::Closed));
                return;
            }
            Ok(_) => {
                total = total.saturating_add(bytes.len());
                if bytes.len() > MAX_LINE_BYTES || total > MAX_OUTPUT_BYTES {
                    let _ = tx.send(Err(SessionError::Protocol));
                    return;
                }
                while matches!(bytes.last(), Some(b'\n' | b'\r')) {
                    bytes.pop();
                }
                match String::from_utf8(bytes) {
                    Ok(line) => {
                        if tx.send(Ok(line)).is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(Err(SessionError::Protocol));
                        return;
                    }
                }
            }
            Err(_) => {
                let _ = tx.send(Err(SessionError::Protocol));
                return;
            }
        }
    }
}

fn wait_for_response(
    rx: &mpsc::Receiver<Result<String, SessionError>>,
    expected_id: u64,
    deadline: Instant,
) -> Result<Value, SessionError> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(SessionError::Timeout)?;
        let line = rx.recv_timeout(remaining).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => SessionError::Timeout,
            mpsc::RecvTimeoutError::Disconnected => SessionError::Closed,
        })??;
        let message: RpcEnvelope =
            serde_json::from_str(&line).map_err(|_| SessionError::Protocol)?;
        if message.id != Some(expected_id) {
            continue;
        }
        if let Some(error) = message.error {
            return Err(SessionError::Rpc(error));
        }
        return message.result.ok_or(SessionError::Protocol);
    }
}

fn classify_initialize_error(error: SessionError) -> FetchFailure {
    match error {
        SessionError::Closed => FetchFailure::Unsupported,
        SessionError::Rpc(error) if is_unsupported_rpc(&error) => FetchFailure::Unsupported,
        _ => FetchFailure::Transient,
    }
}

fn classify_account_error(error: SessionError) -> FetchFailure {
    match error {
        SessionError::Rpc(error) => classify_rpc_error(&error),
        _ => FetchFailure::Transient,
    }
}

fn is_unsupported_rpc(error: &RpcError) -> bool {
    let message = error.message.to_ascii_lowercase();
    error.code == -32601
        || message.contains("method not found")
        || message.contains("unknown method")
        || message.contains("not supported")
}

fn classify_rpc_error(error: &RpcError) -> FetchFailure {
    let message = error.message.to_ascii_lowercase();
    if is_unsupported_rpc(error) {
        FetchFailure::Unsupported
    } else if message.contains("not logged in") || message.contains("signed out") {
        FetchFailure::SignedOut
    } else if error.code == 401
        || message.contains("unauthorized")
        || message.contains("token expired")
        || message.contains("refresh token")
        || message.contains("relogin")
        || message.contains("re-login")
    {
        FetchFailure::NeedRelogin
    } else if error.code == 403
        || message.contains("forbidden")
        || message.contains("permission")
        || message.contains("not allowed")
    {
        FetchFailure::AuthError
    } else {
        FetchFailure::Transient
    }
}

fn classify_account(account: &AccountReadResult) -> Result<Option<String>, FetchFailure> {
    let Some(info) = account.account.as_ref() else {
        let _ = account.requires_openai_auth;
        return Err(FetchFailure::SignedOut);
    };
    let kind = info
        .account_type
        .as_deref()
        .unwrap_or_default()
        .replace(['_', '-'], "")
        .to_ascii_lowercase();
    if kind == "apikey" {
        return Err(FetchFailure::ApiKeyOnly);
    }
    if kind == "amazonbedrock" {
        return Err(FetchFailure::ApiKeyOnly);
    }
    Ok(info
        .plan_type
        .clone()
        .or_else(|| account.chatgpt_plan_type.clone()))
}

fn normalize_limits(
    account_plan: Option<String>,
    limits: RateLimitsReadResult,
) -> Result<UsageSnapshot, FetchFailure> {
    let mut buckets: Vec<(String, RateLimitBucket)> = if limits.rate_limits_by_limit_id.is_empty() {
        limits
            .rate_limits
            .into_iter()
            .map(|bucket| {
                let key = bucket.limit_id.clone().unwrap_or_else(|| "codex".into());
                (key, bucket)
            })
            .collect()
    } else {
        limits.rate_limits_by_limit_id.into_iter().collect()
    };
    buckets.sort_by(|(left, _), (right, _)| {
        (left != "codex", left.as_str()).cmp(&(right != "codex", right.as_str()))
    });

    let mut snapshot = UsageSnapshot::new(DataSource::Codex, SourceStatus::Ok);
    snapshot.extras.plan_tier = non_empty(account_plan);
    for (key, bucket) in buckets {
        if snapshot.extras.plan_tier.is_none() {
            snapshot.extras.plan_tier = non_empty(bucket.plan_type.clone());
        }
        append_bucket_windows(&mut snapshot.windows, &key, &bucket);
    }
    if snapshot.windows.is_empty() {
        return Err(FetchFailure::Transient);
    }
    Ok(snapshot)
}

fn append_bucket_windows(windows: &mut Vec<QuotaWindow>, key: &str, bucket: &RateLimitBucket) {
    let prefix = bucket_prefix(key, bucket);
    let primary_duration = bucket.primary.as_ref().and_then(|w| w.window_duration_mins);
    let secondary_duration = bucket
        .secondary
        .as_ref()
        .and_then(|w| w.window_duration_mins);
    let duplicate_duration = bucket.primary.is_some()
        && bucket.secondary.is_some()
        && primary_duration == secondary_duration;

    if let Some(window) = bucket.primary.as_ref() {
        if let Some(window) = normalize_window(
            window,
            window_label(prefix.as_deref(), window, "主窗口", duplicate_duration),
        ) {
            windows.push(window);
        }
    }
    if let Some(window) = bucket.secondary.as_ref() {
        if let Some(window) = normalize_window(
            window,
            window_label(prefix.as_deref(), window, "次窗口", duplicate_duration),
        ) {
            windows.push(window);
        }
    }
}

fn bucket_prefix(key: &str, bucket: &RateLimitBucket) -> Option<String> {
    if let Some(name) = non_empty(bucket.limit_name.clone()) {
        if !name.eq_ignore_ascii_case("codex") {
            return Some(name);
        }
    }
    let id = bucket.limit_id.as_deref().unwrap_or(key);
    (!id.eq_ignore_ascii_case("codex")).then(|| id.to_string())
}

fn window_label(
    prefix: Option<&str>,
    window: &RateLimitWindow,
    role: &str,
    include_role: bool,
) -> String {
    let mut label = duration_label(window.window_duration_mins, role);
    if include_role {
        label.push_str(&format!("（{role}）"));
    }
    match prefix {
        Some(prefix) => format!("{prefix} · {label}"),
        None => label,
    }
}

fn duration_label(minutes: Option<i64>, fallback: &str) -> String {
    match minutes.filter(|value| *value > 0) {
        Some(300) => "5 小时".into(),
        Some(10_080) => "每周".into(),
        Some(value) if value % 1_440 == 0 => format!("{} 天", value / 1_440),
        Some(value) if value % 60 == 0 => format!("{} 小时", value / 60),
        Some(value) => format!("{value} 分钟"),
        None => fallback.into(),
    }
}

fn normalize_window(window: &RateLimitWindow, label: String) -> Option<QuotaWindow> {
    let used = window.used_percent?.max(0.0);
    let reset_at = window
        .resets_at
        .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0));
    Some(QuotaWindow {
        label,
        used,
        quota: 100.0,
        reset_at,
    })
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn failure_snapshot(error: FetchFailure) -> UsageSnapshot {
    let (status, message, diagnostic) = match error {
        FetchFailure::MissingCli => (
            SourceStatus::NotConfigured,
            "未找到 Codex CLI，请先安装 Codex。",
            "binary not found",
        ),
        FetchFailure::Unsupported => (
            SourceStatus::NotConfigured,
            "当前 Codex CLI 不支持套餐限额查询，请升级 Codex。",
            "app-server capability unavailable",
        ),
        FetchFailure::SignedOut => (
            SourceStatus::NotConfigured,
            "Codex 尚未登录，请运行 codex login 并使用 ChatGPT 账号登录。",
            "not signed in",
        ),
        FetchFailure::ApiKeyOnly => (
            SourceStatus::NotConfigured,
            "Codex 当前使用 API Key；套餐限额监控需要 ChatGPT 登录。",
            "unsupported account mode",
        ),
        FetchFailure::NeedRelogin => (
            SourceStatus::NeedRelogin,
            "Codex 登录已失效，请重新运行 codex login。",
            "login refresh failed",
        ),
        FetchFailure::AuthError => (
            SourceStatus::AuthError,
            "Codex 账号或工作区无权读取套餐限额。",
            "account permission denied",
        ),
        FetchFailure::Transient => (
            SourceStatus::Stale,
            "Codex 用量暂时无法读取，将在下次轮询重试。",
            "transient app-server failure",
        ),
    };
    crate::log::log(&format!("codex refresh: {diagnostic}"));
    let mut snapshot = UsageSnapshot::new(DataSource::Codex, status);
    snapshot.message = Some(message.into());
    snapshot
}

fn codex_command(binary: &Path) -> Command {
    let mut command = Command::new(binary);
    command.args(["app-server", "--stdio"]);
    if let Some(directory) = binary.parent() {
        let existing = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![directory.to_path_buf()];
        paths.extend(std::env::split_paths(&existing));
        if let Ok(path) = std::env::join_paths(paths) {
            command.env("PATH", path);
        }
    }
    command
}

fn find_codex_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH").and_then(|value| find_on_path(&value, "codex")) {
        return Some(path);
    }
    if let Some(home) = dirs::home_dir() {
        if let Some(path) = find_nvm_binary(&home) {
            return Some(path);
        }
    }
    if let Ok(output) = Command::new("npm").args(["prefix", "-g"]).output() {
        if output.status.success() {
            let path =
                PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()).join("bin/codex");
            if path.is_file() {
                return Some(path);
            }
        }
    }
    ["/opt/homebrew/bin/codex", "/usr/local/bin/codex"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

fn find_on_path(path: &OsStr, binary: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|directory| directory.join(binary))
        .find(|candidate| candidate.is_file())
}

fn find_nvm_binary(home: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(home.join(".nvm/versions/node")).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path().join("bin/codex"))
        .filter(|path| path.is_file())
        .collect();
    candidates.sort();
    candidates.pop()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "agent-plan-monitor-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn write_executable(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn discovers_nvm_codex_with_minimal_path() {
        let home = temp_dir("codex-nvm");
        let binary = home.join(".nvm/versions/node/v24.20.0/bin/codex");
        write_executable(&binary, "#!/bin/sh\nexit 0\n");
        assert_eq!(find_nvm_binary(&home), Some(binary));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn app_server_timeout_kills_child_promptly() {
        let directory = temp_dir("codex-timeout");
        let binary = directory.join("codex");
        write_executable(&binary, "#!/bin/sh\nexec sleep 5\n");
        let started = Instant::now();
        let result = query_app_server(&binary, Duration::from_millis(100));
        assert_eq!(result.unwrap_err(), FetchFailure::Transient);
        assert!(started.elapsed() < Duration::from_secs(2));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fake_app_server_produces_normalized_snapshot() {
        let directory = temp_dir("codex-session");
        let binary = directory.join("codex");
        write_executable(
            &binary,
            r#"#!/bin/sh
IFS= read -r _
printf '%s\n' '{"id":0,"result":{}}'
IFS= read -r _
IFS= read -r _
printf '%s\n' '{"id":1,"result":{"account":{"type":"chatgpt","planType":"plus"},"requiresOpenaiAuth":true}}'
IFS= read -r _
printf '%s\n' '{"id":2,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":99,"windowDurationMins":1,"resetsAt":1}},"rateLimitsByLimitId":{"codex":{"limitId":"codex","primary":{"usedPercent":25,"windowDurationMins":300,"resetsAt":1730947200},"secondary":{"usedPercent":50,"windowDurationMins":10080,"resetsAt":1730950800}},"codex_spark":{"limitId":"codex_spark","limitName":"Spark","primary":{"usedPercent":10,"windowDurationMins":300,"resetsAt":1730947200}}}}}'
"#,
        );
        let snapshot = fetch_from_binary(&binary, Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.status, SourceStatus::Ok);
        assert_eq!(snapshot.extras.plan_tier.as_deref(), Some("plus"));
        assert_eq!(
            snapshot.windows.len(),
            3,
            "multi-bucket view wins over fallback"
        );
        assert_eq!(snapshot.windows[0].label, "5 小时");
        assert_eq!(snapshot.windows[1].label, "每周");
        assert_eq!(snapshot.windows[2].label, "Spark · 5 小时");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn single_bucket_fallback_omits_missing_secondary() {
        let limits: RateLimitsReadResult = serde_json::from_value(json!({
            "rateLimits": {
                "limitId": "codex",
                "primary": {"usedPercent": 42, "windowDurationMins": 60, "resetsAt": 1730947200}
            }
        }))
        .unwrap();
        let snapshot = normalize_limits(Some("pro".into()), limits).unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].label, "1 小时");
        assert_eq!(snapshot.windows[0].used, 42.0);
        assert_eq!(snapshot.windows[0].quota, 100.0);
    }

    #[test]
    fn classifies_account_modes_and_rpc_errors() {
        let signed_out = AccountReadResult::default();
        assert_eq!(classify_account(&signed_out), Err(FetchFailure::SignedOut));

        let api_key: AccountReadResult = serde_json::from_value(json!({
            "account": {"type": "apiKey"}
        }))
        .unwrap();
        assert_eq!(classify_account(&api_key), Err(FetchFailure::ApiKeyOnly));

        assert_eq!(
            classify_rpc_error(&RpcError {
                code: -32601,
                message: "Method not found".into()
            }),
            FetchFailure::Unsupported
        );
        assert_eq!(
            classify_rpc_error(&RpcError {
                code: 401,
                message: "Unauthorized".into()
            }),
            FetchFailure::NeedRelogin
        );
        assert_eq!(
            classify_rpc_error(&RpcError {
                code: 403,
                message: "Forbidden".into()
            }),
            FetchFailure::AuthError
        );
        assert_eq!(
            classify_rpc_error(&RpcError {
                code: -32000,
                message: "Server overloaded".into()
            }),
            FetchFailure::Transient
        );
    }
}
