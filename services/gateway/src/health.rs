use axum::Router;
use ecat_health::{FnCheck, HealthRegistry};
use sqlx::MySqlPool;

pub async fn health_router(pool: MySqlPool) -> Router {
    let mysql = FnCheck::new("mysql", move || {
        let pool = pool.clone();
        async move {
            pool.acquire()
                .await
                .map(|_| ())
                .map_err(|e| format!("mysql unavailable: {e}"))
        }
    });
    HealthRegistry::new().with_check(mysql).await.into_router()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn liveness_returns_200() {
        let router = HealthRegistry::new().into_router();
        let res = router
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readiness_503_when_mysql_down() {
        // connect_lazy 不建立真实连接，acquire 时才失败 → 离线可测
        let pool = MySqlPool::connect_lazy("mysql://u:p@127.0.0.1:1/db").unwrap();
        let router = health_router(pool).await;
        let res = router
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
