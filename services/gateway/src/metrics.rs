use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use prometheus::{IntCounter, opts, register_int_counter_with_registry};
use std::sync::OnceLock;

fn requests_counter() -> &'static IntCounter {
    static COUNTER: OnceLock<IntCounter> = OnceLock::new();
    COUNTER.get_or_init(|| {
        register_int_counter_with_registry!(
            opts!(
                "superops_http_requests_total",
                "Total HTTP requests handled by gateway"
            ),
            ecat_metrics::registry()
        )
        .expect("metric registration should not conflict")
    })
}

pub async fn count_requests(req: Request<Body>, next: Next) -> Response {
    requests_counter().inc();
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::routing::get;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn requests_metric_appears_in_text_format() {
        let router = axum::Router::new()
            .route("/", get(|| async {}))
            .layer(axum::middleware::from_fn(count_requests));
        let res = router
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(ecat_metrics::metrics_text().contains("superops_http_requests_total"));
    }
}
