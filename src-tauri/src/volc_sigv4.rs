//! Volcengine OpenAPI signature V4 (HMAC-SHA256).
//!
//! Reference: https://www.volcengine.com/docs/6369/67269
//! Unlike AWS, the derivation chain starts from the raw secret key (no
//! "AWS4" prefix) and the scope terminator is the literal "request".

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    m.update(data);
    m.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Credentials used for signing. `session_token` is present for STS
/// credentials (arkcli SSO fallback), absent for long-term AK/SK.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
}

/// Inputs needed to sign one control-plane request.
pub struct SignRequest<'a> {
    pub method: &'a str,
    pub host: &'a str,
    /// Canonical URI path, e.g. "/".
    pub path: &'a str,
    /// Query parameters WITHOUT leading '?', already parsed into pairs.
    /// Will be sorted and percent-encoded by the signer.
    pub query: Vec<(String, String)>,
    pub content_type: &'a str,
    pub body: &'a [u8],
    /// UTC timestamp of signing.
    pub now: chrono::DateTime<chrono::Utc>,
}

/// Output: headers the caller must attach to the HTTP request.
pub struct SignedHeaders {
    pub authorization: String,
    pub x_date: String,
    pub x_content_sha256: String,
    pub session_token: Option<String>,
}

const ALGORITHM: &str = "HMAC-SHA256";

pub fn sign(creds: &Credentials, region: &str, service: &str, req: &SignRequest) -> SignedHeaders {
    let x_date = req.now.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = req.now.format("%Y%m%d").to_string();
    let payload_hash = sha256_hex(req.body);

    // Canonical query: percent-encode keys/values, sort by encoded key=value.
    let mut pairs: Vec<(String, String)> = req
        .query
        .iter()
        .map(|(k, v)| (urlencoding::encode(k).into_owned(), urlencoding::encode(v).into_owned()))
        .collect();
    pairs.sort();
    let canonical_query = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    // Signed headers: content-type, host, x-content-sha256, x-date (+ x-security-token for STS).
    let mut header_pairs: Vec<(String, String)> = vec![
        ("content-type".into(), req.content_type.trim().to_string()),
        ("host".into(), req.host.trim().to_string()),
        ("x-content-sha256".into(), payload_hash.clone()),
        ("x-date".into(), x_date.clone()),
    ];
    if let Some(token) = &creds.session_token {
        header_pairs.push(("x-security-token".into(), token.trim().to_string()));
    }
    header_pairs.sort();
    let canonical_headers = header_pairs
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect::<String>();
    let signed_headers = header_pairs
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        req.method, req.path, canonical_query, canonical_headers, signed_headers, payload_hash
    );

    let credential_scope = format!("{short_date}/{region}/{service}/request");
    let string_to_sign = format!(
        "{ALGORITHM}\n{x_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let k_date = hmac(creds.secret_key.as_bytes(), short_date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    let k_signing = hmac(&k_service, b"request");
    let signature = hex::encode(hmac(&k_signing, string_to_sign.as_bytes()));

    SignedHeaders {
        authorization: format!(
            "{ALGORITHM} Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            creds.access_key
        ),
        x_date,
        x_content_sha256: payload_hash,
        session_token: creds.session_token.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn test_creds() -> Credentials {
        Credentials {
            access_key: "AKTEST".into(),
            secret_key: "SKTEST".into(),
            session_token: None,
        }
    }

    fn test_req() -> SignRequest<'static> {
        SignRequest {
            method: "POST",
            host: "ark.cn-beijing.volcengineapi.com",
            path: "/",
            query: vec![
                ("Action".into(), "GetAFPUsage".into()),
                ("Version".into(), "2024-01-01".into()),
            ],
            content_type: "application/json",
            body: b"{}",
            now: chrono::Utc.with_ymd_and_hms(2026, 9, 6, 8, 0, 0).unwrap(),
        }
    }

    #[test]
    fn signature_is_deterministic() {
        let a = sign(&test_creds(), "cn-beijing", "ark", &test_req());
        let b = sign(&test_creds(), "cn-beijing", "ark", &test_req());
        assert_eq!(a.authorization, b.authorization);
    }

    /// Golden vector computed independently with Python (hmac/hashlib):
    /// same algorithm, separate implementation — guards against typos in
    /// the derivation chain.
    #[test]
    fn signature_matches_golden_vector() {
        let out = sign(&test_creds(), "cn-beijing", "ark", &test_req());
        assert!(out
            .authorization
            .starts_with("HMAC-SHA256 Credential=AKTEST/20260906/cn-beijing/ark/request, SignedHeaders=content-type;host;x-content-sha256;x-date, Signature="));
        assert_eq!(
            out.authorization,
            "HMAC-SHA256 Credential=AKTEST/20260906/cn-beijing/ark/request, SignedHeaders=content-type;host;x-content-sha256;x-date, Signature=2de79c459a2a5f8482241a41af20f059e09aa2f57eca31243dcbd79f2e42bfa0"
        );
    }

    #[test]
    fn session_token_is_signed() {
        let mut creds = test_creds();
        creds.session_token = Some("tok123".into());
        let out = sign(&creds, "cn-beijing", "ark", &test_req());
        assert!(out.authorization.contains("x-security-token"));
        assert_eq!(out.session_token.as_deref(), Some("tok123"));
        assert!(out.authorization != sign(&test_creds(), "cn-beijing", "ark", &test_req()).authorization);
    }

    #[test]
    fn query_params_are_sorted() {
        let mut req = test_req();
        req.query = vec![
            ("Version".into(), "2024-01-01".into()),
            ("Action".into(), "GetAFPUsage".into()),
        ];
        let a = sign(&test_creds(), "cn-beijing", "ark", &req);
        let b = sign(&test_creds(), "cn-beijing", "ark", &test_req());
        assert_eq!(a.authorization, b.authorization, "query order must not affect signature");
    }
}
