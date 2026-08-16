//! k8s-service gRPC 客户端工厂：统一注入服务间令牌（Bearer）。
//! token 为空时退回明文连接（本地/未配置场景保持兼容）。

use http::{Request as HttpRequest, Response as HttpResponse};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use superops_protos::k8s::v1::k8s_service_client::K8sServiceClient;
use tonic::body::BoxBody;
use tonic::transport::{Channel, Endpoint};
use tower::Service;

/// 透明层：bearer 非空时为每个 gRPC 请求注入 `authorization: Bearer <token>`。
#[derive(Clone)]
pub struct BearerChannel {
    inner: Channel,
    bearer: Arc<str>,
}

impl Service<HttpRequest<BoxBody>> for BearerChannel {
    type Response = HttpResponse<BoxBody>;
    type Error = tonic::transport::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: HttpRequest<BoxBody>) -> Self::Future {
        let auth = if self.bearer.is_empty() {
            None
        } else {
            http::header::HeaderValue::from_str(&self.bearer).ok()
        };
        if let Some(hv) = auth {
            req.headers_mut().insert(http::header::AUTHORIZATION, hv);
        }
        Box::pin(self.inner.call(req))
    }
}

/// 创建 k8s gRPC 客户端；`token` 非空时每个请求注入 `authorization: Bearer <token>`。
pub async fn connect(
    endpoint: &str,
    token: &str,
) -> Result<K8sServiceClient<BearerChannel>, tonic::transport::Error> {
    let channel = Endpoint::new(endpoint.to_string())?.connect().await?;
    let bearer: Arc<str> = if token.is_empty() {
        Arc::from("")
    } else {
        Arc::from(format!("Bearer {token}"))
    };
    Ok(K8sServiceClient::new(BearerChannel {
        inner: channel,
        bearer,
    }))
}
