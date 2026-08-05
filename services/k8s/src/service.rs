use futures::StreamExt;
use kube::ResourceExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use superops_protos::k8s::v1::{
    k8s_service_server::K8sService, AddClusterRequest, AddClusterResponse, Cluster,
    GetClusterRequest, GetClusterResponse, GetMetricsRequest, GetMetricsResponse,
    GetPodLogsRequest, ListClustersRequest, ListClustersResponse, ListDeploymentsRequest,
    ListDeploymentsResponse, ListNodesRequest, ListNodesResponse, ListPodsRequest,
    ListPodsResponse, LogLine, RemoveClusterRequest, RemoveClusterResponse,
    WatchEvent, WatchResourcesRequest,
};

use crate::cluster::manager::ClusterManager;
use crate::resource;

pub struct K8sServiceImpl {
    pub manager: ClusterManager,
}

type GrpcResult<T> = Result<Response<T>, Status>;

#[tonic::async_trait]
impl K8sService for K8sServiceImpl {
    async fn list_clusters(
        &self,
        _request: Request<ListClustersRequest>,
    ) -> GrpcResult<ListClustersResponse> {
        let clients = self.manager.list();
        Ok(Response::new(ListClustersResponse {
            clusters: clients
                .into_iter()
                .map(|c| Cluster {
                    id: c.id,
                    name: c.name,
                    ..Default::default()
                })
                .collect(),
        }))
    }

    async fn get_cluster(
        &self,
        request: Request<GetClusterRequest>,
    ) -> GrpcResult<GetClusterResponse> {
        let req = request.into_inner();
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        Ok(Response::new(GetClusterResponse {
            cluster: Some(Cluster {
                id: client.id,
                name: client.name,
                ..Default::default()
            }),
        }))
    }

