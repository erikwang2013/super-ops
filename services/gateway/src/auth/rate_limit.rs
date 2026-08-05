use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::AppState;

const MAX_LOGIN_ATTEMPTS: u32 = 10;
const WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Default)]
pub struct RateLimiter {
    buckets: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
}

impl RateLimiter {
    fn allow(&self, key: &str, max: u32, window: Duration) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let now = Instant::now();
        match buckets.get_mut(key) {
            Some((start, count)) if now.duration_since(*start) <= window => {
                *count += 1;
                *count <= max
            }
            Some((start, count)) => {
                *start = now;
                *count = 1;
                true
            }
            None => {
                buckets.insert(key.to_string(), (now, 1));
                true
            }
        }
    }
}

pub async fn rate_limit_login(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let key = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown")
        .to_string();
    if !state.rate_limiter.allow(&key, MAX_LOGIN_ATTEMPTS, WINDOW) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "too many login attempts, try again later"})),
        )
            .into_response());
    }
    Ok(next.run(req).await)
}
