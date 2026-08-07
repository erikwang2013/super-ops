use anyhow::Result;
use futures::AsyncBufReadExt;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::LogParams;
use kube::runtime::watcher::{Event, watcher};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::cluster::client::ClusterClient;

pub async fn list_pods(client: &ClusterClient, namespace: &str) -> Result<Vec<Pod>> {
    let api: Api<Pod> = if namespace.is_empty() {
        Api::all(client.client.clone())
    } else {
        Api::namespaced(client.client.clone(), namespace)
    };
    Ok(api.list(&Default::default()).await?.items)
}

pub async fn get_pod_logs(
    client: ClusterClient,
    namespace: String,
    pod_name: String,
    container: Option<String>,
    tail_lines: Option<i64>,
    follow: bool,
) -> Result<impl tokio_stream::Stream<Item = String>> {
    let api: Api<Pod> = Api::namespaced(client.client.clone(), &namespace);
    let mut params = LogParams {
        follow,
        container,
        ..Default::default()
    };
    if let Some(t) = tail_lines {
        params.tail_lines = Some(t);
    }

    let log_stream = api.log_stream(&pod_name, &params).await?;
    let (tx, rx) = mpsc::channel::<String>(64);
    tokio::spawn(async move {
        let mut lines = log_stream.lines();
        while let Some(line) = lines.next().await {
            if let Ok(line) = line
                && tx.send(line).await.is_err()
            {
                break;
            }
        }
    });
    Ok(ReceiverStream::new(rx))
}

pub async fn watch_pods(
    client: ClusterClient,
    namespace: String,
) -> Result<impl tokio_stream::Stream<Item = Event<Pod>>> {
    let api: Api<Pod> = if namespace.is_empty() {
        Api::all(client.client.clone())
    } else {
        Api::namespaced(client.client.clone(), &namespace)
    };
    let (tx, rx) = mpsc::channel::<Event<Pod>>(64);
    tokio::spawn(async move {
        let w = watcher(api, Default::default());
        futures::pin_mut!(w);
        while let Some(event) = w.next().await {
            if let Ok(event) = event
                && tx.send(event).await.is_err()
            {
                break;
            }
        }
    });
    Ok(ReceiverStream::new(rx))
}
