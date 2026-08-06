// Copyright (c) 2026 erik <erik@erik.xyz> — https://erik.xyz
use super::claims::AuthClaims;
use super::helpers::extract_bearer;
use http::{Request, Response, StatusCode};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::{Layer, Service};

/// Errors while constructing or operating the JWT auth layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JwtAuthError {
    /// The shared secret is shorter than 32 bytes, which is too weak for HS256.
    WeakKey,
    /// The token failed to decode or verify (bad signature, malformed, …).
    Invalid(String),
    /// The token signature was valid but the token has expired.
    Expired,
}

impl std::fmt::Display for JwtAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WeakKey => write!(f, "JWT secret must be at least 32 bytes for HS256"),
            Self::Invalid(msg) => write!(f, "invalid JWT: {msg}"),
            Self::Expired => write!(f, "JWT has expired"),
        }
    }
}

impl std::error::Error for JwtAuthError {}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Sign a token with HS256 using the given secret (must be ≥ 32 bytes).
///
/// `iat`/`exp` are derived from `now` and `ttl_secs`, overriding any values
/// carried on the passed claims.
pub fn sign_token(
    secret: &str,
    claims: &AuthClaims,
    ttl_secs: u64,
) -> Result<String, JwtAuthError> {
    if secret.len() < 32 {
        return Err(JwtAuthError::WeakKey);
    }
    let now = now_secs();
    let mut claims = claims.clone();
    claims.iat = Some(now);
    claims.exp = Some(now.saturating_add(ttl_secs));
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| JwtAuthError::Invalid(e.to_string()))
}

/// Verify and decode a token, returning the claims on success.
pub fn verify_token(secret: &str, token: &str) -> Result<AuthClaims, JwtAuthError> {
    if secret.len() < 32 {
        return Err(JwtAuthError::WeakKey);
    }
    let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    jsonwebtoken::decode::<AuthClaims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|d| d.claims)
    .map_err(|e| {
        if matches!(e.kind(), jsonwebtoken::errors::ErrorKind::ExpiredSignature) {
            JwtAuthError::Expired
        } else {
            JwtAuthError::Invalid(e.to_string())
        }
    })
}

enum JwtSecret {
    Shared(Vec<u8>),
    #[allow(dead_code)]
    RsaReserved(Vec<u8>),
}

#[derive(Clone)]
pub struct JwtAuthLayer {
    secret: Arc<JwtSecret>,
    required_claims: Vec<String>,
    header_name: String,
}

impl JwtAuthLayer {
    /// Create a layer for HS256-signed tokens.
    ///
    /// The secret must be at least 32 bytes (the minimum key size HS256
    /// accepts per RFC 7518); shorter keys are rejected with
    /// [`JwtAuthError::WeakKey`].
    pub fn new(secret: impl Into<String>) -> Result<Self, JwtAuthError> {
        let secret = secret.into();
        if secret.len() < 32 {
            return Err(JwtAuthError::WeakKey);
        }
        Ok(Self {
            secret: Arc::new(JwtSecret::Shared(secret.into_bytes())),
            required_claims: vec!["sub".into()],
            header_name: "Authorization".into(),
        })
    }

    pub fn require_claims(mut self, claims: &[&str]) -> Self {
        self.required_claims = claims.iter().map(|c| c.to_string()).collect();
        self
    }

    pub fn header_name(mut self, name: impl Into<String>) -> Self {
        self.header_name = name.into();
        self
    }
}

impl<S> Layer<S> for JwtAuthLayer {
    type Service = JwtAuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        JwtAuthService {
            inner,
            config: Arc::new(self.clone()),
        }
    }
}

#[derive(Clone)]
pub struct JwtAuthService<S> {
    inner: S,
    config: Arc<JwtAuthLayer>,
}

impl<S, B> Service<Request<B>> for JwtAuthService<S>
where
    S: Service<Request<B>, Response = Response<axum::body::Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::error::Error + Send + Sync + 'static,
    B: Send + 'static,
{
    type Response = Response<axum::body::Body>;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(|e| Box::new(e) as _)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let token = extract_bearer(req.headers(), &self.config.header_name);
        let config = Arc::clone(&self.config);
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let token = match token {
                Some(t) => t,
                None => {
                    return Ok(Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(axum::body::Body::from(
                            r#"{"error":"missing authorization token"}"#,
                        ))
                        .unwrap());
                }
            };

            let secret = match config.secret.as_ref() {
                JwtSecret::Shared(b) => String::from_utf8_lossy(b).into_owned(),
                JwtSecret::RsaReserved(b) => String::from_utf8_lossy(b).into_owned(),
            };

            let claims = match verify_token(&secret, &token) {
                Ok(c) => c,
                Err(JwtAuthError::Expired) => {
                    tracing::warn!("jwt token expired");
                    return Ok(Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(axum::body::Body::from(r#"{"error":"invalid token"}"#))
                        .unwrap());
                }
                Err(e) => {
                    tracing::warn!(error = %e, "jwt validation failed");
                    return Ok(Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(axum::body::Body::from(r#"{"error":"invalid token"}"#))
                        .unwrap());
                }
            };

            for claim in &config.required_claims {
                let satisfied = match claim.as_str() {
                    "sub" => !claims.sub.is_empty(),
                    "role" => claims.role.is_some(),
                    _ => claims.extra.contains_key(claim),
                };
                if !satisfied {
                    return Ok(Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .body(axum::body::Body::from(format!(
                            r#"{{"error":"missing required claim: {claim}"}}"#
                        )))
                        .unwrap());
                }
            }

            let mut req = req;
            req.extensions_mut().insert(claims);
            inner.call(req).await.map_err(|e| Box::new(e) as _)
        })
    }
}
