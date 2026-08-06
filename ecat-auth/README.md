# e-cat-auth

Authentication and authorization middleware for e-cat services.

## Modules

- `jwt` — JWT Bearer token validation and signing (HS256)
- `apikey` — API key validation (header or query param)
- `oauth2` — OAuth2 token introspection (RFC 7662)
- `claims` — `AuthClaims` struct with role-based access
- `helpers` — token extraction utilities

## Usage

Sign and verify tokens:

```rust
use ecat_auth::{AuthClaims, sign_token, verify_token};

let secret = "0123456789abcdef0123456789abcdef"; // >= 32 bytes for HS256
let claims = AuthClaims {
    sub: "user42".into(),
    exp: None,
    iat: None,
    role: Some("admin".into()),
    extra: Default::default(),
};

let token = sign_token(secret, &claims, 3600)?; // sets iat/exp from now + ttl
let verified = verify_token(secret, &token)?;   // AuthClaims on success
```

Guard routes with the middleware layer:

```rust
use ecat_auth::JwtAuthLayer;

let auth = JwtAuthLayer::new("my-secret-key-0123456789abcdef0123456789")
    .require_claims(&["sub", "role"]);
```

## Errors

`JwtAuthError` distinguishes:

- `WeakKey` — secret shorter than 32 bytes (rejected by signing and validation)
- `Expired` — signature valid but token past `exp`
- `Invalid(String)` — malformed token, bad signature, or wrong secret
