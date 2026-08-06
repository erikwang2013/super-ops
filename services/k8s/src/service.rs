use futures::{SinkExt, StreamExt};
use kube::ResourceExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use superops_protos::k8s::v1::{
    AddClusterRequest, AddClusterResponse, Cluster, GetClusterRequest, GetClusterResponse,
    GetMetricsRequest, GetMetricsResponse, GetPodLogsRequest, ListClustersRequest,
    ListClustersResponse, ListDeploymentsRequest, ListDeploymentsResponse, ListNodesRequest,
    ListNodesResponse, ListPodsRequest, ListPodsResponse, LogLine, RemoveClusterRequest,
    RemoveClusterResponse, RunJobRequest, RunJobResponse, WatchEvent, WatchResourcesRequest,
    k8s_service_server::K8sService,
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

    #[tracing::instrument(skip(self, request))]
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
                    ready_replicas: d
                        .status
                        .as_ref()
                        .and_then(|s| s.ready_replicas)
                        .unwrap_or(0),
                    ..Default::default()
                })
                .collect(),
        }))
    }

    async fn scale_deployment(
        &self,
        request: Request<superops_protos::k8s::v1::ScaleDeploymentRequest>,
    ) -> GrpcResult<superops_protos::k8s::v1::ScaleDeploymentResponse> {
        let req = request.into_inner();
        resource::write::validate_scale(&req.cluster_id, &req.namespace, &req.name, req.replicas)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let replicas =
            resource::write::scale_deployment(&client, &req.namespace, &req.name, req.replicas)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(
            superops_protos::k8s::v1::ScaleDeploymentResponse { replicas },
        ))
    }

    async fn restart_deployment(
        &self,
        request: Request<superops_protos::k8s::v1::RestartDeploymentRequest>,
    ) -> GrpcResult<superops_protos::k8s::v1::RestartDeploymentResponse> {
        let req = request.into_inner();
        if req.cluster_id.is_empty() || req.namespace.is_empty() || req.name.is_empty() {
            return Err(Status::invalid_argument(
                "cluster_id/namespace/name must not be empty",
            ));
        }
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let restarted = resource::write::restart_deployment(&client, &req.namespace, &req.name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(
            superops_protos::k8s::v1::RestartDeploymentResponse { restarted },
        ))
    }

    async fn delete_deployment(
        &self,
        request: Request<superops_protos::k8s::v1::DeleteDeploymentRequest>,
    ) -> GrpcResult<superops_protos::k8s::v1::DeleteDeploymentResponse> {
        let req = request.into_inner();
        if req.cluster_id.is_empty() || req.namespace.is_empty() || req.name.is_empty() {
            return Err(Status::invalid_argument(
                "cluster_id/namespace/name must not be empty",
            ));
        }
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let deleted = resource::write::delete_deployment(&client, &req.namespace, &req.name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(
            superops_protos::k8s::v1::DeleteDeploymentResponse { deleted },
        ))
    }

    async fn run_job(&self, request: Request<RunJobRequest>) -> GrpcResult<RunJobResponse> {
        let req = request.into_inner();
        resource::job::validate_job(
            &req.cluster_id,
            &req.namespace,
            &req.job_name,
            &req.image,
            &req.command,
        )
        .map_err(|e| Status::invalid_argument(e.to_string()))?;
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        resource::job::run_job(
            &client,
            &req.namespace,
            &req.job_name,
            &req.image,
            &req.command,
            req.timeout_s,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(RunJobResponse { created: true }))
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
                        .map(|c| {
                            if c.status == "True" {
                                "Ready"
                            } else {
                                "NotReady"
                            }
                        })
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

    #[tracing::instrument(skip(self, request))]
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
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            while let Some(event) = pod_stream.next().await {
                let watch_event = match event {
                    kube::runtime::watcher::Event::Apply(pod) => {
                        let name = pod.name_any();
                        let event_type = if seen.insert(name.clone()) {
                            "ADDED"
                        } else {
                            "MODIFIED"
                        };
                        WatchEvent {
                            event_type: event_type.into(),
                            resource_type: "pod".into(),
                            resource_name: name,
                            namespace: pod.namespace().unwrap_or_default(),
                            ..Default::default()
                        }
                    }
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

    #[tracing::instrument(skip(self, request))]
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
            client, namespace, pod_name, container, tail_lines, req.follow,
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

    #[tracing::instrument(skip(self, request))]
    async fn exec_pod(
        &self,
        request: Request<tonic::Streaming<superops_protos::k8s::v1::ExecRequest>>,
    ) -> GrpcResult<Self::ExecPodStream> {
        let mut in_stream = request.into_inner();
        let first = in_stream
            .message()
            .await
            .map_err(|e| Status::internal(format!("read exec request: {e}")))?
            .ok_or_else(|| {
                Status::invalid_argument("exec stream must start with a connect message")
            })?;
        let client = self
            .manager
            .get(&first.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let container = if first.container.is_empty() {
            None
        } else {
            Some(first.container.clone())
        };

        let mut attached = resource::exec::exec_pod(
            &client,
            first.namespace.clone(),
            first.pod_name.clone(),
            container,
            &first.command,
        )
        .await
        .map_err(|e| Status::internal(format!("exec failed: {e}")))?;
        let stdin = attached.stdin();
        let stdout = attached.stdout();
        let stderr = attached.stderr();
        let resize = attached.terminal_size();

        let (tx, rx) = mpsc::channel::<Result<superops_protos::k8s::v1::ExecResponse, Status>>(128);

        // outbound stdout → client
        let out_tx = tx.clone();
        tokio::spawn(async move {
            let mut reader = match stdout {
                Some(r) => r,
                None => return,
            };
            let mut buf = vec![0u8; 8192];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if out_tx
                            .send(Ok(superops_protos::k8s::v1::ExecResponse {
                                stdout: buf[..n].to_vec(),
                                stderr: Vec::new(),
                            }))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });

        // outbound stderr → client
        let out_tx2 = tx.clone();
        tokio::spawn(async move {
            let mut reader = match stderr {
                Some(r) => r,
                None => return,
            };
            let mut buf = vec![0u8; 8192];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if out_tx2
                            .send(Ok(superops_protos::k8s::v1::ExecResponse {
                                stdout: Vec::new(),
                                stderr: buf[..n].to_vec(),
                            }))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });

        // inbound: client → stdin / resize
        tokio::spawn(async move {
            let mut writer = stdin;
            let mut resize = resize;
            if !first.stdin.is_empty() {
                if let Some(w) = writer.as_mut() {
                    if w.write_all(&first.stdin).await.is_err() {
                        return;
                    }
                }
            }
            while let Some(msg) = in_stream.next().await {
                let Ok(msg) = msg else { break };
                if let Some(ws) = resize.as_mut() {
                    if let Some(ts) = msg.terminal_size {
                        if ws
                            .send(kube::api::TerminalSize {
                                height: ts.height as u16,
                                width: ts.width as u16,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                if !msg.stdin.is_empty() {
                    if let Some(w) = writer.as_mut() {
                        if w.write_all(&msg.stdin).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::StreamBody;
    use prost::Message as _;
    use tonic::codec::{Codec as _, ProstCodec};

    type ExecRequest = superops_protos::k8s::v1::ExecRequest;
    type ExecResponse = superops_protos::k8s::v1::ExecResponse;

    fn grpc_frame(msg: &ExecRequest) -> bytes::Bytes {
        let payload = msg.encode_to_vec();
        let mut frame = Vec::with_capacity(5 + payload.len());
        frame.push(0u8); // no compression
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&payload);
        bytes::Bytes::from(frame)
    }

    fn exec_stream(msgs: Vec<ExecRequest>) -> tonic::Streaming<ExecRequest> {
        let frames = msgs
            .into_iter()
            .map(|m| Ok::<_, std::io::Error>(http_body::Frame::data(grpc_frame(&m))));
        // ProstCodec<T, U>: T = Encode, U = Decode → 请求方向 decoder = ProstCodec<_, ExecRequest>
        let mut codec = ProstCodec::<ExecResponse, ExecRequest>::default();
        tonic::Streaming::new_request(
            codec.decoder(),
            StreamBody::new(tokio_stream::iter(frames)),
            None,
            None,
        )
    }

    #[tokio::test]
    async fn exec_pod_rejects_empty_stream() {
        let svc = K8sServiceImpl {
            manager: ClusterManager::new(),
        };
        let req = Request::new(exec_stream(vec![]));
        let err = svc.exec_pod(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn exec_pod_unknown_cluster_is_not_found() {
        let svc = K8sServiceImpl {
            manager: ClusterManager::new(),
        };
        let req = Request::new(exec_stream(vec![ExecRequest {
            cluster_id: "nope".into(),
            ..Default::default()
        }]));
        let err = svc.exec_pod(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn scale_deployment_invalid_replicas_is_invalid_argument() {
        let svc = K8sServiceImpl {
            manager: ClusterManager::new(),
        };
        let req = Request::new(superops_protos::k8s::v1::ScaleDeploymentRequest {
            cluster_id: "c".into(),
            namespace: "ns".into(),
            name: "d".into(),
            replicas: 2000,
        });
        let err = svc.scale_deployment(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn scale_deployment_unknown_cluster_is_not_found() {
        let svc = K8sServiceImpl {
            manager: ClusterManager::new(),
        };
        let req = Request::new(superops_protos::k8s::v1::ScaleDeploymentRequest {
            cluster_id: "nope".into(),
            namespace: "ns".into(),
            name: "d".into(),
            replicas: 3,
        });
        let err = svc.scale_deployment(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn run_job_empty_cluster_is_invalid_argument() {
        let svc = K8sServiceImpl {
            manager: ClusterManager::new(),
        };
        let req = Request::new(RunJobRequest {
            cluster_id: "".into(),
            namespace: "ns".into(),
            job_name: "job-1".into(),
            image: "busybox:1.36".into(),
            command: "echo hi".into(),
            timeout_s: 300,
        });
        let err = svc.run_job(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn run_job_unknown_cluster_is_not_found() {
        let svc = K8sServiceImpl {
            manager: ClusterManager::new(),
        };
        let req = Request::new(RunJobRequest {
            cluster_id: "nope".into(),
            namespace: "ns".into(),
            job_name: "job-1".into(),
            image: "busybox:1.36".into(),
            command: "echo hi".into(),
            timeout_s: 300,
        });
        let err = svc.run_job(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }
}
