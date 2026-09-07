//! Credential access: macOS Keychain for the Kiro API Key, and reuse of
//! arkcli's SSO-derived STS credentials (with silent refresh) for Ark.

use crate::volc_sigv4::Credentials;
use chrono::Utc;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const KEYCHAIN_SERVICE: &str = "com.agentplanmonitor.app";
const KEYCHAIN_KIRO: &str = "kiro_api_key";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArkCredError {
    /// No arkcli login state exists on this machine.
    NotConfigured,
    /// arkcli SSO identity is dead (silent refresh failed); user must run
    /// `arkcli auth login`.
    ArkCliExpired,
}

/// Resolve Ark credentials from arkcli's SSO login state, the only
/// supported source. Triggers a silent STS refresh when the cached token
/// is expired or expiring.
pub fn resolve_ark_credentials() -> Result<Credentials, ArkCredError> {
    arkcli_sts()
}

// ---------------------------------------------------------------- Kiro

pub fn get_kiro_api_key() -> Option<String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_KIRO)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.is_empty())
}

pub fn save_kiro_api_key(key: &str) -> Result<(), String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_KIRO)
        .and_then(|e| e.set_password(key))
        .map_err(|e| e.to_string())
}

pub fn clear_kiro_api_key() -> Result<(), String> {
    let _ = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_KIRO).map(|e| e.delete_credential());
    Ok(())
}

/// Treat tokens expiring within this window as already expired, so a signing
/// request never uses a token that dies mid-flight.
const STS_EXPIRY_SKEW_MS: i64 = 60_000;

/// arkcli's STS lives at `~/.arkcli/identities/<identity>/sts.json` ({ak, sk,
/// session_token, expires_at} in epoch ms). arkcli silently refreshes it on
/// any command while the SSO identity is valid, so an expired sts.json only
/// means "arkcli has not run recently" — run `arkcli auth status` once to
/// trigger that refresh, then re-read. `~/.arkcli/.env` is never consulted:
/// its VOLCENGINE_STS_* values are written only at login time and go stale.
fn arkcli_sts() -> Result<Credentials, ArkCredError> {
    match arkcli_sts_from_identities() {
        Ok(c) => Ok(c),
        Err(e) => {
            // Only users who actually logged in via arkcli get a refresh
            // attempt; never spawn a subprocess for Keychain-only users.
            if !arkcli_identities_dir().is_dir() {
                return Err(e);
            }
            try_refresh_arkcli_sts();
            // After a refresh attempt, "directory exists but no usable
            // credential" means the identity is dead, not "not configured".
            match arkcli_sts_from_identities() {
                Err(ArkCredError::NotConfigured) => Err(ArkCredError::ArkCliExpired),
                r => r,
            }
        }
    }
}

fn arkcli_identities_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".arkcli/identities")
}

fn is_sts_expired(expires_ms: i64, now_ms: i64) -> bool {
    expires_ms <= now_ms + STS_EXPIRY_SKEW_MS
}

fn arkcli_sts_from_identities() -> Result<Credentials, ArkCredError> {
    let entries =
        std::fs::read_dir(arkcli_identities_dir()).map_err(|_| ArkCredError::NotConfigured)?;
    let mut best: Option<(i64, Credentials)> = None;
    for entry in entries.flatten() {
        let content = match std::fs::read_to_string(entry.path().join("sts.json")) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let v: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let (Some(ak), Some(sk), Some(token)) = (
            v.get("ak").and_then(|x| x.as_str()),
            v.get("sk").and_then(|x| x.as_str()),
            v.get("session_token").and_then(|x| x.as_str()),
        ) else {
            continue;
        };
        let expires_ms = v.get("expires_at").and_then(|x| x.as_i64()).unwrap_or(0);
        let creds = Credentials {
            access_key: ak.to_string(),
            secret_key: sk.to_string(),
            session_token: Some(token.to_string()),
        };
        if best.as_ref().map(|(e, _)| expires_ms > *e).unwrap_or(true) {
            best = Some((expires_ms, creds));
        }
    }
    let (expires_ms, creds) = best.ok_or(ArkCredError::NotConfigured)?;
    if is_sts_expired(expires_ms, Utc::now().timestamp_millis()) {
        return Err(ArkCredError::ArkCliExpired);
    }
    Ok(creds)
}

// ---------------------------------------------------------- silent refresh

const REFRESH_COOLDOWN: Duration = Duration::from_secs(90);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(30);

struct RefreshState {
    last_attempt: Option<Instant>,
}

static REFRESH_STATE: OnceLock<Mutex<RefreshState>> = OnceLock::new();

/// Trigger arkcli's silent STS refresh by running `arkcli auth status
/// --format json`. Best-effort: any failure (missing binary, spawn error,
/// timeout, non-zero exit) is swallowed — the caller re-reads sts.json and
/// judges the result. Serialized through a mutex with a cooldown so
/// concurrent/accelerated polls never spawn more than one arkcli process.
fn try_refresh_arkcli_sts() {
    let Some(bin) = find_arkcli_binary() else {
        return;
    };
    let state = REFRESH_STATE.get_or_init(|| Mutex::new(RefreshState { last_attempt: None }));
    {
        // Poisoned lock or a recent attempt: skip silently.
        let mut guard = match state.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if guard
            .last_attempt
            .is_some_and(|t| t.elapsed() < REFRESH_COOLDOWN)
        {
            return;
        }
        // Stamp before releasing so concurrent callers see the cooldown and
        // skip; do not hold the lock across the subprocess wait.
        guard.last_attempt = Some(Instant::now());
    }

    let mut child = match std::process::Command::new(bin)
        .args(["auth", "status", "--format", "json"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < REFRESH_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            Err(_) => break,
        }
    }
}

/// Locate the arkcli binary. GUI apps launched from Finder get a minimal
/// PATH, so probe the common install locations after trying `which`.
fn find_arkcli_binary() -> Option<PathBuf> {
    if let Ok(out) = std::process::Command::new("which").arg("arkcli").output() {
        if out.status.success() {
            let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
            if !p.as_os_str().is_empty() && p.exists() {
                return Some(p);
            }
        }
    }
    // nvm: ~/.nvm/versions/node/*/bin/arkcli — lexicographically last wins.
    if let Some(home) = dirs::home_dir() {
        if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            let mut candidates: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path().join("bin/arkcli"))
                .filter(|p| p.exists())
                .collect();
            candidates.sort();
            if let Some(p) = candidates.pop() {
                return Some(p);
            }
        }
    }
    if let Ok(out) = std::process::Command::new("npm")
        .args(["prefix", "-g"])
        .output()
    {
        if out.status.success() {
            let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()).join("bin/arkcli");
            if p.exists() {
                return Some(p);
            }
        }
    }
    for p in ["/usr/local/bin/arkcli", "/opt/homebrew/bin/arkcli"] {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sts_expiry_uses_safety_skew() {
        let now = 1_000_000;
        // Fresh: expires well beyond the skew window.
        assert!(!is_sts_expired(now + 120_000, now));
        // Exactly at now + skew: treated as expired.
        assert!(is_sts_expired(now + 60_000, now));
        // Already past.
        assert!(is_sts_expired(now - 1, now));
    }
}
