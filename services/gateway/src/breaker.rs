use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use ecat_circuit_breaker::CircuitBreakerLayer;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tower::Service;

pub fn breaker_layer() -> CircuitBreakerLayer {
    CircuitBreakerLayer::new()
        .failure_ratio(0.5)
        .window(Duration::from_secs(30))
        .half_open_probes(3)
        .open_duration(Duration::from_secs(10))
}

pub fn error_to_response(err: Box<dyn std::error::Error + Send + Sync>) -> Response {
    let msg = err.to_string();
    let status = if msg.contains("circuit breaker is open") {
        StatusCode::SERVICE_UNAVAILABLE
    } else if msg.contains("rate limit exceeded") {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    let body = if status == StatusCode::TOO_MANY_REQUESTS {
        "too many login attempts, try again later"
    } else {
        &msg
    };
    (status, axum::Json(serde_json::json!({ "error": body }))).into_response()
}

#[derive(Clone)]
pub struct FiveXxToErrorLayer;

impl<S> tower::Layer<S> for FiveXxToErrorLayer {
    type Service = FiveXxToErrorService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        FiveXxToErrorService { inner }
    }
}

#[derive(Clone)]
pub struct FiveXxToErrorService<S> {
    inner: S,
}

impl<S, B> Service<Request<B>> for FiveXxToErrorService<S>
where
    S: Service<Request<B>>,
    S::Response: IntoResponse,
    S::Error: std::fmt::Display + std::error::Error + Send + Sync + 'static,
    S::Future: Future<Output = Result<S::Response, S::Error>> + Send + 'static,
{
    type Response = Response;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let fut = self.inner.call(req);
        Box::pin(async move {
            let resp = fut
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            let response = resp.into_response();
            if response.status().is_server_error() {
                Err(std::io::Error::other(format!(
                    "upstream returned {}",
                    response.status()
                )))
            } else {
                Ok(response)
            }
        })
    }
}

// axum 0.8 `Router::layer` requires `L::Service::Error: Into<Infallible>`,
// so the outermost layer must never fail: errors become responses here.
#[derive(Clone)]
pub struct ErrorToResponseLayer;

impl<S> tower::Layer<S> for ErrorToResponseLayer {
    type Service = ErrorToResponseService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ErrorToResponseService { inner }
    }
}

#[derive(Clone)]
pub struct ErrorToResponseService<S> {
    inner: S,
}

impl<S, B> Service<Request<B>> for ErrorToResponseService<S>
where
    S: Service<Request<B>>,
    S::Response: IntoResponse,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    S::Future: Future<Output = Result<S::Response, S::Error>> + Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(|_| -> Infallible { unreachable!("inner poll_ready is infallible") })
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let fut = self.inner.call(req);
        Box::pin(async move {
            match fut.await {
                Ok(resp) => Ok(resp.into_response()),
                Err(e) => Ok(error_to_response(e.into())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use std::io;
    use tower::Layer;
    use tower::service_fn;

    #[test]
    fn error_to_response_maps_open_to_503() {
        let err: Box<dyn std::error::Error + Send + Sync> =
            Box::new(io::Error::other("circuit breaker is open"));
        let resp = error_to_response(err);
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn error_to_response_maps_rate_limit_to_429() {
        let err: Box<dyn std::error::Error + Send + Sync> =
            Box::new(io::Error::other("rate limit exceeded"));
        let resp = error_to_response(err);
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn error_to_response_passthrough_500() {
        let err: Box<dyn std::error::Error + Send + Sync> = Box::new(io::Error::other("boom"));
        let resp = error_to_response(err);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn five_xx_to_error_converts_500() {
        let mut svc = FiveXxToErrorLayer.layer(service_fn(|_: Request<()>| async {
            Ok::<Response, io::Error>((StatusCode::INTERNAL_SERVER_ERROR, "boom").into_response())
        }));
        let err = svc.call(Request::new(())).await.unwrap_err();
        assert!(err.to_string().contains("upstream returned 500"));
    }

    #[tokio::test]
    async fn five_xx_to_error_passes_2xx() {
        let mut svc = FiveXxToErrorLayer.layer(service_fn(|_: Request<()>| async {
            Ok::<Response, io::Error>((StatusCode::OK, "ok").into_response())
        }));
        let resp = svc.call(Request::new(())).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn error_to_response_service_converts_err_to_503() {
        let mut svc = ErrorToResponseLayer.layer(service_fn(|_: Request<()>| async {
            Err::<Response, io::Error>(io::Error::other("circuit breaker is open"))
        }));
        let resp = svc.call(Request::new(())).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn breaker_stays_closed_on_success() {
        let layer = breaker_layer();
        let mut svc = layer.layer(service_fn(|_: Request<()>| async {
            Ok::<Response, io::Error>((StatusCode::OK, "ok").into_response())
        }));
        for _ in 0..5 {
            let resp = svc.call(Request::new(())).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn breaker_opens_after_five_failures() {
        let layer = breaker_layer();
        let mut svc = layer.layer(service_fn(|_: Request<()>| async {
            Err::<Response, _>(io::Error::other("boom"))
        }));
        for _ in 0..5 {
            assert!(svc.call(Request::new(())).await.is_err());
        }
        let err = svc.call(Request::new(())).await.unwrap_err();
        assert!(err.to_string().contains("circuit breaker is open"));
    }

    #[tokio::test]
    async fn breaker_passes_through_errors_before_open() {
        let layer = breaker_layer();
        let mut svc = layer.layer(service_fn(|_: Request<()>| async {
            Err::<Response, _>(io::Error::other("boom"))
        }));
        let err = svc.call(Request::new(())).await.unwrap_err();
        assert!(err.to_string().contains("boom"));
        assert!(!err.to_string().contains("circuit breaker is open"));
    }

    #[tokio::test]
    async fn breaker_probes_inner_after_open_duration() {
        let layer = CircuitBreakerLayer::new()
            .failure_ratio(0.5)
            .window(Duration::from_secs(30))
            .half_open_probes(3)
            .open_duration(Duration::from_millis(50));
        let mut svc = layer.layer(service_fn(|_: Request<()>| async {
            Err::<Response, _>(io::Error::other("boom"))
        }));
        for _ in 0..5 {
            let _ = svc.call(Request::new(())).await;
        }
        let err = svc.call(Request::new(())).await.unwrap_err();
        assert!(err.to_string().contains("circuit breaker is open"));
        tokio::time::sleep(Duration::from_millis(80)).await;
        let err = svc.call(Request::new(())).await.unwrap_err();
        assert!(err.to_string().contains("boom"));
    }

    #[tokio::test]
    async fn chain_fivexx_feeds_breaker_until_open() {
        let mut svc = tower::ServiceBuilder::new()
            .layer(ErrorToResponseLayer)
            .layer(breaker_layer())
            .layer(FiveXxToErrorLayer)
            .service(service_fn(|_: Request<()>| async {
                Ok::<Response, io::Error>(
                    (StatusCode::INTERNAL_SERVER_ERROR, "upstream broken").into_response(),
                )
            }));
        for _ in 0..5 {
            let resp = svc.call(Request::new(())).await.unwrap();
            assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
        let resp = svc.call(Request::new(())).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
