// 安全扫描错误 → HTTP 响应转换层。
// Router::layer 要求最外层 Service 的 Error: Into<Infallible>，故仿 breaker 的
// ErrorToResponseLayer：错误在此转成响应（SecurityError → 403/500，其余 → 500）。
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Clone)]
pub struct SecurityErrorToResponseLayer;

impl<S> tower::Layer<S> for SecurityErrorToResponseLayer {
    type Service = SecurityErrorToResponseService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SecurityErrorToResponseService { inner }
    }
}

#[derive(Clone)]
pub struct SecurityErrorToResponseService<S> {
    inner: S,
}

impl<S, B> tower::Service<Request<B>> for SecurityErrorToResponseService<S>
where
    S: tower::Service<Request<B>>,
    S::Response: IntoResponse,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    S::Future: Future<Output = Result<S::Response, S::Error>> + Send + 'static,
{
    type Response = Response;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(|_| -> std::convert::Infallible { unreachable!("inner poll_ready is infallible") })
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let fut = self.inner.call(req);
        Box::pin(async move {
            match fut.await {
                Ok(resp) => Ok(resp.into_response()),
                Err(e) => {
                    let boxed: Box<dyn std::error::Error + Send + Sync> = e.into();
                    let response = if let Some(err) =
                        boxed.downcast_ref::<ecat_security::SecurityError>()
                    {
                        let status = err.to_http_status();
                        let msg = err.to_string();
                        (status, axum::Json(serde_json::json!({ "error": msg }))).into_response()
                    } else {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            axum::Json(serde_json::json!({ "error": boxed.to_string() })),
                        )
                            .into_response()
                    };
                    Ok(response)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt as _;

    fn echo_app() -> axum::Router {
        axum::Router::new()
            .route(
                "/api/echo",
                axum::routing::post(|body: String| async move { body }),
            )
            .layer(
                tower::ServiceBuilder::new()
                    .layer(SecurityErrorToResponseLayer)
                    .layer(ecat_security::SecurityBodyLayer::new()),
            )
    }

    #[tokio::test]
    async fn blocks_sqli_in_body_with_403() {
        let resp = echo_app()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/echo")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"q":"SELECT * FROM users; DROP TABLE users;"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn blocks_xss_in_body_with_403() {
        let resp = echo_app()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/echo")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"note":"<script>alert('xss')</script>"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn blocks_attack_in_header_with_403() {
        let resp = echo_app()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/echo")
                    .header("x-tenant-id", "<script>alert(1)</script>")
                    .body(axum::body::Body::from("{\"ok\":1}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn passes_clean_request_and_replays_body() {
        let resp = echo_app()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/echo")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"name":"nginx:1.27"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
