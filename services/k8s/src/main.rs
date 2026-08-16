mod cluster;
mod config;
mod resource;
mod service;

use ecat::App;
use ecat_registry::{Registration, Registry, ServiceInfo};
use ecat_registry_consul::ConsulRegistry;
use ecat_transport_grpc::GrpcServer;
use std::sync::{Arc, Mutex};
use superops_protos::k8s::v1::k8s_service_server::K8sServiceServer;

use crate::cluster::manager::ClusterManager;
use crate::config::Config;
use crate::service::K8sServiceImpl;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let _otlp = match &config.otlp {
        Some(endpoint) => Some(
            ecat_tracing_otlp::init("superops-k8s", endpoint)
                .map_err(|e| anyhow::anyhow!("otlp init: {e}"))?,
        ),
        None => None,
    };
    let manager = ClusterManager::new();
    let auth_token = config.auth.token.clone();
    let service = K8sServiceImpl { manager };

    // P0 安全基线：配置 auth.token 时启用 Bearer 鉴权（无凭据调用被拒），未配置时 WARN 保持兼容
    let routes = if auth_token.is_empty() {
        tracing::warn!("k8s gRPC auth disabled: no auth.token configured (insecure)");
        tonic::service::Routes::new(K8sServiceServer::new(service))
    } else {
        let bearer = format!("Bearer {auth_token}");
        tonic::service::Routes::new(K8sServiceServer::with_interceptor(
            service,
            auth_interceptor(bearer),
        ))
    };
    let grpc = GrpcServer::new(format!("0.0.0.0:{}", config.server.grpc_port)).routes(routes);

    let reg_holder = Arc::new(Mutex::new(None::<Registration>));
    let reg_start = Arc::clone(&reg_holder);
    let cfg_start = config.clone();

    let mut app = App::builder()
        .name("superops-k8s")
        .version(env!("CARGO_PKG_VERSION"))
        .server(grpc)
        .on_start(move || {
            let reg = Arc::clone(&reg_start);
            let cfg = cfg_start.clone();
            async move {
                if let Some(consul) = &cfg.consul {
                    let registry = ConsulRegistry::new(&consul.address);
                    let info = ServiceInfo::new("superops-k8s", env!("CARGO_PKG_VERSION"))
                        .with_endpoint(format!("http://localhost:{}", cfg.server.grpc_port));
                    let registration = registry.register(info).await?;
                    tracing::info!(service = "superops-k8s", "registered in consul");
                    *reg.lock().unwrap() = Some(registration);
                }
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        })
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    app.run().await.map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}

/// Bearer 令牌校验：metadata `authorization` 恰等于 `expected`（如 "Bearer <token>"）。
fn bearer_authorized(metadata: &tonic::metadata::MetadataMap, expected: &str) -> bool {
    metadata
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|a| a == expected)
        .unwrap_or(false)
}

/// 构造 Bearer 校验拦截器（result_large_err：tonic::Status 为 API 要求，无法缩小）。
/// 闭包仅捕获 String，自动实现 Clone，满足 tonic Routes 的服务 Clone 约束。
#[allow(clippy::result_large_err)]
fn auth_interceptor(
    bearer: String,
) -> impl Clone + Fn(tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
    move |req: tonic::Request<()>| {
        if bearer_authorized(req.metadata(), &bearer) {
            Ok(req)
        } else {
            Err(tonic::Status::unauthenticated(
                "missing or invalid bearer token",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::bearer_authorized;
    use tonic::metadata::MetadataMap;

    #[test]
    fn auth_rejects_missing_token() {
        assert!(!bearer_authorized(&MetadataMap::new(), "Bearer secret"));
    }

    #[test]
    fn auth_accepts_matching_bearer() {
        let mut m = MetadataMap::new();
        m.insert("authorization", "Bearer secret".parse().unwrap());
        assert!(bearer_authorized(&m, "Bearer secret"));
    }

    #[test]
    fn auth_rejects_wrong_or_malformed_token() {
        let mut m = MetadataMap::new();
        m.insert("authorization", "Bearer other".parse().unwrap());
        assert!(!bearer_authorized(&m, "Bearer secret"));
        m.insert("authorization", "secret".parse().unwrap());
        assert!(!bearer_authorized(&m, "Bearer secret"));
    }
}