    async fn add_cluster(
        &self,
        request: Request<AddClusterRequest>,
    ) -> GrpcResult<AddClusterResponse> {
        let req = request.into_inner();
        let client = self
            .manager
            .add(req.name, req.kubeconfig)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(AddClusterResponse {
            cluster: Some(Cluster {
                id: client.id,
                name: client.name,
                ..Default::default()
            }),
        }))
    }

    async fn remove_cluster(
        &self,
        request: Request<RemoveClusterRequest>,
    ) -> GrpcResult<RemoveClusterResponse> {
        let req = request.into_inner();
        self.manager
            .remove(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        Ok(Response::new(RemoveClusterResponse {}))
    }

    async fn list_pods(&self, request: Request<ListPodsRequest>) -> GrpcResult<ListPodsResponse> {
        let req = request.into_inner();
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let namespace = req.namespace.clone();
        let pods = resource::pod::list_pods(&client, &namespace)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListPodsResponse {
            pods: pods
                .into_iter()
                .map(|p| superops_protos::k8s::v1::Pod {
                    name: p.name_any(),
                    namespace: p.namespace().unwrap_or_default(),
                    status: p
                        .status
                        .as_ref()
                        .and_then(|s| s.phase.clone())
                        .unwrap_or_default(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }))
    }

    async fn list_deployments(
        &self,
        request: Request<ListDeploymentsRequest>,
    ) -> GrpcResult<ListDeploymentsResponse> {
        let req = request.into_inner();
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let namespace = req.namespace.clone();
        let deps = resource::deploy::list_deployments(&client, &namespace)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListDeploymentsResponse {
            deployments: deps
                .into_iter()
                .map(|d| superops_protos::k8s::v1::Deployment {
                    name: d.name_any(),
                    namespace: d.namespace().unwrap_or_default(),
                    replicas: d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0),
                    ready_replicas: d.status.as_ref().and_then(|s| s.ready_replicas).unwrap_or(0),
                    ..Default::default()
                })
                .collect(),
        }))
    }

    async fn list_nodes(
        &self,
        request: Request<ListNodesRequest>,
    ) -> GrpcResult<ListNodesResponse> {
        let req = request.into_inner();
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let nodes = resource::node::list_nodes(&client)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListNodesResponse {
            nodes: nodes
                .into_iter()
                .map(|n| superops_protos::k8s::v1::Node {
                    name: n.name_any(),
                    status: n
                        .status
                        .as_ref()
                        .and_then(|s| s.conditions.as_ref())
                        .and_then(|c| c.iter().find(|c| c.type_ == "Ready"))
                        .map(|c| if c.status == "True" { "Ready" } else { "NotReady" })
                        .unwrap_or("Unknown")
                        .into(),
                    version: n
                        .status
                        .as_ref()
                        .and_then(|s| s.node_info.as_ref())
                        .map(|i| i.kubelet_version.clone())
                        .unwrap_or_default(),
                    ..Default::default()
                })
                .collect(),
        }))
    }

    type WatchResourcesStream = ReceiverStream<Result<WatchEvent, Status>>;

    async fn watch_resources(
        &self,
        request: Request<WatchResourcesRequest>,
    ) -> GrpcResult<Self::WatchResourcesStream> {
        let req = request.into_inner();
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let namespace = req.namespace.clone();
        let pod_stream = resource::pod::watch_pods(client, namespace)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let (tx, rx) = mpsc::channel::<Result<WatchEvent, Status>>(128);
        tokio::spawn(async move {
            futures::pin_mut!(pod_stream);
            while let Some(event) = pod_stream.next().await {
                let watch_event = match event {
                    kube::runtime::watcher::Event::Apply(pod) => WatchEvent {
                        event_type: "MODIFIED".into(),
                        resource_type: "pod".into(),
                        resource_name: pod.name_any(),
                        namespace: pod.namespace().unwrap_or_default(),
                        ..Default::default()
                    },
                    kube::runtime::watcher::Event::Delete(pod) => WatchEvent {
                        event_type: "DELETED".into(),
                        resource_type: "pod".into(),
                        resource_name: pod.name_any(),
                        namespace: pod.namespace().unwrap_or_default(),
                        ..Default::default()
                    },
                    _ => continue,
                };
                if tx.send(Ok(watch_event)).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    type GetPodLogsStream = ReceiverStream<Result<LogLine, Status>>;

    async fn get_pod_logs(
        &self,
        request: Request<GetPodLogsRequest>,
    ) -> GrpcResult<Self::GetPodLogsStream> {
        let req = request.into_inner();
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let namespace = req.namespace.clone();
        let pod_name = req.pod_name.clone();
        let container = if req.container.is_empty() {
            None
        } else {
            Some(req.container.clone())
        };
        let tail_lines = if req.tail_lines > 0 {
            Some(req.tail_lines as i64)
        } else {
            None
        };

        let log_stream = resource::pod::get_pod_logs(
            client,
            namespace,
            pod_name,
            container,
            tail_lines,
            req.follow,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        let (tx, rx) = mpsc::channel::<Result<LogLine, Status>>(64);
        tokio::spawn(async move {
            futures::pin_mut!(log_stream);
            while let Some(line) = log_stream.next().await {
                let _ = tx
                    .send(Ok(LogLine {
                        content: line,
                        ..Default::default()
                    }))
                    .await;
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    type ExecPodStream = ReceiverStream<Result<superops_protos::k8s::v1::ExecResponse, Status>>;

    async fn exec_pod(
        &self,
        _request: Request<tonic::Streaming<superops_protos::k8s::v1::ExecRequest>>,
    ) -> GrpcResult<Self::ExecPodStream> {
        Err(Status::unimplemented("exec pod not yet implemented"))
    }

    async fn get_metrics(
        &self,
        _request: Request<GetMetricsRequest>,
    ) -> GrpcResult<GetMetricsResponse> {
        Ok(Response::new(GetMetricsResponse {
            metrics: Vec::new(),
        }))
    }
}
