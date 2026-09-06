//! Credential access: macOS Keychain for user-configured Ark AK/SK,
//! and read-only reuse of arkcli's SSO-derived STS credentials.

use crate::volc_sigv4::Credentials;
use chrono::Utc;

const KEYCHAIN_SERVICE: &str = "com.agentplanmonitor.app";
const KEYCHAIN_AK: &str = "ark_access_key";
const KEYCHAIN_SK: &str = "ark_secret_key";
const KEYCHAIN_KIRO: &str = "kiro_api_key";

/// Where the effective Ark credentials came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArkCredSource {
    /// Explicitly configured in the settings window, stored in Keychain.
    AkSk,
    /// Reused from arkcli's SSO login state (~/.arkcli/.env).
    ArkCli,
}

#[derive(Debug, Clone)]
pub struct ArkCredentials {
    pub creds: Credentials,
    pub source: ArkCredSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArkCredError {
    /// Neither Keychain AK/SK nor a valid arkcli login state exists.
    NotConfigured,
    /// arkcli STS exists but is expired; user must run `arkcli auth login`.
    ArkCliExpired,
}

/// Resolve Ark credentials: explicit AK/SK first, arkcli STS as fallback.
pub fn resolve_ark_credentials() -> Result<ArkCredentials, ArkCredError> {
    choose(keychain_aksk(), arkcli_sts())
}

/// Pure priority logic, unit-testable without touching Keychain or disk.
fn choose(
    aksk: Option<Credentials>,
    arkcli: Result<Credentials, ArkCredError>,
) -> Result<ArkCredentials, ArkCredError> {
    if let Some(c) = aksk {
        return Ok(ArkCredentials {
            creds: c,
            source: ArkCredSource::AkSk,
        });
    }
    arkcli.map(|c| ArkCredentials {
        creds: c,
        source: ArkCredSource::ArkCli,
    })
}

fn keychain_aksk() -> Option<Credentials> {
    let ak = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_AK).ok()?.get_password().ok()?;
    let sk = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_SK).ok()?.get_password().ok()?;
    if ak.is_empty() || sk.is_empty() {
        return None;
    }
    Some(Credentials {
        access_key: ak,
        secret_key: sk,
        session_token: None,
    })
}

pub fn save_ark_aksk(ak: &str, sk: &str) -> Result<(), String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_AK)
        .and_then(|e| e.set_password(ak))
        .map_err(|e| e.to_string())?;
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_SK)
        .and_then(|e| e.set_password(sk))
        .map_err(|e| e.to_string())
}

pub fn clear_ark_aksk() -> Result<(), String> {
    // Deleting a non-existent entry is fine.
    let _ = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_AK).map(|e| e.delete_credential());
    let _ = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_SK).map(|e| e.delete_credential());
    Ok(())
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

/// arkcli keeps its fresh SSO-derived STS at
/// `~/.arkcli/identities/<identity>/sts.json` ({ak, sk, session_token,
/// expires_at} in epoch ms). `~/.arkcli/.env` also has VOLCENGINE_STS_* but
/// is only refreshed at login time — try identities first, then .env.
fn arkcli_sts() -> Result<Credentials, ArkCredError> {
    arkcli_sts_from_identities().or_else(|_| arkcli_sts_from_env())
}

fn arkcli_sts_from_identities() -> Result<Credentials, ArkCredError> {
    let dir = dirs::home_dir()
        .ok_or(ArkCredError::NotConfigured)?
        .join(".arkcli/identities");
    let entries = std::fs::read_dir(&dir).map_err(|_| ArkCredError::NotConfigured)?;
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
    if expires_ms <= Utc::now().timestamp_millis() {
        return Err(ArkCredError::ArkCliExpired);
    }
    Ok(creds)
}

fn arkcli_sts_from_env() -> Result<Credentials, ArkCredError> {
    let env_path = dirs::home_dir()
        .ok_or(ArkCredError::NotConfigured)?
        .join(".arkcli/.env");
    let content = std::fs::read_to_string(&env_path).map_err(|_| ArkCredError::NotConfigured)?;
    let get = |key: &str| -> Option<String> {
        content
            .lines()
            .find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let ak = get("VOLCENGINE_STS_ACCESS_KEY").ok_or(ArkCredError::NotConfigured)?;
    let sk = get("VOLCENGINE_STS_SECRET_KEY").ok_or(ArkCredError::NotConfigured)?;
    let token = get("VOLCENGINE_STS_SESSION_TOKEN").ok_or(ArkCredError::NotConfigured)?;
    let expires_ms: i64 = get("VOLCENGINE_STS_EXPIRES_AT_MS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if expires_ms <= Utc::now().timestamp_millis() {
        return Err(ArkCredError::ArkCliExpired);
    }
    Ok(Credentials {
        access_key: ak,
        secret_key: sk,
        session_token: Some(token),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred() -> Credentials {
        Credentials {
            access_key: "ak".into(),
            secret_key: "sk".into(),
            session_token: None,
        }
    }

    #[test]
    fn explicit_aksk_wins_over_arkcli() {
        let r = choose(Some(cred()), Ok(cred())).unwrap();
        assert_eq!(r.source, ArkCredSource::AkSk);
    }

    #[test]
    fn falls_back_to_arkcli_when_no_aksk() {
        let r = choose(None, Ok(cred())).unwrap();
        assert_eq!(r.source, ArkCredSource::ArkCli);
    }

    #[test]
    fn propagates_arkcli_expiry() {
        assert!(matches!(
            choose(None, Err(ArkCredError::ArkCliExpired)),
            Err(ArkCredError::ArkCliExpired)
        ));
        assert!(matches!(
            choose(None, Err(ArkCredError::NotConfigured)),
            Err(ArkCredError::NotConfigured)
        ));
    }

    #[test]
    fn env_parse_skips_empty_values() {
        let parsed: Option<String> = "A=1\nB=\n".lines()
            .find_map(|l| l.strip_prefix("B").and_then(|v| v.strip_prefix('=')))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        assert_eq!(parsed, None);
    }
}
