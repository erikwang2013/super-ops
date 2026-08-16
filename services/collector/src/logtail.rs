use crate::ch::{clickhouse_from, now_secs};
use crate::config::Config;
use ecat_data::{DataPoint, FieldValue, TsdbClient};
use std::sync::Arc;
use superops_protos::k8s::v1::{GetPodLogsRequest, ListPodsRequest};

pub fn truncate_line(line: &str, max_bytes: usize) -> String {
    if line.len() <= max_bytes {
        line.to_string()
    } else {
        // 截断到 ≤ max_bytes 的最后一个字符边界，避免多字节字符被切断
        let end = line
            .char_indices()
            .take_while(|(i, _)| *i <= max_bytes)
            .map(|(i, _)| i)
            .last()
            .unwrap_or(0);
        format!("{}…(truncated)", &line[..end])
    }
}

pub fn dedup_continuous(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if out.last().map(|p| p != l).unwrap_or(true) {
            out.push(l.clone());
        }
    }
    out
}

pub async fn collect_once(cfg: &Config) -> anyhow::Result<()> {
    if cfg.logtail.namespaces.is_empty() {
        return Ok(());
    }
    let Some(cluster_id) = crate::cluster::resolve_cluster_id(cfg).await? else {
        tracing::warn!("no registered cluster; skipping logtail round");
        return Ok(());
    };
    let mut client = crate::cluster::k8s_client(cfg).await?;
    let ch = clickhouse_from(cfg)?;
    // 日志检索后端：search 段配置时，同一批日志同时索引到 ES/OpenSearch
    let search_client: Option<(Arc<dyn ecat_data::SearchClient>, String)> = match &cfg.search {
        Some(s) => {
            let client: Arc<dyn ecat_data::SearchClient> = match s.provider.as_str() {
                "opensearch" => Arc::new(
                    ecat_data_opensearch::OpenSearchClient::from_config(
                        ecat_data_opensearch::OpenSearchConfig {
                            base_url: s.base_url.clone(),
                            username: s.username.clone(),
                            password: s.password.clone(),
                            tls: None,
                        },
                    )
                    .map_err(|e| anyhow::anyhow!("opensearch config: {e}"))?,
                ),
                _ => Arc::new(
                    ecat_data_elasticsearch::ElasticsearchClient::from_config(
                        ecat_data_elasticsearch::ElasticsearchConfig {
                            base_url: s.base_url.clone(),
                            username: s.username.clone(),
                            password: s.password.clone(),
                            tls: None,
                        },
                    )
                    .map_err(|e| anyhow::anyhow!("elasticsearch config: {e}"))?,
                ),
            };
            Some((client, s.index.clone()))
        }
        None => None,
    };
    for ns in &cfg.logtail.namespaces {
        let pods = client
            .list_pods(ListPodsRequest {
                cluster_id: cluster_id.clone(),
                namespace: ns.clone(),
                ..Default::default()
            })
            .await?
            .into_inner()
            .pods;
        for pod in pods {
            let stream = match client
                .get_pod_logs(GetPodLogsRequest {
                    cluster_id: cluster_id.clone(),
                    namespace: ns.clone(),
                    pod_name: pod.name.clone(),
                    container: String::new(),
                    tail_lines: cfg.logtail.tail_lines,
                    follow: false,
                })
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    // 单 Pod 失败不影响其余 Pod 的采集
                    tracing::warn!("logs for {}/{}: {e}", ns, pod.name);
                    continue;
                }
            };
            let mut lines = Vec::new();
            let mut stream = stream.into_inner();
            loop {
                match stream.message().await {
                    Ok(Some(line)) => lines.push(line.content),
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!("log stream for {}/{}: {e}", ns, pod.name);
                        break;
                    }
                }
            }
            let points: Vec<DataPoint> = dedup_continuous(&lines)
                .into_iter()
                .map(|content| {
                    let content = truncate_line(&content, cfg.logtail.max_line_bytes as usize);
                    DataPoint::new("pod_log")
                        .with_tag("namespace", ns.clone())
                        .with_tag("pod", pod.name.clone())
                        .with_field("content", FieldValue::String(content))
                        .with_timestamp(now_secs())
                })
                .collect();
            if !points.is_empty() {
                TsdbClient::write(ch.as_ref(), &points).await?;
                if let Some((search, index)) = &search_client {
                    let ts = now_secs();
                    for (i, content) in lines.iter().enumerate() {
                        let id = format!("{ns}/{}/{ts}-{i}", pod.name);
                        let doc = serde_json::json!({
                            "namespace": ns,
                            "pod": pod.name,
                            "content": truncate_line(content, cfg.logtail.max_line_bytes as usize),
                            "timestamp": ts,
                        });
                        if let Err(e) = search.index(index, &id, &doc).await {
                            tracing::warn!("log search index {id} failed: {e}");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_line_cuts_long_lines() {
        let long = "x".repeat(1000);
        assert_eq!(truncate_line(&long, 64).len(), 64 + "…(truncated)".len());
        assert_eq!(truncate_line("short", 64), "short");
    }

    #[test]
    fn truncate_line_handles_multibyte_boundary() {
        let long = "中".repeat(500);
        let out = truncate_line(&long, 64);
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        assert!(out.len() <= 64 + "…(truncated)".len());
    }

    #[test]
    fn dedup_continuous_keeps_only_runs() {
        let lines = vec![
            "a".into(),
            "a".into(),
            "b".into(),
            "b".into(),
            "b".into(),
            "a".into(),
        ];
        assert_eq!(
            dedup_continuous(&lines),
            vec!["a".to_string(), "b".to_string(), "a".to_string()]
        );
    }
}
