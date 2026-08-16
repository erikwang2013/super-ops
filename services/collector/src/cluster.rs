//! 目标集群解析：collector 各周期任务统一携带真实 `cluster_id`。
//!
//! 优先级：显式配置 `k8s.cluster_id` > k8s-service `ListClusters` 首个注册集群 > `None`（任务跳过本轮）。
//! 背景：k8s-service 的集群 id 由 `AddCluster` 随机 UUID 生成，collector 无法预知；
//! 此前六类周期任务以空 `cluster_id` 调用，`ClusterManager.get("")` 必返回 NotFound，数据通路全链路失效。

use crate::config::Config;
use std::sync::Arc;
use superops_protos::k8s::v1::k8s_service_client::K8sServiceClient;

/// 透明层：bearer 非空时为每个 gRPC 请求注入 `authorization: Bearer <token>`。
#[derive(Clone)]
pub struct BearerChannel {
    inner: tonic::transport::Channel,
    bearer: Arc<str>,
}

impl tower::Service<http::Request<tonic::body::BoxBody>> for BearerChannel {
    type Response = http::Response<tonic::body::BoxBody>;
    type Error = tonic::transport::Error;
    type Future = std::pin::Pin<
        Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<tonic::body::BoxBody>) -> Self::Future {
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

/// 创建带服务间令牌的 k8s gRPC 客户端（token 为空时明文连接，保持本地兼容）。
pub async fn k8s_client(cfg: &Config) -> anyhow::Result<K8sServiceClient<BearerChannel>> {
    let channel = tonic::transport::Endpoint::new(cfg.k8s.endpoint.clone())?
        .connect()
        .await?;
    let bearer: Arc<str> = match cfg.k8s.token.as_deref() {
        Some(t) if !t.is_empty() => Arc::from(format!("Bearer {t}")),
        _ => Arc::from(""),
    };
    Ok(K8sServiceClient::new(BearerChannel {
        inner: channel,
        bearer,
    }))
}

/// 解析目标集群 id：配置优先，否则连接 k8s-service 取首个注册集群。
/// `Ok(None)` 表示当前无可用集群，调用方应跳过本轮并告警（不阻断服务）。
pub async fn resolve_cluster_id(cfg: &Config) -> anyhow::Result<Option<String>> {
    if let Some(id) = &cfg.k8s.cluster_id {
        return Ok(Some(id.clone()));
    }
    let mut client = k8s_client(cfg).await?;
    let clusters = client
        .list_clusters(superops_protos::k8s::v1::ListClustersRequest {})
        .await?
        .into_inner()
        .clusters;
    Ok(pick_first_cluster(&clusters))
}

/// 取首个注册集群的 id（ListClusters 返回顺序即注册顺序，首个即「默认集群」）。
pub fn pick_first_cluster(clusters: &[superops_protos::k8s::v1::Cluster]) -> Option<String> {
    clusters.first().map(|c| c.id.clone())
}

/// 任务入口辅助：解析 cluster_id 并 attach 到 cfg 的克隆，供后续请求统一携带。
/// `Ok(None)` 表示无可用集群（调用方 `return Ok(())` 跳过本轮并告警）。
pub async fn resolve_and_attach(cfg: &Config) -> anyhow::Result<Option<Config>> {
    let Some(id) = resolve_cluster_id(cfg).await? else {
        return Ok(None);
    };
    let mut c = cfg.clone();
    c.k8s.cluster_id = Some(id);
    Ok(Some(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::K8sConfig;
    use ecat_data_clickhouse::ClickhouseConfig;
    use ecat_data_redis::RedisConfig;
    use ecat_mq_kafka::KafkaConfig;
    use superops_protos::k8s::v1::k8s_service_server::{K8sService, K8sServiceServer};
    use superops_protos::k8s::v1::{
        AddClusterRequest, AddClusterResponse, Cluster, DeleteDeploymentRequest,
        DeleteDeploymentResponse, ExecRequest, ExecResponse, GetClusterRequest, GetClusterResponse,
        GetMetricsRequest, GetMetricsResponse, GetPodLogsRequest, ListClustersRequest,
        ListClustersResponse, ListDeploymentsRequest, ListDeploymentsResponse, ListNodesRequest,
        ListNodesResponse, ListPodsRequest, ListPodsResponse, LogLine, RemoveClusterRequest,
        RemoveClusterResponse, RestartDeploymentRequest, RestartDeploymentResponse, RunJobRequest,
        RunJobResponse, ScaleDeploymentRequest, ScaleDeploymentResponse,
        UpdateDeploymentImageRequest, UpdateDeploymentImageResponse, WatchEvent,
        WatchResourcesRequest,
    };
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;
    use tonic::{Request, Response, Status};

    /// mock k8s-service：仅 ListClusters 真实返回，其余 RPC 未实现
    struct MockK8s {
        clusters: Vec<Cluster>,
    }

    #[tonic::async_trait]
    impl K8sService for MockK8s {
        type WatchResourcesStream = tokio_stream::Empty<Result<WatchEvent, Status>>;
        type GetPodLogsStream = tokio_stream::Empty<Result<LogLine, Status>>;
        type ExecPodStream = tokio_stream::Empty<Result<ExecResponse, Status>>;

        async fn list_clusters(
            &self,
            _request: Request<ListClustersRequest>,
        ) -> Result<Response<ListClustersResponse>, Status> {
            Ok(Response::new(ListClustersResponse {
                clusters: self.clusters.clone(),
            }))
        }

        async fn get_cluster(
            &self,
            _r: Request<GetClusterRequest>,
        ) -> Result<Response<GetClusterResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn add_cluster(
            &self,
            _r: Request<AddClusterRequest>,
        ) -> Result<Response<AddClusterResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn remove_cluster(
            &self,
            _r: Request<RemoveClusterRequest>,
        ) -> Result<Response<RemoveClusterResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn list_pods(
            &self,
            _r: Request<ListPodsRequest>,
        ) -> Result<Response<ListPodsResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn list_deployments(
            &self,
            _r: Request<ListDeploymentsRequest>,
        ) -> Result<Response<ListDeploymentsResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn list_nodes(
            &self,
            _r: Request<ListNodesRequest>,
        ) -> Result<Response<ListNodesResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn watch_resources(
            &self,
            _r: Request<WatchResourcesRequest>,
        ) -> Result<Response<Self::WatchResourcesStream>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn get_pod_logs(
            &self,
            _r: Request<GetPodLogsRequest>,
        ) -> Result<Response<Self::GetPodLogsStream>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn exec_pod(
            &self,
            _r: Request<tonic::Streaming<ExecRequest>>,
        ) -> Result<Response<Self::ExecPodStream>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn get_metrics(
            &self,
            _r: Request<GetMetricsRequest>,
        ) -> Result<Response<GetMetricsResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn scale_deployment(
            &self,
            _r: Request<ScaleDeploymentRequest>,
        ) -> Result<Response<ScaleDeploymentResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn restart_deployment(
            &self,
            _r: Request<RestartDeploymentRequest>,
        ) -> Result<Response<RestartDeploymentResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn delete_deployment(
            &self,
            _r: Request<DeleteDeploymentRequest>,
        ) -> Result<Response<DeleteDeploymentResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn update_deployment_image(
            &self,
            _r: Request<UpdateDeploymentImageRequest>,
        ) -> Result<Response<UpdateDeploymentImageResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }

        async fn run_job(
            &self,
            _r: Request<RunJobRequest>,
        ) -> Result<Response<RunJobResponse>, Status> {
            Err(Status::unimplemented("mock"))
        }
    }

    async fn spawn_mock(clusters: Vec<Cluster>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            Server::builder()
                .add_service(K8sServiceServer::new(MockK8s { clusters }))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        format!("http://{addr}")
    }

    fn test_cfg(endpoint: &str) -> Config {
        Config {
            ch: ClickhouseConfig {
                base_url: "http://localhost:8124".into(),
                database: "superops".into(),
                username: None,
                password: None,
                tls: None,
            },
            mq: KafkaConfig {
                brokers: "localhost:9092".into(),
                group_id: None,
            },
            lock: RedisConfig {
                url: "redis://localhost:6380".into(),
                password: None,
                tls: None,
            },
            k8s: K8sConfig {
                endpoint: endpoint.into(),
                cluster_id: None,
                token: None,
            },
            collector: Default::default(),
            mqtt: None,
            nats: None,
            search: None,
            etcd: None,
            consul: None,
            otlp: None,
            notify: Default::default(),
            logtail: Default::default(),
            housekeeping: Default::default(),
            drift: Default::default(),
            selfheal: Default::default(),
            rollback: Default::default(),
            mysql: None,
            smtp: None,
        }
    }

    fn mk_cluster(id: &str) -> superops_protos::k8s::v1::Cluster {
        superops_protos::k8s::v1::Cluster {
            id: id.into(),
            name: id.into(),
            version: String::new(),
            node_count: 0,
            pod_count: 0,
            status: String::new(),
            created_at: 0,
        }
    }

    #[test]
    fn pick_first_returns_first_registered_cluster() {
        assert_eq!(
            pick_first_cluster(&[mk_cluster("c1"), mk_cluster("c2")]),
            Some("c1".into())
        );
        assert_eq!(pick_first_cluster(&[]), None);
    }

    #[tokio::test]
    async fn resolve_prefers_explicit_config() {
        // 配置优先：无需网络，直接返回配置值
        let mut cfg = test_cfg("http://localhost:9091");
        cfg.k8s.cluster_id = Some("explicit-cluster".into());
        let resolved = resolve_cluster_id(&cfg).await.unwrap();
        assert_eq!(resolved, Some("explicit-cluster".into()));
    }

    #[tokio::test]
    async fn resolve_unreachable_endpoint_returns_error() {
        // 未配置且端点不可达：错误向上传播，由任务层决定跳过/告警
        let cfg = test_cfg("http://localhost:1");
        assert!(resolve_cluster_id(&cfg).await.is_err());
    }

    #[tokio::test]
    async fn resolve_falls_back_to_first_registered_cluster() {
        // 契约：未配置 cluster_id 时经 ListClusters 取首个注册集群（注册顺序）
        let endpoint = spawn_mock(vec![mk_cluster("c2"), mk_cluster("c1")]).await;
        let cfg = test_cfg(&endpoint);
        let resolved = resolve_cluster_id(&cfg).await.unwrap();
        assert_eq!(resolved, Some("c2".into()));
    }

    #[tokio::test]
    async fn resolve_returns_none_when_no_cluster_registered() {
        // 契约：无注册集群时返回 None，任务层应跳过本轮并告警
        let endpoint = spawn_mock(vec![]).await;
        let cfg = test_cfg(&endpoint);
        let resolved = resolve_cluster_id(&cfg).await.unwrap();
        assert_eq!(resolved, None);
    }
}
