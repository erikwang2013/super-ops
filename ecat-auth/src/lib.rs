// Copyright (c) 2026 erik <erik@erik.xyz> — https://erik.xyz
mod apikey;
mod claims;
mod helpers;
mod jwt;
mod oauth2;

pub use apikey::{ApiKeyLayer, ApiKeyService};
pub use claims::AuthClaims;
pub use helpers::{claims_from_request, extract_bearer, extract_header, extract_query_param};
pub use jwt::{JwtAuthError, JwtAuthLayer, JwtAuthService, sign_token, verify_token};
pub use oauth2::{OAuth2Layer, OAuth2Service};

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;
    use std::collections::HashMap;

    #[test]
    fn bearer_extraction() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_static("Bearer mytoken123"),
        );
        assert_eq!(
            helpers::extract_bearer(&headers, "Authorization"),
            Some("mytoken123".into())
        );
    }

    #[test]
    fn bearer_extraction_no_header() {
        let headers = http::HeaderMap::new();
        assert_eq!(helpers::extract_bearer(&headers, "Authorization"), None);
    }

    #[test]
    fn bearer_extraction_wrong_prefix() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_static("Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="),
        );
        assert_eq!(helpers::extract_bearer(&headers, "Authorization"), None);
    }

    #[test]
    fn query_param_extraction() {
        assert_eq!(
            helpers::extract_query_param(Some("key=abc123&other=val"), "key"),
            Some("abc123".into())
        );
    }

    #[test]
    fn query_param_not_found() {
        assert_eq!(helpers::extract_query_param(Some("a=1&b=2"), "c"), None);
    }

    #[test]
    fn layer_construction() {
        let layer = JwtAuthLayer::new("secret-key-0123456789abcdefghijklmnopqrstuv")
            .expect("32+ byte secret accepted");
        let _layer = layer
            .require_claims(&["sub", "role"])
            .header_name("X-Auth-Token");
    }

    #[test]
    fn layer_rejects_weak_secret() {
        assert!(matches!(
            JwtAuthLayer::new("too-short"),
            Err(JwtAuthError::WeakKey)
        ));
    }

    const SECRET: &str = "test-secret-0123456789abcdef0123456789abcdef";

    fn sample_claims() -> AuthClaims {
        AuthClaims {
            sub: "user42".into(),
            exp: None,
            iat: None,
            role: Some("admin".into()),
            extra: HashMap::new(),
        }
    }

    #[test]
    fn jwt_sign_verify_roundtrip() {
        let token = sign_token(SECRET, &sample_claims(), 3600).unwrap();
        let verified = verify_token(SECRET, &token).unwrap();
        assert_eq!(verified.subject(), "user42");
        assert!(verified.has_role("admin"));
        assert!(verified.exp.is_some());
        assert!(verified.iat.is_some());
    }

    #[test]
    fn jwt_sign_verify_extra_claims_survive() {
        let mut claims = sample_claims();
        claims
            .extra
            .insert("username".into(), serde_json::json!("erik"));
        let token = sign_token(SECRET, &claims, 60).unwrap();
        let verified = verify_token(SECRET, &token).unwrap();
        assert_eq!(
            verified.extra.get("username").and_then(|v| v.as_str()),
            Some("erik")
        );
    }

    #[test]
    fn jwt_sign_rejects_weak_secret() {
        assert!(matches!(
            sign_token("short", &sample_claims(), 60),
            Err(JwtAuthError::WeakKey)
        ));
    }

    #[test]
    fn jwt_verify_rejects_expired() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut claims = sample_claims();
        claims.exp = Some(now.saturating_sub(120));
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .unwrap();
        assert!(matches!(
            verify_token(SECRET, &token),
            Err(JwtAuthError::Expired)
        ));
    }

    #[test]
    fn jwt_verify_rejects_tampered_signature() {
        let token = sign_token(SECRET, &sample_claims(), 3600).unwrap();
        let bad = format!("{}x", &token[..token.len() - 2]);
        assert!(matches!(
            verify_token(SECRET, &bad),
            Err(JwtAuthError::Invalid(_))
        ));
    }

    #[test]
    fn jwt_verify_rejects_wrong_secret() {
        let token = sign_token(SECRET, &sample_claims(), 3600).unwrap();
        let other = "another-secret-0123456789abcdef0123456789";
        assert!(matches!(
            verify_token(other, &token),
            Err(JwtAuthError::Invalid(_))
        ));
    }

    #[test]
    fn api_key_layer_construction() {
        let mut keys = HashMap::new();
        keys.insert(
            "key1".into(),
            AuthClaims {
                sub: "user1".into(),
                exp: None,
                iat: None,
                role: Some("admin".into()),
                extra: HashMap::new(),
            },
        );
        let _layer = ApiKeyLayer::new(keys).query_param("api_key");
    }

    #[test]
    fn claims_subject_and_role() {
        let claims = AuthClaims {
            sub: "user42".into(),
            exp: None,
            iat: None,
            role: Some("editor".into()),
            extra: HashMap::new(),
        };
        assert_eq!(claims.subject(), "user42");
        assert!(claims.has_role("editor"));
        assert!(!claims.has_role("admin"));
    }
}
